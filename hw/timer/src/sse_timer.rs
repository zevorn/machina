use std::sync::Arc;

use machina_core::device_cell::DeviceRefCell;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_memory::region::MmioOps;

use super::sse_counter::SseCounter;

const A_CNTPCT_LO: u64 = 0x00;
const A_CNTPCT_HI: u64 = 0x04;
const A_CNTFRQ: u64 = 0x10;
const A_CNTP_CVAL_LO: u64 = 0x20;
const A_CNTP_CVAL_HI: u64 = 0x24;
const A_CNTP_TVAL: u64 = 0x28;
const A_CNTP_CTL: u64 = 0x2C;
const A_CNTP_AIVAL_LO: u64 = 0x40;
const A_CNTP_AIVAL_HI: u64 = 0x44;
const A_CNTP_AIVAL_RELOAD: u64 = 0x48;
const A_CNTP_AIVAL_CTL: u64 = 0x4C;
const A_CNTP_CFG: u64 = 0x50;
const A_PID4: u64 = 0xFD0;

// CNTP_CTL fields
const CTL_ENABLE: u32 = 1 << 0;
const CTL_IMASK: u32 = 1 << 1;
const CTL_ISTATUS: u32 = 1 << 2;

// CNTP_AIVAL_CTL fields
const AIVAL_CTL_EN: u32 = 1 << 0;
const AIVAL_CTL_CLR: u32 = 1 << 1;

const TIMER_ID: [u8; 12] = [
    0x04, 0x00, 0x00, 0x00, // PID4..7
    0xB7, 0xB0, 0x0B, 0x00, // PID0..3
    0x0D, 0xF0, 0x05, 0xB1, // CID0..3
];

struct SseTimerRegs {
    cntp_ctl: u32,
    cntp_cval: u64,
    cntp_aival: u64,
    cntp_aival_ctl: u32,
    cntp_aival_reload: u32,
    cntfrq: u32,
}

impl SseTimerRegs {
    fn new() -> Self {
        Self {
            cntp_ctl: 0,
            cntp_cval: 0,
            cntp_aival: 0,
            cntp_aival_ctl: 0,
            cntp_aival_reload: 0,
            cntfrq: 0,
        }
    }

    fn reset(&mut self) {
        self.cntp_ctl = 0;
        self.cntp_cval = 0;
        self.cntp_aival = 0;
        self.cntp_aival_ctl = 0;
        self.cntp_aival_reload = 0;
        self.cntfrq = 0;
    }

    fn enabled(&self) -> bool {
        (self.cntp_ctl & CTL_ENABLE) != 0
    }

    fn is_autoinc(&self) -> bool {
        (self.cntp_aival_ctl & AIVAL_CTL_EN) != 0
    }

    fn timer_status(&self, counter_val: u64) -> bool {
        if !self.enabled() {
            return false;
        }
        if self.is_autoinc() {
            (self.cntp_aival_ctl & AIVAL_CTL_CLR) != 0
        } else {
            counter_val >= self.cntp_cval
        }
    }
}

pub struct SseTimer {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<SseTimerRegs>,
    counter: Arc<SseCounter>,
    output: parking_lot::Mutex<Option<InterruptSource>>,
}

impl SseTimer {
    #[must_use]
    pub fn new(counter: Arc<SseCounter>) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new("sse_timer")),
            regs: DeviceRefCell::new(SseTimerRegs::new()),
            counter,
            output: parking_lot::Mutex::new(None),
        }
    }

    machina_hw_core::machina_parking_lot_sysbus_accessors!(
        state,
        before_unrealize = lower_outputs
    );

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
        let counter_val = self.counter.counter_value();
        let status = self.regs.borrow().timer_status(counter_val);
        let imasked = (self.regs.borrow().cntp_ctl & CTL_IMASK) != 0;
        let irq = status && !imasked;
        if let Some(ref line) = *self.output.lock() {
            line.set(irq);
        }
    }

    /// Advance time and check timer conditions.
    pub fn tick(&self) {
        // Check if auto-increment condition is met
        let mut regs = self.regs.borrow();
        let counter_val = self.counter.counter_value();
        if regs.is_autoinc() && regs.enabled() && counter_val >= regs.cntp_aival
        {
            regs.cntp_aival_ctl |= AIVAL_CTL_CLR;
            regs.cntp_aival = counter_val + u64::from(regs.cntp_aival_reload);
        }
        // Update ISTATUS
        if regs.timer_status(counter_val) {
            regs.cntp_ctl |= CTL_ISTATUS;
        } else {
            regs.cntp_ctl &= !CTL_ISTATUS;
        }
        drop(regs);
        self.update_irq();
    }
}

