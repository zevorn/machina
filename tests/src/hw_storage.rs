use machina_hw_storage::{
    BlockBackend, BlockMedia, FileBackend, FlashMedia, MemBackend, StorageError,
};
use tempfile::NamedTempFile;

// -- StorageError tests --

#[test]
fn test_storage_error_display() {
    assert_eq!(
        format!("{}", StorageError::ReadOnly),
        "backend is read-only"
    );
    assert_eq!(
        format!("{}", StorageError::Overflow),
        "offset + length overflow"
    );
    assert_eq!(
        format!("{}", StorageError::OutOfRange),
        "access out of range"
    );
    assert_eq!(
        format!(
            "{}",
            StorageError::ShortIO {
                expected: 8,
                actual: 4
            }
        ),
        "short I/O: expected 8 bytes, got 4"
    );
    assert_eq!(
        format!("{}", StorageError::Backend("boom".to_string())),
        "boom"
    );
}

#[test]
fn test_storage_error_eq() {
    assert_eq!(StorageError::ReadOnly, StorageError::ReadOnly);
    assert_ne!(StorageError::ReadOnly, StorageError::Overflow);
    assert_eq!(
        StorageError::ShortIO {
            expected: 4,
            actual: 2
        },
        StorageError::ShortIO {
            expected: 4,
            actual: 2
        }
    );
    assert_ne!(
        StorageError::ShortIO {
            expected: 4,
            actual: 2
        },
        StorageError::ShortIO {
            expected: 4,
            actual: 1
        }
    );
}

// -- MemBackend tests (updated for typed errors) --

#[test]
fn test_mem_backend_read() {
    let backend = MemBackend::new(vec![0x01, 0x02, 0x03, 0x04], false);
    let mut buf = [0u8; 4];
    let n = backend.read(0, &mut buf).unwrap();
    assert_eq!(n, 4);
    assert_eq!(buf, [0x01, 0x02, 0x03, 0x04]);
}

#[test]
fn test_mem_backend_read_partial() {
    let backend = MemBackend::new(vec![0xAA, 0xBB], false);
    let mut buf = [0u8; 4];
    let n = backend.read(0, &mut buf).unwrap();
    assert_eq!(n, 2);
    assert_eq!(buf[..2], [0xAA, 0xBB]);
}

