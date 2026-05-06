//! Block storage backend abstraction.
//!
//! Provides the [`BlockBackend`] trait, concrete implementations for
//! file-backed and in-memory storage, and higher-level wrappers
//! ([`BlockMedia`], [`FlashMedia`]) that enforce sector / erase-block
//! semantics. Used by block devices (SD card model, pflash, m25p80,
//! virtio-blk, etc.) to decouple storage I/O from device logic.

use std::fmt;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Mutex;

/// Typed storage error returned by all backend operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    /// The backend is read-only and a write / program / erase was
    /// attempted.
    ReadOnly,
    /// Offset + length overflowed u64.
    Overflow,
    /// Offset or the span [offset, offset+len) is out of range.
    OutOfRange,
    /// Fewer bytes were transferred than requested.
    ShortIO {
        /// Number of bytes requested.
        expected: usize,
        /// Number of bytes actually transferred.
        actual: usize,
    },
    /// A backend-specific I/O failure.
    Backend(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadOnly => f.write_str("backend is read-only"),
            Self::Overflow => f.write_str("offset + length overflow"),
            Self::OutOfRange => f.write_str("access out of range"),
            Self::ShortIO { expected, actual } => {
                write!(f, "short I/O: expected {expected} bytes, got {actual}")
            }
            Self::Backend(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for StorageError {}

/// A block storage backend providing raw byte-level read/write access.
pub trait BlockBackend: Send + Sync {
    /// Read up to `buf.len()` bytes at `offset` into `buf`.
    ///
    /// Returns the number of bytes actually read. Fewer than `buf.len()`
    /// bytes indicates end-of-medium was reached. Returns an error for
    /// overflow or backend failures.
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError>;

    /// Write `buf` at `offset`.
    ///
    /// Returns the number of bytes actually written. Returns an error
    /// for read-only backends, overflow, or backend failures.
    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, StorageError>;

    /// Flush any pending writes to the backing medium.
    fn flush(&self) -> Result<(), StorageError>;

    /// Total size of the backend in bytes.
    fn size(&self) -> u64;

    /// Return true if the backend is read-only.
    fn readonly(&self) -> bool;

    /// Read exactly `buf.len()` bytes at `offset`.
    ///
    /// Returns `StorageError::ShortIO` if fewer bytes are available,
    /// `StorageError::Overflow` if the span wraps around, and
    /// `StorageError::OutOfRange` if offset is past the end.
    fn read_exact(
        &self,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<(), StorageError> {
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(StorageError::Overflow)?;
        let size = self.size();
        if offset > size || end > size {
            return Err(StorageError::OutOfRange);
        }
        let n = self.read(offset, buf)?;
        if n != buf.len() {
            return Err(StorageError::ShortIO {
                expected: buf.len(),
                actual: n,
            });
        }
        Ok(())
    }

    /// Write exactly `buf.len()` bytes at `offset`.
    ///
    /// Returns `StorageError::ShortIO` if fewer bytes were written,
    /// `StorageError::ReadOnly` on read-only backends, and
    /// `StorageError::Overflow` if the span wraps around.
    fn write_exact(&self, offset: u64, buf: &[u8]) -> Result<(), StorageError> {
        if self.readonly() {
            return Err(StorageError::ReadOnly);
        }
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(StorageError::Overflow)?;
        // write may extend the backend, so we only check offset overflow
        let _ = end;
        let n = self.write(offset, buf)?;
        if n != buf.len() {
            return Err(StorageError::ShortIO {
                expected: buf.len(),
                actual: n,
            });
        }
        Ok(())
    }
}

// ------------------------------------------------------------------
// FileBackend
// ------------------------------------------------------------------

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
    ) -> Result<Self, StorageError> {
        let path = path.into();
        let file = if readonly {
            fs::OpenOptions::new().read(true).open(&path).map_err(|e| {
                StorageError::Backend(format!(
                    "cannot open {path:?} read-only: {e}"
                ))
            })?
        } else {
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .map_err(|e| {
                    StorageError::Backend(format!(
                        "cannot open {path:?} read-write: {e}"
                    ))
                })?
        };
        let size = file.metadata().map(|m| m.len()).map_err(|e| {
            StorageError::Backend(format!("cannot stat {path:?}: {e}"))
        })?;
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
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let _ = offset
            .checked_add(buf.len() as u64)
            .ok_or(StorageError::Overflow)?;
        let mut f = self.file.lock().map_err(|e| {
            StorageError::Backend(format!("file lock error: {e}"))
        })?;
        f.seek(SeekFrom::Start(offset))
            .map_err(|e| StorageError::Backend(format!("seek error: {e}")))?;
        f.read(buf)
            .map_err(|e| StorageError::Backend(format!("read error: {e}")))
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, StorageError> {
        if self.readonly {
            return Err(StorageError::ReadOnly);
        }
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(StorageError::Overflow)?;
        let mut f = self.file.lock().map_err(|e| {
            StorageError::Backend(format!("file lock error: {e}"))
        })?;
        f.seek(SeekFrom::Start(offset))
            .map_err(|e| StorageError::Backend(format!("seek error: {e}")))?;
        let n = f
            .write(buf)
            .map_err(|e| StorageError::Backend(format!("write error: {e}")))?;
        let new_size = offset + n as u64;
        let mut size = self.size.lock().map_err(|e| {
            StorageError::Backend(format!("size lock error: {e}"))
        })?;
        if new_size > *size {
            *size = new_size;
        }
        drop(size);
        drop(f);
        if end > new_size {
            if let Ok(m) = fs::metadata(&self.path) {
                *self.size.lock().map_err(|e| {
                    StorageError::Backend(format!("size lock error: {e}"))
                })? = m.len();
            }
        }
        Ok(n)
    }

    fn flush(&self) -> Result<(), StorageError> {
        let f = self.file.lock().map_err(|e| {
            StorageError::Backend(format!("file lock error: {e}"))
        })?;
        f.sync_all()
            .map_err(|e| StorageError::Backend(format!("sync error: {e}")))
    }

    fn size(&self) -> u64 {
        *self.size.lock().unwrap()
    }

    fn readonly(&self) -> bool {
        self.readonly
    }
}

// ------------------------------------------------------------------
// MemBackend
// ------------------------------------------------------------------

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
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let data = self
            .data
            .lock()
            .map_err(|e| StorageError::Backend(format!("lock error: {e}")))?;
        let start = offset as usize;
        if start as u64 != offset {
            return Err(StorageError::Overflow);
        }
        if start >= data.len() {
            return Ok(0);
        }
        let end = start
            .checked_add(buf.len())
            .ok_or(StorageError::Overflow)?
            .min(data.len());
        let n = end - start;
        buf[..n].copy_from_slice(&data[start..end]);
        Ok(n)
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, StorageError> {
        if self.readonly {
            return Err(StorageError::ReadOnly);
        }
        let mut data = self
            .data
            .lock()
            .map_err(|e| StorageError::Backend(format!("lock error: {e}")))?;
        let start = offset as usize;
        if start as u64 != offset {
            return Err(StorageError::Overflow);
        }
        let end = start.checked_add(buf.len()).ok_or(StorageError::Overflow)?;
        if end > data.len() {
            data.resize(end, 0);
        }
        data[start..end].copy_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&self) -> Result<(), StorageError> {
        Ok(())
    }

    fn size(&self) -> u64 {
        self.data.lock().unwrap().len() as u64
    }

    fn readonly(&self) -> bool {
        self.readonly
    }
}

// ------------------------------------------------------------------
// BlockMedia — sector-aware block-device wrapper
// ------------------------------------------------------------------

/// A sector-aware wrapper around a [`BlockBackend`].
///
/// Enforces sector-size alignment and translates sector-number
/// addresses to byte offsets.
pub struct BlockMedia<B: BlockBackend> {
    backend: B,
    block_size: u32,
}

impl<B: BlockBackend> BlockMedia<B> {
    /// Wrap `backend` as a block device with the given sector size.
    ///
    /// `block_size` must be a power of two and at least 512.
    #[must_use]
    pub fn new(backend: B, block_size: u32) -> Self {
        Self {
            backend,
            block_size,
        }
    }

    /// Return the sector size in bytes.
    #[must_use]
    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    /// Return the total number of sectors.
    #[must_use]
    pub fn sector_count(&self) -> u64 {
        self.backend.size() / u64::from(self.block_size)
    }

    /// Read one sector into `buf`.
    ///
    /// `buf` must be exactly `block_size` bytes.
    pub fn read_block(
        &self,
        sector: u64,
        buf: &mut [u8],
    ) -> Result<(), StorageError> {
        if buf.len() != self.block_size as usize {
            return Err(StorageError::ShortIO {
                expected: self.block_size as usize,
                actual: buf.len(),
            });
        }
        let offset = sector
            .checked_mul(u64::from(self.block_size))
            .ok_or(StorageError::Overflow)?;
        self.backend.read_exact(offset, buf)
    }

    /// Write one sector from `buf`.
    ///
    /// `buf` must be exactly `block_size` bytes.
    /// Rejects writes past the last sector.
    pub fn write_block(
        &self,
        sector: u64,
        buf: &[u8],
    ) -> Result<(), StorageError> {
        if buf.len() != self.block_size as usize {
            return Err(StorageError::ShortIO {
                expected: self.block_size as usize,
                actual: buf.len(),
            });
        }
        let offset = sector
            .checked_mul(u64::from(self.block_size))
            .ok_or(StorageError::Overflow)?;
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(StorageError::Overflow)?;
        if end > self.backend.size() {
            return Err(StorageError::OutOfRange);
        }
        self.backend.write_exact(offset, buf)
    }

    /// Read `count` sectors starting at `sector` into `buf`.
    ///
    /// `buf.len()` must be a multiple of `block_size`.
    pub fn read_blocks(
        &self,
        sector: u64,
        buf: &mut [u8],
    ) -> Result<(), StorageError> {
        let bs = self.block_size as usize;
        if !buf.len().is_multiple_of(bs) {
            return Err(StorageError::ShortIO {
                expected: (buf.len() / bs + 1) * bs,
                actual: buf.len(),
            });
        }
        let offset = sector
            .checked_mul(u64::from(self.block_size))
            .ok_or(StorageError::Overflow)?;
        self.backend.read_exact(offset, buf)
    }

    /// Write `count` sectors starting at `sector` from `buf`.
    ///
    /// `buf.len()` must be a multiple of `block_size`.
    /// Rejects writes past the last sector.
    pub fn write_blocks(
        &self,
        sector: u64,
        buf: &[u8],
    ) -> Result<(), StorageError> {
        let bs = self.block_size as usize;
        if !buf.len().is_multiple_of(bs) {
            return Err(StorageError::ShortIO {
                expected: (buf.len() / bs + 1) * bs,
                actual: buf.len(),
            });
        }
        let offset = sector
            .checked_mul(u64::from(self.block_size))
            .ok_or(StorageError::Overflow)?;
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(StorageError::Overflow)?;
        if end > self.backend.size() {
            return Err(StorageError::OutOfRange);
        }
        self.backend.write_exact(offset, buf)
    }

    /// Access the inner backend.
    #[must_use]
    pub fn backend(&self) -> &B {
        &self.backend
    }
}

// ------------------------------------------------------------------
// FlashMedia — NOR-flash wrapper with erase-block semantics
// ------------------------------------------------------------------

/// A NOR-flash wrapper around a [`BlockBackend`].
///
/// Models typical NOR flash behavior:
/// - Erased state: all bits are 1 (default erase value 0xFF).
/// - `program` can only turn 1 bits to 0; it reads existing data and
///   applies new data as a bit-mask (`existing & data`).
/// - `erase` resets an erase-block-aligned region to the erase value.
/// - `erase_all` resets the entire backend.
pub struct FlashMedia<B: BlockBackend> {
    backend: B,
    erase_block_size: u32,
    erase_value: u8,
    readonly: bool,
}

impl<B: BlockBackend> FlashMedia<B> {
    /// Wrap `backend` as NOR flash.
    ///
    /// `erase_block_size` is the minimum erase unit in bytes.
    /// Default `erase_value` is `0xFF`.
    #[must_use]
    pub fn new(backend: B, erase_block_size: u32) -> Self {
        Self {
            backend,
            erase_block_size,
            erase_value: 0xFF,
            readonly: false,
        }
    }

    /// Set the erase value (default `0xFF`).
    #[must_use]
    pub fn with_erase_value(mut self, value: u8) -> Self {
        self.erase_value = value;
        self
    }

    /// Mark the flash as read-only.
    #[must_use]
    pub fn with_readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    /// Return the erase block size.
    #[must_use]
    pub fn erase_block_size(&self) -> u32 {
        self.erase_block_size
    }

    /// Return the erase value.
    #[must_use]
    pub fn erase_value(&self) -> u8 {
        self.erase_value
    }

    /// Read `buf.len()` bytes at `offset`.
    pub fn read(
        &self,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<(), StorageError> {
        self.backend.read_exact(offset, buf)
    }

    /// Program (write) data at `offset`.
    ///
    /// NOR flash programming can only change 1 bits to 0. The
    /// operation reads existing data and applies `data` as a bit
    /// mask: each byte becomes `existing & data_byte`.
    pub fn program(&self, offset: u64, buf: &[u8]) -> Result<(), StorageError> {
        if self.readonly {
            return Err(StorageError::ReadOnly);
        }
        if buf.is_empty() {
            return Ok(());
        }
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(StorageError::Overflow)?;
        let size = self.backend.size();
        if offset > size || end > size {
            return Err(StorageError::OutOfRange);
        }
        // Read-modify-write: existing & data
        let mut existing = vec![0u8; buf.len()];
        self.backend
            .read_exact(offset, &mut existing)
            .map_err(|e| {
                if matches!(e, StorageError::OutOfRange) {
                    StorageError::OutOfRange
                } else {
                    e
                }
            })?;
        for (e, d) in existing.iter_mut().zip(buf.iter()) {
            *e &= d;
        }
        self.backend.write_exact(offset, &existing)
    }

    /// Erase `len` bytes starting at `offset`.
    ///
    /// Both `offset` and `len` must be multiples of
    /// `erase_block_size`.
    pub fn erase(&self, offset: u64, len: u64) -> Result<(), StorageError> {
        if self.readonly {
            return Err(StorageError::ReadOnly);
        }
        let ebs = u64::from(self.erase_block_size);
        if !offset.is_multiple_of(ebs) || !len.is_multiple_of(ebs) {
            return Err(StorageError::Backend(
                "erase region not erase-block aligned".to_string(),
            ));
        }
        let end = offset.checked_add(len).ok_or(StorageError::Overflow)?;
        let size = self.backend.size();
        if offset > size || end > size {
            return Err(StorageError::OutOfRange);
        }
        let fill = vec![self.erase_value; len as usize];
        self.backend.write_exact(offset, &fill)
    }

    /// Erase the entire backend to the erase value.
    pub fn erase_all(&self) -> Result<(), StorageError> {
        if self.readonly {
            return Err(StorageError::ReadOnly);
        }
        let size = self.backend.size();
        if size == 0 {
            return Ok(());
        }
        let fill = vec![self.erase_value; size as usize];
        self.backend.write_exact(0, &fill)
    }

    /// Access the inner backend.
    #[must_use]
    pub fn backend(&self) -> &B {
        &self.backend
    }
}
