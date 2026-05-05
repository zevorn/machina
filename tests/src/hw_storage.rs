use machina_hw_storage::{BlockBackend, FileBackend, MemBackend};
use tempfile::NamedTempFile;

// -- MemBackend tests --

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
    assert!(backend.write(0, &[1]).is_err());
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

// -- FileBackend tests --

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

    // Write to read-only should fail
    assert!(backend.write(0, &[0]).is_err());
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
    // Size must reflect the write, not the stale open-time metadata
    assert_eq!(backend.size(), 3);

    backend.write(3, &[0x44, 0x55]).unwrap();
    assert_eq!(backend.size(), 5);
}

#[test]
fn test_mem_backend_overflow_read() {
    let backend = MemBackend::new(vec![0; 4], false);
    let mut buf = [0u8; 4];
    // On 64-bit, u64::MAX is a valid usize; past-end returns Ok(0)
    let n = backend.read(u64::MAX, &mut buf).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn test_mem_backend_overflow_write() {
    let backend = MemBackend::new(vec![0; 4], false);
    // On 64-bit, u64::MAX offset triggers error (past end + overflow)
    let result = backend.write(u64::MAX, &[0xAA]);
    // u64::MAX as usize + 1 wraps → checked_add returns None → error
    assert!(result.is_err());
}