#[test]
fn test_mem_backend_read_past_end() {
    let backend = MemBackend::new(vec![0x01], false);
    let mut buf = [0u8; 4];
    let n = backend.read(5, &mut buf).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn test_mem_backend_write() {
    let backend = MemBackend::new(vec![0; 4], false);
    let n = backend.write(0, &[0xDE, 0xAD]).unwrap();
    assert_eq!(n, 2);
    assert_eq!(backend.to_vec(), vec![0xDE, 0xAD, 0, 0]);
}

#[test]
fn test_mem_backend_write_expands() {
    let backend = MemBackend::new(vec![0; 2], false);
    backend.write(0, &[1, 2, 3, 4]).unwrap();
    assert_eq!(backend.to_vec(), vec![1, 2, 3, 4]);
}

#[test]
fn test_mem_backend_write_readonly() {
    let backend = MemBackend::new(vec![0; 4], true);
    assert_eq!(backend.write(0, &[1]).unwrap_err(), StorageError::ReadOnly);
}

#[test]
fn test_mem_backend_size() {
    let backend = MemBackend::new(vec![0; 1024], false);
    assert_eq!(backend.size(), 1024);
}

#[test]
fn test_mem_backend_readonly() {
    let rw = MemBackend::new(vec![], false);
    assert!(!rw.readonly());

    let ro = MemBackend::new(vec![], true);
    assert!(ro.readonly());
}

#[test]
fn test_mem_backend_flush() {
    let backend = MemBackend::new(vec![], false);
    assert!(backend.flush().is_ok());
}

// -- Exact read/write tests --

#[test]
fn test_read_exact_success() {
    let backend = MemBackend::new(vec![0xAA; 128], false);
    let mut buf = [0u8; 64];
    backend.read_exact(32, &mut buf).unwrap();
    assert_eq!(buf, [0xAA; 64]);
}

#[test]
fn test_read_exact_out_of_range() {
    let backend = MemBackend::new(vec![0; 16], false);
    let mut buf = [0u8; 8];
    // offset past end of backend
    let err = backend.read_exact(20, &mut buf).unwrap_err();
    assert_eq!(err, StorageError::OutOfRange);
}

#[test]
fn test_read_exact_span_out_of_range() {
    let backend = MemBackend::new(vec![0; 16], false);
    let mut buf = [0u8; 8];
    // offset within range but offset+len exceeds size
    let err = backend.read_exact(12, &mut buf).unwrap_err();
    assert_eq!(err, StorageError::OutOfRange);
}

#[test]
fn test_read_exact_overflow() {
    let backend = MemBackend::new(vec![0; 16], false);
    let mut buf = [0u8; 8];
    // offset + len wraps around u64
    let err = backend.read_exact(u64::MAX - 3, &mut buf).unwrap_err();
    assert_eq!(err, StorageError::Overflow);
}

#[test]
fn test_write_exact_success() {
    let backend = MemBackend::new(vec![0; 64], false);
    backend.write_exact(16, &[0xBB; 32]).unwrap();
    let data = backend.to_vec();
    assert_eq!(&data[..16], &[0; 16]);
    assert_eq!(&data[16..48], &[0xBB; 32]);
    assert_eq!(&data[48..], &[0; 16]);
}

#[test]
fn test_write_exact_readonly() {
    let backend = MemBackend::new(vec![0; 16], true);
    let err = backend.write_exact(0, &[0xAA]).unwrap_err();
    assert_eq!(err, StorageError::ReadOnly);
}

#[test]
fn test_write_exact_overflow() {
    let backend = MemBackend::new(vec![0; 16], false);
    // offset + len wraps around u64
    let err = backend
        .write_exact(u64::MAX - 1, &[0xAA, 0xBB])
        .unwrap_err();
    assert_eq!(err, StorageError::Overflow);
}

// -- Overflow rejection tests (MemBackend usiz conversion) --

#[test]
fn test_mem_backend_read_overflow_usiz() {
    let backend = MemBackend::new(vec![0; 4], false);
    let mut buf = [0u8; 4];
    // On 64-bit, u64::MAX is a valid usize; past-end returns Ok(0)
    let n = backend.read(u64::MAX, &mut buf).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn test_mem_backend_write_overflow_usiz() {
    let backend = MemBackend::new(vec![0; 4], false);
    // u64::MAX offset; on 64-bit usize == u64, so no truncation;
    // but u64::MAX as usize + buf.len() wraps, so checked_add fails
    let result = backend.write(u64::MAX, &[0xAA]);
    // u64::MAX as usize + 1 wraps → checked_add returns None → error
    assert!(result.is_err());
}

// -- FileBackend tests (updated for typed errors) --

#[test]
fn test_file_backend_write_read() {
    let tmp = NamedTempFile::new().unwrap();
    let backend = FileBackend::open(tmp.path(), false).unwrap();

    backend.write(0, &[0x11, 0x22, 0x33]).unwrap();
    backend.flush().unwrap();

    let mut buf = [0u8; 5];
    let n = backend.read(0, &mut buf).unwrap();
    assert_eq!(n, 3);
    assert_eq!(buf[..3], [0x11, 0x22, 0x33]);
}

#[test]
fn test_file_backend_readonly() {
    let tmp = NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &[0xAB, 0xCD]).unwrap();

    let backend = FileBackend::open(tmp.path(), true).unwrap();
    assert!(backend.readonly());

    let mut buf = [0u8; 2];
    let n = backend.read(0, &mut buf).unwrap();
    assert_eq!(n, 2);
    assert_eq!(buf, [0xAB, 0xCD]);

    assert_eq!(backend.write(0, &[0]).unwrap_err(), StorageError::ReadOnly);
}

#[test]
fn test_file_backend_seek_read() {
    let tmp = NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &[0x00, 0x00, 0xAA, 0xBB]).unwrap();

    let backend = FileBackend::open(tmp.path(), false).unwrap();

    let mut buf = [0u8; 2];
    let n = backend.read(2, &mut buf).unwrap();
    assert_eq!(n, 2);
    assert_eq!(buf, [0xAA, 0xBB]);
}

#[test]
fn test_file_backend_path() {
    let tmp = NamedTempFile::new().unwrap();
    let backend = FileBackend::open(tmp.path(), false).unwrap();
    assert_eq!(backend.path(), &tmp.path().to_path_buf());
}

