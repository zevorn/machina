//! Watchdog devices.

use std::sync::Arc;

use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_hw_timer::{Ptimer, PtimerCallback};
use machina_memory::region::MmioOps;

pub const SBSA_GWDT_REFRESH_SIZE: u64 = 0x1000;
pub const SBSA_GWDT_CONTROL_SIZE: u64 = 0x1000;

const WRR: u64 = 0x000;
const WCS: u64 = 0x000;
const WOR: u64 = 0x008;
const WORU: u64 = 0x00c;
const WCV: u64 = 0x010;
const WCVU: u64 = 0x014;
const W_IIDR: u64 = 0xfcc;

const WCS_EN: u32 = 1 << 0;
const WCS_WS0: u32 = 1 << 1;
const WCS_WS1: u32 = 1 << 2;
const WORU_MASK: u32 = 0x0000_ffff;
const WATCHDOG_ID: u32 = 0x0001_043b;
const DEFAULT_CLOCK_FREQUENCY: u32 = 62_500_000;
const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefreshType {
    Explicit,
    Timeout,
}

#[derive(Clone, Copy)]
struct SbsaGwdtRegs {
    id: u32,
    wcs: u32,
    worl: u32,
    woru: u32,
    wcvl: u32,
    wcvu: u32,
}

impl Default for SbsaGwdtRegs {
    fn default() -> Self {
        Self {
            id: WATCHDOG_ID,
            wcs: 0,
            worl: 0,
            woru: 0,
            wcvl: 0,
            wcvu: 0,
        }
    }
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot", irq = "manual", before_unrealize = [lower_irq, stop_timer])]
pub struct SbsaGwdt {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: parking_lot::Mutex<SbsaGwdtRegs>,
    irq: parking_lot::Mutex<Option<InterruptSource>>,
    clock_frequency: u32,
    timer: Arc<Ptimer>,
}

impl SbsaGwdt {
    pub fn new() -> Arc<Self> {
        Self::new_named("sbsa-gwdt")
    }

    pub fn new_with_clock_frequency(clock_frequency: u32) -> Arc<Self> {
        Self::new_named_with_clock_frequency("sbsa-gwdt", clock_frequency)
    }

    pub fn new_named(local_id: &str) -> Arc<Self> {
        Self::new_named_with_clock_frequency(local_id, DEFAULT_CLOCK_FREQUENCY)
    }

    pub fn new_named_with_clock_frequency(
        local_id: &str,
        clock_frequency: u32,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak: &std::sync::Weak<Self>| {
            let weak = weak.clone();
            let callback: PtimerCallback = Arc::new(move || {
                if let Some(wdt) = weak.upgrade() {
                    wdt.handle_timeout();
                }
            });
            Self {
                state: parking_lot::Mutex::new(SysBusDeviceState::new(
                    local_id,
                )),
                regs: parking_lot::Mutex::new(SbsaGwdtRegs::default()),
                irq: parking_lot::Mutex::new(None),
                clock_frequency,
                timer: Ptimer::new(Some(callback), 0),
            }
        })
    }

    pub fn reset_runtime(&self) {
        *self.regs.lock() = SbsaGwdtRegs::default();
        self.stop_timer();
        self.lower_irq();
    }

    pub fn connect_irq(&self, irq: InterruptSource) {
        *self.irq.lock() = Some(irq);
    }

    pub fn refresh_read(&self, offset: u64, size: u32) -> u64 {
        if size != 4 {
            return 0;
        }
        let regs = self.regs.lock();
        match offset {
            WRR => 0,
            W_IIDR => u64::from(regs.id),
            _ => 0,
        }
    }

    pub fn refresh_write(&self, offset: u64, size: u32, _val: u64) {
        if size != 4 || offset != WRR {
            return;
        }
        self.regs.lock().wcs &= !(WCS_WS0 | WCS_WS1);
        self.update_timer(RefreshType::Explicit);
    }

