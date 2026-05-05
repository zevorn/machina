//! Block storage backend abstraction.
//!
//! Provides the [`BlockBackend`] trait and concrete implementations
//! for file-backed and in-memory storage. Used by block devices
//! (SD card model, pflash, m25p80, virtio-blk, etc.) to decouple
//! storage I/O from device logic.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Mutex;

/// A block storage backend providing raw byte-level read/write access.
pub trait BlockBackend: Send + Sync {
    /// Read `len` bytes at `offset` into `buf`.
    ///
    /// Returns the number of bytes actually read, or an error.
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, String>;

    /// Write `buf` at `offset`.
    ///
    /// Returns the number of bytes actually written, or an error.
    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, String>;

    /// Flush any pending writes to the backing medium.
    fn flush(&self) -> Result<(), String>;

    /// Total size of the backend in bytes.
    fn size(&self) -> u64;

    /// Return true if the backend is read-only.
    fn readonly(&self) -> bool;
}

/// File-backed block storage.
///
/// Opens (or creates) a regular file and presents it as a flat
/// address space. Supports optional read-only mode.
pub struct FileBackend {
    file: Mutex<fs::File>,
    size: Mutex<u64>,
    readonly: bool,
    path: PathBuf,
}

impl FileBackend {
    /// Open `path` as a block backend.
    ///
    /// If `readonly` is true, opens the file read-only.
    /// Otherwise opens for reading and writing, creating the file
    /// if it does not exist.
    pub fn open(
        path: impl Into<PathBuf>,
        readonly: bool,
    ) -> Result<Self, String> {
        let path = path.into();
        let file = if readonly {
            fs::OpenOptions::new()
                .read(true)
                .open(&path)
                .map_err(|e| format!("cannot open {path:?} read-only: {e}"))?
        } else {
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .map_err(|e| format!("cannot open {path:?} read-write: {e}"))?
        };
        let size = file
            .metadata()
            .map(|m| m.len())
            .map_err(|e| format!("cannot stat {path:?}: {e}"))?;
        Ok(Self {
            file: Mutex::new(file),
            size: Mutex::new(size),
            readonly,
            path,
        })
    }

    /// Return the file path.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl BlockBackend for FileBackend {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, String> {
        let _end = offset
            .checked_add(buf.len() as u64)
            .ok_or("read offset overflow")?;
        let mut f = self
            .file
            .lock()
            .map_err(|e| format!("file lock error: {e}"))?;
        f.seek(SeekFrom::Start(offset))
            .map_err(|e| format!("seek error: {e}"))?;
        f.read(buf).map_err(|e| format!("read error: {e}"))
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, String> {
        if self.readonly {
            return Err("file is read-only".to_string());
        }
        // Guard against overflow in offset + len
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or("write offset overflow")?;
        let mut f = self
            .file
            .lock()
            .map_err(|e| format!("file lock error: {e}"))?;
        f.seek(SeekFrom::Start(offset))
            .map_err(|e| format!("seek error: {e}"))?;
        let n = f.write(buf).map_err(|e| format!("write error: {e}"))?;
        // Update cached size if the write extended the file
        let new_size = offset + n as u64;
        let mut size = self
            .size
            .lock()
            .map_err(|e| format!("size lock error: {e}"))?;
        if new_size > *size {
            *size = new_size;
        }
        drop(size);
        drop(f);
        if end > new_size {
            // Partial write — refresh from metadata
            if let Ok(m) = fs::metadata(&self.path) {
                *self
                    .size
                    .lock()
                    .map_err(|e| format!("size lock error: {e}"))? = m.len();
            }
        }
        Ok(n)
    }

    fn flush(&self) -> Result<(), String> {
        let f = self
            .file
            .lock()
            .map_err(|e| format!("file lock error: {e}"))?;
        f.sync_all().map_err(|e| format!("sync error: {e}"))
    }

    fn size(&self) -> u64 {
        *self.size.lock().unwrap()
    }

    fn readonly(&self) -> bool {
        self.readonly
    }
}

/// In-memory block storage (for testing and small firmware images).
pub struct MemBackend {
    data: Mutex<Vec<u8>>,
    readonly: bool,
}

impl MemBackend {
    /// Create a new in-memory backend pre-filled with `data`.
    pub fn new(data: Vec<u8>, readonly: bool) -> Self {
        Self {
            data: Mutex::new(data),
            readonly,
        }
    }

    /// Return a copy of the stored data.
    pub fn to_vec(&self) -> Vec<u8> {
        self.data.lock().unwrap().clone()
    }
}

impl BlockBackend for MemBackend {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, String> {
        let data = self.data.lock().map_err(|e| format!("lock error: {e}"))?;
        let start = offset as usize;
        // Cast back to u64 to detect truncation on 32-bit usize
        if start as u64 != offset {
            return Err("read offset overflow".to_string());
        }
        if start >= data.len() {
            return Ok(0);
        }
        let end = start
            .checked_add(buf.len())
            .ok_or("read offset overflow")?
            .min(data.len());
        let n = end - start;
        buf[..n].copy_from_slice(&data[start..end]);
        Ok(n)
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, String> {
        if self.readonly {
            return Err("memory backend is read-only".to_string());
        }
        let mut data =
            self.data.lock().map_err(|e| format!("lock error: {e}"))?;
        let start = offset as usize;
        if start as u64 != offset {
            return Err("write offset overflow".to_string());
        }
        let end = start
            .checked_add(buf.len())
            .ok_or("write offset overflow")?;
        if end > data.len() {
            data.resize(end, 0);
        }
        data[start..end].copy_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&self) -> Result<(), String> {
        Ok(())
    }

    fn size(&self) -> u64 {
        self.data.lock().unwrap().len() as u64
    }

    fn readonly(&self) -> bool {
        self.readonly
    }
}
