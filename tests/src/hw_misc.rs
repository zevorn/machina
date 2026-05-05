// Tests for hw/misc devices: PRCI, pvpanic, unimp, led, virt_ctrl.

use std::sync::Arc;

use machina_core::address::GPA;
use machina_hw_core::bus::SysBus;
use machina_hw_misc::{
    Led, LedColor, Pvpanic, PvpanicEvent, PvpanicMmio, SifiveEPRCI,
    SifiveEPRCIMmio, SifiveUPRCI, SifiveUPRCIMmio, Unimp, UnimpMmio, VirtCtrl,
    VirtCtrlAction, VirtCtrlMmio,
};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

fn make_test_aspace() -> (AddressSpace, SysBus) {
    let root = MemoryRegion::container("root", 0x1_0000_0000);
    let aspace = AddressSpace::new(root);
    let bus = SysBus::new("sysbus");
    (aspace, bus)
}

// ---- SiFive E PRCI ----

#[test]
fn test_sifive_e_prci_defaults() {
    let prci = SifiveEPRCI::new();
    assert_eq!(prci.do_read(0x00, 4) as u32, 0xC000_0000);
    assert_eq!(prci.do_read(0x04, 4) as u32, 0xC000_0000);
    assert_eq!(
        prci.do_read(0x08, 4) as u32,
        0x8000_0000 | (1 << 17) | (1 << 18)
    );
    assert_eq!(prci.do_read(0x0C, 4) as u32, 1 << 8);
}

#[test]
fn test_sifive_e_prci_write_read_back() {
    let prci = SifiveEPRCI::new();
    prci.do_write(0x00, 4, 0x1234_5678);
    assert_eq!(prci.do_read(0x00, 4) as u32, 0x1234_5678 | 0x8000_0000);
    prci.do_write(0x04, 4, 0x0ABC_DEF0);
    assert_eq!(prci.do_read(0x04, 4) as u32, 0x0ABC_DEF0 | 0x8000_0000);
    prci.do_write(0x08, 4, 0x0000_0001);
    assert_eq!(prci.do_read(0x08, 4) as u32, 0x0000_0001 | 0x8000_0000);
    prci.do_write(0x0C, 4, 0x0000_00FF);
    assert_eq!(prci.do_read(0x0C, 4) as u32, 0x0000_00FF);
}

#[test]
fn test_sifive_e_prci_invalid_offset() {
    let prci = SifiveEPRCI::new();
    assert_eq!(prci.do_read(0x10, 4), 0);
    prci.do_write(0x10, 4, 0xDEAD_BEEF);
}

#[test]
fn test_sifive_e_prci_rejects_non_4byte_access() {
    let prci = SifiveEPRCI::new();
    assert_eq!(prci.do_read(0x00, 1), 0);
    assert_eq!(prci.do_read(0x00, 2), 0);
    assert_eq!(prci.do_read(0x00, 8), 0);
    prci.do_write(0x00, 1, 0xFF);
    assert_eq!(prci.do_read(0x00, 4) as u32, 0xC000_0000);
    prci.do_write(0x00, 8, 0xDEAD_BEEF_1234_5678);
    assert_eq!(prci.do_read(0x00, 4) as u32, 0xC000_0000);
}

#[test]
fn test_sifive_e_prci_reset_runtime() {
    let prci = SifiveEPRCI::new();
    prci.do_write(0x00, 4, 0x0000_0000);
    prci.do_write(0x0C, 4, 0x0000_0000);
    prci.reset_runtime();
    assert_eq!(prci.do_read(0x00, 4) as u32, 0xC000_0000);
    assert_eq!(prci.do_read(0x0C, 4) as u32, 1 << 8);
}

