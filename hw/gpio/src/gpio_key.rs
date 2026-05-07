// GPIO key device.
//
// Models a human keypress: every GPIO input callback raises the
// outbound IRQ and schedules/rearms a 100ms virtual timer, matching
// the reference behavior. Reset cancels the pending timer without
// lowering the IRQ line.

use std::sync::Arc;

use machina_accel::timer::VirtualClock;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::IrqLine;

const GPIO_KEY_LATENCY_NS: i64 = 100_000_000; // 100ms

pub struct GpioKey {
    state: parking_lot::Mutex<SysBusDeviceState>,
    irq: IrqLine,
    clock: Arc<VirtualClock>,
    timer_id: parking_lot::Mutex<Option<u64>>,
}

impl GpioKey {
    pub fn new(irq: IrqLine, clock: Arc<VirtualClock>) -> Arc<Self> {
        Self::new_named("gpio_key", irq, clock)
    }

    pub fn new_named(
        local_id: &str,
        irq: IrqLine,
        clock: Arc<VirtualClock>,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            irq,
            clock,
            timer_id: parking_lot::Mutex::new(None),
        })
    }

    /// GPIO input: every callback models a keypress.
    ///
    /// Raises IRQ and schedules/rearms the 100ms timer regardless
    /// of the input level.
    pub fn set_gpio(&self, _level: bool) {
        self.cancel_timer();
        self.irq.raise();

        let irq = self.irq.clone();
        let clock = self.clock.clone();
        let expire = self.clock.get_ns() + GPIO_KEY_LATENCY_NS;
        let id = clock.add_timer(expire, move || {
            irq.lower();
        });

        *self.timer_id.lock() = Some(id);
    }

    /// Reset: cancel the pending timer without lowering IRQ.
    pub fn reset_runtime(&self) {
        self.cancel_timer();
    }

    machina_hw_core::machina_parking_lot_sysbus_child_accessors!(state);

    fn cancel_timer(&self) {
        let mut tid = self.timer_id.lock();
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
