use std::sync::Arc;

use machina_accel::timer::VirtualClock;
use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::irq::InterruptSource;
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

// Register offsets
const AON_WDT_WDOGCFG: u64 = 0x00;
const AON_WDT_WDOGCOUNT: u64 = 0x08;
const AON_WDT_WDOGS: u64 = 0x10;
const AON_WDT_WDOGFEED: u64 = 0x18;
const AON_WDT_WDOGKEY: u64 = 0x1C;
const AON_WDT_WDOGCMP0: u64 = 0x20;

const SIFIVE_E_AON_RTC: u64 = 0x40;
const SIFIVE_E_AON_MAX: u64 = 0x150;

pub const SIFIVE_E_AON_WDOGKEY: u32 = 0x0051_F15E;
pub const SIFIVE_E_AON_WDOGFEED: u32 = 0x0D09_F00D;
pub const SIFIVE_E_LFCLK_DEFAULT_FREQ: u64 = 32768;

// WDOGCFG field positions
const WDOGCFG_SCALE: u8 = 0;
const WDOGCFG_RSTEN: u8 = 8;
const WDOGCFG_ZEROCMP: u8 = 9;
const WDOGCFG_EN_ALWAYS: u8 = 12;
const WDOGCFG_EN_CORE_AWAKE: u8 = 13;
const WDOGCFG_IP0: u8 = 28;

const WDOGCOUNT_VALUE_MASK: u32 = 0x7FFF_FFFF;

struct SiFiveEAonRegs {
    wdogcfg: u32,
    wdogcmp0: u16,
    wdogcount: u32,
    wdogunlock: u8,
    restart_time: i64,
    wdogclk_freq: u64,
}

impl SiFiveEAonRegs {
    fn new(wdogclk_freq: u64) -> Self {
        Self {
            wdogcfg: 0,
            wdogcmp0: 0xbeef,
            wdogcount: 0,
            wdogunlock: 0,
            restart_time: 0,
            wdogclk_freq,
        }
    }

    fn reset(&mut self) {
        self.wdogcfg &= !((1u32 << WDOGCFG_RSTEN)
            | (1u32 << WDOGCFG_EN_ALWAYS)
            | (1u32 << WDOGCFG_EN_CORE_AWAKE));
        self.wdogcmp0 = 0xbeef;
    }
}

pub struct SiFiveEAon {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<SiFiveEAonRegs>,
    irq: parking_lot::Mutex<Option<InterruptSource>>,
    clock: Arc<VirtualClock>,
}

impl SiFiveEAon {
    #[must_use]
    pub fn new(clock: Arc<VirtualClock>) -> Self {
        Self::with_freq(clock, SIFIVE_E_LFCLK_DEFAULT_FREQ)
    }

    #[must_use]
    pub fn with_freq(clock: Arc<VirtualClock>, wdogclk_freq: u64) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(
                "sifive_e_aon",
            )),
            regs: DeviceRefCell::new(SiFiveEAonRegs::new(wdogclk_freq)),
            irq: parking_lot::Mutex::new(None),
            clock,
        }
    }

    pub fn connect_irq(&self, irq: InterruptSource) {
        *self.irq.lock() = Some(irq);
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
        self.lower_irq();
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
        self.regs.borrow().reset();
        self.lower_irq();
    }

    fn lower_irq(&self) {
        if let Some(ref line) = *self.irq.lock() {
            line.lower();
        }
    }
}

impl Default for SiFiveEAon {
    fn default() -> Self {
        Self::new(Arc::new(VirtualClock::new(
            machina_accel::timer::ClockType::Virtual,
        )))
    }
}

fn muldiv64(val: i64, num: u64, den: u64) -> i64 {
    (val * num as i64) / den as i64
}

fn field_get(val: u32, pos: u8) -> u32 {
    (val >> pos) & 1
}

fn scale_get(val: u32) -> u32 {
    (val >> WDOGCFG_SCALE) & 0xF
}

fn field_set(val: u32, pos: u8, bit: u32) -> u32 {
    let mask = 1u32 << pos;
    if bit != 0 {
        val | mask
    } else {
        val & !mask
    }
}

fn update_wdogcount(regs: &mut SiFiveEAonRegs, now: i64) {
    let en_always = field_get(regs.wdogcfg, WDOGCFG_EN_ALWAYS);
    let en_core_awake = field_get(regs.wdogcfg, WDOGCFG_EN_CORE_AWAKE);
    if en_always == 0 && en_core_awake == 0 {
        return;
    }
    let delta_ns = now - regs.restart_time;
    let ticks = muldiv64(delta_ns, regs.wdogclk_freq, 1_000_000_000);
    regs.wdogcount =
        regs.wdogcount.wrapping_add(ticks as u32) & WDOGCOUNT_VALUE_MASK;
    regs.restart_time = now;
}