#[test]
fn test_sifive_e_prci_lifecycle() {
    let prci = SifiveEPRCI::new();
    assert!(!prci.realized());

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA(0x1000_0000);

    // Pre-realize: address space has no mapping
    assert_eq!(aspace.read(base, 4), 0);

    prci.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "e-prci",
        0x1000,
        Arc::new(SifiveEPRCIMmio(prci.clone())),
    );
    prci.register_mmio(region, base).unwrap();
    prci.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(prci.realized());

    // Realized: address space reads return register defaults
    assert_eq!(aspace.read(base, 4) as u32, 0xC000_0000);

    // Late mutation rejected
    let region2 = MemoryRegion::io(
        "e-prci-b",
        0x1000,
        Arc::new(SifiveEPRCIMmio(prci.clone())),
    );
    assert!(prci.register_mmio(region2, GPA(0x2000_0000)).is_err());

    // Second realize_onto fails (mapping already recorded in bus)
    let err = prci.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("overlaps"));

    prci.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!prci.realized());

    // Post-unrealize: address space read returns 0 (unmapped)
    assert_eq!(aspace.read(base, 4), 0);

    let err = prci.unrealize_from(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("not realized"));
}

#[test]
fn test_sifive_e_prci_mmio_wrapper() {
    let prci = SifiveEPRCI::new();
    let mmio = SifiveEPRCIMmio(prci.clone());
    assert_eq!(mmio.read(0x00, 4) as u32, 0xC000_0000);
    mmio.write(0x0C, 4, 0x42);
    assert_eq!(mmio.read(0x0C, 4) as u32, 0x42);
    // Non-4-byte through wrapper returns 0
    assert_eq!(mmio.read(0x00, 1), 0);
}

// ---- SiFive U PRCI ----

#[test]
fn test_sifive_u_prci_defaults() {
    let prci = SifiveUPRCI::new();
    assert_eq!(prci.do_read(0x00, 4) as u32, 0xC000_0000);
    let pllcfg0_default =
        (1 << 0) | (31 << 6) | (3 << 15) | (1 << 25) | (1 << 31);
    assert_eq!(prci.do_read(0x04, 4) as u32, pllcfg0_default);
    assert_eq!(prci.do_read(0x0C, 4) as u32, pllcfg0_default);
    assert_eq!(prci.do_read(0x10, 4) as u32, 0);
    assert_eq!(prci.do_read(0x1C, 4) as u32, pllcfg0_default);
    assert_eq!(prci.do_read(0x20, 4) as u32, 0);
    assert_eq!(prci.do_read(0x24, 4) as u32, 1 << 0);
    assert_eq!(prci.do_read(0x28, 4) as u32, 0);
    assert_eq!(prci.do_read(0x2C, 4) as u32, 0);
}

#[test]
fn test_sifive_u_prci_write_read_back() {
    let prci = SifiveUPRCI::new();
    prci.do_write(0x00, 4, 0x0000_0001);
    assert_eq!(prci.do_read(0x00, 4) as u32, 0x0000_0001 | 0x8000_0000);
    prci.do_write(0x04, 4, 0x0000_0001);
    assert_eq!(
        prci.do_read(0x04, 4) as u32,
        0x0000_0001 | (1 << 25) | (1 << 31)
    );
    prci.do_write(0x10, 4, 0xCAFE_0000);
    assert_eq!(prci.do_read(0x10, 4) as u32, 0xCAFE_0000);
    prci.do_write(0x24, 4, 0x0000_0003);
    assert_eq!(prci.do_read(0x24, 4) as u32, 0x0000_0003);
}

#[test]
fn test_sifive_u_prci_invalid_offset() {
    let prci = SifiveUPRCI::new();
    assert_eq!(prci.do_read(0x30, 4), 0);
    prci.do_write(0x1000, 4, 0xDEAD_BEEF);
}

#[test]
fn test_sifive_u_prci_rejects_non_4byte_access() {
    let prci = SifiveUPRCI::new();
    assert_eq!(prci.do_read(0x00, 1), 0);
    assert_eq!(prci.do_read(0x00, 2), 0);
    assert_eq!(prci.do_read(0x00, 8), 0);
    prci.do_write(0x00, 1, 0xFF);
    assert_eq!(prci.do_read(0x00, 4) as u32, 0xC000_0000);
}

#[test]
fn test_sifive_u_prci_reset_runtime() {
    let prci = SifiveUPRCI::new();
    prci.do_write(0x00, 4, 0x0000_0000);
    prci.do_write(0x24, 4, 0x0000_0000);
    prci.reset_runtime();
    assert_eq!(prci.do_read(0x00, 4) as u32, 0xC000_0000);
    assert_eq!(prci.do_read(0x24, 4) as u32, 1 << 0);
}

