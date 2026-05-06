// Tests for hw/gpio devices: gpio_key, gpio_pwr.

use std::sync::{Arc, Mutex};

use machina_accel::timer::{ClockType, VirtualClock};
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{InterruptSource, IrqLine, IrqSink};
use machina_hw_gpio::pl061::{Pl061, Pl061Mmio};
use machina_hw_gpio::sifive_gpio::{SiFiveGpio, SiFiveGpioMmio};
use machina_hw_gpio::{GpioKey, GpioPwr, GpioPwrAction};
use machina_memory::region::MmioOps;

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

    // Reject realize before bus attach
    let err = key.realize().unwrap_err();
    assert!(err.to_string().contains("parent bus"));

    let mut bus = SysBus::new("sysbus");
    key.attach_to_bus(&mut bus).unwrap();
    key.realize().unwrap();
    assert!(key.realized());

    // Double realize rejected
    let err = key.realize().unwrap_err();
    assert!(err.to_string().contains("already realized"));

    key.unrealize().unwrap();
    assert!(!key.realized());

    // Double unrealize rejected
    let err = key.unrealize().unwrap_err();
    assert!(err.to_string().contains("not realized"));
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

    // Reject realize before bus attach
    let err = pwr.realize().unwrap_err();
    assert!(err.to_string().contains("parent bus"));

    let mut bus = SysBus::new("sysbus");
    pwr.attach_to_bus(&mut bus).unwrap();
    pwr.realize().unwrap();
    assert!(pwr.realized());

    // Double realize rejected
    let err = pwr.realize().unwrap_err();
    assert!(err.to_string().contains("already realized"));

    pwr.unrealize().unwrap();
    assert!(!pwr.realized());

    // Double unrealize rejected
    let err = pwr.unrealize().unwrap_err();
    assert!(err.to_string().contains("not realized"));
}

// -- PL061 tests --

#[test]
fn test_pl061_defaults() {
    let pl061 = Arc::new(Pl061::new());
    let mmio = Pl061Mmio(Arc::clone(&pl061));

    assert_eq!(mmio.read(0x400, 4), 0); // dir
    assert_eq!(mmio.read(0x404, 4), 0); // isense
    assert_eq!(mmio.read(0x408, 4), 0); // ibe
    assert_eq!(mmio.read(0x40C, 4), 0); // iev
    assert_eq!(mmio.read(0x410, 4), 0); // im
    assert_eq!(mmio.read(0x414, 4), 0); // istate
    assert_eq!(mmio.read(0x418, 4), 0); // mis
    assert_eq!(mmio.read(0x420, 4), 0); // afsel
}

#[test]
fn test_pl061_id_registers() {
    let pl061 = Arc::new(Pl061::new());
    let mmio = Pl061Mmio(Arc::clone(&pl061));

    // PL061 ID: check specific values
    assert_eq!(mmio.read(0xFE0, 4), 0x61);
    assert_eq!(mmio.read(0xFE4, 4), 0x10);
    assert_eq!(mmio.read(0xFE8, 4), 0x04);
    assert_eq!(mmio.read(0xFF0, 4), 0x0D);
    assert_eq!(mmio.read(0xFF4, 4), 0xF0);
    assert_eq!(mmio.read(0xFF8, 4), 0x05);
    assert_eq!(mmio.read(0xFFC, 4), 0xB1);
}

#[test]
fn test_pl061_dir_controls_output() {
    let pl061 = Arc::new(Pl061::new());
    let mmio = Pl061Mmio(Arc::clone(&pl061));

    // Set direction to output on pin 0
    mmio.write(0x400, 4, 0x01); // dir bit 0 = 1

    // Write data to pin 0 (offset 0, mask from offset >> 2 = 0)
    // Actually data register uses mask = (offset >> 2) & dir
    // At offset 0, mask = 0 & 1 = 0, so no data is written
    // At offset 4, mask = 1 & 1 = 1
    mmio.write(0x004, 4, 0x01);
    // Read back through data register
    // Data reg at offset 0 returns data & 0 = 0
    // Data reg at offset 4 returns data & 1
    assert_eq!(mmio.read(0x004, 4), 1);
}

#[test]
fn test_pl061_interrupt_sense_level() {
    let pl061 = Arc::new(Pl061::new());
    let mmio = Pl061Mmio(Arc::clone(&pl061));
    let sink = Arc::new(TestSink::new());
    pl061.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    // Set pin 0 as input (dir=0), level-sensitive interrupt, unmask
    mmio.write(0x404, 4, 0x01); // isense bit 0 = 1 (level)
    mmio.write(0x40C, 4, 0x01); // iev bit 0 = 1 (high level)
    mmio.write(0x410, 4, 0x01); // im bit 0 = 1 (unmasked)

    // Set GPIO input pin 0 high
    pl061.set_gpio_input(0, true);
    assert!(*sink.level.lock().unwrap());
}

