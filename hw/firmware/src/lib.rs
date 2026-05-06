//! Firmware configuration (fw_cfg) interface.
//!
//! Provides [`FwCfg`] for registering firmware configuration entries,
//! and the [`FwCfgDataGenerator`] trait for devices that contribute
//! data items. IO and DMA access follow the fw_cfg ABI (selector/data
//! register pair and big-endian DMA descriptor protocol).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use machina_core::device_cell::DeviceRefCell;

/// Well-known fw_cfg selector keys.
pub mod keys {
    pub const SIGNATURE: u16 = 0x0000;
    pub const ID: u16 = 0x0001;
    pub const UUID: u16 = 0x0002;
    pub const RAM_SIZE: u16 = 0x0003;
    pub const NOGRAPHIC: u16 = 0x0004;
    pub const NB_CPUS: u16 = 0x0005;
    pub const MACHINE_ID: u16 = 0x0006;
    pub const KERNEL_ADDR: u16 = 0x0007;
    pub const KERNEL_SIZE: u16 = 0x0008;
    pub const KERNEL_CMDLINE: u16 = 0x0009;
    pub const INITRD_ADDR: u16 = 0x000a;
    pub const INITRD_SIZE: u16 = 0x000b;
    pub const BOOT_DEVICE: u16 = 0x000c;
    pub const NUMA: u16 = 0x000d;
    pub const BOOT_MENU: u16 = 0x000e;
    pub const MAX_CPUS: u16 = 0x000f;
    pub const KERNEL_ENTRY: u16 = 0x0010;
    pub const KERNEL_DATA: u16 = 0x0011;
    pub const INITRD_DATA: u16 = 0x0012;
    pub const CMDLINE_ADDR: u16 = 0x0013;
    pub const CMDLINE_SIZE: u16 = 0x0014;
    pub const CMDLINE_DATA: u16 = 0x0015;
    pub const SETUP_ADDR: u16 = 0x0016;
    pub const SETUP_SIZE: u16 = 0x0017;
    pub const SETUP_DATA: u16 = 0x0018;
    pub const FILE_DIR: u16 = 0x0019;
}

/// Signature bytes returned for key 0x0000 (fw_cfg protocol).
pub const FW_CFG_SIGNATURE: &[u8] = &[0x51, 0x45, 0x4D, 0x55];

/// DMA control bits (fw_cfg ABI).
pub mod dma_ctl {
    pub const ERROR: u16 = 0x0001;
    pub const READ: u16 = 0x0002;
    pub const SKIP: u16 = 0x0004;
    pub const SELECT: u16 = 0x0008;
    pub const WRITE: u16 = 0x0010;
}

/// DMA descriptor layout (big-endian fields, fw_cfg ABI).
#[derive(Debug, Clone, Copy, Default)]
pub struct FwCfgDmaDescriptor {
    pub control: u32,
    pub length: u32,
    pub address: u64,
}

impl FwCfgDmaDescriptor {
    /// Size of the descriptor in guest memory (16 bytes).
    pub const SIZE: usize = 16;

    /// Decode a descriptor from big-endian guest-memory bytes.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 16 {
            return None;
        }
        let control =
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let length =
            u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let address = u64::from_be_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13],
            bytes[14], bytes[15],
        ]);
        Some(Self {
            control,
            length,
            address,
        })
    }

    /// Encode this descriptor to big-endian guest-memory bytes.
    pub fn encode(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&self.control.to_be_bytes());
        b[4..8].copy_from_slice(&self.length.to_be_bytes());
        b[8..16].copy_from_slice(&self.address.to_be_bytes());
        b
    }
}

/// Trait for bounded memory access during DMA transfers.
pub trait FwCfgDmaAccess {
    /// Read `buf` from guest-physical `addr`.
    ///
    /// Returns the number of bytes read, or an error string.
    fn dma_read(&self, addr: u64, buf: &mut [u8]) -> Result<usize, String>;

    /// Write `buf` to guest-physical `addr`.
    ///
    /// Returns the number of bytes written, or an error string.
    fn dma_write(&self, addr: u64, buf: &[u8]) -> Result<usize, String>;
}

