// Tests for hw/misc devices: PRCI, pvpanic, unimp, led, virt_ctrl.

use machina_hw_misc::{
    Led, LedColor, PvpanicEvent, PvpanicMmio, SifiveEPRCI, SifiveUPRCI, Unimp,
    VirtCtrl, VirtCtrlAction,
};
use machina_memory::region::MmioOps;

// ---- SiFive E PRCI ----

#[test]
fn test_sifive_e_prci_defaults() {
    let prci = SifiveEPRCI::new();
    // HFROSCCFG: 0x8000_0000 (RDY) | 0x4000_0000 (EN) = 0xC000_0000
    assert_eq!(prci.read(0x00, 4) as u32, 0xC000_0000);
    // HFXOSCCFG: same default
    assert_eq!(prci.read(0x04, 4) as u32, 0xC000_0000);
    assert_eq!(
        prci.read(0x08, 4) as u32,
        0x8000_0000 | (1 << 17) | (1 << 18)
    ); // LOCK|REFSEL|BYPASS
    assert_eq!(prci.read(0x0C, 4) as u32, 1 << 8); // DIV1
}

#[test]
fn test_sifive_e_prci_write_read_back() {
    let prci = SifiveEPRCI::new();
    // HFROSCCFG: write value, RDY stays set
    prci.write(0x00, 4, 0x1234_5678);
    assert_eq!(prci.read(0x00, 4) as u32, 0x1234_5678 | 0x8000_0000);
    // HFXOSCCFG
    prci.write(0x04, 4, 0x0ABC_DEF0);
    assert_eq!(prci.read(0x04, 4) as u32, 0x0ABC_DEF0 | 0x8000_0000);
    // PLLCFG: LOCK stays set
    prci.write(0x08, 4, 0x0000_0001);
    assert_eq!(prci.read(0x08, 4) as u32, 0x0000_0001 | 0x8000_0000);
    // PLLOUTDIV
    prci.write(0x0C, 4, 0x0000_00FF);
    assert_eq!(prci.read(0x0C, 4) as u32, 0x0000_00FF);
}

#[test]
fn test_sifive_e_prci_invalid_offset() {
    let prci = SifiveEPRCI::new();
    assert_eq!(prci.read(0x10, 4), 0);
    // Write to invalid offset should be a no-op (no panic)
    prci.write(0x10, 4, 0xDEAD_BEEF);
}

#[test]
fn test_sifive_e_prci_access_sizes() {
    let prci = SifiveEPRCI::new();
    // 8-bit read (device accepts any size, returns register value)
    assert_eq!(prci.read(0x00, 1) as u32, 0xC000_0000);
    // 2-byte read
    assert_eq!(prci.read(0x00, 2) as u32, 0xC000_0000);
    // 8-byte read
    let val = prci.read(0x00, 8);
    assert_eq!(val as u32, 0xC000_0000);
}

// ---- SiFive U PRCI ----

#[test]
fn test_sifive_u_prci_defaults() {
    let prci = SifiveUPRCI::new();
    // HFXOSCCFG: 0x8000_0000 (RDY) | 0x4000_0000 (EN) = 0xC000_0000
    assert_eq!(prci.read(0x00, 4) as u32, 0xC000_0000);
    // PLLCFG0 with DIVR|DIVF|DIVQ|FSE|LOCK
    let pllcfg0_default =
        (1 << 0) | (31 << 6) | (3 << 15) | (1 << 25) | (1 << 31);
    assert_eq!(prci.read(0x04, 4) as u32, pllcfg0_default); // corepllcfg0
    assert_eq!(prci.read(0x0C, 4) as u32, pllcfg0_default); // ddrpllcfg0
    assert_eq!(prci.read(0x10, 4) as u32, 0); // ddrpllcfg1
    assert_eq!(prci.read(0x1C, 4) as u32, pllcfg0_default); // gemgxlpllcfg0
    assert_eq!(prci.read(0x20, 4) as u32, 0); // gemgxlpllcfg1
    assert_eq!(prci.read(0x24, 4) as u32, 1 << 0); // coreclksel = HFCLK
    assert_eq!(prci.read(0x28, 4) as u32, 0); // devicesreset
    assert_eq!(prci.read(0x2C, 4) as u32, 0); // clkmuxstatus
}

