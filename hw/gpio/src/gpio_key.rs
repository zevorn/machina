// GPIO key device.
//
// Models a human keypress: when the GPIO input is asserted,
// the IRQ output is raised for 100ms (virtual time) before
// being lowered again.

use std::sync::Mutex;

use machina_accel::timer::VirtualClock;
use machina_hw_core::irq::IrqLine;

const GPIO_KEY_LATENCY_NS: i64 = 100_000_000; // 100ms

pub struct GpioKey {
    irq: IrqLine,
    clock: std::sync::Arc<VirtualClock>,
    timer_id: Mutex<Option<u64>>,
}

impl GpioKey {
    pub fn new(irq: IrqLine, clock: std::sync::Arc<VirtualClock>) -> Self {
        Self {
            irq,
            clock,
            timer_id: Mutex::new(None),
        }
    }

    /// GPIO input: assert to trigger a keypress.
    ///
    /// On assertion, raises IRQ and schedules a 100ms timer
    /// to lower it.
    pub fn set_gpio(&self, level: bool) {
        if !level {
            return;
        }
        // Cancel any previous timer
        self.cancel_timer();

        self.irq.raise();

        let irq = self.irq.clone();
        let clock = self.clock.clone();
        let expire = self.clock.get_ns() + GPIO_KEY_LATENCY_NS;
        let id = clock.add_timer(expire, move || {
            irq.lower();
        });

        *self.timer_id.lock().unwrap() = Some(id);
    }

    fn cancel_timer(&self) {
        let mut tid = self.timer_id.lock().unwrap();
        if let Some(id) = *tid {
            self.clock.remove_timer(id);
            *tid = None;
        }
    }
}

impl Drop for GpioKey {
    fn drop(&mut self) {
        self.cancel_timer();
    }
}