#[test]
fn test_sifive_u_prci_reset_preserves_untouched_regs() {
    let prci = SifiveUPRCI::new();
    // Write values to registers the reference reset does NOT touch.
    prci.do_write(0x10, 4, 0xCAFE_0000); // DDRPLLCFG1
    prci.do_write(0x20, 4, 0xBEEF_0000); // GEMGXLPLLCFG1
    prci.do_write(0x28, 4, 0x0000_00FF); // DEVICESRESET
    prci.do_write(0x2C, 4, 0x0000_00AB); // CLKMUXSTATUS
    prci.reset_runtime();
    // Reference-reset registers are restored.
    assert_eq!(prci.do_read(0x00, 4) as u32, 0xC000_0000);
    let pllcfg0 = (1 << 0) | (31 << 6) | (3 << 15) | (1 << 25) | (1 << 31);
    assert_eq!(prci.do_read(0x04, 4) as u32, pllcfg0);
    assert_eq!(prci.do_read(0x0C, 4) as u32, pllcfg0);
    assert_eq!(prci.do_read(0x1C, 4) as u32, pllcfg0);
    assert_eq!(prci.do_read(0x24, 4) as u32, 1 << 0);
    // Untouched registers preserve their written values.
    assert_eq!(prci.do_read(0x10, 4) as u32, 0xCAFE_0000);
    assert_eq!(prci.do_read(0x20, 4) as u32, 0xBEEF_0000);
    assert_eq!(prci.do_read(0x28, 4) as u32, 0x0000_00FF);
    assert_eq!(prci.do_read(0x2C, 4) as u32, 0x0000_00AB);
}

#[test]
fn test_sifive_u_prci_lifecycle() {
    let prci = SifiveUPRCI::new();
    assert!(!prci.realized());

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA(0x1000_0000);

    // Pre-realize: unmapped
    assert_eq!(aspace.read(base, 4), 0);

    prci.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "u-prci",
        0x1000,
        Arc::new(SifiveUPRCIMmio(prci.clone())),
    );
    prci.register_mmio(region, base).unwrap();
    prci.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(prci.realized());

    // Realized: address space read returns register defaults
    assert_eq!(aspace.read(base, 4) as u32, 0xC000_0000);

    // Second realize_onto fails (mapping already recorded in bus)
    let err = prci.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("overlaps"));

    prci.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!prci.realized());

    // Post-unrealize: unmapped
    assert_eq!(aspace.read(base, 4), 0);

    let err = prci.unrealize_from(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("not realized"));
}

// ---- Pvpanic ----

#[test]
fn test_pvpanic_mmio_read_events() {
    let pvp = Pvpanic::new(PvpanicEvent::PANICKED | PvpanicEvent::CRASH_LOADED);
    let mmio = PvpanicMmio(pvp.clone());
    let events = mmio.read(0x00, 1) as u8;
    assert_eq!(events, PvpanicEvent::PANICKED | PvpanicEvent::CRASH_LOADED);
}

#[test]
fn test_pvpanic_mmio_write_dispatches_priority() {
    use std::sync::Mutex;

    let pvp = Pvpanic::new(
        PvpanicEvent::PANICKED
            | PvpanicEvent::CRASH_LOADED
            | PvpanicEvent::SHUTDOWN,
    );
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);
    pvp.set_event_handler(Box::new(move |event| {
        received_clone.lock().unwrap().push(event);
    }));

    let mmio = PvpanicMmio(pvp.clone());

    // Write only PANICKED
    mmio.write(0x00, 1, u64::from(PvpanicEvent::PANICKED));
    assert_eq!(*received.lock().unwrap(), vec![PvpanicEvent::PANICKED]);

    // Write PANICKED|CRASH_LOADED — dispatches PANICKED only (first by priority)
    mmio.write(
        0x00,
        1,
        u64::from(PvpanicEvent::PANICKED | PvpanicEvent::CRASH_LOADED),
    );
    assert_eq!(
        *received.lock().unwrap(),
        vec![PvpanicEvent::PANICKED, PvpanicEvent::PANICKED]
    );

    // Write CRASH_LOADED|SHUTDOWN — dispatches CRASH_LOADED only
    mmio.write(
        0x00,
        1,
        u64::from(PvpanicEvent::CRASH_LOADED | PvpanicEvent::SHUTDOWN),
    );
    assert_eq!(
        *received.lock().unwrap(),
        vec![
            PvpanicEvent::PANICKED,
            PvpanicEvent::PANICKED,
            PvpanicEvent::CRASH_LOADED,
        ]
    );
}

