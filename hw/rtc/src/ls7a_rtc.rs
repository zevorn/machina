use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::irq::InterruptSource;
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

#[allow(dead_code)]
const SYS_TOYTRIM: u64 = 0x20;
const SYS_TOYWRITE0: u64 = 0x24;
const SYS_TOYWRITE1: u64 = 0x28;
const SYS_TOYREAD0: u64 = 0x2C;
const SYS_TOYREAD1: u64 = 0x30;
const SYS_TOYMATCH0: u64 = 0x34;
const SYS_TOYMATCH1: u64 = 0x38;
const SYS_TOYMATCH2: u64 = 0x3C;
const SYS_RTCCTRL: u64 = 0x40;
#[allow(dead_code)]
const SYS_RTCTRIM: u64 = 0x60;
const SYS_RTCWRTIE0: u64 = 0x64;
const SYS_RTCREAD0: u64 = 0x68;
const SYS_RTCMATCH0: u64 = 0x6C;
const SYS_RTCMATCH1: u64 = 0x70;
const SYS_RTCMATCH2: u64 = 0x74;

const LS7A_RTC_FREQ: u64 = 32768;
const TIMER_NUMS: usize = 3;

// TOY register fields
const TOY_SEC_SHIFT: u32 = 4;
const TOY_SEC_MASK: u32 = 0x3F;
const TOY_MIN_SHIFT: u32 = 10;
const TOY_MIN_MASK: u32 = 0x3F;
const TOY_HOUR_SHIFT: u32 = 16;
const TOY_HOUR_MASK: u32 = 0x1F;
const TOY_DAY_SHIFT: u32 = 21;
const TOY_DAY_MASK: u32 = 0x1F;
const TOY_MON_SHIFT: u32 = 26;
const TOY_MON_MASK: u32 = 0x3F;

// TOY match register fields (slightly different)
#[allow(dead_code)]
const TOY_MATCH_SEC_SHIFT: u32 = 0;
#[allow(dead_code)]
const TOY_MATCH_SEC_MASK: u32 = 0x3F;
#[allow(dead_code)]
const TOY_MATCH_MIN_SHIFT: u32 = 6;
#[allow(dead_code)]
const TOY_MATCH_MIN_MASK: u32 = 0x3F;
#[allow(dead_code)]
const TOY_MATCH_HOUR_SHIFT: u32 = 12;
#[allow(dead_code)]
const TOY_MATCH_HOUR_MASK: u32 = 0x1F;
#[allow(dead_code)]
const TOY_MATCH_DAY_SHIFT: u32 = 17;
#[allow(dead_code)]
const TOY_MATCH_DAY_MASK: u32 = 0x1F;
#[allow(dead_code)]
const TOY_MATCH_MON_SHIFT: u32 = 22;
#[allow(dead_code)]
const TOY_MATCH_MON_MASK: u32 = 0x0F;
#[allow(dead_code)]
const TOY_MATCH_YEAR_SHIFT: u32 = 26;
#[allow(dead_code)]
const TOY_MATCH_YEAR_MASK: u32 = 0x3F;

const RTC_CTRL_TOYEN: u32 = 1 << 11;
const RTC_CTRL_RTCEN: u32 = 1 << 13;
const RTC_CTRL_EO: u32 = 1 << 8;

struct Ls7aRtcRegs {
    // TOY time offset (seconds since epoch)
    toy_offset: i64,
    // RTC tick offset (32kHz ticks)
    rtc_offset: i64,
    // Trimming registers
    #[allow(dead_code)]
    toytrim: u32,
    #[allow(dead_code)]
    rtctrim: u32,
    // Control register
    cntrctl: u32,
    // Match registers
    toymatch: [u32; TIMER_NUMS],
    rtcmatch: [u32; TIMER_NUMS],
    // Year (separate from TOY encoding, stored as full year)
    toy_year: u32,
    // IRQ pending flag
    irq_pending: bool,
}

impl Ls7aRtcRegs {
    fn new() -> Self {
        Self {
            toy_offset: 0,
            rtc_offset: 0,
            toytrim: 0,
            rtctrim: 0,
            cntrctl: 0,
            toymatch: [0; TIMER_NUMS],
            rtcmatch: [0; TIMER_NUMS],
            toy_year: 0,
            irq_pending: false,
        }
    }

