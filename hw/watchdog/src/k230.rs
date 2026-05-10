use std::sync::Arc;

use machina_core::device_cell::DeviceRegs;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_hw_timer::{policy, Ptimer, PtimerCallback};
use machina_memory::region::MmioOps;

pub const CR: u64 = 0x00;
pub const TORR: u64 = 0x04;
pub const CCVR: u64 = 0x08;
pub const CRR: u64 = 0x0c;
pub const STAT: u64 = 0x10;
pub const EOI: u64 = 0x14;
pub const PROT_LEVEL: u64 = 0x1c;
pub const COMP_PARAM_5: u64 = 0xe4;
pub const COMP_PARAM_4: u64 = 0xe8;
pub const COMP_PARAM_3: u64 = 0xec;
pub const COMP_PARAM_2: u64 = 0xf0;
pub const COMP_PARAM_1: u64 = 0xf4;
pub const COMP_VERSION: u64 = 0xf8;
pub const COMP_TYPE: u64 = 0xfc;
pub const MMIO_SIZE: u64 = 0x100;

pub const CR_RPL_MASK: u32 = 0x7;
pub const CR_RPL_SHIFT: u32 = 2;
pub const CR_RMOD: u32 = 1 << 1;
pub const CR_WDT_EN: u32 = 1 << 0;
pub const TORR_TOP_MASK: u32 = 0xf;
pub const STAT_INT: u32 = 1 << 0;
pub const CRR_RESTART: u32 = 0x76;

pub const DEFAULT_FREQ: u32 = 32_768;
pub const RPL_16_CYCLES: u32 = 0x3;
pub const CNT_WIDTH_SHIFT: u32 = 24;
pub const DFLT_TOP_INIT_SHIFT: u32 = 20;
pub const DFLT_TOP_SHIFT: u32 = 16;
pub const DFLT_RPL_SHIFT: u32 = 10;
pub const APB_DATA_WIDTH_SHIFT: u32 = 8;
pub const USE_FIX_TOP: u32 = 1 << 6;
pub const COMP_PARAM_1_VAL: u32 = (32 << CNT_WIDTH_SHIFT)
    | (RPL_16_CYCLES << DFLT_RPL_SHIFT)
    | (2 << APB_DATA_WIDTH_SHIFT)
    | USE_FIX_TOP;
pub const COMP_TYPE_VAL: u32 = 0x4457_0120;
pub const COMP_VERSION_VAL: u32 = 0x3131_302a;

#[derive(Clone, Copy)]
struct K230WdtRegs {
    cr: u32,
    torr: u32,
    current_count: u32,
    stat: u32,
    prot_level: u32,
    timeout_value: u32,
    interrupt_pending: bool,
    enabled: bool,
}

impl Default for K230WdtRegs {
    fn default() -> Self {
        Self {
            cr: 0,
            torr: 0,
            current_count: u32::MAX,
            stat: 0,
            prot_level: 0x2,
            timeout_value: 0,
            interrupt_pending: false,
            enabled: false,
        }
    }
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot", irq = "manual", before_unrealize = [lower_irq, stop_timer])]
pub struct K230Wdt {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRegs<K230WdtRegs>,
    irq: parking_lot::Mutex<Option<InterruptSource>>,
    timer: Arc<Ptimer>,
}

impl K230Wdt {
    pub fn new() -> Arc<Self> {
        Self::new_named("k230-wdt")
    }

    pub fn new_named(local_id: &str) -> Arc<Self> {
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
                regs: DeviceRegs::new(K230WdtRegs::default()),
                irq: parking_lot::Mutex::new(None),
                timer: Ptimer::new(
                    Some(callback),
                    policy::NO_IMMEDIATE_TRIGGER
                        | policy::NO_IMMEDIATE_RELOAD
                        | policy::NO_COUNTER_ROUND_DOWN,
                ),
            }
        })
    }

    pub fn reset_runtime(&self) {
        *self.regs.lock() = K230WdtRegs::default();
        self.stop_timer();
        self.lower_irq();
    }

    pub fn connect_irq(&self, irq: InterruptSource) {
        *self.irq.lock() = Some(irq);
    }

    pub fn read(&self, offset: u64, size: u32) -> u64 {
        if size != 4 {
            return 0;
        }

        let regs = self.regs.lock();
        let value = match offset {
            CR => regs.cr,
            TORR => regs.torr,
            CCVR if regs.enabled => self.timer.get_count() as u32,
            CCVR => regs.current_count,
            STAT => regs.stat,
            PROT_LEVEL => regs.prot_level,
            COMP_PARAM_5 | COMP_PARAM_4 | COMP_PARAM_3 => 0,
            COMP_PARAM_2 => u32::MAX,
            COMP_PARAM_1 => COMP_PARAM_1_VAL,
            COMP_VERSION => COMP_VERSION_VAL,
            COMP_TYPE => COMP_TYPE_VAL,
            _ => 0,
        };
        u64::from(value)
    }

    pub fn write(&self, offset: u64, size: u32, val: u64) {
        if size != 4 {
            return;
        }

        let value = val as u32;
        match offset {
            CR => {
                {
                    let mut regs = self.regs.lock();
                    regs.cr = value
                        & ((CR_RPL_MASK << CR_RPL_SHIFT) | CR_RMOD | CR_WDT_EN);
                    regs.enabled = regs.cr & CR_WDT_EN != 0;
                }
                self.update_timer();
            }
            TORR => {
                {
                    let mut regs = self.regs.lock();
                    regs.torr = value & TORR_TOP_MASK;
                    regs.timeout_value = calculate_timeout(regs.torr);
                    regs.current_count = regs.timeout_value;
                }
                self.update_timer();
            }
            CRR if value & 0xff == CRR_RESTART => {
                {
                    let mut regs = self.regs.lock();
                    regs.current_count = regs.timeout_value;
                }
                self.clear_interrupt();
                self.update_timer();
            }
            EOI => self.clear_interrupt(),
            PROT_LEVEL => self.regs.lock().prot_level = value & 0x7,
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
        {
            let mut regs = self.regs.lock();
            if !regs.enabled {
                return;
            }
            if regs.cr & CR_RMOD != 0 {
                regs.stat |= STAT_INT;
                regs.interrupt_pending = true;
                raise = true;
            }
            regs.current_count = regs.timeout_value;
        }

        self.update_timer();
        if raise {
            self.raise_irq();
        }
    }

    fn update_timer(&self) {
        let (enabled, timeout_value, current_count) = {
            let regs = self.regs.lock();
            (regs.enabled, regs.timeout_value, regs.current_count)
        };

        self.timer.begin();
        self.timer.stop();
        if enabled && timeout_value > 0 {
            self.timer.set_freq(DEFAULT_FREQ);
            self.timer.set_limit(u64::from(current_count), true);
            self.timer.run(false);
        }
        self.timer.commit();
    }

    fn stop_timer(&self) {
        self.timer.begin();
        self.timer.stop();
        self.timer.commit();
    }

    fn clear_interrupt(&self) {
        {
            let mut regs = self.regs.lock();
            regs.stat &= !STAT_INT;
            regs.interrupt_pending = false;
        }
        self.lower_irq();
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

fn calculate_timeout(top_value: u32) -> u32 {
    if top_value <= 15 {
        1 << (16 + top_value)
    } else {
        1 << 31
    }
}

pub struct K230WdtMmio(pub Arc<K230Wdt>);

impl MmioOps for K230WdtMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.write(offset, size, val);
    }
}