impl Default for SseTimer {
    fn default() -> Self {
        Self::new(Arc::new(SseCounter::new()))
    }
}

fn read_id(offset: u64) -> u64 {
    let idx = ((offset - A_PID4) / 4) as usize;
    if idx < TIMER_ID.len() {
        u64::from(TIMER_ID[idx])
    } else {
        0
    }
}

pub struct SseTimerMmio(pub Arc<SseTimer>);

impl MmioOps for SseTimerMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        if size != 4 {
            return 0;
        }

        let counter_val = self.0.counter.counter_value();
        let regs = self.0.regs.borrow();
        match offset {
            A_CNTPCT_LO => counter_val & 0xFFFF_FFFF,
            A_CNTPCT_HI => (counter_val >> 32) & 0xFFFF_FFFF,
            A_CNTFRQ => u64::from(regs.cntfrq),
            A_CNTP_CVAL_LO => regs.cntp_cval & 0xFFFF_FFFF,
            A_CNTP_CVAL_HI => (regs.cntp_cval >> 32) & 0xFFFF_FFFF,
            A_CNTP_TVAL => {
                let diff = regs.cntp_cval.wrapping_sub(counter_val);
                diff & 0xFFFF_FFFF
            }
            A_CNTP_CTL => {
                let mut val = regs.cntp_ctl;
                if regs.timer_status(counter_val) {
                    val |= CTL_ISTATUS;
                }
                u64::from(val)
            }
            A_CNTP_AIVAL_LO => regs.cntp_aival & 0xFFFF_FFFF,
            A_CNTP_AIVAL_HI => (regs.cntp_aival >> 32) & 0xFFFF_FFFF,
            A_CNTP_AIVAL_RELOAD => u64::from(regs.cntp_aival_reload),
            A_CNTP_AIVAL_CTL => u64::from(regs.cntp_aival_ctl),
            A_CNTP_CFG => 1, // AIVAL implemented
            A_PID4..=0xFFC => read_id(offset),
            _ => 0,
        }
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        if size != 4 {
            return;
        }

        let mut regs = self.0.regs.borrow();
        match offset {
            A_CNTFRQ => {
                regs.cntfrq = val as u32;
            }
            A_CNTP_CVAL_LO => {
                regs.cntp_cval = (regs.cntp_cval & 0xFFFF_FFFF_0000_0000)
                    | (val & 0xFFFF_FFFF);
                drop(regs);
                self.0.tick();
                return;
            }
            A_CNTP_CVAL_HI => {
                regs.cntp_cval = (regs.cntp_cval & 0xFFFF_FFFF)
                    | ((val & 0xFFFF_FFFF) << 32);
                drop(regs);
                self.0.tick();
                return;
            }
            A_CNTP_TVAL => {
                let counter_val = self.0.counter.counter_value();
                let tval = val as u32 as i32;
                let new_cval = counter_val.wrapping_add(tval as i64 as u64);
                regs.cntp_cval = new_cval;
                drop(regs);
                self.0.tick();
                return;
            }
            A_CNTP_CTL => {
                let value = val as u32;
                let old_ctl = regs.cntp_ctl;
                let new_ctl = value & (CTL_ENABLE | CTL_IMASK);
                regs.cntp_ctl = new_ctl;
                let enabled_changed = (old_ctl ^ new_ctl) & CTL_ENABLE != 0;
                if enabled_changed && regs.enabled() && regs.is_autoinc() {
                    let counter_val = self.0.counter.counter_value();
                    regs.cntp_aival =
                        counter_val + u64::from(regs.cntp_aival_reload);
                }
            }
            A_CNTP_AIVAL_RELOAD => {
                regs.cntp_aival_reload = val as u32;
            }
            A_CNTP_AIVAL_CTL => {
                let value = val as u32;
                let old_ctl = regs.cntp_aival_ctl;
                // EN bit writable, CLR write-0-to-clear
                regs.cntp_aival_ctl &= !AIVAL_CTL_EN;
                regs.cntp_aival_ctl |= value & AIVAL_CTL_EN;
                if (value & AIVAL_CTL_CLR) == 0 {
                    regs.cntp_aival_ctl &= !AIVAL_CTL_CLR;
                }
                let en_changed =
                    (old_ctl ^ regs.cntp_aival_ctl) & AIVAL_CTL_EN != 0;
                if en_changed && regs.enabled() && regs.is_autoinc() {
                    let counter_val = self.0.counter.counter_value();
                    regs.cntp_aival =
                        counter_val + u64::from(regs.cntp_aival_reload);
                }
            }
            // RO registers: ignore
            _ => {}
        }
        drop(regs);
        self.0.tick();
    }
}
