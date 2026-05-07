//! I2C bus infrastructure.
//!
//! Provides [`I2cBus`] and the [`I2cSlave`] trait, implementing
//! the I2C/SMBus two-wire serial protocol used by RTCs, EEPROMs,
//! temperature sensors, and other low-speed peripherals.

use std::sync::{Arc, Mutex};

pub mod eeprom_at24c;
pub mod smbus_eeprom;

/// I2C broadcast address.
pub const I2C_BROADCAST: u8 = 0x00;

/// I2C bus events sent to slave devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I2cEvent {
    /// Master is starting a send (write) transaction to this slave.
    StartSend,
    /// Master is starting a receive (read) transaction from this slave.
    StartRecv,
    /// Transaction has finished (STOP condition).
    Finish,
    /// Master sent a NACK.
    Nack,
}

/// A device attached to an I2C bus.
pub trait I2cSlave: Send + Sync {
    /// Return the 7-bit I2C address of this device.
    fn address(&self) -> u8;

    /// Handle a bus event.
    ///
    /// Returns `Ok(())` on success, `Err(I2cError::Nack)` to signal
    /// the device is not responding.
    fn event(&self, event: I2cEvent) -> Result<(), I2cError>;

    /// Receive a byte from the master.
    ///
    /// Returns `Ok(())` on ACK, `Err(I2cError::Nack)` on NACK.
    fn send(&self, data: u8) -> Result<(), I2cError>;

    /// Transmit a byte to the master.
    fn recv(&self) -> u8;
}

/// Errors that can occur during I2C bus operations.
#[derive(Debug, PartialEq, Eq)]
pub enum I2cError {
    /// Device did not acknowledge (NACK).
    Nack,
    /// No device matched the address.
    NoDevice,
    /// Bus is busy with another transaction.
    BusBusy,
}

impl std::fmt::Display for I2cError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nack => write!(f, "I2C NACK"),
            Self::NoDevice => write!(f, "I2C no device at address"),
            Self::BusBusy => write!(f, "I2C bus busy"),
        }
    }
}

impl std::error::Error for I2cError {}

struct I2cNode {
    slave: Arc<dyn I2cSlave>,
}

/// An I2C bus with attached slave devices.
///
/// Supports multi-master arbitration, repeated start,
/// and broadcast addressing.
pub struct I2cBus {
    slaves: Mutex<Vec<I2cNode>>,
    /// Devices currently addressed in the active transaction.
    current_devs: Mutex<Vec<Arc<dyn I2cSlave>>>,
    /// Whether the current transaction is a broadcast.
    broadcast: Mutex<bool>,
}

impl I2cBus {
    #[must_use]
    pub fn new() -> Self {
        Self {
            slaves: Mutex::new(Vec::new()),
            current_devs: Mutex::new(Vec::new()),
            broadcast: Mutex::new(false),
        }
    }

    /// Attach a slave to the bus.
    ///
    /// Returns an error if another device already uses the same
    /// non-broadcast address.
    pub fn attach(&self, slave: Arc<dyn I2cSlave>) -> Result<(), I2cError> {
        let addr = slave.address();
        let slaves = self.slaves.lock().unwrap();
        if addr != I2C_BROADCAST
            && slaves.iter().any(|n| n.slave.address() == addr)
        {
            return Err(I2cError::Nack);
        }
        drop(slaves);
        self.slaves.lock().unwrap().push(I2cNode { slave });
        Ok(())
    }

    /// Detach a slave from the bus by address.
    pub fn detach(&self, address: u8) -> Option<Arc<dyn I2cSlave>> {
        let mut slaves = self.slaves.lock().unwrap();
        if let Some(pos) =
            slaves.iter().position(|n| n.slave.address() == address)
        {
            Some(slaves.remove(pos).slave)
        } else {
            None
        }
    }

    /// Return true if the bus is busy (has an active transaction).
    #[must_use]
    pub fn busy(&self) -> bool {
        !self.current_devs.lock().unwrap().is_empty()
    }

    /// Scan the bus for devices matching `address` and populate
    /// `current_devs`.
    fn scan_bus(&self, address: u8, broadcast: bool) -> Vec<Arc<dyn I2cSlave>> {
        let slaves = self.slaves.lock().unwrap();
        let mut found = Vec::new();
        for node in slaves.iter() {
            if node.slave.address() == address || broadcast {
                found.push(Arc::clone(&node.slave));
                if !broadcast {
                    break; // Only first match for directed transfer
                }
            }
        }
        found
    }

    /// Start a transfer on the bus.
    ///
    /// `address` — 7-bit I2C address (0x00 for broadcast).
    /// `is_recv` — true for read, false for write.
    ///
    /// Returns `Ok(())` if at least one device acknowledged,
    /// `Err(I2cError::NoDevice)` if no device matched.
    pub fn start_transfer(
        &self,
        address: u8,
        is_recv: bool,
    ) -> Result<(), I2cError> {
        let broadcast = address == I2C_BROADCAST;
        *self.broadcast.lock().unwrap() = broadcast;

        let mut current = self.current_devs.lock().unwrap();

        // Finish any previous transfer before starting a new one.
        // Repeated START can change the target address.
        if !current.is_empty() {
            for dev in current.iter() {
                let _ = dev.event(I2cEvent::Finish);
            }
            current.clear();
        }

        // Scan the bus for the new address
        let found = self.scan_bus(address, broadcast);
        if found.is_empty() {
            return Err(I2cError::NoDevice);
        }
        *current = found;

        let event = if is_recv {
            I2cEvent::StartRecv
        } else {
            I2cEvent::StartSend
        };

        for dev in current.iter() {
            if let Err(e) = dev.event(event) {
                if !broadcast {
                    // First call failed — terminate
                    drop(current);
                    self.end_transfer();
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// End the current transfer (STOP condition).
    pub fn end_transfer(&self) {
        let current = self.current_devs.lock().unwrap();
        for dev in current.iter() {
            let _ = dev.event(I2cEvent::Finish);
        }
        drop(current);
        *self.current_devs.lock().unwrap() = Vec::new();
        *self.broadcast.lock().unwrap() = false;
    }

    /// Send a byte to addressed slaves.
    ///
    /// For directed transfers, returns `Err(I2cError::Nack)` if the
    /// addressed slave NACKs. For broadcast, returns NACK if any
    /// slave NACKs.
    pub fn send(&self, data: u8) -> Result<(), I2cError> {
        let current = self.current_devs.lock().unwrap();
        if current.is_empty() {
            return Err(I2cError::NoDevice);
        }
        for dev in current.iter() {
            dev.send(data)?;
        }
        Ok(())
    }

    /// Receive a byte from the first addressed device.
    ///
    /// Returns 0xFF if no device is addressed.
    #[must_use]
    pub fn recv(&self) -> u8 {
        let current = self.current_devs.lock().unwrap();
        let broadcast = *self.broadcast.lock().unwrap();
        if !current.is_empty() && !broadcast {
            current[0].recv()
        } else {
            0xFF
        }
    }

    /// Send a NACK to all addressed devices.
    pub fn nack(&self) {
        let current = self.current_devs.lock().unwrap();
        for dev in current.iter() {
            let _ = dev.event(I2cEvent::Nack);
        }
    }

    /// Return the number of attached slaves.
    #[must_use]
    pub fn slave_count(&self) -> usize {
        self.slaves.lock().unwrap().len()
    }
}

impl Default for I2cBus {
    fn default() -> Self {
        Self::new()
    }
}
