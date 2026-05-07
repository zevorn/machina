use std::sync::Arc;

use machina_core::device_cell::DeviceRefCell;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_memory::region::MmioOps;

const RTC_TIME_LOW: u64 = 0x00;
const RTC_TIME_HIGH: u64 = 0x04;
const RTC_ALARM_LOW: u64 = 0x08;
const RTC_ALARM_HIGH: u64 = 0x0c;
const RTC_IRQ_ENABLED: u64 = 0x10;
const RTC_CLEAR_ALARM: u64 = 0x14;
const RTC_ALARM_STATUS: u64 = 0x18;
const RTC_CLEAR_INTERRUPT: u64 = 0x1c;

struct GoldfishRtcRegs {
    /// Offset from rtc_clock nanoseconds to guest-visible time.
    tick_offset: i64,
    /// Alarm target in nanoseconds.
    alarm_next: u64,
    /// Whether alarm timer is running.
    alarm_running: u32,
    /// IRQ pending flag.
    irq_pending: u32,
    /// IRQ enable flag.
    irq_enabled: u32,
    /// Cached high 32 bits of time from last TIME_LOW read.
    time_high: u32,
}

impl GoldfishRtcRegs {
    fn new() -> Self {
        Self {
            tick_offset: 0,
            alarm_next: 0,
            alarm_running: 0,
            irq_pending: 0,
            irq_enabled: 0,
            time_high: 0,
        }
    }

    fn reset(&mut self) {
        self.alarm_next = 0;
        self.alarm_running = 0;
        self.irq_pending = 0;
        self.irq_enabled = 0;
        self.time_high = 0;
    }

    fn current_tick(&self) -> u64 {
        // time = tick_offset + 0 (no real rtc_clock, offset is the time)
        self.tick_offset as u64
    }

    fn update(&self) -> bool {
        (self.irq_pending & self.irq_enabled) != 0
    }

    fn check_alarm(&mut self) -> bool {
        let ticks = self.current_tick();
        if self.alarm_next <= ticks && self.alarm_running != 0 {
            self.alarm_running = 0;
            self.irq_pending = 1;
        }
        self.update()
    }
}

pub struct GoldfishRtc {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<GoldfishRtcRegs>,
    output: parking_lot::Mutex<Option<InterruptSource>>,
}

impl GoldfishRtc {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("goldfish_rtc")
    }

    #[must_use]
    pub fn new_named(local_id: &str) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRefCell::new(GoldfishRtcRegs::new()),
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

    fn update_irq(&self, flags: bool) {
        if let Some(ref line) = *self.output.lock() {
            line.set(flags);
        }
    }

    /// Advance time by `ns` nanoseconds and check alarm.
    pub fn tick(&self, ns: u64) {
        let mut regs = self.regs.borrow();
        regs.tick_offset = regs.tick_offset.wrapping_add(ns as i64);
        let flags = regs.check_alarm();
        drop(regs);
        self.update_irq(flags);
    }

    /// Get current time in nanoseconds.
    #[must_use]
    pub fn current_time(&self) -> u64 {
        self.regs.borrow().current_tick()
    }

    /// Set absolute time.
    pub fn set_time(&self, ns: u64) {
        self.regs.borrow().tick_offset = ns as i64;
    }
}

impl Default for GoldfishRtc {
    fn default() -> Self {
        Self::new()
    }
}

pub struct GoldfishRtcMmio(pub Arc<GoldfishRtc>);

impl MmioOps for GoldfishRtcMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        if size != 4 {
            return 0;
        }

        let mut regs = self.0.regs.borrow();
        match offset {
            RTC_TIME_LOW => {
                let tick = regs.current_tick();
                regs.time_high = (tick >> 32) as u32;
                tick & 0xFFFF_FFFF
            }
            RTC_TIME_HIGH => u64::from(regs.time_high),
            RTC_ALARM_LOW => regs.alarm_next & 0xFFFF_FFFF,
            RTC_ALARM_HIGH => regs.alarm_next >> 32,
            RTC_IRQ_ENABLED => u64::from(regs.irq_enabled),
            RTC_ALARM_STATUS => u64::from(regs.alarm_running),
            _ => 0,
        }
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        if size != 4 {
            return;
        }

        let value = val as u32;
        match offset {
            RTC_TIME_LOW => {
                let mut regs = self.0.regs.borrow();
                let current = regs.current_tick();
                let new_tick = (current & !0xFFFF_FFFFu64) | u64::from(value);
                regs.tick_offset += new_tick as i64 - current as i64;
            }
            RTC_TIME_HIGH => {
                let mut regs = self.0.regs.borrow();
                let current = regs.current_tick();
                let new_tick =
                    (u64::from(value) << 32) | (current & 0xFFFF_FFFF);
                regs.tick_offset += new_tick as i64 - current as i64;
            }
            RTC_ALARM_LOW => {
                let mut regs = self.0.regs.borrow();
                regs.alarm_next =
                    (regs.alarm_next & !0xFFFF_FFFFu64) | u64::from(value);
                regs.alarm_running = 1;
                let flags = regs.check_alarm();
                drop(regs);
                self.0.update_irq(flags);
            }
            RTC_ALARM_HIGH => {
                let mut regs = self.0.regs.borrow();
                regs.alarm_next =
                    (u64::from(value) << 32) | (regs.alarm_next & 0xFFFF_FFFF);
            }
            RTC_IRQ_ENABLED => {
                let mut regs = self.0.regs.borrow();
                regs.irq_enabled = value & 0x1;
                let flags = regs.update();
                drop(regs);
                self.0.update_irq(flags);
            }
            RTC_CLEAR_ALARM => {
                let mut regs = self.0.regs.borrow();
                regs.alarm_running = 0;
                let flags = regs.update();
                drop(regs);
                self.0.update_irq(flags);
            }
            RTC_CLEAR_INTERRUPT => {
                let mut regs = self.0.regs.borrow();
                regs.irq_pending = 0;
                let flags = regs.update();
                drop(regs);
                self.0.update_irq(flags);
            }
            _ => {}
        }
    }
}