#[test]
fn test_pvpanic_mmio_unadvertised_event_still_dispatches() {
    use std::sync::Mutex;

    let pvp = Pvpanic::new(PvpanicEvent::PANICKED);
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);
    pvp.set_event_handler(Box::new(move |event| {
        received_clone.lock().unwrap().push(event);
    }));

    let mmio = PvpanicMmio(pvp.clone());

    // Read still returns only PANICKED
    assert_eq!(mmio.read(0x00, 1) as u8, PvpanicEvent::PANICKED);

    // Write CRASH_LOADED — should still dispatch (priority, no mask on write)
    mmio.write(0x00, 1, u64::from(PvpanicEvent::CRASH_LOADED));
    assert_eq!(*received.lock().unwrap(), vec![PvpanicEvent::CRASH_LOADED]);
}

#[test]
fn test_pvpanic_mmio_write_no_recognized_event() {
    use std::sync::Mutex;

    let pvp = Pvpanic::new(PvpanicEvent::PANICKED);
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);
    pvp.set_event_handler(Box::new(move |event| {
        received_clone.lock().unwrap().push(event);
    }));

    let mmio = PvpanicMmio(pvp.clone());

    // Write 0 — no recognized event, handler not called
    mmio.write(0x00, 1, 0);
    assert!(received.lock().unwrap().is_empty());
}

#[test]
fn test_pvpanic_lifecycle() {
    let pvp = Pvpanic::new(PvpanicEvent::PANICKED);
    assert!(!pvp.realized());

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA(0x1000_0000);

    // Pre-realize: unmapped
    assert_eq!(aspace.read(base, 1), 0);

    pvp.attach_to_bus(&mut bus).unwrap();
    let region =
        MemoryRegion::io("pvpanic", 0x2, Arc::new(PvpanicMmio(pvp.clone())));
    pvp.register_mmio(region, base).unwrap();
    pvp.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(pvp.realized());

    // Realized: read returns events mask
    assert_eq!(aspace.read(base, 1) as u8, PvpanicEvent::PANICKED);

    // Second realize_onto fails (mapping already recorded in bus)
    let err = pvp.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("overlaps"));

    pvp.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!pvp.realized());

    // Post-unrealize: unmapped
    assert_eq!(aspace.read(base, 1), 0);
}

// ---- Unimp ----

#[test]
fn test_unimp_read_returns_zero() {
    let unimp = Unimp::new("test-device", 0x1000);
    assert_eq!(unimp.do_read(0x00, 4), 0);
    assert_eq!(unimp.do_read(0x04, 4), 0);
    assert_eq!(unimp.do_read(0xFFC, 1), 0);
}

#[test]
fn test_unimp_write_no_panic() {
    let unimp = Unimp::new("test-device", 0x1000);
    unimp.do_write(0x00, 4, 0xDEAD_BEEF);
    unimp.do_write(0x800, 8, 0x1234_5678_9ABC_DEF0);
}

#[test]
fn test_unimp_access_sizes() {
    let unimp = Unimp::new("test-device", 0x1000);
    assert_eq!(unimp.do_read(0x00, 1), 0);
    assert_eq!(unimp.do_read(0x00, 2), 0);
    assert_eq!(unimp.do_read(0x00, 4), 0);
    assert_eq!(unimp.do_read(0x00, 8), 0);
    unimp.do_write(0x00, 1, 0xFF);
    unimp.do_write(0x00, 2, 0xFFFF);
    unimp.do_write(0x00, 4, 0xFFFF_FFFF);
    unimp.do_write(0x00, 8, 0xFFFF_FFFF_FFFF_FFFF);
}

#[test]
fn test_unimp_name_and_size() {
    let unimp = Unimp::new("my-device", 0x2000);
    assert_eq!(unimp.name(), "my-device");
    assert_eq!(unimp.size(), 0x2000);
}