/// A single fw_cfg data entry.
#[derive(Debug, Clone)]
pub struct FwCfgEntry {
    /// Selector key.
    pub key: u16,
    /// Raw data blob.
    pub data: Vec<u8>,
    /// Optional file metadata for the file directory.
    pub file: Option<FwCfgFile>,
}

/// File metadata for file-directory entries.
#[derive(Debug, Clone)]
pub struct FwCfgFile {
    /// File size in bytes.
    pub size: u32,
    /// Selector key.
    pub select: u16,
    /// File name (NUL-terminated, max 56 chars).
    pub name: String,
}

/// Trait for objects that can generate fw_cfg data items.
pub trait FwCfgDataGenerator: Send + Sync {
    /// Generate a byte array of data for this generator.
    fn get_data(&self) -> Result<Option<Vec<u8>>, String>;
}

/// The fw_cfg interface — a registry of firmware configuration entries
/// with IO-style selector/data access and DMA support.
pub struct FwCfg {
    entries: DeviceRefCell<BTreeMap<u16, FwCfgEntry>>,
    generators: DeviceRefCell<BTreeMap<u16, Box<dyn FwCfgDataGenerator>>>,
    cur_entry: Mutex<u16>,
    cur_offset: Mutex<u32>,
    dma_enabled: Mutex<bool>,
    /// Accumulate two IO writes to build the 16-bit selector.
    selector_lo: Mutex<u8>,
    selector_hi: Mutex<u8>,
    selector_top: Mutex<bool>, // true = next write is hi byte
}

impl FwCfg {
    /// Create a new fw_cfg instance.
    #[must_use]
    pub fn new() -> Arc<Self> {
        let s = Arc::new(Self {
            entries: DeviceRefCell::new(BTreeMap::new()),
            generators: DeviceRefCell::new(BTreeMap::new()),
            cur_entry: Mutex::new(0),
            cur_offset: Mutex::new(0),
            dma_enabled: Mutex::new(false),
            selector_lo: Mutex::new(0),
            selector_hi: Mutex::new(0),
            selector_top: Mutex::new(false),
        });
        // Signature entry is always present
        s.add_bytes(keys::SIGNATURE, FW_CFG_SIGNATURE.to_vec());
        s
    }

    /// Add a raw byte entry at the given key.
    pub fn add_bytes(&self, key: u16, data: Vec<u8>) {
        self.entries.borrow().insert(
            key,
            FwCfgEntry {
                key,
                data,
                file: None,
            },
        );
    }

    /// Add a string entry at the given key.
    pub fn add_string(&self, key: u16, value: &str) {
        let mut data = value.as_bytes().to_vec();
        data.push(0); // NUL terminator
        self.add_bytes(key, data);
    }

    /// Add a file entry (appears in the file directory at key 0x0019).
    pub fn add_file(&self, key: u16, name: &str, data: Vec<u8>) {
        let size = data.len() as u32;
        self.entries.borrow().insert(
            key,
            FwCfgEntry {
                key,
                data,
                file: Some(FwCfgFile {
                    size,
                    select: key,
                    name: name.to_string(),
                }),
            },
        );
    }

    /// Add a 16-bit little-endian integer entry.
    pub fn add_i16(&self, key: u16, value: u16) {
        self.add_bytes(key, value.to_le_bytes().to_vec());
    }

    /// Add a 32-bit little-endian integer entry.
    pub fn add_i32(&self, key: u16, value: u32) {
        self.add_bytes(key, value.to_le_bytes().to_vec());
    }

    /// Add a 64-bit little-endian integer entry.
    pub fn add_i64(&self, key: u16, value: u64) {
        self.add_bytes(key, value.to_le_bytes().to_vec());
    }

    /// Register a lazy data generator for the given key.
    ///
    /// The generator is evaluated the first time the key is selected.
    /// This is intended for device-driven fw_cfg entries (e.g. boot
    /// order, CPU count) that are not known at fw_cfg construction time.
    pub fn add_generator(&self, key: u16, gen: Box<dyn FwCfgDataGenerator>) {
        self.generators.borrow().insert(key, gen);
    }

    /// Return the entry for the given key, if it exists.
    #[must_use]
    pub fn get_entry(&self, key: u16) -> Option<FwCfgEntry> {
        self.entries.borrow().get(&key).cloned()
    }

    /// Return true if an entry exists for the key.
    #[must_use]
    pub fn has_entry(&self, key: u16) -> bool {
        self.entries.borrow().contains_key(&key)
    }