    pub fn control_read(&self, offset: u64, size: u32) -> u64 {
        if size != 4 {
            return 0;
        }
        let regs = self.regs.lock();
        match offset {
            WCS => u64::from(regs.wcs),
            WOR => u64::from(regs.worl),
            WORU => u64::from(regs.woru),
            WCV => u64::from(regs.wcvl),
            WCVU => u64::from(regs.wcvu),
            W_IIDR => u64::from(regs.id),
            _ => 0,
        }
    }

    pub fn control_write(&self, offset: u64, size: u32, val: u64) {
        if size != 4 {
            return;
        }
        let value = val as u32;
        match offset {
            WCS => {
                self.regs.lock().wcs = value & WCS_EN;
                self.lower_irq();
                self.update_timer(RefreshType::Explicit);
            }
            WOR => {
                let mut regs = self.regs.lock();
                regs.worl = value;
                regs.wcs &= !(WCS_WS0 | WCS_WS1);
                drop(regs);
                self.lower_irq();
                self.update_timer(RefreshType::Explicit);
            }
            WORU => {
                let mut regs = self.regs.lock();
                regs.woru = value & WORU_MASK;
                regs.wcs &= !(WCS_WS0 | WCS_WS1);
                drop(regs);
                self.lower_irq();
                self.update_timer(RefreshType::Explicit);
            }
            WCV => self.regs.lock().wcvl = value,
            WCVU => self.regs.lock().wcvu = value,
            _ => {}
        }
    }

    pub fn trigger_timeout(&self) {
        self.handle_timeout();
    }

    pub fn step_timer(&self, ticks: u64) -> u64 {
        self.timer.step(ticks)
    }

    fn handle_timeout(&self) {
        let mut raise = false;
        let mut stop = false;
        {
            let mut regs = self.regs.lock();
            if regs.wcs & WCS_EN == 0 {
                return;
            }
            if regs.wcs & WCS_WS0 == 0 {
                regs.wcs |= WCS_WS0;
                raise = true;
            } else {
                regs.wcs |= WCS_WS1;
                stop = true;
            }
        }
        if stop {
            self.stop_timer();
        } else {
            self.update_timer(RefreshType::Timeout);
        }
        if raise {
            self.raise_irq();
        }
    }

    fn update_timer(&self, refresh_type: RefreshType) {
        let (enabled, offset) = {
            let mut regs = self.regs.lock();
            let enabled = regs.wcs & WCS_EN != 0;
            let offset = (u64::from(regs.woru) << 32) | u64::from(regs.worl);
            if enabled
                && (refresh_type == RefreshType::Explicit
                    || regs.wcs & WCS_WS0 == 0)
            {
                let timeout =
                    watchdog_ticks_to_virtual_ns(offset, self.clock_frequency);
                regs.wcvl = timeout as u32;
                regs.wcvu = (timeout >> 32) as u32;
            }
            (enabled, offset)
        };

        self.timer.begin();
        self.timer.stop();
        if enabled && offset != 0 {
            self.timer.set_freq(self.clock_frequency);
            self.timer.set_limit(offset, true);
            self.timer.run(false);
        }
        self.timer.commit();
    }

    fn stop_timer(&self) {
        self.timer.begin();
        self.timer.stop();
        self.timer.commit();
    }

    fn raise_irq(&self) {
        if let Some(ref irq) = *self.irq.lock() {
            irq.raise();
        }
    }

    fn lower_irq(&self) {
        if let Some(ref irq) = *self.irq.lock() {
            irq.lower();
        }
    }
}

fn watchdog_ticks_to_virtual_ns(ticks: u64, freq_hz: u32) -> u64 {
    if freq_hz == 0 {
        return 0;
    }
    let ns = u128::from(ticks) * u128::from(NANOSECONDS_PER_SECOND)
        / u128::from(freq_hz);
    ns.min(u128::from(u64::MAX)) as u64
}

pub struct SbsaGwdtRefreshMmio(pub Arc<SbsaGwdt>);

impl MmioOps for SbsaGwdtRefreshMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.refresh_read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.refresh_write(offset, size, val);
    }
}

pub struct SbsaGwdtControlMmio(pub Arc<SbsaGwdt>);

impl MmioOps for SbsaGwdtControlMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.control_read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.control_write(offset, size, val);
    }
}
