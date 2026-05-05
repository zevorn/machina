//! SD/MMC bus infrastructure.
//!
//! Provides [`SdBus`] and the [`SdCard`] trait, implementing the
//! SD/MMC host-to-card command/response/data protocol used by
//! SDHCI controllers, SPI-SD bridges, and PL181.

use std::sync::{Arc, Mutex};

use machina_core::device_cell::DeviceRefCell;

/// Errors returned by SD bus operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdError {
    /// No card is inserted in the bus.
    NoCard,
    /// Command timed out (no response from card).
    Timeout,
}

impl std::fmt::Display for SdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCard => write!(f, "SD no card"),
            Self::Timeout => write!(f, "SD command timeout"),
        }
    }
}

impl std::error::Error for SdError {}

/// SD command request from host to card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SdRequest {
    /// Command index (0-63).
    pub cmd: u8,
    /// Command argument.
    pub arg: u32,
    /// CRC7 for the command.
    pub crc: u8,
}

impl SdRequest {
    #[must_use]
    pub fn new(cmd: u8, arg: u32) -> Self {
        Self { cmd, arg, crc: 0 }
    }
}

/// SD card status bits (R1 response format).
pub mod status {
    pub const OUT_OF_RANGE: u32 = 1 << 31;
    pub const ADDRESS_ERROR: u32 = 1 << 30;
    pub const BLOCK_LEN_ERROR: u32 = 1 << 29;
    pub const ERASE_SEQ_ERROR: u32 = 1 << 28;
    pub const ERASE_PARAM: u32 = 1 << 27;
    pub const WP_VIOLATION: u32 = 1 << 26;
    pub const CARD_IS_LOCKED: u32 = 1 << 25;
    pub const LOCK_UNLOCK_FAILED: u32 = 1 << 24;
    pub const COM_CRC_ERROR: u32 = 1 << 23;
    pub const ILLEGAL_COMMAND: u32 = 1 << 22;
    pub const CARD_ECC_FAILED: u32 = 1 << 21;
    pub const CC_ERROR: u32 = 1 << 20;
    pub const SD_ERROR: u32 = 1 << 19;
    pub const CID_CSD_OVERWRITE: u32 = 1 << 16;
    pub const WP_ERASE_SKIP: u32 = 1 << 15;
    pub const CARD_ECC_DISABLED: u32 = 1 << 14;
    pub const ERASE_RESET: u32 = 1 << 13;
    pub const CURRENT_STATE: u32 = 7 << 9;
    pub const READY_FOR_DATA: u32 = 1 << 8;
    pub const APP_CMD: u32 = 1 << 5;
    pub const AKE_SEQ_ERROR: u32 = 1 << 3;
}

/// SD voltage levels in millivolts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdVoltage {
    /// 3.3V
    V33 = 3300,
    /// 3.0V
    V30 = 3000,
    /// 1.8V
    V18 = 1800,
}

/// An SD/MMC card attached to the bus.
pub trait SdCard: Send + Sync {
    /// Process an SD command and return response bytes.
    ///
    /// Returns the number of response bytes written to `resp`.
    /// The response buffer must be at least 16 bytes.
    fn do_command(&self, req: &SdRequest, resp: &mut [u8]) -> usize;

    /// Write a byte to the card (data phase).
    fn write_byte(&self, value: u8);

    /// Read a byte from the card (data phase).
    fn read_byte(&self) -> u8;

    /// Return true if the card is ready to receive data.
    fn receive_ready(&self) -> bool;

    /// Return true if the card has data ready to read.
    fn data_ready(&self) -> bool;

    /// Return true if a card is inserted.
    fn get_inserted(&self) -> bool;

    /// Return true if the card is write-protected.
    fn get_readonly(&self) -> bool;

    /// Set the supply voltage (in millivolts).
    fn set_voltage(&self, millivolts: u16);

    /// Return the DAT line mask (0b1111 = 4-bit bus).
    fn get_dat_lines(&self) -> u8;

    /// Return the CMD line state.
    fn get_cmd_line(&self) -> bool;
}

/// Event callbacks from the bus to the host controller.
pub trait SdBusHost: Send + Sync {
    /// Called when card insertion state changes.
    fn set_inserted(&self, inserted: bool);
    /// Called when write-protect state changes.
    fn set_readonly(&self, readonly: bool);
}

struct SdCardEntry {
    card: Arc<dyn SdCard>,
}

/// An SD/MMC bus connecting a host controller to a card.
///
/// Only one card may be attached at a time.
pub struct SdBus {
    cards: Mutex<Vec<SdCardEntry>>,
    host: DeviceRefCell<Option<Arc<dyn SdBusHost>>>,
}