#[test]
fn test_file_backend_size_after_write() {
    let tmp = NamedTempFile::new().unwrap();
    let backend = FileBackend::open(tmp.path(), false).unwrap();
    assert_eq!(backend.size(), 0);

    backend.write(0, &[0x11, 0x22, 0x33]).unwrap();
    assert_eq!(backend.size(), 3);

    backend.write(3, &[0x44, 0x55]).unwrap();
    assert_eq!(backend.size(), 5);
}

// -- File persistence round-trip --

#[test]
fn test_file_backend_persistence_round_trip() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();

    // Write data through one backend instance
    {
        let backend = FileBackend::open(&path, false).unwrap();
        backend.write_exact(0, &[0x10, 0x20, 0x30, 0x40]).unwrap();
        backend.write_exact(4, &[0x50, 0x60]).unwrap();
        backend.flush().unwrap();
    }

    // Re-open and verify
    {
        let backend = FileBackend::open(&path, false).unwrap();
        assert_eq!(backend.size(), 6);
        let mut buf = [0u8; 6];
        backend.read_exact(0, &mut buf).unwrap();
        assert_eq!(buf, [0x10, 0x20, 0x30, 0x40, 0x50, 0x60]);
    }
}

// -- File exact read/write tests --

#[test]
fn test_file_backend_read_exact_out_of_range() {
    let tmp = NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &[0xAA; 8]).unwrap();
    let backend = FileBackend::open(tmp.path(), false).unwrap();

    let mut buf = [0u8; 4];
    let err = backend.read_exact(10, &mut buf).unwrap_err();
    assert_eq!(err, StorageError::OutOfRange);
}

#[test]
fn test_file_backend_write_exact_readonly_rejection() {
    let tmp = NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &[0; 8]).unwrap();
    let backend = FileBackend::open(tmp.path(), true).unwrap();

    let err = backend.write_exact(0, &[0xAA; 4]).unwrap_err();
    assert_eq!(err, StorageError::ReadOnly);
}

// -- BlockMedia tests --

fn make_block_media(data: Vec<u8>) -> BlockMedia<MemBackend> {
    let backend = MemBackend::new(data, false);
    BlockMedia::new(backend, 512)
}

#[test]
fn test_block_media_read_write_one_sector() {
    let mut sector_data = vec![0u8; 512];
    sector_data[0] = 0xAB;
    sector_data[511] = 0xCD;

    let media = make_block_media(vec![0; 1024]);

    media.write_block(0, &sector_data).unwrap();

    let mut buf = vec![0u8; 512];
    media.read_block(0, &mut buf).unwrap();
    assert_eq!(buf, sector_data);

    // Sector 1 should still be zero
    media.read_block(1, &mut buf).unwrap();
    assert_eq!(buf, vec![0u8; 512]);
}

#[test]
fn test_block_media_read_block_out_of_range() {
    let media = make_block_media(vec![0; 1024]); // 2 sectors
    let mut buf = vec![0u8; 512];
    let err = media.read_block(2, &mut buf).unwrap_err();
    assert_eq!(err, StorageError::OutOfRange);
}

#[test]
fn test_block_media_write_block_out_of_range() {
    let media = make_block_media(vec![0; 1024]);
    let err = media.write_block(2, &vec![0u8; 512]).unwrap_err();
    assert_eq!(err, StorageError::OutOfRange);
}

#[test]
fn test_block_media_buf_size_mismatch() {
    let media = make_block_media(vec![0; 1024]);
    // Buffer smaller than block size
    let mut buf = vec![0u8; 256];
    let err = media.read_block(0, &mut buf).unwrap_err();
    assert_eq!(
        err,
        StorageError::ShortIO {
            expected: 512,
            actual: 256
        }
    );

    let err = media.write_block(0, &buf).unwrap_err();
    assert_eq!(
        err,
        StorageError::ShortIO {
            expected: 512,
            actual: 256
        }
    );
}

#[test]
fn test_block_media_multi_sector_read_write() {
    let media = make_block_media(vec![0; 2048]); // 4 sectors
    let data = vec![0xCC; 1024]; // 2 sectors worth
    media.write_blocks(1, &data).unwrap();

    let mut buf = vec![0u8; 1024];
    media.read_blocks(1, &mut buf).unwrap();
    assert_eq!(buf, data);

    // Sector 0 and 3 should still be zero
    let mut buf512 = vec![0u8; 512];
    media.read_block(0, &mut buf512).unwrap();
    assert_eq!(buf512, vec![0u8; 512]);
    media.read_block(3, &mut buf512).unwrap();
    assert_eq!(buf512, vec![0u8; 512]);
}

