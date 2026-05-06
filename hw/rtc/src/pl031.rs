use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::irq::InterruptSource;
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

const RTC_DR: u64 = 0x00;
const RTC_MR: u64 = 0x04;
const RTC_LR: u64 = 0x08;
const RTC_CR: u64 = 0x0c;
const RTC_IMSC: u64 = 0x10;
const RTC_RIS: u64 = 0x14;
const RTC_MIS: u64 = 0x18;
const RTC_ICR: u64 = 0x1c;

const PL031_ID: [u8; 8] = [0x31, 0x10, 0x14, 0x00, 0x0d, 0xf0, 0x05, 0xb1];

struct Pl031Regs {
    // DR: data register (current time in seconds)
    dr: u32,
    // MR: match register (alarm time)
    mr: u32,
    // LR: load register (sets time)
    lr: u32,
    // CR: control register (always reads 1)
    // IMSC: interrupt mask
    im: u32,
    // RIS: raw interrupt status
    is: u32,
}

impl Pl031Regs {
    fn new() -> Self {
        Self {
            dr: 0,
            mr: 0,
            lr: 0,
            im: 0,
            is: 0,
        }
    }

    fn reset(&mut self) {
        self.dr = 0;
        self.mr = 0;
        self.lr = 0;
        self.im = 0;
        self.is = 0;
    }

    fn update(&mut self) -> bool {
        let flags = self.is & self.im;
        flags != 0
    }

    fn set_alarm(&mut self) {
        // Only fire if mr is non-zero and matches dr.
        // Default mr=0, dr=0 should not trigger.
        if self.mr != 0 && self.mr == self.dr {
            self.is = 1;
        }
    }

    fn tick(&mut self, seconds: u32) -> bool {
        self.dr = self.dr.wrapping_add(seconds);
        self.set_alarm();
        self.update()
    }
}

pub struct Pl031 {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<Pl031Regs>,
    output: parking_lot::Mutex<Option<InterruptSource>>,
}

impl Pl031 {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("pl031")
    }

    #[must_use]
    pub fn new_named(local_id: &str) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRefCell::new(Pl031Regs::new()),
            output: parking_lot::Mutex::new(None),
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
        self.lower_outputs();
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

    pub fn connect_output(&self, irq: InterruptSource) {
        *self.output.lock() = Some(irq);
    }

    pub fn reset_runtime(&self) {
        self.regs.borrow().reset();
        self.lower_outputs();
    }

    fn lower_outputs(&self) {
        if let Some(ref line) = *self.output.lock() {
            line.lower();
        }
    }

    fn update_irq(&self, flags: bool) {
        if let Some(ref line) = *self.output.lock() {
            line.set(flags);
        }
    }

    /// Advance the RTC counter by `seconds` and check alarm.
    pub fn tick(&self, seconds: u32) {
        let flags = self.regs.borrow().tick(seconds);
        self.update_irq(flags);
    }

    /// Get current DR value (seconds counter).
    #[must_use]
    pub fn current_time(&self) -> u32 {
        self.regs.borrow().dr
    }
}

impl Default for Pl031 {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Pl031Mmio(pub Arc<Pl031>);

impl MmioOps for Pl031Mmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        let regs = self.0.regs.borrow();
        match offset {
            RTC_DR => u64::from(regs.dr),
            RTC_MR => u64::from(regs.mr),
            RTC_LR => u64::from(regs.lr),
            RTC_CR => 1,
            RTC_IMSC => u64::from(regs.im),
            RTC_RIS => u64::from(regs.is),
            RTC_MIS => u64::from(regs.is & regs.im),
            RTC_ICR => 0,
            idx @ 0xFE0..=0xFFF => {
                let off = ((idx - 0xFE0) >> 2) as usize;
                if off < PL031_ID.len() {
                    u64::from(PL031_ID[off])
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        let value = val as u32;
        match offset {
            RTC_LR => {
                let mut regs = self.0.regs.borrow();
                regs.lr = value;
                regs.dr = value;
                regs.set_alarm();
                let flags = regs.update();
                drop(regs);
                self.0.update_irq(flags);
            }
            RTC_MR => {
                let mut regs = self.0.regs.borrow();
                regs.mr = value;
                regs.set_alarm();
                let flags = regs.update();
                drop(regs);
                self.0.update_irq(flags);
            }
            RTC_IMSC => {
                let mut regs = self.0.regs.borrow();
                regs.im = value & 1;
                let flags = regs.update();
                drop(regs);
                self.0.update_irq(flags);
            }
            RTC_ICR => {
                let mut regs = self.0.regs.borrow();
                regs.is &= !value;
                let flags = regs.update();
                drop(regs);
                self.0.update_irq(flags);
            }
            RTC_CR => { /* writes ignored */ }
            // Read-only registers: DR, MIS, RIS — silently ignore
            _ => {}
        }
    }
}
