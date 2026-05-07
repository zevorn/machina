//! AT24C-style I2C EEPROM.
//!
//! The model implements the common byte-addressed EEPROM protocol:
//! write transactions first load the internal address pointer, read
//! transactions return bytes from that pointer and auto-increment it.

use std::fmt;
use std::sync::Mutex;

use machina_hw_core::mdev::MDeviceState;
use machina_hw_storage::{BlockBackend, StorageError};

use crate::{I2cError, I2cEvent, I2cSlave};

/// Runtime configuration for an AT24C-compatible EEPROM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct At24cEepromConfig {
    /// 7-bit I2C address.
    pub address: u8,
    /// Number of backing bytes visible through the EEPROM protocol.
    pub size: u32,
    /// Number of address bytes consumed at the start of a write
    /// transaction.
    pub address_width: u8,
    /// EEPROM page size used by page-write wraparound.
    pub page_size: u16,
}

impl Default for At24cEepromConfig {
    fn default() -> Self {
        Self {
            address: 0x50,
            size: 256,
            address_width: 1,
            page_size: 8,
        }
    }
}

/// Errors returned while constructing an AT24C device.
#[derive(Debug, PartialEq, Eq)]
pub enum At24cError {
    /// Configuration values are not usable for this model.
    InvalidConfig(String),
    /// The backing store is smaller than the configured EEPROM size.
    BackingTooSmall { needed: u32, actual: u64 },
}

impl fmt::Display for At24cError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(msg) => f.write_str(msg),
            Self::BackingTooSmall { needed, actual } => {
                write!(
                    f,
                    "backing store too small: need {needed} bytes, have {actual}"
                )
            }
        }
    }
}

impl std::error::Error for At24cError {}

struct At24cState {
    pointer: u32,
    address_bytes: Vec<u8>,
}

impl At24cState {
    fn new() -> Self {
        Self {
            pointer: 0,
            address_bytes: Vec::new(),
        }
    }
}

/// Byte-addressed AT24C-compatible EEPROM.
#[derive(machina_hw_core::MDevice)]
#[mom(state = mdevice, lock = "std")]
pub struct At24cEeprom<B: BlockBackend> {
    mdevice: Mutex<MDeviceState>,
    backend: B,
    config: At24cEepromConfig,
    state: Mutex<At24cState>,
}

impl<B: BlockBackend> At24cEeprom<B> {
    /// Create an EEPROM over `backend`.
    pub fn new(
        backend: B,
        config: At24cEepromConfig,
    ) -> Result<Self, At24cError> {
        Self::new_named("at24c", backend, config)
    }

    /// Create an EEPROM over `backend` with a MOM local id.
    pub fn new_named(
        local_id: &str,
        backend: B,
        config: At24cEepromConfig,
    ) -> Result<Self, At24cError> {
        validate_config(&config, backend.size())?;
        Ok(Self {
            mdevice: Mutex::new(MDeviceState::new(local_id)),
            backend,
            config,
            state: Mutex::new(At24cState::new()),
        })
    }

    fn read_byte(&self, offset: u32) -> u8 {
        let mut byte = [0xff];
        match self.backend.read_exact(u64::from(offset), &mut byte) {
            Ok(()) => byte[0],
            Err(_) => 0xff,
        }
    }

    fn load_pointer(&self, state: &mut At24cState) {
        let mut pointer = 0u32;
        for &byte in &state.address_bytes {
            pointer = (pointer << 8) | u32::from(byte);
        }
        state.pointer = pointer % self.config.size;
    }

    fn write_byte(&self, offset: u32, value: u8) -> Result<(), I2cError> {
        self.backend
            .write_exact(u64::from(offset), &[value])
            .map_err(storage_to_i2c_error)
    }

    fn increment_pointer(&self, state: &mut At24cState) {
        state.pointer = (state.pointer + 1) % self.config.size;
    }
}

impl<B: BlockBackend> I2cSlave for At24cEeprom<B> {
    fn address(&self) -> u8 {
        self.config.address
    }

    fn event(&self, event: I2cEvent) -> Result<(), I2cError> {
        if event == I2cEvent::StartSend {
            let mut state = self.state.lock().unwrap();
            state.address_bytes.clear();
        }
        Ok(())
    }

    fn send(&self, data: u8) -> Result<(), I2cError> {
        let mut state = self.state.lock().unwrap();
        if state.address_bytes.len() < usize::from(self.config.address_width) {
            state.address_bytes.push(data);
            if state.address_bytes.len()
                == usize::from(self.config.address_width)
            {
                self.load_pointer(&mut state);
            }
            return Ok(());
        }

        self.write_byte(state.pointer, data)?;
        self.increment_pointer(&mut state);
        Ok(())
    }

    fn recv(&self) -> u8 {
        let mut state = self.state.lock().unwrap();
        let value = self.read_byte(state.pointer);
        self.increment_pointer(&mut state);
        value
    }
}

fn validate_config(
    config: &At24cEepromConfig,
    backing_size: u64,
) -> Result<(), At24cError> {
    if config.address > 0x7f {
        return Err(At24cError::InvalidConfig(
            "address must be a 7-bit I2C address".to_string(),
        ));
    }
    if config.size == 0 {
        return Err(At24cError::InvalidConfig(
            "size must be non-zero".to_string(),
        ));
    }
    if !matches!(config.address_width, 1 | 2) {
        return Err(At24cError::InvalidConfig(
            "address_width must be 1 or 2".to_string(),
        ));
    }
    if config.page_size == 0 || u32::from(config.page_size) > config.size {
        return Err(At24cError::InvalidConfig(
            "page_size must be non-zero and no larger than size".to_string(),
        ));
    }
    if backing_size < u64::from(config.size) {
        return Err(At24cError::BackingTooSmall {
            needed: config.size,
            actual: backing_size,
        });
    }
    Ok(())
}

fn storage_to_i2c_error(err: StorageError) -> I2cError {
    match err {
        StorageError::ReadOnly
        | StorageError::Overflow
        | StorageError::OutOfRange
        | StorageError::ShortIO { .. }
        | StorageError::InvalidInput(_)
        | StorageError::Backend(_) => I2cError::Nack,
    }
}
