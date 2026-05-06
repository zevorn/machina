use machina_hw_storage::{
    BlockBackend, BlockMedia, FileBackend, FlashMedia, MemBackend, StorageError,
};
use std::sync::Mutex;
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
        format!("{}", StorageError::InvalidInput("bad".to_string())),
        "bad"
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
    assert_ne!(
        StorageError::ReadOnly,
        StorageError::InvalidInput("".to_string())
    );
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

// -- MemBackend empty write regression --

#[test]
fn test_mem_backend_empty_write_is_noop() {
    let backend = MemBackend::new(vec![0xCC; 16], false);
    let sz_before = backend.size();
    let n = backend.write(0, &[]).unwrap();
    assert_eq!(n, 0);
    assert_eq!(backend.size(), sz_before);
    // Data must be unchanged
    let mut buf = [0u8; 16];
    backend.read_exact(0, &mut buf).unwrap();
    assert_eq!(buf, [0xCC; 16]);
}

#[test]
fn test_mem_backend_huge_offset_returns_error() {
    let backend = MemBackend::new(vec![0; 4], false);
    // 1 TiB offset — this must not panic with allocation failure
    let offset = 1u64 << 40;
    let err = backend.write(offset, &[0xAA; 4096]).unwrap_err();
    assert!(
        matches!(err, StorageError::Overflow | StorageError::Backend(_)),
        "expected Overflow or Backend, got {err}"
    );
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
    let err = backend.read_exact(20, &mut buf).unwrap_err();
    assert_eq!(err, StorageError::OutOfRange);
}

#[test]
fn test_read_exact_span_out_of_range() {
    let backend = MemBackend::new(vec![0; 16], false);
    let mut buf = [0u8; 8];
    let err = backend.read_exact(12, &mut buf).unwrap_err();
    assert_eq!(err, StorageError::OutOfRange);
}

