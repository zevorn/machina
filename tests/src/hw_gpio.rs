// Tests for hw/gpio devices: gpio_key, gpio_pwr.

use std::sync::{Arc, Mutex};

use machina_accel::timer::{ClockType, VirtualClock};
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{IrqLine, IrqSink};
use machina_hw_gpio::{GpioKey, GpioPwr, GpioPwrAction};

struct TestSink {
    level: Mutex<bool>,
    calls: Mutex<u32>,
}

impl TestSink {
    fn new() -> Self {
        Self {
            level: Mutex::new(false),
            calls: Mutex::new(0),
        }
    }

    fn level(&self) -> bool {
        *self.level.lock().unwrap()
    }

    fn call_count(&self) -> u32 {
        *self.calls.lock().unwrap()
    }
}

impl IrqSink for TestSink {
    fn set_irq(&self, _irq: u32, level: bool) {
        *self.level.lock().unwrap() = level;
        *self.calls.lock().unwrap() += 1;
    }
}

// ---- GpioKey ----

#[test]
fn test_gpio_key_trigger_raises_irq() {
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let sink = Arc::new(TestSink::new());
    let irq = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0);

    let key = GpioKey::new(irq, clock.clone());
    key.set_gpio(true);
    assert!(sink.level());
}

#[test]
fn test_gpio_key_trigger_on_low_level() {
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let sink = Arc::new(TestSink::new());
    let irq = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0);

    let key = GpioKey::new(irq, clock.clone());
    // Even low level triggers (per QEMU reference)
    key.set_gpio(false);
    assert!(sink.level());
}

#[test]
fn test_gpio_key_irq_lowers_after_timer() {
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let sink = Arc::new(TestSink::new());
    let irq = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0);

    let key = GpioKey::new(irq, clock.clone());
    key.set_gpio(true);
    assert!(sink.level());

    // Advance past 100ms
    clock.step(200_000_000);
    assert!(!sink.level());
}

#[test]
fn test_gpio_key_multiple_presses() {
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let sink = Arc::new(TestSink::new());
    let irq = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0);

    let key = GpioKey::new(irq, clock.clone());
    key.set_gpio(true);
    assert!(sink.level());

    // Second press before timer expires (rearms timer)
    clock.step(50_000_000);
    key.set_gpio(true);
    assert!(sink.level());

    // 60ms from retrigger — timer hasn't fired yet
    clock.step(60_000_000);
    assert!(sink.level());

    // Past the retriggered timer
    clock.step(50_000_000);
    assert!(!sink.level());
}

#[test]
fn test_gpio_key_reset_cancels_timer() {
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let sink = Arc::new(TestSink::new());
    let irq = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0);

    let key = GpioKey::new(irq, clock.clone());
    key.set_gpio(true);
    assert!(sink.level());

    // Reset cancels timer without lowering IRQ
    key.reset_runtime();
    assert!(sink.level());

    // Advance past 100ms — timer was cancelled, IRQ stays high
    clock.step(200_000_000);
    assert!(sink.level());
}

#[test]
fn test_gpio_key_lifecycle() {
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let sink = Arc::new(TestSink::new());
    let irq = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0);

    let key = GpioKey::new(irq, clock.clone());
    assert!(!key.realized());

    let mut bus = SysBus::new("sysbus");
    key.attach_to_bus(&mut bus).unwrap();
    key.realize().unwrap();
    assert!(key.realized());

    let err = key.realize().unwrap_err();
    assert!(err.to_string().contains("already realized"));

    key.unrealize().unwrap();
    assert!(!key.realized());
}

// ---- GpioPwr ----

#[test]
fn test_gpio_pwr_reset_on_rising_edge() {
    let pwr = GpioPwr::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    pwr.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    pwr.gpio_reset(true);
    assert_eq!(*actions.lock().unwrap(), vec![GpioPwrAction::Reset]);
}

#[test]
fn test_gpio_pwr_reset_low_does_nothing() {
    let pwr = GpioPwr::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    pwr.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    pwr.gpio_reset(false);
    assert!(actions.lock().unwrap().is_empty());
}

#[test]
fn test_gpio_pwr_shutdown_on_rising_edge() {
    let pwr = GpioPwr::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    pwr.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    pwr.gpio_shutdown(true);
    assert_eq!(*actions.lock().unwrap(), vec![GpioPwrAction::Shutdown]);
}

#[test]
fn test_gpio_pwr_shutdown_low_does_nothing() {
    let pwr = GpioPwr::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    pwr.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    pwr.gpio_shutdown(false);
    assert!(actions.lock().unwrap().is_empty());
}

#[test]
fn test_gpio_pwr_no_handler_safe() {
    let pwr = GpioPwr::new();
    pwr.gpio_reset(true);
    pwr.gpio_shutdown(true);
}

#[test]
fn test_gpio_pwr_lifecycle() {
    let pwr = GpioPwr::new();
    assert!(!pwr.realized());

    let mut bus = SysBus::new("sysbus");
    pwr.attach_to_bus(&mut bus).unwrap();
    pwr.realize().unwrap();
    assert!(pwr.realized());

    let err = pwr.realize().unwrap_err();
    assert!(err.to_string().contains("already realized"));

    pwr.unrealize().unwrap();
    assert!(!pwr.realized());
}