#[test]
fn test_sifive_u_prci_write_read_back() {
    let prci = SifiveUPRCI::new();
    // HFXOSCCFG — RDY stays set
    prci.write(0x00, 4, 0x0000_0001);
    assert_eq!(prci.read(0x00, 4) as u32, 0x0000_0001 | 0x8000_0000);
    // corepllcfg0 — FSE|LOCK stay set
    prci.write(0x04, 4, 0x0000_0001);
    assert_eq!(
        prci.read(0x04, 4) as u32,
        0x0000_0001 | (1 << 25) | (1 << 31)
    );
    // ddrpllcfg1
    prci.write(0x10, 4, 0xCAFE_0000);
    assert_eq!(prci.read(0x10, 4) as u32, 0xCAFE_0000);
    // coreclksel
    prci.write(0x24, 4, 0x0000_0003);
    assert_eq!(prci.read(0x24, 4) as u32, 0x0000_0003);
    // devicesreset
    prci.write(0x28, 4, 0x0000_00FF);
    assert_eq!(prci.read(0x28, 4) as u32, 0x0000_00FF);
    // clkmuxstatus
    prci.write(0x2C, 4, 0x0000_0007);
    assert_eq!(prci.read(0x2C, 4) as u32, 0x0000_0007);
}

#[test]
fn test_sifive_u_prci_invalid_offset() {
    let prci = SifiveUPRCI::new();
    assert_eq!(prci.read(0x30, 4), 0);
    prci.write(0x1000, 4, 0xDEAD_BEEF); // no panic
}

// ---- Pvpanic MMIO ----

#[test]
fn test_pvpanic_mmio_read_events() {
    let pvp =
        PvpanicMmio::new(PvpanicEvent::PANICKED | PvpanicEvent::CRASH_LOADED);
    let events = pvp.read(0x00, 1) as u8;
    assert_eq!(events, PvpanicEvent::PANICKED | PvpanicEvent::CRASH_LOADED);
}

#[test]
fn test_pvpanic_mmio_write_triggers_event() {
    use std::sync::{Arc, Mutex};

    let pvp = PvpanicMmio::new(
        PvpanicEvent::PANICKED
            | PvpanicEvent::CRASH_LOADED
            | PvpanicEvent::SHUTDOWN,
    );
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);
    pvp.set_event_handler(Box::new(move |event| {
        received_clone.lock().unwrap().push(event);
    }));

    // Write PANICKED event
    pvp.write(0x00, 1, u64::from(PvpanicEvent::PANICKED));
    assert_eq!(*received.lock().unwrap(), vec![PvpanicEvent::PANICKED]);

    // Write SHUTDOWN event
    pvp.write(0x00, 1, u64::from(PvpanicEvent::SHUTDOWN));
    assert_eq!(
        *received.lock().unwrap(),
        vec![PvpanicEvent::PANICKED, PvpanicEvent::SHUTDOWN]
    );
}

#[test]
fn test_pvpanic_mmio_unsupported_event_ignored() {
    use std::sync::{Arc, Mutex};

    let pvp = PvpanicMmio::new(PvpanicEvent::PANICKED);
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);
    pvp.set_event_handler(Box::new(move |event| {
        received_clone.lock().unwrap().push(event);
    }));

    // Write CRASH_LOADED — not in events mask, should be filtered
    pvp.write(0x00, 1, u64::from(PvpanicEvent::CRASH_LOADED));
    assert!(received.lock().unwrap().is_empty());
}

#[test]
fn test_pvpanic_mmio_multiple_events() {
    use std::sync::{Arc, Mutex};

    let pvp = PvpanicMmio::new(PvpanicEvent::PANICKED | PvpanicEvent::SHUTDOWN);
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);
    pvp.set_event_handler(Box::new(move |event| {
        received_clone.lock().unwrap().push(event);
    }));

    // Write both PANICKED and SHUTDOWN in one write
    pvp.write(
        0x00,
        1,
        u64::from(PvpanicEvent::PANICKED | PvpanicEvent::SHUTDOWN),
    );
    let events = received.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], PvpanicEvent::PANICKED | PvpanicEvent::SHUTDOWN);
}

// ---- Unimp ----

#[test]
fn test_unimp_read_returns_zero() {
    let unimp = Unimp::new("test-device", 0x1000);
    assert_eq!(unimp.read(0x00, 4), 0);
    assert_eq!(unimp.read(0x04, 4), 0);
    assert_eq!(unimp.read(0xFFC, 1), 0);
}

#[test]
fn test_unimp_write_no_panic() {
    let unimp = Unimp::new("test-device", 0x1000);
    unimp.write(0x00, 4, 0xDEAD_BEEF);
    unimp.write(0x800, 8, 0x1234_5678_9ABC_DEF0);
}

#[test]
fn test_unimp_access_sizes() {
    let unimp = Unimp::new("test-device", 0x1000);
    // All access sizes are accepted
    assert_eq!(unimp.read(0x00, 1), 0);
    assert_eq!(unimp.read(0x00, 2), 0);
    assert_eq!(unimp.read(0x00, 4), 0);
    assert_eq!(unimp.read(0x00, 8), 0);
    unimp.write(0x00, 1, 0xFF);
    unimp.write(0x00, 2, 0xFFFF);
    unimp.write(0x00, 4, 0xFFFF_FFFF);
    unimp.write(0x00, 8, 0xFFFF_FFFF_FFFF_FFFF);
}

