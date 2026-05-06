use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

// Control frame registers
const A_CNTCR: u64 = 0x00;
const A_CNTSR: u64 = 0x04;
const A_CNTCV_LO: u64 = 0x08;
const A_CNTCV_HI: u64 = 0x0C;
const A_CNTSCR: u64 = 0x10;
const A_CNTID: u64 = 0x1C;
const A_CNTSCR0: u64 = 0xD0;
const A_CNTSCR1: u64 = 0xD4;

// Status frame registers
const A_STATUS_CNTCV_LO: u64 = 0x00;
const A_STATUS_CNTCV_HI: u64 = 0x04;

// PID/CID base
const A_PID4: u64 = 0xFD0;
const A_CID3: u64 = 0xFFC;

// CNTCR fields
const CNTCR_EN: u32 = 1 << 0;
const CNTCR_HDBG: u32 = 1 << 1;
const CNTCR_SCEN: u32 = 1 << 2;
const CNTCR_INTRMASK: u32 = 1 << 3;
const CNTCR_PSLVERRDIS: u32 = 1 << 4;

const CNTCR_VALID_MASK: u32 =
    CNTCR_EN | CNTCR_HDBG | CNTCR_SCEN | CNTCR_INTRMASK | CNTCR_PSLVERRDIS;

const CONTROL_ID: [u8; 12] = [
    0x04, 0x00, 0x00, 0x00, // PID4..7
    0xBA, 0xB0, 0x0B, 0x00, // PID0..3
    0x0D, 0xF0, 0x05, 0xB1, // CID0..3
];

const STATUS_ID: [u8; 12] = [
    0x04, 0x00, 0x00, 0x00, // PID4..7
    0xBB, 0xB0, 0x0B, 0x00, // PID0..3
    0x0D, 0xF0, 0x05, 0xB1, // CID0..3
];

struct SseCounterRegs {
    cntcr: u32,
    cntscr0: u32,
    /// Counter ticks at last sync point.
    ticks_then: u64,
    /// Virtual nanoseconds at last sync point.
    ns_then: u64,
    /// Clock frequency in Hz.
    freq_hz: u64,
}

impl SseCounterRegs {
    fn new(freq_hz: u64) -> Self {
        Self {
            cntcr: 0,
            cntscr0: 0x0100_0000,
            ticks_then: 0,
            ns_then: 0,
            freq_hz,
        }
    }

    fn reset(&mut self) {
        self.cntcr = 0;
        self.cntscr0 = 0x0100_0000;
        self.ticks_then = 0;
        // ns_then is set to current time during reset
    }

    fn enabled(&self) -> bool {
        (self.cntcr & CNTCR_EN) != 0
    }

    fn counter_value(&self) -> u64 {
        if !self.enabled() {
            return self.ticks_then;
        }
        // In test mode, just return ticks_then (we advance via tick())
        self.ticks_then
    }

    /// Advance the counter by `ns` nanoseconds.
    fn tick(&mut self, ns: u64) {
        if !self.enabled() {
            return;
        }
        let freq = if self.freq_hz > 0 { self.freq_hz } else { 1 };
        // Ticks = ns * freq / 1_000_000_000
        let ticks = (ns as u128 * freq as u128 / 1_000_000_000u128) as u64;
        if (self.cntcr & CNTCR_SCEN) != 0 && self.cntscr0 > 0 {
            let scaled =
                (ticks as u128 * self.cntscr0 as u128 / 0x0100_0000u128) as u64;
            self.ticks_then = self.ticks_then.wrapping_add(scaled);
        } else {
            self.ticks_then = self.ticks_then.wrapping_add(ticks);
        }
        self.ns_then = self.ns_then.wrapping_add(ns);
    }

    /// Set counter value directly (for CNTCV write).
    fn set_counter(&mut self, val: u64) {
        self.ticks_then = val;
    }
}

pub type CounterCallback = Box<dyn Fn() + Send + Sync>;

pub struct SseCounter {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<SseCounterRegs>,
    callbacks: parking_lot::Mutex<Vec<CounterCallback>>,
}