    fn reset(&mut self) {
        self.toymatch = [0; TIMER_NUMS];
        self.rtcmatch = [0; TIMER_NUMS];
        self.cntrctl = 0;
        self.irq_pending = false;
    }

    fn toy_enabled(&self) -> bool {
        (self.cntrctl & RTC_CTRL_TOYEN) != 0
            && (self.cntrctl & RTC_CTRL_EO) != 0
    }

    fn rtc_enabled(&self) -> bool {
        (self.cntrctl & RTC_CTRL_RTCEN) != 0
            && (self.cntrctl & RTC_CTRL_EO) != 0
    }

    fn rtc_ticks(&self) -> u32 {
        self.rtc_offset as u32
    }

    fn encode_toy(&self) -> u32 {
        let total_secs = self.toy_offset as u64;
        let sec = (total_secs % 60) as u32;
        let mins = (total_secs / 60) as u32;
        let min = mins % 60;
        let hours = mins / 60;
        let hour = hours % 24;
        let days_total = hours / 24;
        // Simple day/month encoding from days since epoch
        let day = (days_total % 31) + 1;
        let mon = ((days_total / 31) % 12) + 1;
        let mut val = 0;
        val |= (sec & TOY_SEC_MASK) << TOY_SEC_SHIFT;
        val |= (min & TOY_MIN_MASK) << TOY_MIN_SHIFT;
        val |= (hour & TOY_HOUR_MASK) << TOY_HOUR_SHIFT;
        val |= (day & TOY_DAY_MASK) << TOY_DAY_SHIFT;
        val |= (mon & TOY_MON_MASK) << TOY_MON_SHIFT;
        val
    }

    fn decode_toy_write0(&mut self, val: u32) {
        // Extract time fields and compute new offset
        let sec = (val >> TOY_SEC_SHIFT) & TOY_SEC_MASK;
        let min = (val >> TOY_MIN_SHIFT) & TOY_MIN_MASK;
        let hour = (val >> TOY_HOUR_SHIFT) & TOY_HOUR_MASK;
        let day = (val >> TOY_DAY_SHIFT) & TOY_DAY_MASK;
        let mon = (val >> TOY_MON_SHIFT) & TOY_MON_MASK;
        // Convert to seconds since epoch (simplified: assume 31 days/month)
        let total = sec as i64
            + min as i64 * 60
            + hour as i64 * 3600
            + (day as i64 - 1) * 86400
            + (mon as i64 - 1) * 31 * 86400
            + self.toy_year as i64 * 365 * 86400;
        self.toy_offset = total;
    }

    fn decode_toy_write1(&mut self, year: u32) {
        self.toy_year = year;
    }

    fn check_toy_matches(&mut self) {
        if !self.toy_enabled() {
            return;
        }
        let toy_val = self.encode_toy();
        let mut fired = false;
        let mut to_clear = [false; TIMER_NUMS];
        for (i, &m) in self.toymatch.iter().enumerate() {
            if m != 0 && (toy_val & 0x3FFF_FFFF) == (m & 0x3FFF_FFFF) {
                fired = true;
                to_clear[i] = true;
            }
        }
        if fired {
            self.irq_pending = true;
            for (i, &clear) in to_clear.iter().enumerate() {
                if clear {
                    self.toymatch[i] = 0;
                }
            }
        }
    }

    fn check_rtc_matches(&mut self) {
        if !self.rtc_enabled() {
            return;
        }
        let ticks = self.rtc_ticks();
        let mut fired = false;
        let mut to_clear = [false; TIMER_NUMS];
        for (i, &m) in self.rtcmatch.iter().enumerate() {
            if m != 0 && ticks >= m {
                fired = true;
                to_clear[i] = true;
            }
        }
        if fired {
            self.irq_pending = true;
            for (i, &clear) in to_clear.iter().enumerate() {
                if clear {
                    self.rtcmatch[i] = 0;
                }
            }
        }
    }
}

pub struct Ls7aRtc {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<Ls7aRtcRegs>,
    output: parking_lot::Mutex<Option<InterruptSource>>,
}