impl SdBus {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cards: Mutex::new(Vec::new()),
            host: DeviceRefCell::new(None),
        }
    }

    /// Attach the host controller callback.
    pub fn set_host(&self, host: Arc<dyn SdBusHost>) {
        *self.host.borrow() = Some(host);
    }

    /// Insert a card onto the bus.
    ///
    /// Replaces any previously inserted card.
    pub fn insert_card(&self, card: Arc<dyn SdCard>) {
        let mut cards = self.cards.lock().unwrap();
        cards.clear();
        cards.push(SdCardEntry { card });
        if let Some(ref host) = *self.host.borrow() {
            let h: &Arc<dyn SdBusHost> = host;
            h.set_inserted(true);
        }
    }

    /// Remove the card from the bus.
    pub fn remove_card(&self) {
        self.cards.lock().unwrap().clear();
        if let Some(ref host) = *self.host.borrow() {
            let h: &Arc<dyn SdBusHost> = host;
            h.set_inserted(false);
        }
    }

    /// Return the card, if inserted.
    fn card(&self) -> Option<Arc<dyn SdCard>> {
        let cards = self.cards.lock().unwrap();
        cards.first().map(|e| Arc::clone(&e.card))
    }

    /// Send a command to the card and collect the response.
    ///
    /// Returns `Ok(n)` with the number of response bytes on success,
    /// or `Err(SdError::NoCard)` if no card is present.
    pub fn do_command(
        &self,
        req: &SdRequest,
        resp: &mut [u8],
    ) -> Result<usize, SdError> {
        if let Some(card) = self.card() {
            Ok(card.do_command(req, resp))
        } else {
            Err(SdError::NoCard)
        }
    }

    /// Write a byte to the card.
    pub fn write_byte(&self, value: u8) {
        if let Some(card) = self.card() {
            card.write_byte(value);
        }
    }

    /// Read a byte from the card. Returns 0 if no card.
    #[must_use]
    pub fn read_byte(&self) -> u8 {
        if let Some(card) = self.card() {
            card.read_byte()
        } else {
            0
        }
    }

    /// Write multiple bytes to the card.
    pub fn write_data(&self, buf: &[u8]) {
        if let Some(card) = self.card() {
            for &b in buf {
                card.write_byte(b);
            }
        }
    }

    /// Read multiple bytes from the card.
    pub fn read_data(&self, buf: &mut [u8]) {
        if let Some(card) = self.card() {
            for dst in buf.iter_mut() {
                *dst = card.read_byte();
            }
        }
    }

    /// Return true if the card is ready to receive data.
    #[must_use]
    pub fn receive_ready(&self) -> bool {
        self.card().is_some_and(|card| card.receive_ready())
    }

    /// Return true if the card has data ready.
    #[must_use]
    pub fn data_ready(&self) -> bool {
        self.card().is_some_and(|card| card.data_ready())
    }

    /// Return true if a card is present.
    #[must_use]
    pub fn get_inserted(&self) -> bool {
        self.card().is_some_and(|card| card.get_inserted())
    }

    /// Return true if the card is read-only.
    #[must_use]
    pub fn get_readonly(&self) -> bool {
        self.card().is_some_and(|card| card.get_readonly())
    }

    /// Set the voltage supplied to the card.
    pub fn set_voltage(&self, millivolts: u16) {
        if let Some(card) = self.card() {
            card.set_voltage(millivolts);
        }
    }

    /// Return the supported DAT line mask.
    #[must_use]
    pub fn get_dat_lines(&self) -> u8 {
        self.card().map_or(0b1111, |card| card.get_dat_lines())
    }

    /// Return the CMD line state.
    #[must_use]
    pub fn get_cmd_line(&self) -> bool {
        self.card().is_none_or(|card| card.get_cmd_line())
    }

    /// Remove and return the card, notifying the host.
    fn take_card(&self) -> Option<Arc<dyn SdCard>> {
        let mut cards = self.cards.lock().unwrap();
        let card = cards.pop().map(|e| e.card);
        if card.is_some() {
            drop(cards);
            if let Some(ref host) = *self.host.borrow() {
                host.set_inserted(false);
            }
        }
        card
    }

    /// Move a card from another bus to this one.
    pub fn reparent_card(&self, from: &SdBus) {
        let readonly = from.get_readonly();
        if let Some(card) = from.take_card() {
            self.cards.lock().unwrap().clear();
            self.cards.lock().unwrap().push(SdCardEntry { card });
            if let Some(ref host) = *self.host.borrow() {
                host.set_inserted(true);
                host.set_readonly(readonly);
            }
        }
    }
}

impl Default for SdBus {
    fn default() -> Self {
        Self::new()
    }
}
