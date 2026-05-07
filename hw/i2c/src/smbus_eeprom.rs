//! SMBus EEPROM.
//!
//! The command byte selects the EEPROM offset. Read transactions return
//! bytes from that offset and auto-increment; write transactions store
//! data bytes after the command byte.

use std::fmt;
use std::sync::Mutex;

use machina_hw_core::mdev::MDeviceState;
use machina_hw_storage::{BlockBackend, StorageError};

use crate::{I2cError, I2cEvent, I2cSlave};

const SMBUS_EEPROM_MAX_SIZE: u64 = 256;

/// Errors returned while constructing an SMBus EEPROM device.
#[derive(Debug, PartialEq, Eq)]
pub enum SmbusEepromError {
    /// The I2C address is outside the 7-bit address range.
    InvalidAddress(u8),
    /// The backing store has no visible bytes.
    EmptyBacking,
}

impl fmt::Display for SmbusEepromError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAddress(address) => {
                write!(f, "invalid 7-bit I2C address: {address:#x}")
            }
            Self::EmptyBacking => {
                f.write_str("backing store must be non-empty")
            }
        }
    }
}

impl std::error::Error for SmbusEepromError {}

struct SmbusEepromState {
    command: u8,
    have_command: bool,
}

impl SmbusEepromState {
    fn new() -> Self {
        Self {
            command: 0,
            have_command: false,
        }
    }
}

/// Byte-wide SMBus EEPROM.
pub struct SmbusEeprom<B: BlockBackend> {
    mdevice: Mutex<MDeviceState>,
    address: u8,
    backend: B,
    visible_size: u16,
    state: Mutex<SmbusEepromState>,
}

impl<B: BlockBackend> SmbusEeprom<B> {
    /// Create an SMBus EEPROM at `address`.
    pub fn new(address: u8, backend: B) -> Result<Self, SmbusEepromError> {
        Self::new_named("smbus-eeprom", address, backend)
    }

    /// Create an SMBus EEPROM at `address` with a MOM local id.
    pub fn new_named(
        local_id: &str,
        address: u8,
        backend: B,
    ) -> Result<Self, SmbusEepromError> {
        if address > 0x7f {
            return Err(SmbusEepromError::InvalidAddress(address));
        }
        let visible = backend.size().min(SMBUS_EEPROM_MAX_SIZE);
        if visible == 0 {
            return Err(SmbusEepromError::EmptyBacking);
        }
        Ok(Self {
            mdevice: Mutex::new(MDeviceState::new(local_id)),
            address,
            backend,
            visible_size: visible as u16,
            state: Mutex::new(SmbusEepromState::new()),
        })
    }

    machina_hw_core::machina_std_mutex_mdevice_accessors!(mdevice);

    fn offset(&self, command: u8) -> u64 {
        u64::from(u16::from(command) % self.visible_size)
    }

    fn read_byte(&self, command: u8) -> u8 {
        let mut byte = [0xff];
        match self.backend.read_exact(self.offset(command), &mut byte) {
            Ok(()) => byte[0],
            Err(_) => 0xff,
        }
    }

    fn write_byte(&self, command: u8, value: u8) -> Result<(), I2cError> {
        self.backend
            .write_exact(self.offset(command), &[value])
            .map_err(storage_to_i2c_error)
    }
}

impl<B: BlockBackend> I2cSlave for SmbusEeprom<B> {
    fn address(&self) -> u8 {
        self.address
    }

    fn event(&self, event: I2cEvent) -> Result<(), I2cError> {
        if event == I2cEvent::StartSend {
            self.state.lock().unwrap().have_command = false;
        }
        Ok(())
    }

    fn send(&self, data: u8) -> Result<(), I2cError> {
        let mut state = self.state.lock().unwrap();
        if !state.have_command {
            state.command = data;
            state.have_command = true;
            return Ok(());
        }
        self.write_byte(state.command, data)?;
        state.command = state.command.wrapping_add(1);
        Ok(())
    }

    fn recv(&self) -> u8 {
        let mut state = self.state.lock().unwrap();
        let value = self.read_byte(state.command);
        state.command = state.command.wrapping_add(1);
        value
    }
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