impl Ls7aRtc {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("ls7a_rtc")
    }

    #[must_use]
    pub fn new_named(local_id: &str) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRefCell::new(Ls7aRtcRegs::new()),
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

    fn update_irq(&self) {
        let pending = self.regs.borrow().irq_pending;
        if let Some(ref line) = *self.output.lock() {
            line.set(pending);
        }
    }

    /// Advance time by `seconds` and check match conditions.
    pub fn tick(&self, seconds: u32) {
        let mut regs = self.regs.borrow();
        let secs = seconds as i64;
        regs.toy_offset = regs.toy_offset.wrapping_add(secs);
        regs.rtc_offset =
            regs.rtc_offset.wrapping_add(secs * LS7A_RTC_FREQ as i64);
        regs.check_toy_matches();
        regs.check_rtc_matches();
        let pending = regs.irq_pending;
        drop(regs);
        if pending {
            self.update_irq();
        }
    }

    /// Set TOY offset directly (for testing).
    pub fn set_toy_offset(&self, offset: i64) {
        self.regs.borrow().toy_offset = offset;
    }

    /// Set RTC offset directly (for testing).
    pub fn set_rtc_offset(&self, offset: i64) {
        self.regs.borrow().rtc_offset = offset;
    }
}

impl Default for Ls7aRtc {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Ls7aRtcMmio(pub Arc<Ls7aRtc>);

impl MmioOps for Ls7aRtcMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        if size != 4 {
            return 0;
        }

        let regs = self.0.regs.borrow();
        match offset {
            SYS_TOYREAD0 if regs.toy_enabled() => u64::from(regs.encode_toy()),
            SYS_TOYREAD1 if regs.toy_enabled() => u64::from(regs.toy_year),
            SYS_TOYMATCH0 => u64::from(regs.toymatch[0]),
            SYS_TOYMATCH1 => u64::from(regs.toymatch[1]),
            SYS_TOYMATCH2 => u64::from(regs.toymatch[2]),
            SYS_RTCCTRL => u64::from(regs.cntrctl),
            SYS_RTCREAD0 if regs.rtc_enabled() => u64::from(regs.rtc_ticks()),
            SYS_RTCMATCH0 => u64::from(regs.rtcmatch[0]),
            SYS_RTCMATCH1 => u64::from(regs.rtcmatch[1]),
            SYS_RTCMATCH2 => u64::from(regs.rtcmatch[2]),
            _ => 0,
        }
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        if size != 4 {
            return;
        }

        let value = val as u32;
        match offset {
            SYS_TOYWRITE0 => {
                let mut regs = self.0.regs.borrow();
                if regs.toy_enabled() {
                    regs.decode_toy_write0(value);
                }
            }
            SYS_TOYWRITE1 => {
                let mut regs = self.0.regs.borrow();
                if regs.toy_enabled() {
                    regs.decode_toy_write1(value);
                }
            }
            SYS_TOYMATCH0 => {
                let mut regs = self.0.regs.borrow();
                if regs.toy_enabled() {
                    regs.toymatch[0] = value;
                }
            }
            SYS_TOYMATCH1 => {
                let mut regs = self.0.regs.borrow();
                if regs.toy_enabled() {
                    regs.toymatch[1] = value;
                }
            }
            SYS_TOYMATCH2 => {
                let mut regs = self.0.regs.borrow();
                if regs.toy_enabled() {
                    regs.toymatch[2] = value;
                }
            }
            SYS_RTCCTRL => {
                self.0.regs.borrow().cntrctl = value;
            }
            SYS_RTCWRTIE0 => {
                let mut regs = self.0.regs.borrow();
                if regs.rtc_enabled() {
                    regs.rtc_offset = value as i64 - regs.rtc_ticks() as i64;
                }
            }
            SYS_RTCMATCH0 => {
                let mut regs = self.0.regs.borrow();
                if regs.rtc_enabled() {
                    regs.rtcmatch[0] = value;
                }
            }
            SYS_RTCMATCH1 => {
                let mut regs = self.0.regs.borrow();
                if regs.rtc_enabled() {
                    regs.rtcmatch[1] = value;
                }
            }
            SYS_RTCMATCH2 => {
                let mut regs = self.0.regs.borrow();
                if regs.rtc_enabled() {
                    regs.rtcmatch[2] = value;
                }
            }
            _ => {}
        }
    }
}