#[test]
fn test_pl061_interrupt_edge() {
    let pl061 = Arc::new(Pl061::new());
    let mmio = Pl061Mmio(Arc::clone(&pl061));
    let sink = Arc::new(TestSink::new());
    pl061.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    // Configure edge interrupt on pin 0, rising edge
    mmio.write(0x40C, 4, 0x01); // iev bit 0 = 1 (rising edge)
    mmio.write(0x410, 4, 0x01); // im bit 0 = 1

    // Set input high → triggers edge
    pl061.set_gpio_input(0, true);
    assert!(*sink.level.lock().unwrap());
}

#[test]
fn test_pl061_interrupt_clear() {
    let pl061 = Arc::new(Pl061::new());
    let mmio = Pl061Mmio(Arc::clone(&pl061));
    let sink = Arc::new(TestSink::new());
    pl061.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    // Trigger interrupt
    mmio.write(0x40C, 4, 0x01); // iev
    mmio.write(0x410, 4, 0x01); // im
    pl061.set_gpio_input(0, true);
    assert!(*sink.level.lock().unwrap());

    // Clear via ICR
    mmio.write(0x41C, 4, 0x01); // ICR write 1 to clear
    assert!(!*sink.level.lock().unwrap());
    assert_eq!(mmio.read(0x414, 4), 0); // istate cleared
}

#[test]
fn test_pl061_afsel_write() {
    let pl061 = Arc::new(Pl061::new());
    let mmio = Pl061Mmio(Arc::clone(&pl061));

    // CR defaults to 0xFF, so AFSEL writes should work
    mmio.write(0x420, 4, 0x03);
    assert_eq!(mmio.read(0x420, 4), 0x03);
}

#[test]
fn test_pl061_luminary_registers() {
    let pl061 = Arc::new(Pl061::new());
    let mmio = Pl061Mmio(Arc::clone(&pl061));

    // Luminary-specific registers
    assert_eq!(mmio.read(0x500, 4), 0xFF); // dr2r default
    assert_eq!(mmio.read(0x504, 4), 0); // dr4r
    assert_eq!(mmio.read(0x508, 4), 0); // dr8r
    assert_eq!(mmio.read(0x50C, 4), 0); // odr
    assert_eq!(mmio.read(0x510, 4), 0); // pur
    assert_eq!(mmio.read(0x514, 4), 0); // pdr
    assert_eq!(mmio.read(0x520, 4), 1); // locked=1
    assert_eq!(mmio.read(0x524, 4), 0xFF); // cr

    // Unlock
    mmio.write(0x520, 4, 0xACCE_551);
    assert_eq!(mmio.read(0x520, 4), 0); // locked=0

    // Write to CR when unlocked
    mmio.write(0x524, 4, 0x0F);
    assert_eq!(mmio.read(0x524, 4), 0x0F);
}

#[test]
fn test_pl061_reset_runtime() {
    let pl061 = Arc::new(Pl061::new());
    let mmio = Pl061Mmio(Arc::clone(&pl061));

    mmio.write(0x400, 4, 0xFF);
    mmio.write(0x410, 4, 0xFF);
    mmio.write(0x500, 4, 0x00);

    pl061.reset_runtime();

    assert_eq!(mmio.read(0x400, 4), 0); // dir reset
    assert_eq!(mmio.read(0x410, 4), 0); // im reset
    assert_eq!(mmio.read(0x500, 4), 0xFF); // dr2r back to default
}

// -- SiFive GPIO tests --

#[test]
fn test_sifive_gpio_defaults() {
    let gpio = Arc::new(SiFiveGpio::new());
    let mmio = SiFiveGpioMmio(Arc::clone(&gpio));

    assert_eq!(mmio.read(0x000, 4), 0); // value
    assert_eq!(mmio.read(0x004, 4), 0); // input_en
    assert_eq!(mmio.read(0x008, 4), 0); // output_en
    assert_eq!(mmio.read(0x00C, 4), 0); // port
    assert_eq!(mmio.read(0x010, 4), 0); // pue
    assert_eq!(mmio.read(0x018, 4), 0); // rise_ie
    assert_eq!(mmio.read(0x01C, 4), 0); // rise_ip
    assert_eq!(mmio.read(0x020, 4), 0); // fall_ie
    assert_eq!(mmio.read(0x024, 4), 0); // fall_ip
    assert_eq!(mmio.read(0x028, 4), 0); // high_ie
    assert_eq!(mmio.read(0x02C, 4), 0); // high_ip
    assert_eq!(mmio.read(0x030, 4), 0); // low_ie
    assert_eq!(mmio.read(0x034, 4), 0); // low_ip
}