impl SseCounter {
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_freq(1_000_000)
    }

    #[must_use]
    pub fn new_with_freq(freq_hz: u64) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(
                "sse_counter",
            )),
            regs: DeviceRefCell::new(SseCounterRegs::new(freq_hz)),
            callbacks: parking_lot::Mutex::new(Vec::new()),
        }
    }

    pub fn attach_to_bus(&self, bus: &mut SysBus) -> Result<(), SysBusError> {
        self.state.lock().attach_to_bus(bus)
    }

    pub fn register_mmio(
        &self,
        region: MemoryRegion,
        base: GPA,
    ) -> Result<(), SysBusError> {
        self.state.lock().register_mmio(region, base)
    }

    pub fn realize_onto(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.state.lock().realize_onto(bus, address_space)?;
        Ok(())
    }

    pub fn unrealize_from(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.state.lock().unrealize_from(bus, address_space)?;
        Ok(())
    }

    #[must_use]
    pub fn realized(&self) -> bool {
        self.state.lock().device().is_realized()
    }

    #[must_use]
    pub fn object_info(&self) -> MObjectInfo {
        self.state.lock().object_info()
    }

    pub fn with_mdevice<T>(&self, f: impl FnOnce(&dyn MDevice) -> T) -> T {
        let guard = self.state.lock();
        f(&*guard)
    }

    pub fn reset_runtime(&self) {
        let mut regs = self.regs.borrow();
        regs.reset();
        regs.ns_then = 0;
    }

    /// Register a callback invoked when the counter value changes.
    pub fn register_callback(&self, cb: CounterCallback) {
        self.callbacks.lock().push(cb);
    }

    fn notify_callbacks(&self) {
        for cb in self.callbacks.lock().iter() {
            cb();
        }
    }

    /// Advance the counter by `ns` nanoseconds.
    pub fn tick(&self, ns: u64) {
        self.regs.borrow().tick(ns);
        self.notify_callbacks();
    }

    /// Get the current counter value (CNTCV).
    #[must_use]
    pub fn counter_value(&self) -> u64 {
        self.regs.borrow().counter_value()
    }

    /// Set counter value (for CNTCV write).
    pub fn set_counter(&self, val: u64) {
        self.regs.borrow().set_counter(val);
        self.notify_callbacks();
    }
}

impl Default for SseCounter {
    fn default() -> Self {
        Self::new()
    }
}

fn read_id(frame: &[u8; 12], offset: u64) -> u64 {
    let idx = ((offset - A_PID4) / 4) as usize;
    if idx < 12 {
        u64::from(frame[idx])
    } else {
        0
    }
}

pub struct SseCounterControlMmio(pub Arc<SseCounter>);

impl MmioOps for SseCounterControlMmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        let regs = self.0.regs.borrow();
        match offset {
            A_CNTCR => u64::from(regs.cntcr),
            A_CNTSR => 0, // DBGH always 0
            A_CNTCV_LO => regs.counter_value() & 0xFFFF_FFFF,
            A_CNTCV_HI => (regs.counter_value() >> 32) & 0xFFFF_FFFF,
            A_CNTID => {
                // CNTSC implemented (bit 0), CNTSELCLK = 1 (bit 16)
                (1 << 16) | 1
            }
            A_CNTSCR | A_CNTSCR0 => u64::from(regs.cntscr0),
            A_CNTSCR1 => 0,
            A_PID4..=A_CID3 => read_id(&CONTROL_ID, offset),
            _ => 0,
        }
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        let value = val as u32;
        match offset {
            A_CNTCR => {
                let mut regs = self.0.regs.borrow();
                regs.cntcr = value & CNTCR_VALID_MASK;
            }
            A_CNTCV_LO => {
                let mut regs = self.0.regs.borrow();
                let cv = regs.counter_value();
                let new_cv = (cv & 0xFFFF_FFFF_0000_0000) | (val & 0xFFFF_FFFF);
                regs.set_counter(new_cv);
            }
            A_CNTCV_HI => {
                let mut regs = self.0.regs.borrow();
                let new_cv = (val & 0xFFFF_FFFF) << 32;
                regs.set_counter(new_cv);
            }
            A_CNTSCR | A_CNTSCR0 => {
                self.0.regs.borrow().cntscr0 = value;
            }
            A_CNTSCR1 => { /* RAZ/WI */ }
            // RO registers: CNTSR, CNTID, PID/CID
            _ => {}
        }
        self.0.notify_callbacks();
    }
}

pub struct SseCounterStatusMmio(pub Arc<SseCounter>);

impl MmioOps for SseCounterStatusMmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        let regs = self.0.regs.borrow();
        match offset {
            A_STATUS_CNTCV_LO => regs.counter_value() & 0xFFFF_FFFF,
            A_STATUS_CNTCV_HI => (regs.counter_value() >> 32) & 0xFFFF_FFFF,
            A_PID4..=A_CID3 => read_id(&STATUS_ID, offset),
            _ => 0,
        }
    }

    fn write(&self, _offset: u64, _size: u32, _val: u64) {
        // Status frame is entirely read-only
    }
}
