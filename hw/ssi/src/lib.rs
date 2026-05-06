//! SPI (SSI) bus infrastructure.
//!
//! Provides [`SpiBus`] and the [`SpiSlave`] trait, corresponding to
//! the Synchronous Serial Interface bus model used by embedded
//! peripherals (flash, sensors, SD card SPI mode).

pub mod pl022;
pub mod sifive_spi;

use std::sync::{Arc, Mutex};

/// Chip-select polarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiCsPolarity {
    /// CS is active-high.
    High,
    /// CS is active-low.
    Low,
    /// CS polarity is irrelevant (always active).
    None,
}

/// A device attached to an SPI bus.
pub trait SpiSlave: Send + Sync {
    /// Transfer a word to the peripheral and return the response.
    fn transfer(&self, val: u32) -> u32;

    /// Called when the chip-select line changes.
    fn set_cs(&self, cs: bool);

    /// Return the CS polarity this slave expects.
    fn cs_polarity(&self) -> SpiCsPolarity;

    /// Return the chip-select index this slave occupies.
    fn cs_index(&self) -> u8;
}

struct SpiSlaveEntry {
    slave: Arc<dyn SpiSlave>,
    /// None = never configured by set_cs; treated as deselected.
    cs_state: Option<bool>,
}

/// An SPI bus with attached peripheral devices.
///
/// Only one slave should be selected (CS asserted) at a time.
/// When no slave is selected, `transfer` returns 0xFF (bus pull-up).
pub struct SpiBus {
    slaves: Mutex<Vec<SpiSlaveEntry>>,
    /// Cached last transfer result for testing.
    last_result: Mutex<u32>,
}

impl SpiBus {
    #[must_use]
    pub fn new() -> Self {
        Self {
            slaves: Mutex::new(Vec::new()),
            last_result: Mutex::new(0),
        }
    }

    /// Attach a slave to the bus at its declared CS index.
    ///
    /// Returns an error if the CS index is already occupied.
    /// Newly-attached slaves start deselected regardless of CS
    /// polarity.
    pub fn attach(&self, slave: Arc<dyn SpiSlave>) -> Result<(), SpiBusError> {
        let mut slaves = self.slaves.lock().unwrap();
        let idx = slave.cs_index();
        if slaves.iter().any(|e| e.slave.cs_index() == idx) {
            return Err(SpiBusError::CsIndexConflict(idx));
        }
        slaves.push(SpiSlaveEntry {
            slave,
            cs_state: None,
        });
        Ok(())
    }

    /// Detach a slave from the bus by its CS index.
    pub fn detach(&self, cs_index: u8) -> Option<Arc<dyn SpiSlave>> {
        let mut slaves = self.slaves.lock().unwrap();
        if let Some(pos) =
            slaves.iter().position(|e| e.slave.cs_index() == cs_index)
        {
            Some(slaves.remove(pos).slave)
        } else {
            None
        }
    }

    /// Find a slave by CS index.
    #[must_use]
    pub fn get_cs(&self, cs_index: u8) -> Option<Arc<dyn SpiSlave>> {
        let slaves = self.slaves.lock().unwrap();
        slaves
            .iter()
            .find(|e| e.slave.cs_index() == cs_index)
            .map(|e| Arc::clone(&e.slave))
    }

    /// Set the chip-select line for the slave at `cs_index`.
    ///
    /// When CS transitions, the slave's `set_cs` is called.
    /// Only one slave should have CS asserted at a time.
    pub fn set_cs(&self, cs_index: u8, level: bool) {
        let mut slaves = self.slaves.lock().unwrap();
        for entry in slaves.iter_mut() {
            if entry.slave.cs_index() == cs_index {
                if entry.cs_state != Some(level) {
                    entry.slave.set_cs(level);
                }
                entry.cs_state = Some(level);
            }
        }
    }

    /// Transfer a word on the bus.
    ///
    /// Each selected slave contributes to the result via bitwise OR.
    /// When no slave is selected, returns 0xFF (bus pull-up).
    #[must_use]
    pub fn transfer(&self, val: u32) -> u32 {
        let slaves = self.slaves.lock().unwrap();
        let mut result: u32 = 0;
        let mut any_selected = false;

        for entry in slaves.iter() {
            let slave = &entry.slave;
            let selected = match slave.cs_polarity() {
                SpiCsPolarity::High => entry.cs_state == Some(true),
                SpiCsPolarity::Low => entry.cs_state == Some(false),
                SpiCsPolarity::None => true,
            };
            if selected {
                result |= slave.transfer(val);
                any_selected = true;
            }
        }

        if any_selected {
            *self.last_result.lock().unwrap() = result;
            result
        } else {
            *self.last_result.lock().unwrap() = 0xFF;
            0xFF
        }
    }

    /// Return the raw result of the last `transfer`.
    #[must_use]
    pub fn last_result(&self) -> u32 {
        *self.last_result.lock().unwrap()
    }
}

impl Default for SpiBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during SPI bus operations.
#[derive(Debug, PartialEq, Eq)]
pub enum SpiBusError {
    /// The requested CS index is already occupied.
    CsIndexConflict(u8),
}

impl std::fmt::Display for SpiBusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CsIndexConflict(idx) => {
                write!(f, "SPI CS index 0x{idx:02x} already in use")
            }
        }
    }
}

impl std::error::Error for SpiBusError {}