#[test]
fn test_sifive_gpio_output_en() {
    let gpio = Arc::new(SiFiveGpio::new());
    let mmio = SiFiveGpioMmio(Arc::clone(&gpio));

    // Set output enable for pin 0
    mmio.write(0x008, 4, 0x01); // output_en
                                // Set port output high for pin 0
    mmio.write(0x00C, 4, 0x01); // port
                                // GPIO value should reflect the output
    assert_eq!(mmio.read(0x00C, 4), 0x01);
}

#[test]
fn test_sifive_gpio_rise_interrupt() {
    let gpio = Arc::new(SiFiveGpio::new());
    let mmio = SiFiveGpioMmio(Arc::clone(&gpio));
    let sink = Arc::new(TestSink::new());
    gpio.connect_output(
        0,
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0),
    );

    // Enable rise interrupt, input
    mmio.write(0x004, 4, 0x01); // input_en
    mmio.write(0x018, 4, 0x01); // rise_ie

    assert!(!*sink.level.lock().unwrap());

    // Drive pin 0 high externally
    gpio.set_input(0, true);
    assert!(*sink.level.lock().unwrap());
}

#[test]
fn test_sifive_gpio_ip_clear() {
    let gpio = Arc::new(SiFiveGpio::new());
    let mmio = SiFiveGpioMmio(Arc::clone(&gpio));
    let sink = Arc::new(TestSink::new());
    gpio.connect_output(
        0,
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0),
    );

    // Trigger rise interrupt
    mmio.write(0x004, 4, 0x01); // input_en
    mmio.write(0x018, 4, 0x01); // rise_ie
    gpio.set_input(0, true);
    assert_eq!(mmio.read(0x01C, 4), 0x01); // rise_ip = 1
    assert!(*sink.level.lock().unwrap());

    // Clear by writing 1 to IP
    mmio.write(0x01C, 4, 0x01); // rise_ip write 1 to clear
    assert_eq!(mmio.read(0x01C, 4), 0); // rise_ip = 0
    assert!(!*sink.level.lock().unwrap());
}

#[test]
fn test_sifive_gpio_output_port() {
    let gpio = Arc::new(SiFiveGpio::new());
    let mmio = SiFiveGpioMmio(Arc::clone(&gpio));

    // Configure pin 0 as output, invert via out_xor
    mmio.write(0x008, 4, 0x01); // output_en
    mmio.write(0x040, 4, 0x01); // out_xor
    mmio.write(0x00C, 4, 0x01); // port = 1

    // With out_xor, output = port ^ out_xor = 0
    // value reads back 0 since input_en=0
    assert_eq!(mmio.read(0x000, 4), 0);

    // Change port to 0 → output = 0 ^ 1 = 1, but input_en=0 so value stays 0
    mmio.write(0x00C, 4, 0x00);
    assert_eq!(mmio.read(0x000, 4), 0);
}

#[test]
fn test_sifive_gpio_pullup() {
    let gpio = Arc::new(SiFiveGpio::new());
    let mmio = SiFiveGpioMmio(Arc::clone(&gpio));

    // Enable input and pull-up on pin 0
    mmio.write(0x004, 4, 0x01); // input_en
    mmio.write(0x010, 4, 0x01); // pue

    // With pull-up and no output, value should be 1
    assert_eq!(mmio.read(0x000, 4), 0x01);
}

#[test]
fn test_sifive_gpio_reset_runtime() {
    let gpio = Arc::new(SiFiveGpio::new());
    let mmio = SiFiveGpioMmio(Arc::clone(&gpio));

    mmio.write(0x004, 4, 0xFF);
    mmio.write(0x008, 4, 0xFF);
    mmio.write(0x00C, 4, 0xFF);
    mmio.write(0x018, 4, 0xFF);

    gpio.reset_runtime();

    assert_eq!(mmio.read(0x004, 4), 0);
    assert_eq!(mmio.read(0x008, 4), 0);
    assert_eq!(mmio.read(0x00C, 4), 0);
    assert_eq!(mmio.read(0x018, 4), 0);
}

#[test]
fn test_sifive_gpio_write_value_read_only() {
    let gpio = Arc::new(SiFiveGpio::new());
    let mmio = SiFiveGpioMmio(Arc::clone(&gpio));

    // VALUE is read-only, write should be ignored
    mmio.write(0x000, 4, 0xDEAD);
    assert_eq!(mmio.read(0x000, 4), 0);
}