    // -- Selector register (IO port, 16-bit big-endian) --

    /// Write a byte to the selector register.
    ///
    /// Two sequential writes assemble a 16-bit big-endian selector.
    /// The first write sets the low byte, the second sets the high
    /// byte and commits the selector.
    pub fn write_selector_byte(&self, value: u8) {
        let mut top = self.selector_top.lock().unwrap();
        if *top {
            *self.selector_hi.lock().unwrap() = value;
            let sel = u16::from_be_bytes([
                *self.selector_lo.lock().unwrap(),
                *self.selector_hi.lock().unwrap(),
            ]);
            drop(top);
            self.commit_selector(sel);
        } else {
            *self.selector_lo.lock().unwrap() = value;
            *top = true;
        }
    }

    /// Set the selector directly (for DMA or test convenience).
    pub fn set_selector(&self, key: u16) {
        self.commit_selector(key);
    }

    fn commit_selector(&self, key: u16) {
        // Evaluate a registered generator on first selection
        if !self.has_entry(key) {
            if let Some(gen) = self.generators.borrow().get(&key) {
                if let Ok(Some(data)) = gen.get_data() {
                    self.add_bytes(key, data);
                }
            }
        }
        // Build file directory on every selection to reflect
        // any entries/files added since the last access.
        if key == keys::FILE_DIR {
            let dir = self.build_file_dir();
            // Replace rather than add — FILE_DIR must be fresh.
            self.entries.borrow().insert(
                keys::FILE_DIR,
                FwCfgEntry {
                    key: keys::FILE_DIR,
                    data: dir,
                    file: None,
                },
            );
        }
        *self.cur_entry.lock().unwrap() = key;
        *self.cur_offset.lock().unwrap() = 0;
        *self.selector_top.lock().unwrap() = false;
    }

    /// Return the current selector.
    #[must_use]
    pub fn selector(&self) -> u16 {
        *self.cur_entry.lock().unwrap()
    }

    // -- Data register (IO port, byte/word/dword reads) --

    /// Read one byte from the currently selected entry at the current
    /// offset. Advances the offset. Returns 0 if no entry is selected
    /// or the offset is past the end.
    #[must_use]
    pub fn read_data_byte(&self) -> u8 {
        let entry = *self.cur_entry.lock().unwrap();
        let mut offset = self.cur_offset.lock().unwrap();

        let entries = self.entries.borrow();
        if let Some(e) = entries.get(&entry) {
            if (*offset as usize) < e.data.len() {
                let val = e.data[*offset as usize];
                *offset += 1;
                return val;
            }
        }
        0
    }

    /// Read a 16-bit word from the current entry.
    ///
    /// Bytes are composed big-endian (first byte = high byte).
    /// Bytes past the entry end are read as 0.
    #[must_use]
    pub fn read_data_word(&self) -> u16 {
        let hi = self.read_data_byte();
        let lo = self.read_data_byte();
        u16::from_be_bytes([hi, lo])
    }

    /// Read a 32-bit dword from the current entry.
    ///
    /// Bytes are composed big-endian (first byte = high byte).
    /// Bytes past the entry end are read as 0.
    #[must_use]
    pub fn read_data_dword(&self) -> u32 {
        let b0 = self.read_data_byte();
        let b1 = self.read_data_byte();
        let b2 = self.read_data_byte();
        let b3 = self.read_data_byte();
        u32::from_be_bytes([b0, b1, b2, b3])
    }

    // -- DMA --

    /// Enable or disable DMA.
    pub fn set_dma_enabled(&self, enabled: bool) {
        *self.dma_enabled.lock().unwrap() = enabled;
    }

    /// Return true if DMA is enabled.
    #[must_use]
    pub fn dma_enabled(&self) -> bool {
        *self.dma_enabled.lock().unwrap()
    }