#[test]
fn test_block_media_multi_sector_unaligned_buf() {
    let media = make_block_media(vec![0; 4096]);
    let buf = vec![0u8; 700]; // not a multiple of 512
    let err = media.read_blocks(0, &mut buf.clone()).unwrap_err();
    assert!(matches!(err, StorageError::ShortIO { .. }));

    let err = media.write_blocks(0, &buf).unwrap_err();
    assert!(matches!(err, StorageError::ShortIO { .. }));
}

#[test]
fn test_block_media_sector_count() {
    let media = make_block_media(vec![0; 2048]);
    assert_eq!(media.sector_count(), 4);
    assert_eq!(media.block_size(), 512);
}

#[test]
fn test_block_media_sector_overflow() {
    // sector * block_size overflows u64
    let media = make_block_media(vec![0; 2048]);
    let err = media
        .read_block(u64::MAX / 512 + 1, &mut vec![0u8; 512])
        .unwrap_err();
    assert_eq!(err, StorageError::Overflow);
}

// -- FlashMedia tests --

fn make_flash(data: Vec<u8>) -> FlashMedia<MemBackend> {
    let backend = MemBackend::new(data, false);
    FlashMedia::new(backend, 4096)
}

#[test]
fn test_flash_media_read() {
    let flash = make_flash(vec![0x11, 0x22, 0x33, 0x44]);
    let mut buf = [0u8; 4];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf, [0x11, 0x22, 0x33, 0x44]);
}

#[test]
fn test_flash_media_read_out_of_range() {
    let flash = make_flash(vec![0; 16]);
    let mut buf = [0u8; 8];
    let err = flash.read(20, &mut buf).unwrap_err();
    assert_eq!(err, StorageError::OutOfRange);
}

#[test]
fn test_flash_media_program_fresh() {
    // Programming onto an erased (0xFF) region
    let flash = make_flash(vec![0xFF; 128]);
    flash.program(0, &[0x0F, 0xF0, 0x55, 0xAA]).unwrap();

    let mut buf = [0u8; 4];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf, [0x0F, 0xF0, 0x55, 0xAA]);
}

#[test]
fn test_flash_media_program_modifies_existing() {
    // Start with 0xF0, program 0x0F → result 0x00
    let flash = make_flash(vec![0xF0; 128]);
    flash.program(0, &[0x0F]).unwrap();

    let mut buf = [0u8; 1];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf[0], 0x00); // 0xF0 & 0x0F = 0x00
}

#[test]
fn test_flash_media_program_preserves_ones() {
    // Start with 0x00, program 0xFF → preserves existing (0x00)
    let flash = make_flash(vec![0x00; 128]);
    flash.program(0, &[0xFF]).unwrap();

    let mut buf = [0u8; 1];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf[0], 0x00); // 0x00 & 0xFF = 0x00 (no change)
}

#[test]
fn test_flash_media_program_readonly() {
    let flash = FlashMedia::new(MemBackend::new(vec![0xFF; 128], false), 4096)
        .with_readonly(true);
    let err = flash.program(0, &[0x00]).unwrap_err();
    assert_eq!(err, StorageError::ReadOnly);
}

#[test]
fn test_flash_media_program_out_of_range() {
    let flash = make_flash(vec![0xFF; 16]);
    let err = flash.program(20, &[0x00]).unwrap_err();
    assert_eq!(err, StorageError::OutOfRange);
}

#[test]
fn test_flash_media_program_overflow() {
    let flash = make_flash(vec![0xFF; 16]);
    let err = flash.program(u64::MAX - 3, &[0x00; 4]).unwrap_err();
    assert_eq!(err, StorageError::Overflow);
}

#[test]
fn test_flash_media_program_empty_buf() {
    let flash = make_flash(vec![0xFF; 16]);
    flash.program(0, &[]).unwrap(); // no-op
}

#[test]
fn test_flash_media_erase_region() {
    let flash = make_flash(vec![0x00; 16384]); // 4 erase blocks of 4096
                                               // Erase the second block
    flash.erase(4096, 4096).unwrap();

    let mut buf = [0u8; 4];
    // First block still 0x00
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf, [0x00, 0x00, 0x00, 0x00]);
    // Second block now 0xFF
    flash.read(4096, &mut buf).unwrap();
    assert_eq!(buf, [0xFF, 0xFF, 0xFF, 0xFF]);
}