#[test]
fn test_unimp_mmio_wrapper() {
    let unimp = Unimp::new("test-device", 0x1000);
    let mmio = UnimpMmio(unimp.clone());
    assert_eq!(mmio.read(0x00, 4), 0);
    mmio.write(0x00, 4, 0xDEAD_BEEF);
}

#[test]
fn test_unimp_lifecycle() {
    let unimp = Unimp::new("test-device", 0x1000);
    assert!(!unimp.realized());

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA(0x1000_0000);

    // Pre-realize: unmapped
    assert_eq!(aspace.read(base, 4), 0);

    unimp.attach_to_bus(&mut bus).unwrap();
    let region =
        MemoryRegion::io("unimp", 0x1000, Arc::new(UnimpMmio(unimp.clone())));
    unimp.register_mmio(region, base).unwrap();
    unimp.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(unimp.realized());

    // Realized: reads return 0 and writes don't panic
    assert_eq!(aspace.read(base, 4), 0);
    aspace.write(base, 4, 0xDEAD_BEEF);

    // Second realize_onto fails (mapping already recorded in bus)
    let err = unimp.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("overlaps"));

    unimp.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!unimp.realized());

    // Post-unrealize: unmapped
    assert_eq!(aspace.read(base, 4), 0);
}

#[test]
fn test_unimp_rejects_zero_size_realize() {
    let unimp = Unimp::new("zero-device", 0);
    assert!(!unimp.realized());

    let (mut aspace, mut bus) = make_test_aspace();
    unimp.attach_to_bus(&mut bus).unwrap();
    let region =
        MemoryRegion::io("zero", 0, Arc::new(UnimpMmio(unimp.clone())));
    unimp.register_mmio(region, GPA(0x1000_0000)).unwrap();
    let err = unimp.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("must be non-zero"));
    assert!(!unimp.realized());
}

// ---- LED ----

#[test]
fn test_led_defaults() {
    let led = Led::new(LedColor::Green, "status", true);
    assert_eq!(led.color(), LedColor::Green);
    assert_eq!(led.description(), "status");
    assert!(led.gpio_active_high());
    assert_eq!(led.get_intensity(), 100);
}

#[test]
fn test_led_defaults_active_low() {
    let led = Led::new(LedColor::Red, "error", false);
    assert!(!led.gpio_active_high());
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
    led.set_intensity(200);
    assert_eq!(led.get_intensity(), 100);
}

#[test]
fn test_led_set_state() {
    let led = Led::new(LedColor::Green, "active", true);
    led.set_state(true);
    assert_eq!(led.get_intensity(), 100);
    led.set_state(false);
    assert_eq!(led.get_intensity(), 0);
}

#[test]
fn test_led_set_gpio_active_high() {
    let led = Led::new(LedColor::Green, "active-high", true);
    led.set_gpio(true);
    assert_eq!(led.get_intensity(), 100);
    led.set_gpio(false);
    assert_eq!(led.get_intensity(), 0);
}

