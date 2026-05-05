// Tests for hw/gpio devices: gpio_key and gpio_pwr.

use std::sync::{Arc, Mutex};

use machina_accel::timer::{ClockType, VirtualClock};
use machina_hw_core::irq::{IrqLine, IrqSink};
use machina_hw_gpio::{GpioKey, GpioPwr, GpioPwrAction};

// Test sink that records IRQ transitions
struct TestIrqSink {
    events: Mutex<Vec<(u32, bool)>>,
}

impl TestIrqSink {
    fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    fn events(&self) -> Vec<(u32, bool)> {
        self.events.lock().unwrap().clone()
    }
}

impl IrqSink for TestIrqSink {
    fn set_irq(&self, irq: u32, level: bool) {
        self.events.lock().unwrap().push((irq, level));
    }
}

fn make_irq_line(sink: Arc<TestIrqSink>, irq: u32) -> IrqLine {
    IrqLine::new(sink, irq)
}

// ---- GpioKey ----

#[test]
fn test_gpio_key_new() {
    let sink = Arc::new(TestIrqSink::new());
    let irq = make_irq_line(sink.clone(), 0);
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let _key = GpioKey::new(irq, clock);
}

#[test]
fn test_gpio_key_assert_raises_irq() {
    let sink = Arc::new(TestIrqSink::new());
    let irq = make_irq_line(sink.clone(), 5);
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let key = GpioKey::new(irq, clock);

    key.set_gpio(true);

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], (5, true)); // IRQ 5 raised
}

#[test]
fn test_gpio_key_deassert_does_nothing() {
    let sink = Arc::new(TestIrqSink::new());
    let irq = make_irq_line(sink.clone(), 5);
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let key = GpioKey::new(irq, clock);

    // Deassert without prior assert should have no effect
    key.set_gpio(false);
    assert!(sink.events().is_empty());
}

#[test]
fn test_gpio_key_irq_lowered_after_latency() {
    let sink = Arc::new(TestIrqSink::new());
    let irq = make_irq_line(sink.clone(), 3);
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let key = GpioKey::new(irq, clock.clone());

    key.set_gpio(true);

    // IRQ should be raised
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], (3, true));

    // Advance time by 100ms — the timer should fire and lower IRQ
    clock.step(100_000_000); // 100ms in ns

    let events = sink.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], (3, true));
    assert_eq!(events[1], (3, false));
}

#[test]
fn test_gpio_key_irq_not_lowered_before_latency() {
    let sink = Arc::new(TestIrqSink::new());
    let irq = make_irq_line(sink.clone(), 3);
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let key = GpioKey::new(irq, clock.clone());

    key.set_gpio(true);

    // Advance time by 50ms — IRQ should still be high
    clock.step(50_000_000); // 50ms in ns

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], (3, true));
}

#[test]
fn test_gpio_key_multiple_presses() {
    let sink = Arc::new(TestIrqSink::new());
    let irq = make_irq_line(sink.clone(), 7);
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let key = GpioKey::new(irq, clock.clone());

    // First press
    key.set_gpio(true);
    clock.step(100_000_000);

    // Second press
    key.set_gpio(true);
    clock.step(100_000_000);

    let events = sink.events();
    assert_eq!(events.len(), 4);
    assert_eq!(events[0], (7, true)); // first press raise
    assert_eq!(events[1], (7, false)); // first press timeout lower
    assert_eq!(events[2], (7, true)); // second press raise
    assert_eq!(events[3], (7, false)); // second press timeout lower
}

// ---- GpioPwr ----

#[test]
fn test_gpio_pwr_reset() {
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
fn test_gpio_pwr_shutdown() {
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
fn test_gpio_pwr_level_low_ignored() {
    let pwr = GpioPwr::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    pwr.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    // Deassert should not trigger
    pwr.gpio_reset(false);
    pwr.gpio_shutdown(false);
    assert!(actions.lock().unwrap().is_empty());
}

#[test]
fn test_gpio_pwr_both_reset_and_shutdown() {
    let pwr = GpioPwr::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    pwr.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    pwr.gpio_reset(true);
    pwr.gpio_shutdown(true);
    assert_eq!(
        *actions.lock().unwrap(),
        vec![GpioPwrAction::Reset, GpioPwrAction::Shutdown]
    );
}

#[test]
fn test_gpio_pwr_no_handler_no_panic() {
    let pwr = GpioPwr::new();
    // Without a handler, these should not panic
    pwr.gpio_reset(true);
    pwr.gpio_shutdown(true);
}