#[test]
fn test_flash_media_erase_unaligned_offset() {
    let flash = make_flash(vec![0x00; 16384]);
    let err = flash.erase(1, 4096).unwrap_err();
    assert!(format!("{err}").contains("not erase-block aligned"));
}

#[test]
fn test_flash_media_erase_unaligned_len() {
    let flash = make_flash(vec![0x00; 16384]);
    let err = flash.erase(0, 4097).unwrap_err();
    assert!(format!("{err}").contains("not erase-block aligned"));
}

#[test]
fn test_flash_media_erase_out_of_range() {
    let flash = make_flash(vec![0x00; 8192]); // 2 erase blocks
    let err = flash.erase(8192, 4096).unwrap_err();
    assert_eq!(err, StorageError::OutOfRange);
}

#[test]
fn test_flash_media_erase_readonly() {
    let flash = FlashMedia::new(MemBackend::new(vec![0x00; 4096], false), 4096)
        .with_readonly(true);
    let err = flash.erase(0, 4096).unwrap_err();
    assert_eq!(err, StorageError::ReadOnly);
}

#[test]
fn test_flash_media_erase_all() {
    let flash = make_flash(vec![0x00; 8192]);
    flash.erase_all().unwrap();

    let mut buf = vec![0u8; 8192];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf, vec![0xFF; 8192]);
}

#[test]
fn test_flash_media_erase_all_readonly() {
    let flash = FlashMedia::new(MemBackend::new(vec![0x00; 4096], false), 4096)
        .with_readonly(true);
    let err = flash.erase_all().unwrap_err();
    assert_eq!(err, StorageError::ReadOnly);
}

#[test]
fn test_flash_media_erase_all_empty() {
    let flash = make_flash(vec![]);
    flash.erase_all().unwrap(); // no-op, should not panic
}

#[test]
fn test_flash_media_erase_value_custom() {
    let flash = FlashMedia::new(MemBackend::new(vec![0x00; 4096], false), 4096)
        .with_erase_value(0x00);
    flash.erase_all().unwrap();

    let mut buf = [0u8; 4];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf, [0x00, 0x00, 0x00, 0x00]); // erase value is 0x00
}

#[test]
fn test_flash_media_erase_value_default() {
    let flash = make_flash(vec![0x00; 16]);
    assert_eq!(flash.erase_value(), 0xFF);
    assert_eq!(flash.erase_block_size(), 4096);
}

#[test]
fn test_flash_media_program_then_erase_round_trip() {
    // Full lifecycle: erase → program → read → erase → read
    let flash = make_flash(vec![0xFF; 4096]);

    // Program some data
    let data: Vec<u8> = (0..128).map(|i| i as u8).collect();
    flash.program(0, &data).unwrap();

    // Verify programmed data
    let mut buf = vec![0u8; 128];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf, data);

    // Erase the block
    flash.erase(0, 4096).unwrap();

    // Verify erased
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf, vec![0xFF; 128]);
}

#[test]
fn test_flash_media_multiple_programs() {
    // Two consecutive programs on the same region
    let flash = make_flash(vec![0xFF; 128]);

    // First program: write 0xF0 → 0xFF & 0xF0 = 0xF0
    flash.program(0, &[0xF0]).unwrap();
    // Second program: write 0x0F → 0xF0 & 0x0F = 0x00
    flash.program(0, &[0x0F]).unwrap();

    let mut buf = [0u8; 1];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf[0], 0x00);
}

// -- BlockMedia on FileBackend tests --

#[test]
fn test_block_media_file_round_trip() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();

    let mut data = vec![0u8; 512];
    data[0] = 0xDE;
    data[511] = 0xAD;

    {
        // Pre-allocate the file to the expected size via the raw
        // backend since BlockMedia rejects writes past capacity.
        let fb = FileBackend::open(&path, false).unwrap();
        fb.write_exact(0, &vec![0u8; 512]).unwrap();
        fb.flush().unwrap();
    }

    {
        let fb = FileBackend::open(&path, false).unwrap();
        let media = BlockMedia::new(fb, 512);
        media.write_block(0, &data).unwrap();
        media.backend().flush().unwrap();
    }

    {
        let fb = FileBackend::open(&path, false).unwrap();
        let media = BlockMedia::new(fb, 512);
        assert_eq!(media.sector_count(), 1);
        let mut buf = vec![0u8; 512];
        media.read_block(0, &mut buf).unwrap();
        assert_eq!(buf, data);
    }
}