#[test]
fn test_led_set_gpio_active_low() {
    let led = Led::new(LedColor::Red, "active-low", false);
    led.set_gpio(false);
    assert_eq!(led.get_intensity(), 100);
    led.set_gpio(true);
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

#[test]
fn test_led_reset_runtime() {
    let led = Led::new(LedColor::Green, "reset-test", true);
    led.set_intensity(42);
    led.reset_runtime();
    assert_eq!(led.get_intensity(), 100);

    let led_low = Led::new(LedColor::Red, "reset-low", false);
    led_low.set_intensity(42);
    led_low.reset_runtime();
    assert_eq!(led_low.get_intensity(), 0);
}

#[test]
fn test_led_lifecycle() {
    let led = Led::new(LedColor::Green, "lifecycle", true);
    assert!(!led.realized());
    led.realize().unwrap();
    assert!(led.realized());
    let err = led.realize().unwrap_err();
    assert!(err.to_string().contains("already realized"));
    led.unrealize().unwrap();
    assert!(!led.realized());
    let err = led.unrealize().unwrap_err();
    assert!(err.to_string().contains("not realized"));
}

// ---- VirtCtrl ----

#[test]
fn test_virt_ctrl_features() {
    let vc = VirtCtrl::new();
    assert_eq!(vc.do_read(0x00, 4) as u32, 0x0000_0001);
}

#[test]
fn test_virt_ctrl_cmd_read_returns_zero() {
    let vc = VirtCtrl::new();
    assert_eq!(vc.do_read(0x04, 4), 0);
}

#[test]
fn test_virt_ctrl_cmd_reset() {
    use std::sync::Mutex;

    let vc = VirtCtrl::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    vc.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    vc.do_write(0x04, 4, 1);
    assert_eq!(*actions.lock().unwrap(), vec![VirtCtrlAction::Reset]);
}

#[test]
fn test_virt_ctrl_cmd_halt() {
    use std::sync::Mutex;

    let vc = VirtCtrl::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    vc.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    vc.do_write(0x04, 4, 2);
    assert_eq!(*actions.lock().unwrap(), vec![VirtCtrlAction::Halt]);
}

#[test]
fn test_virt_ctrl_cmd_panic() {
    use std::sync::Mutex;

    let vc = VirtCtrl::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    vc.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    vc.do_write(0x04, 4, 3);
    assert_eq!(*actions.lock().unwrap(), vec![VirtCtrlAction::Panic]);
}

#[test]
fn test_virt_ctrl_cmd_noop_ignored() {
    use std::sync::Mutex;

    let vc = VirtCtrl::new();
    let actions = Arc::new(Mutex::new(Vec::new()));
    let actions_clone = Arc::clone(&actions);
    vc.set_action_handler(Box::new(move |action| {
        actions_clone.lock().unwrap().push(action);
    }));

    vc.do_write(0x04, 4, 0);
    assert!(actions.lock().unwrap().is_empty());
}

#[test]
fn test_virt_ctrl_invalid_offset() {
    let vc = VirtCtrl::new();
    assert_eq!(vc.do_read(0x08, 4), 0);
    vc.do_write(0x08, 4, 0xDEAD_BEEF);
}

#[test]
fn test_virt_ctrl_mmio_wrapper() {
    let vc = VirtCtrl::new();
    let mmio = VirtCtrlMmio(vc.clone());
    assert_eq!(mmio.read(0x00, 4) as u32, 0x0000_0001);
    mmio.write(0x04, 4, 0);
    assert_eq!(mmio.read(0x04, 4), 0);
}

#[test]
fn test_virt_ctrl_lifecycle() {
    let vc = VirtCtrl::new();
    assert!(!vc.realized());

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA(0x1000_0000);

    // Pre-realize: unmapped
    assert_eq!(aspace.read(base, 4), 0);

    vc.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "virt_ctrl",
        0x100,
        Arc::new(VirtCtrlMmio(vc.clone())),
    );
    vc.register_mmio(region, base).unwrap();
    vc.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(vc.realized());

    // Realized: read returns FEATURES
    assert_eq!(aspace.read(base, 4) as u32, 0x0000_0001);

    // Second realize_onto fails (mapping already recorded in bus)
    let err = vc.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("overlaps"));

    vc.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!vc.realized());

    // Post-unrealize: unmapped
    assert_eq!(aspace.read(base, 4), 0);
}

#[test]
fn test_virt_ctrl_rejects_large_access() {
    let vc = VirtCtrl::new();
    // 1/2/4-byte reads work
    assert_eq!(vc.do_read(0x00, 1) as u32, 0x0000_0001);
    assert_eq!(vc.do_read(0x00, 2) as u32, 0x0000_0001);
    assert_eq!(vc.do_read(0x00, 4) as u32, 0x0000_0001);
    // 8-byte read returns 0
    assert_eq!(vc.do_read(0x00, 8), 0);
    // 8-byte write to CMD is ignored
    {
        use std::sync::Mutex;
        let actions = Arc::new(Mutex::new(Vec::new()));
        let actions_clone = Arc::clone(&actions);
        vc.set_action_handler(Box::new(move |action| {
            actions_clone.lock().unwrap().push(action);
        }));
        vc.do_write(0x04, 8, 1); // CMD_RESET in 8-byte access
        assert!(actions.lock().unwrap().is_empty());
    }
}