#[test]
fn test_read_exact_overflow() {
    let backend = MemBackend::new(vec![0; 16], false);
    let mut buf = [0u8; 8];
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
    let n = backend.read(u64::MAX, &mut buf).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn test_mem_backend_write_overflow_usiz() {
    let backend = MemBackend::new(vec![0; 4], false);
    let result = backend.write(u64::MAX, &[0xAA]);
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

// -- FileBackend empty write regression --

#[test]
fn test_file_backend_empty_write_preserves_size() {
    let tmp = NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &[0xAA; 8]).unwrap();

    let backend = FileBackend::open(tmp.path(), false).unwrap();
    assert_eq!(backend.size(), 8);

    // Empty write at a sparse offset must not change cached or real size
    let n = backend.write(1u64 << 40, &[]).unwrap();
    assert_eq!(n, 0);
    assert_eq!(backend.size(), 8);
    // Real file metadata must still be 8
    assert_eq!(std::fs::metadata(tmp.path()).unwrap().len(), 8);
}

// -- File persistence round-trip --

#[test]
fn test_file_backend_persistence_round_trip() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();

    {
        let backend = FileBackend::open(&path, false).unwrap();
        backend.write_exact(0, &[0x10, 0x20, 0x30, 0x40]).unwrap();
        backend.write_exact(4, &[0x50, 0x60]).unwrap();
        backend.flush().unwrap();
    }

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

// -- BlockMedia geometry validation --

#[test]
fn test_block_media_rejects_zero_block_size() {
    let backend = MemBackend::new(vec![0; 1024], false);
    match BlockMedia::new(backend, 0) {
        Err(e @ StorageError::InvalidInput(_)) => {
            assert!(format!("{e}").contains("below minimum 512"));
        }
        Ok(_) => panic!("expected InvalidInput, got Ok"),
        Err(other) => panic!("expected InvalidInput, got {other}"),
    }
}

#[test]
fn test_block_media_rejects_small_block_size() {
    let backend = MemBackend::new(vec![0; 1024], false);
    match BlockMedia::new(backend, 256) {
        Err(e @ StorageError::InvalidInput(_)) => {
            assert!(format!("{e}").contains("below minimum 512"));
        }
        Ok(_) => panic!("expected InvalidInput, got Ok"),
        Err(other) => panic!("expected InvalidInput, got {other}"),
    }
}

#[test]
fn test_block_media_rejects_non_power_of_two() {
    let backend = MemBackend::new(vec![0; 1024], false);
    match BlockMedia::new(backend, 768) {
        Err(e @ StorageError::InvalidInput(_)) => {
            assert!(format!("{e}").contains("not a power of two"));
        }
        Ok(_) => panic!("expected InvalidInput, got Ok"),
        Err(other) => panic!("expected InvalidInput, got {other}"),
    }
}

#[test]
fn test_block_media_accepts_512() {
    let backend = MemBackend::new(vec![0; 1024], false);
    let media = BlockMedia::new(backend, 512).unwrap();
    assert_eq!(media.block_size(), 512);
    assert_eq!(media.sector_count(), 2);
}

#[test]
fn test_block_media_accepts_4096() {
    let backend = MemBackend::new(vec![0; 8192], false);
    let media = BlockMedia::new(backend, 4096).unwrap();
    assert_eq!(media.block_size(), 4096);
    assert_eq!(media.sector_count(), 2);
}

// -- BlockMedia tests --

fn make_block_media(data: Vec<u8>) -> BlockMedia<MemBackend> {
    let backend = MemBackend::new(data, false);
    BlockMedia::new(backend, 512).unwrap()
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

    media.read_block(1, &mut buf).unwrap();
    assert_eq!(buf, vec![0u8; 512]);
}

#[test]
fn test_block_media_read_block_out_of_range() {
    let media = make_block_media(vec![0; 1024]);
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
    let media = make_block_media(vec![0; 2048]);
    let data = vec![0xCC; 1024];
    media.write_blocks(1, &data).unwrap();

    let mut buf = vec![0u8; 1024];
    media.read_blocks(1, &mut buf).unwrap();
    assert_eq!(buf, data);

    let mut buf512 = vec![0u8; 512];
    media.read_block(0, &mut buf512).unwrap();
    assert_eq!(buf512, vec![0u8; 512]);
    media.read_block(3, &mut buf512).unwrap();
    assert_eq!(buf512, vec![0u8; 512]);
}

#[test]
fn test_block_media_multi_sector_unaligned_buf() {
    let media = make_block_media(vec![0; 4096]);
    let buf = vec![0u8; 700];
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
    let media = make_block_media(vec![0; 2048]);
    let err = media
        .read_block(u64::MAX / 512 + 1, &mut vec![0u8; 512])
        .unwrap_err();
    assert_eq!(err, StorageError::Overflow);
}

// -- FlashMedia geometry validation --

#[test]
fn test_flash_media_rejects_zero_erase_block_size() {
    let backend = MemBackend::new(vec![0xFF; 4096], false);
    match FlashMedia::new(backend, 0) {
        Err(e @ StorageError::InvalidInput(_)) => {
            assert!(
                format!("{e}").contains("erase_block_size must be non-zero",)
            );
        }
        Ok(_) => panic!("expected InvalidInput, got Ok"),
        Err(other) => panic!("expected InvalidInput, got {other}"),
    }
}

#[test]
fn test_flash_media_accepts_valid_erase_block_size() {
    let backend = MemBackend::new(vec![0xFF; 4096], false);
    let flash = FlashMedia::new(backend, 4096).unwrap();
    assert_eq!(flash.erase_block_size(), 4096);
}

// -- FlashMedia tests --

fn make_flash(data: Vec<u8>) -> FlashMedia<MemBackend> {
    let backend = MemBackend::new(data, false);
    FlashMedia::new(backend, 4096).unwrap()
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
    let flash = make_flash(vec![0xFF; 128]);
    flash.program(0, &[0x0F, 0xF0, 0x55, 0xAA]).unwrap();

    let mut buf = [0u8; 4];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf, [0x0F, 0xF0, 0x55, 0xAA]);
}

#[test]
fn test_flash_media_program_modifies_existing() {
    let flash = make_flash(vec![0xF0; 128]);
    flash.program(0, &[0x0F]).unwrap();

    let mut buf = [0u8; 1];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf[0], 0x00);
}

#[test]
fn test_flash_media_program_preserves_ones() {
    let flash = make_flash(vec![0x00; 128]);
    flash.program(0, &[0xFF]).unwrap();

    let mut buf = [0u8; 1];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf[0], 0x00);
}

#[test]
fn test_flash_media_program_readonly() {
    let flash = FlashMedia::new(MemBackend::new(vec![0xFF; 128], false), 4096)
        .unwrap()
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
    flash.program(0, &[]).unwrap();
}

#[test]
fn test_flash_media_erase_region() {
    let flash = make_flash(vec![0x00; 16384]);
    flash.erase(4096, 4096).unwrap();

    let mut buf = [0u8; 4];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf, [0x00, 0x00, 0x00, 0x00]);
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
    let flash = make_flash(vec![0x00; 8192]);
    let err = flash.erase(8192, 4096).unwrap_err();
    assert_eq!(err, StorageError::OutOfRange);
}

#[test]
fn test_flash_media_erase_readonly() {
    let flash = FlashMedia::new(MemBackend::new(vec![0x00; 4096], false), 4096)
        .unwrap()
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
        .unwrap()
        .with_readonly(true);
    let err = flash.erase_all().unwrap_err();
    assert_eq!(err, StorageError::ReadOnly);
}