    /// Execute a DMA transfer.
    ///
    /// Reads the 16-byte big-endian descriptor from `desc_addr` via
    /// `access`, then performs the highest-priority operation among
    /// READ / WRITE / SKIP (priority: READ > WRITE > SKIP).
    /// SELECT is independent and always applied first.
    ///
    /// On completion, writes back only the 4-byte big-endian control
    /// field (0 on success, ERROR bit on failure).  Length and address
    /// fields in guest memory are preserved.
    ///
    /// Guest-visible DMA errors are reported via the descriptor ERROR
    /// status bit — `do_dma` returns `Ok(())` for these.  It returns
    /// `Err` only when the DMA access layer itself fails.
    pub fn do_dma(
        &self,
        desc_addr: u64,
        access: &dyn FwCfgDmaAccess,
    ) -> Result<(), String> {
        // DMA is a no-op when the interface hasn't been enabled
        if !*self.dma_enabled.lock().unwrap() {
            return Ok(());
        }
        let mut desc_buf = [0u8; 16];
        let n = access.dma_read(desc_addr, &mut desc_buf)?;

        if n < 16 {
            // Short descriptor read — set ERROR in guest descriptor
            let _ = access
                .dma_write(desc_addr, &(dma_ctl::ERROR as u32).to_be_bytes());
            return Ok(());
        }

        let desc = FwCfgDmaDescriptor::decode(&desc_buf).unwrap();
        let ctl = desc.control as u16;
        let mut dma_error = false;

        if ctl & dma_ctl::SELECT != 0 {
            let key = (desc.control >> 16) as u16;
            self.set_selector(key);
        }

        // Priority: READ, then WRITE, then SKIP (mutually exclusive)
        if ctl & dma_ctl::READ != 0 {
            let entry = *self.cur_entry.lock().unwrap();
            let entries = self.entries.borrow();
            let entry_data = entries.get(&entry);
            let base = *self.cur_offset.lock().unwrap() as usize;

            let mut offset = 0u32;
            while offset < desc.length {
                let remaining = (desc.length - offset) as usize;
                let chunk = remaining.min(4096);
                let mut buf = vec![0u8; chunk];

                if let Some(e) = entry_data {
                    for (i, dst) in buf.iter_mut().enumerate() {
                        let pos = base + offset as usize + i;
                        if pos < e.data.len() {
                            *dst = e.data[pos];
                        }
                    }
                }
                let written =
                    access.dma_write(desc.address + offset as u64, &buf)?;
                if written < chunk {
                    dma_error = true;
                    offset += written as u32;
                    break;
                }
                offset += written as u32;
            }
            *self.cur_offset.lock().unwrap() += offset;
        } else if ctl & dma_ctl::WRITE != 0 {
            // Guest WRITE is denied.  Still consume bytes and advance
            // cur_offset — denied WRITE against a valid entry skips
            // the available data and sets ERROR.
            dma_error = true;
            let entry = *self.cur_entry.lock().unwrap();
            let entries = self.entries.borrow();
            let consumed = if let Some(e) = entries.get(&entry) {
                let base = *self.cur_offset.lock().unwrap() as usize;
                let remaining = e.data.len().saturating_sub(base);
                (desc.length as usize).min(remaining) as u32
            } else {
                0
            };
            *self.cur_offset.lock().unwrap() += consumed;
        } else if ctl & dma_ctl::SKIP != 0 {
            *self.cur_offset.lock().unwrap() += desc.length;
        }

        // Write back only the 4-byte big-endian control field
        let status = if dma_error {
            dma_ctl::ERROR as u32
        } else {
            0u32
        };
        access.dma_write(desc_addr, &status.to_be_bytes())?;

        Ok(())
    }

    // -- Utilities --

    /// Build and return the file directory blob.
    #[must_use]
    pub fn build_file_dir(&self) -> Vec<u8> {
        let entries = self.entries.borrow();
        let files: Vec<&FwCfgFile> =
            entries.values().filter_map(|e| e.file.as_ref()).collect();

        let count = files.len() as u32;
        let mut dir = Vec::new();
        dir.extend_from_slice(&count.to_be_bytes());

        for file in &files {
            dir.extend_from_slice(&file.size.to_be_bytes());
            dir.extend_from_slice(&file.select.to_be_bytes());
            // reserved
            dir.extend_from_slice(&[0u8, 0u8]);

            let mut name_bytes = file.name.as_bytes().to_vec();
            name_bytes.truncate(56);
            while name_bytes.len() < 56 {
                name_bytes.push(0);
            }
            dir.extend_from_slice(&name_bytes);
        }
        dir
    }

    /// Return the number of registered entries.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.borrow().len()
    }
}