#[test]
fn test_unimp_name_and_size() {
    let unimp = Unimp::new("my-device", 0x2000);
    assert_eq!(unimp.name(), "my-device");
    assert_eq!(unimp.size(), 0x2000);
}

// ---- LED ----

#[test]
fn test_led_defaults() {
    let led = Led::new(LedColor::Green, "status", true);
    assert_eq!(led.color(), LedColor::Green);
    assert_eq!(led.description(), "status");
    assert!(led.gpio_active_high());
    // With gpio_active_high=true, reset state is 100%
    assert_eq!(led.get_intensity(), 100);
}

#[test]
fn test_led_defaults_active_low() {
    let led = Led::new(LedColor::Red, "error", false);
    assert!(!led.gpio_active_high());
    // With gpio_active_high=false, reset state is 0%
    assert_eq!(led.get_intensity(), 0);
}

#[test]
fn test_led_set_intensity() {
    let led = Led::new(LedColor::Blue, "indicator", true);
    led.set_intensity(50);
    assert_eq!(led.get_intensity(), 50);
    led.set_intensity(0);
    assert_eq!(led.get_intensity(), 0);
}

#[test]
fn test_led_set_intensity_clamped() {
    let led = Led::new(LedColor::Yellow, "warn", true);
    led.set_intensity(200); // exceeds max, clamped to 100
    assert_eq!(led.get_intensity(), 100);
}

#[test]
fn test_led_set_state_active_high() {
    let led = Led::new(LedColor::Green, "active-high", true);
    led.set_state(true);
    assert_eq!(led.get_intensity(), 100);
    led.set_state(false);
    assert_eq!(led.get_intensity(), 0);
}

#[test]
fn test_led_set_state_active_low() {
    let led = Led::new(LedColor::Red, "active-low", false);
    // active-low: GPIO low means emitting
    led.set_state(false);
    assert_eq!(led.get_intensity(), 100);
    led.set_state(true);
    assert_eq!(led.get_intensity(), 0);
}

#[test]
fn test_led_color_names() {
    assert_eq!(LedColor::Violet.as_str(), "violet");
    assert_eq!(LedColor::Blue.as_str(), "blue");
    assert_eq!(LedColor::Cyan.as_str(), "cyan");
    assert_eq!(LedColor::Green.as_str(), "green");
    assert_eq!(LedColor::Yellow.as_str(), "yellow");
    assert_eq!(LedColor::Amber.as_str(), "amber");
    assert_eq!(LedColor::Orange.as_str(), "orange");
    assert_eq!(LedColor::Red.as_str(), "red");
}

// ---- VirtCtrl ----

#[test]
fn test_virt_ctrl_features() {
    let vc = VirtCtrl::new();
    // FEATURES register reports FEAT_POWER_CTRL
    assert_eq!(vc.read(0x00, 4) as u32, 0x0000_0001);
}

#[test]
fn test_virt_ctrl_cmd_read_returns_zero() {
    let vc = VirtCtrl::new();
    assert_eq!(vc.read(0x04, 4), 0);
}

#[test]
fn test_virt_ctrl_cmd_reset() {
    use std::sync::{Arc, Mutex};

    let vc = VirtCtrl::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    vc.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    vc.write(0x04, 4, 1); // CMD_RESET
    assert_eq!(*actions.lock().unwrap(), vec![VirtCtrlAction::Reset]);
}

#[test]
fn test_virt_ctrl_cmd_halt() {
    use std::sync::{Arc, Mutex};

    let vc = VirtCtrl::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    vc.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    vc.write(0x04, 4, 2); // CMD_HALT
    assert_eq!(*actions.lock().unwrap(), vec![VirtCtrlAction::Halt]);
}

#[test]
fn test_virt_ctrl_cmd_panic() {
    use std::sync::{Arc, Mutex};

    let vc = VirtCtrl::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    vc.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    vc.write(0x04, 4, 3); // CMD_PANIC
    assert_eq!(*actions.lock().unwrap(), vec![VirtCtrlAction::Panic]);
}

#[test]
fn test_virt_ctrl_cmd_noop_ignored() {
    use std::sync::{Arc, Mutex};

    let vc = VirtCtrl::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    vc.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    vc.write(0x04, 4, 0); // CMD_NOOP
    assert!(actions.lock().unwrap().is_empty());
}

#[test]
fn test_virt_ctrl_invalid_offset() {
    let vc = VirtCtrl::new();
    assert_eq!(vc.read(0x08, 4), 0);
    vc.write(0x08, 4, 0xDEAD_BEEF); // no panic
}