#[test]
fn test_flash_media_erase_all_empty() {
    let flash = make_flash(vec![]);
    flash.erase_all().unwrap();
}

#[test]
fn test_flash_media_erase_value_custom() {
    let flash = FlashMedia::new(MemBackend::new(vec![0x00; 4096], false), 4096)
        .unwrap()
        .with_erase_value(0x00);
    flash.erase_all().unwrap();

    let mut buf = [0u8; 4];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf, [0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn test_flash_media_erase_value_default() {
    let flash = make_flash(vec![0x00; 16]);
    assert_eq!(flash.erase_value(), 0xFF);
    assert_eq!(flash.erase_block_size(), 4096);
}

#[test]
fn test_flash_media_program_then_erase_round_trip() {
    let flash = make_flash(vec![0xFF; 4096]);

    let data: Vec<u8> = (0..128).map(|i| i as u8).collect();
    flash.program(0, &data).unwrap();

    let mut buf = vec![0u8; 128];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf, data);

    flash.erase(0, 4096).unwrap();

    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf, vec![0xFF; 128]);
}

#[test]
fn test_flash_media_multiple_programs() {
    let flash = make_flash(vec![0xFF; 128]);

    flash.program(0, &[0xF0]).unwrap();
    flash.program(0, &[0x0F]).unwrap();

    let mut buf = [0u8; 1];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf[0], 0x00);
}

#[test]
fn test_flash_media_erase_chunked_large_span() {
    // 256 KiB — larger than ERASE_CHUNK_BYTES (64 KiB) to
    // prove erase uses bounded writes.
    let flash = make_flash(vec![0x00; 256 * 1024]);
    flash.erase(0, 256 * 1024).unwrap();

    let mut buf = vec![0u8; 256 * 1024];
    flash.read(0, &mut buf).unwrap();
    assert_eq!(buf, vec![0xFF; 256 * 1024]);
}

// -- MockBackend for ShortIO testing --

/// A backend that reports a large `size()` but performs partial
/// transfers, forcing `read_exact` / `write_exact` to return
/// `StorageError::ShortIO`.
struct MockBackend {
    reported_size: u64,
    read_limit: usize,
    write_limit: usize,
    data: Mutex<Vec<u8>>,
}

impl MockBackend {
    fn new(reported_size: u64, read_limit: usize, write_limit: usize) -> Self {
        Self {
            reported_size,
            read_limit,
            write_limit,
            data: Mutex::new(vec![0u8; reported_size as usize]),
        }
    }
}

impl BlockBackend for MockBackend {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let data = self.data.lock().unwrap();
        let start = offset as usize;
        if start >= data.len() {
            return Ok(0);
        }
        let max = buf.len().min(self.read_limit);
        let n = max.min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, StorageError> {
        let mut data = self.data.lock().unwrap();
        let start = offset as usize;
        if start >= data.len() {
            return Ok(0);
        }
        let max = buf.len().min(self.write_limit);
        let n = max.min(data.len() - start);
        data[start..start + n].copy_from_slice(&buf[..n]);
        Ok(n)
    }

    fn flush(&self) -> Result<(), StorageError> {
        Ok(())
    }

    fn size(&self) -> u64 {
        self.reported_size
    }

    fn readonly(&self) -> bool {
        false
    }
}

#[test]
fn test_read_exact_short_io() {
    let backend = MockBackend::new(1024, 64, 1024);
    let mut buf = vec![0u8; 256];
    let err = backend.read_exact(0, &mut buf).unwrap_err();
    assert_eq!(
        err,
        StorageError::ShortIO {
            expected: 256,
            actual: 64
        }
    );
}

#[test]
fn test_write_exact_short_io() {
    let backend = MockBackend::new(1024, 1024, 128);
    let data = vec![0xAA; 512];
    let err = backend.write_exact(0, &data).unwrap_err();
    assert_eq!(
        err,
        StorageError::ShortIO {
            expected: 512,
            actual: 128
        }
    );
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
        let fb = FileBackend::open(&path, false).unwrap();
        fb.write_exact(0, &vec![0u8; 512]).unwrap();
        fb.flush().unwrap();
    }

    {
        let fb = FileBackend::open(&path, false).unwrap();
        let media = BlockMedia::new(fb, 512).unwrap();
        media.write_block(0, &data).unwrap();
        media.backend().flush().unwrap();
    }

    {
        let fb = FileBackend::open(&path, false).unwrap();
        let media = BlockMedia::new(fb, 512).unwrap();
        assert_eq!(media.sector_count(), 1);
        let mut buf = vec![0u8; 512];
        media.read_block(0, &mut buf).unwrap();
        assert_eq!(buf, data);
    }
}