fn update_state(regs: &mut SiFiveEAonRegs, now: i64) {
    update_wdogcount(regs, now);

    let scale = scale_get(regs.wdogcfg);
    let wdogs = (regs.wdogcount >> scale) as u16;
    let cmp_signal = wdogs >= regs.wdogcmp0;

    if cmp_signal {
        if field_get(regs.wdogcfg, WDOGCFG_ZEROCMP) == 1 {
            regs.wdogcount = 0;
        }
        regs.wdogcfg = field_set(regs.wdogcfg, WDOGCFG_IP0, 1);
    }
}

pub struct SiFiveEAonMmio(pub Arc<SiFiveEAon>);

impl MmioOps for SiFiveEAonMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        if size != 4 {
            return 0;
        }

        if offset >= SIFIVE_E_AON_MAX {
            return 0;
        }
        // WDT region
        if offset < SIFIVE_E_AON_RTC {
            let now = self.0.clock.get_ns();
            let mut regs = self.0.regs.borrow();
            match offset {
                AON_WDT_WDOGCFG => u64::from(regs.wdogcfg),
                AON_WDT_WDOGCOUNT => {
                    update_wdogcount(&mut regs, now);
                    u64::from(regs.wdogcount)
                }
                AON_WDT_WDOGS => {
                    update_wdogcount(&mut regs, now);
                    let scale = scale_get(regs.wdogcfg);
                    u64::from(regs.wdogcount >> scale)
                }
                AON_WDT_WDOGFEED => 0,
                AON_WDT_WDOGKEY => u64::from(regs.wdogunlock),
                AON_WDT_WDOGCMP0 => u64::from(regs.wdogcmp0),
                _ => 0,
            }
        } else {
            // RTC, LFROSC, BACKUP, PMU — unimplemented
            0
        }
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        if size != 4 {
            return;
        }

        if offset >= SIFIVE_E_AON_MAX {
            return;
        }
        if offset >= SIFIVE_E_AON_RTC {
            // RTC, LFROSC, BACKUP, PMU — unimplemented
            return;
        }

        let value = val as u32;
        let now = self.0.clock.get_ns();
        let mut regs = self.0.regs.borrow();

        match offset {
            AON_WDT_WDOGCFG => {
                if regs.wdogunlock == 0 {
                    return;
                }
                let old_en = field_get(regs.wdogcfg, WDOGCFG_EN_ALWAYS)
                    | field_get(regs.wdogcfg, WDOGCFG_EN_CORE_AWAKE);
                let new_en = field_get(value, WDOGCFG_EN_ALWAYS)
                    | field_get(value, WDOGCFG_EN_CORE_AWAKE);
                if old_en == 1 && new_en == 0 {
                    update_wdogcount(&mut regs, now);
                } else if old_en == 0 && new_en == 1 {
                    regs.restart_time = now;
                }
                regs.wdogcfg = value;
                regs.wdogunlock = 0;
            }
            AON_WDT_WDOGCOUNT => {
                if regs.wdogunlock == 0 {
                    return;
                }
                regs.wdogcount = value & WDOGCOUNT_VALUE_MASK;
                regs.restart_time = now;
                regs.wdogunlock = 0;
            }
            AON_WDT_WDOGS => {
                return;
            }
            AON_WDT_WDOGFEED => {
                if regs.wdogunlock == 0 {
                    return;
                }
                if value == SIFIVE_E_AON_WDOGFEED {
                    regs.wdogcount = 0;
                    regs.restart_time = now;
                }
                regs.wdogunlock = 0;
            }
            AON_WDT_WDOGKEY => {
                if value == SIFIVE_E_AON_WDOGKEY {
                    regs.wdogunlock = 1;
                }
                return;
            }
            AON_WDT_WDOGCMP0 => {
                if regs.wdogunlock == 0 {
                    return;
                }
                regs.wdogcmp0 = value as u16;
                regs.wdogunlock = 0;
            }
            _ => return,
        }

        update_state(&mut regs, now);
        let irq = field_get(regs.wdogcfg, WDOGCFG_IP0) != 0;
        drop(regs);
        if let Some(ref line) = *self.0.irq.lock() {
            line.set(irq);
        }
    }
}
