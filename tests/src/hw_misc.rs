// Tests for hw/misc devices: PRCI, pvpanic, unimp, led, virt_ctrl,
// PL050, SiFive E AON, SiFive U OTP.

use std::sync::Arc;

use machina_core::address::GPA;
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_misc::{
    Led, LedColor, Pl050, Pl050Mmio, Pvpanic, PvpanicEvent, PvpanicMmio,
    SiFiveEAon, SiFiveEAonMmio, SiFiveUOtp, SiFiveUOtpMmio, SifiveEPRCI,
    SifiveEPRCIMmio, SifiveUPRCI, SifiveUPRCIMmio, Unimp, UnimpMmio, VirtCtrl,
    VirtCtrlAction, VirtCtrlMmio,
};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

pub(crate) fn make_test_aspace() -> (AddressSpace, SysBus) {
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
fn test_sifive_e_prci_lifecycle_and_mom_identity() {
    let prci = SifiveEPRCI::new();
    assert!(!prci.realized());
    prci.with_mdevice(|device| assert_eq!(device.local_id(), "sifive_e_prci"));
    assert_eq!(prci.object_info().local_id, "sifive_e_prci");

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

    // Second realize_onto fails (already realized)
    let err = prci.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

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
fn test_sifive_u_prci_lifecycle_and_mom_identity() {
    let prci = SifiveUPRCI::new();
    assert!(!prci.realized());
    prci.with_mdevice(|device| assert_eq!(device.local_id(), "sifive_u_prci"));
    assert_eq!(prci.object_info().local_id, "sifive_u_prci");

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

    // Second realize_onto fails (already realized)
    let err = prci.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

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
fn test_pvpanic_wide_reads_repeat_event_byte() {
    let pvp = Pvpanic::new(
        PvpanicEvent::PANICKED
            | PvpanicEvent::CRASH_LOADED
            | PvpanicEvent::SHUTDOWN,
    );
    let mmio = PvpanicMmio(pvp.clone());

    assert_eq!(mmio.read(0x00, 1), 0x07);
    assert_eq!(mmio.read(0x00, 2), 0x0707);
    assert_eq!(mmio.read(0x00, 4), 0x0707_0707);
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
fn test_pvpanic_wide_write_dispatches_split_event_bytes() {
    use std::sync::Mutex;

    let pvp = Pvpanic::new(PvpanicEvent::PANICKED);
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);
    pvp.set_event_handler(Box::new(move |event| {
        received_clone.lock().unwrap().push(event);
    }));

    let mmio = PvpanicMmio(pvp.clone());

    mmio.write(0x00, 2, u64::from(PvpanicEvent::PANICKED) << 8);

    assert_eq!(*received.lock().unwrap(), vec![PvpanicEvent::PANICKED]);
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
fn test_pvpanic_lifecycle_and_mom_identity() {
    let pvp = Pvpanic::new(PvpanicEvent::PANICKED);
    assert!(!pvp.realized());
    pvp.with_mdevice(|device| assert_eq!(device.local_id(), "pvpanic"));
    assert_eq!(pvp.object_info().local_id, "pvpanic");

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

    // Second realize_onto fails (already realized)
    let err = pvp.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

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
fn test_unimp_lifecycle_and_mom_identity() {
    let unimp = Unimp::new("test-device", 0x1000);
    assert!(!unimp.realized());
    unimp.with_mdevice(|device| assert_eq!(device.local_id(), "test-device"));
    assert_eq!(unimp.object_info().local_id, "test-device");

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA(0x1000_0000);

    // Pre-realize: unmapped
    assert_eq!(aspace.read(base, 4), 0);

    unimp.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "test-device",
        0x1000,
        Arc::new(UnimpMmio(unimp.clone())),
    );
    unimp.register_mmio(region, base).unwrap();
    unimp.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(unimp.realized());

    // Realized: reads return 0 and writes don't panic
    assert_eq!(aspace.read(base, 4), 0);
    aspace.write(base, 4, 0xDEAD_BEEF);

    // Second realize_onto fails (already realized)
    let err = unimp.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

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
        MemoryRegion::io("zero-device", 0, Arc::new(UnimpMmio(unimp.clone())));
    unimp.register_mmio(region, GPA(0x1000_0000)).unwrap();
    let err = unimp.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("must be non-zero"));
    assert!(!unimp.realized());
}

#[test]
fn test_unimp_rejects_name_mismatch() {
    let unimp = Unimp::new("test-device", 0x1000);
    let (mut _aspace, mut bus) = make_test_aspace();
    unimp.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "wrong-name",
        0x1000,
        Arc::new(UnimpMmio(unimp.clone())),
    );
    let err = unimp.register_mmio(region, GPA(0x1000_0000)).unwrap_err();
    assert!(err.to_string().contains("must match"));
}

#[test]
fn test_unimp_rejects_size_mismatch() {
    let unimp = Unimp::new("test-device", 0x1000);
    let (mut _aspace, mut bus) = make_test_aspace();
    unimp.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "test-device",
        0x2000,
        Arc::new(UnimpMmio(unimp.clone())),
    );
    let err = unimp.register_mmio(region, GPA(0x1000_0000)).unwrap_err();
    assert!(err.to_string().contains("must match"));
}

#[test]
fn test_unimp_realize_with_correct_properties() {
    let unimp = Unimp::new("prop-device", 0x800);
    assert!(!unimp.realized());

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA(0x2000_0000);
    assert_eq!(aspace.read(base, 4), 0);

    unimp.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "prop-device",
        0x800,
        Arc::new(UnimpMmio(unimp.clone())),
    );
    unimp.register_mmio(region, base).unwrap();
    unimp.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(unimp.realized());

    // Realized: read returns 0 with the propagated region
    assert_eq!(aspace.read(base, 4), 0);
    aspace.write(base, 4, 0xFEED_FACE);

    unimp.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!unimp.realized());
    assert_eq!(aspace.read(base, 4), 0);
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
fn test_led_lifecycle_and_mom_identity() {
    let led = Led::new(LedColor::Green, "lifecycle", true);
    assert!(!led.realized());
    led.with_mdevice(|device| assert_eq!(device.local_id(), "led"));
    assert_eq!(led.object_info().local_id, "led");

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
fn test_virt_ctrl_lifecycle_and_mom_identity() {
    let vc = VirtCtrl::new();
    assert!(!vc.realized());
    vc.with_mdevice(|device| assert_eq!(device.local_id(), "virt_ctrl"));
    assert_eq!(vc.object_info().local_id, "virt_ctrl");

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

    // Second realize_onto fails (already realized)
    let err = vc.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

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

// ---- PL050 tests ----

struct Sink {
    level: std::sync::Mutex<bool>,
}

impl Sink {
    fn new() -> Self {
        Self {
            level: std::sync::Mutex::new(false),
        }
    }

    fn level(&self) -> bool {
        *self.level.lock().unwrap()
    }
}

impl IrqSink for Sink {
    fn set_irq(&self, _irq: u32, level: bool) {
        *self.level.lock().unwrap() = level;
    }
}

#[test]
fn test_pl050_lifecycle_and_mom_identity() {
    let pl050 = Arc::new(Pl050::new());
    assert!(!pl050.realized());
    pl050.with_mdevice(|device| assert_eq!(device.local_id(), "pl050"));
    assert_eq!(pl050.object_info().local_id, "pl050");

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA(0x1000);
    let region = MemoryRegion::io(
        "pl050",
        0x1000,
        Arc::new(Pl050Mmio(Arc::clone(&pl050))),
    );

    pl050.attach_to_bus(&mut bus).unwrap();
    pl050.register_mmio(region, base).unwrap();
    pl050.realize_onto(&mut bus, &mut aspace).unwrap();

    assert!(pl050.realized());
    assert_eq!(aspace.read(GPA(base.0 + 0xfe0), 4), 0x50);

    let err = pl050.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));
}

#[test]
fn test_pl050_defaults() {
    let pl050 = Arc::new(Pl050::new());
    let mmio = Pl050Mmio(Arc::clone(&pl050));

    assert_eq!(mmio.read(0x00, 4), 0); // CR
    assert_eq!(mmio.read(0x0C, 4), 0); // CLK
    assert_eq!(mmio.read(0x10, 4), 0x02); // IIR (pending|2)
}

#[test]
fn test_pl050_id_registers() {
    let pl050 = Arc::new(Pl050::new());
    let mmio = Pl050Mmio(Arc::clone(&pl050));

    assert_eq!(mmio.read(0xFE0, 4), 0x50);
    assert_eq!(mmio.read(0xFE4, 4), 0x10);
    assert_eq!(mmio.read(0xFE8, 4), 0x04);
    assert_eq!(mmio.read(0xFEC, 4), 0x00);
    assert_eq!(mmio.read(0xFF0, 4), 0x0D);
    assert_eq!(mmio.read(0xFF4, 4), 0xF0);
    assert_eq!(mmio.read(0xFF8, 4), 0x05);
    assert_eq!(mmio.read(0xFFC, 4), 0xB1);
}

#[test]
fn test_pl050_stat_txempty() {
    let pl050 = Arc::new(Pl050::new());
    let mmio = Pl050Mmio(Arc::clone(&pl050));

    let stat = mmio.read(0x04, 4) as u32;
    assert!(stat & 0x40 != 0); // TXEMPTY set
}

#[test]
fn test_pl050_wide_mmio_read_splits_into_32bit_callbacks() {
    let pl050 = Arc::new(Pl050::new());
    let mmio = Pl050Mmio(Arc::clone(&pl050));

    mmio.write(0x00, 4, 0x12);

    assert_eq!(mmio.read(0x00, 8), 0x0000_0040_0000_0012);
}

#[test]
fn test_pl050_cr_enables_irq() {
    let pl050 = Arc::new(Pl050::new());
    let mmio = Pl050Mmio(Arc::clone(&pl050));
    let sink = Arc::new(Sink::new());
    let irq = InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0);
    pl050.connect_irq(irq);

    assert!(!sink.level());

    // Set CR bit 4 (RX enable) — IRQ asserts if pending && cr & 0x10
    // With no pending, bit 4 alone shouldn't trigger unless bit 3 also set
    mmio.write(0x00, 4, 0x10);
    assert!(!sink.level());

    // Set CR bit 3 — direct IRQ assert
    mmio.write(0x00, 4, 0x08);
    assert!(sink.level());
}

#[test]
fn test_pl050_ps2_irq_input() {
    let pl050 = Arc::new(Pl050::new());
    let mmio = Pl050Mmio(Arc::clone(&pl050));
    let sink = Arc::new(Sink::new());
    let irq = InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0);
    pl050.connect_irq(irq);

    // Enable RX interrupt (CR bit 4)
    mmio.write(0x00, 4, 0x10);
    assert!(!sink.level());

    // PS2 IRQ input sets pending
    pl050.set_ps2_irq(true);
    assert!(sink.level());

    // Deassert
    pl050.set_ps2_irq(false);
    assert!(!sink.level());
}

#[test]
fn test_pl050_clk_write() {
    let pl050 = Arc::new(Pl050::new());
    let mmio = Pl050Mmio(Arc::clone(&pl050));

    mmio.write(0x0C, 4, 0x1234);
    assert_eq!(mmio.read(0x0C, 4), 0x1234);
}

#[test]
fn test_pl050_data_write_returns_ps2_resend_response() {
    let pl050 = Arc::new(Pl050::new());
    let mmio = Pl050Mmio(Arc::clone(&pl050));

    mmio.write(0x08, 4, 0xAB);

    assert_eq!(mmio.read(0x04, 4), 0x50);
    assert_eq!(mmio.read(0x08, 4), 0xFE);
    assert_eq!(mmio.read(0x04, 4), 0x44);
    assert_eq!(mmio.read(0x10, 4), 0x02);
}

#[test]
fn test_pl050_wide_mmio_write_splits_into_32bit_callbacks() {
    let pl050 = Arc::new(Pl050::new());
    let mmio = Pl050Mmio(Arc::clone(&pl050));

    mmio.write(0x08, 8, 0x1234_5678_0000_00ab);

    assert_eq!(mmio.read(0x04, 4), 0x50);
    assert_eq!(mmio.read(0x08, 4), 0xfe);
    assert_eq!(mmio.read(0x0c, 4), 0x1234_5678);
    assert_eq!(mmio.read(0x10, 4), 0x02);
}

#[test]
fn test_pl050_unaligned_wide_accesses_split_like_qemu() {
    let pl050 = Arc::new(Pl050::new());
    let mmio = Pl050Mmio(Arc::clone(&pl050));

    mmio.write(0x00, 4, 0);
    mmio.write(0x01, 4, 0x0102_0304);
    assert_eq!(mmio.read(0x00, 4), 0x0203);
    assert_eq!(mmio.read(0x04, 4), 0x40);

    mmio.write(0x0c, 4, 0);
    mmio.write(0x0d, 4, 0x0102_0304);
    assert_eq!(mmio.read(0x0c, 4), 0x0203);
    assert_eq!(mmio.read(0x10, 4), 0x02);

    assert_eq!(mmio.read(0xfe1, 4), 0x1000_5050);
    assert_eq!(mmio.read(0xfe2, 4), 0x0010_0050);
    assert_eq!(mmio.read(0xfe3, 4), 0x1000_1050);
}

#[test]
fn test_pl050_narrow_accesses_use_access_width_bits() {
    let pl050 = Arc::new(Pl050::new());
    let mmio = Pl050Mmio(Arc::clone(&pl050));

    mmio.write(0x00, 4, 0x1234_5678);
    assert_eq!(mmio.read(0x00, 1), 0x78);
    assert_eq!(mmio.read(0x00, 2), 0x5678);
    assert_eq!(mmio.read(0x01, 1), 0x78);
    assert_eq!(mmio.read(0x02, 2), 0x5678);

    mmio.write(0x00, 1, 0x1234);
    assert_eq!(mmio.read(0x00, 4), 0x34);
    mmio.write(0x0c, 2, 0x1234_5678);
    assert_eq!(mmio.read(0x0c, 4), 0x5678);
}

#[test]
fn test_pl050_reset_runtime() {
    let pl050 = Arc::new(Pl050::new());
    let mmio = Pl050Mmio(Arc::clone(&pl050));

    mmio.write(0x00, 4, 0x18);
    mmio.write(0x0C, 4, 0x5555);

    pl050.reset_runtime();

    assert_eq!(mmio.read(0x00, 4), 0);
    assert_eq!(mmio.read(0x0C, 4), 0);
}

// -- SiFive E AON tests --

#[test]
fn test_sifive_e_aon_lifecycle_and_mom_identity() {
    let aon = Arc::new(SiFiveEAon::default());
    assert!(!aon.realized());
    aon.with_mdevice(|device| assert_eq!(device.local_id(), "sifive_e_aon"));
    assert_eq!(aon.object_info().local_id, "sifive_e_aon");

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA(0x2000);
    let region = MemoryRegion::io(
        "sifive_e_aon",
        0x1000,
        Arc::new(SiFiveEAonMmio(Arc::clone(&aon))),
    );

    aon.attach_to_bus(&mut bus).unwrap();
    aon.register_mmio(region, base).unwrap();
    aon.realize_onto(&mut bus, &mut aspace).unwrap();

    assert!(aon.realized());
    assert_eq!(aspace.read(GPA(base.0 + 0x20), 4), 0xbeef);

    let err = aon.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));
}

#[test]
fn test_sifive_e_aon_defaults() {
    let aon = Arc::new(SiFiveEAon::default());
    let mmio = SiFiveEAonMmio(Arc::clone(&aon));

    assert_eq!(mmio.read(0x00, 4), 0); // WDOGCFG
    assert_eq!(mmio.read(0x20, 4), 0xbeef); // WDOGCMP0
}

#[test]
fn test_sifive_e_aon_rejects_non_4byte_mmio_accesses() {
    let aon = Arc::new(SiFiveEAon::default());
    let mmio = SiFiveEAonMmio(Arc::clone(&aon));

    assert_eq!(mmio.read(0x20, 1), 0);
    assert_eq!(mmio.read(0x20, 2), 0);
    assert_eq!(mmio.read(0x20, 8), 0);

    mmio.write(0x1C, 1, 0x51F1_5E);
    mmio.write(0x1C, 2, 0x51F1_5E);
    mmio.write(0x1C, 8, 0x51F1_5E);
    assert_eq!(mmio.read(0x1C, 4), 0);
}

#[test]
fn test_sifive_e_aon_key_unlock_flow() {
    let aon = Arc::new(SiFiveEAon::default());
    let mmio = SiFiveEAonMmio(Arc::clone(&aon));

    // WDOGKEY starts at 0
    assert_eq!(mmio.read(0x1C, 4), 0);

    // Write unlock key
    mmio.write(0x1C, 4, 0x51F1_5E);
    assert_eq!(mmio.read(0x1C, 4), 1); // unlocked
}

#[test]
fn test_sifive_e_aon_cfg_write_needs_unlock() {
    let aon = Arc::new(SiFiveEAon::default());
    let mmio = SiFiveEAonMmio(Arc::clone(&aon));

    // Write WDOGCFG without unlock — should be ignored
    mmio.write(0x00, 4, 0x1000);
    assert_eq!(mmio.read(0x00, 4), 0);

    // Unlock then write
    mmio.write(0x1C, 4, 0x51F1_5E);
    mmio.write(0x00, 4, 0x1000); // EN_ALWAYS
    assert_eq!(mmio.read(0x00, 4), 0x1000);
}

#[test]
fn test_sifive_e_aon_feed_resets_count() {
    let aon = Arc::new(SiFiveEAon::default());
    let mmio = SiFiveEAonMmio(Arc::clone(&aon));

    // Unlock, set EN_ALWAYS, set non-zero count
    mmio.write(0x1C, 4, 0x51F1_5E);
    mmio.write(0x00, 4, 0x1000); // EN_ALWAYS
    mmio.write(0x1C, 4, 0x51F1_5E);
    mmio.write(0x08, 4, 100); // WDOGCOUNT

    // Feed
    mmio.write(0x1C, 4, 0x51F1_5E);
    mmio.write(0x18, 4, 0xD09F_00D);

    // Counter should be reset to 0
    assert_eq!(mmio.read(0x08, 4), 0);
}

#[test]
fn test_sifive_e_aon_wdogs_uses_four_bit_scale_field() {
    let aon = Arc::new(SiFiveEAon::default());
    let mmio = SiFiveEAonMmio(Arc::clone(&aon));

    mmio.write(0x1C, 4, 0x51F1_5E);
    mmio.write(0x08, 4, 0x80);
    mmio.write(0x1C, 4, 0x51F1_5E);
    mmio.write(0x00, 4, 0x03);

    assert_eq!(mmio.read(0x10, 4), 0x10);
}

#[test]
fn test_sifive_e_aon_cmp_ip_trigger() {
    let aon = Arc::new(SiFiveEAon::default());
    let mmio = SiFiveEAonMmio(Arc::clone(&aon));
    let sink = Arc::new(Sink::new());
    let irq = InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0);
    aon.connect_irq(irq);

    // Set EN_ALWAYS, set low compare
    mmio.write(0x1C, 4, 0x51F1_5E);
    mmio.write(0x00, 4, 0x1000); // EN_ALWAYS
    mmio.write(0x1C, 4, 0x51F1_5E);
    mmio.write(0x20, 4, 0); // WDOGCMP0 = 0

    // Count >= 0 cmp → IP should be set
    let cfg = mmio.read(0x00, 4) as u32;
    assert!(cfg & (1 << 28) != 0); // IP0 set

    // IRQ should be asserted
    assert!(sink.level());
}

#[test]
fn test_sifive_e_aon_rtc_region_unimplemented() {
    let aon = Arc::new(SiFiveEAon::default());
    let mmio = SiFiveEAonMmio(Arc::clone(&aon));

    // RTC and beyond are unimplemented, return 0 on read
    assert_eq!(mmio.read(0x40, 4), 0);
    assert_eq!(mmio.read(0x70, 4), 0);
    assert_eq!(mmio.read(0x80, 4), 0);
    assert_eq!(mmio.read(0x100, 4), 0);

    // Beyond max
    assert_eq!(mmio.read(0x150, 4), 0);
}

#[test]
fn test_sifive_e_aon_reset_runtime() {
    let aon = Arc::new(SiFiveEAon::default());
    let mmio = SiFiveEAonMmio(Arc::clone(&aon));

    // Unlock and set config
    mmio.write(0x1C, 4, 0x51F1_5E);
    mmio.write(0x00, 4, 0x1000); // EN_ALWAYS
    mmio.write(0x1C, 4, 0x51F1_5E);
    mmio.write(0x20, 4, 0x1234); // WDOGCMP0

    aon.reset_runtime();

    // EN_ALWAYS should be cleared
    let cfg = mmio.read(0x00, 4) as u32;
    assert!(cfg & 0x1000 == 0);

    // WDOGCMP0 reset to 0xbeef
    assert_eq!(mmio.read(0x20, 4), 0xbeef);
}

// -- SiFive U OTP tests --

#[test]
fn test_sifive_u_otp_lifecycle_and_mom_identity() {
    let otp = Arc::new(SiFiveUOtp::new());
    assert!(!otp.realized());
    otp.with_mdevice(|device| assert_eq!(device.local_id(), "sifive_u_otp"));
    assert_eq!(otp.object_info().local_id, "sifive_u_otp");

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA(0x3000);
    let region = MemoryRegion::io(
        "sifive_u_otp",
        0x1000,
        Arc::new(SiFiveUOtpMmio(Arc::clone(&otp))),
    );

    otp.attach_to_bus(&mut bus).unwrap();
    otp.register_mmio(region, base).unwrap();
    otp.realize_onto(&mut bus, &mut aspace).unwrap();

    assert!(otp.realized());
    aspace.write(base, 4, 0x1234);
    assert_eq!(aspace.read(base, 4), 0x234);

    let err = otp.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));
}

#[test]
fn test_sifive_u_otp_defaults() {
    let otp = Arc::new(SiFiveUOtp::new());
    let mmio = SiFiveUOtpMmio(Arc::clone(&otp));

    assert_eq!(mmio.read(0x00, 4), 0); // PA
    assert_eq!(mmio.read(0x04, 4), 0); // PAIO
    assert_eq!(mmio.read(0x08, 4), 0); // PAS
    assert_eq!(mmio.read(0x0C, 4), 0); // PCE
    assert_eq!(mmio.read(0x10, 4), 0); // PCLK
    assert_eq!(mmio.read(0x14, 4), 0); // PDIN
    assert_eq!(mmio.read(0x1C, 4), 0); // PDSTB
    assert_eq!(mmio.read(0x20, 4), 0); // PPROG
    assert_eq!(mmio.read(0x34, 4), 0); // PTRIM
    assert_eq!(mmio.read(0x38, 4), 0); // PWE
}

#[test]
fn test_sifive_u_otp_pa_masked() {
    let otp = Arc::new(SiFiveUOtp::new());
    let mmio = SiFiveUOtpMmio(Arc::clone(&otp));

    mmio.write(0x00, 4, 0xDEAD);
    assert_eq!(mmio.read(0x00, 4), 0xDEAD & 0xFFF);
}

#[test]
fn test_sifive_u_otp_rejects_non_4byte_mmio_accesses() {
    let otp = Arc::new(SiFiveUOtp::new());
    let mmio = SiFiveUOtpMmio(Arc::clone(&otp));

    assert_eq!(mmio.read(0x18, 1), 0);
    assert_eq!(mmio.read(0x18, 2), 0);
    assert_eq!(mmio.read(0x18, 8), 0);

    mmio.write(0x00, 1, 0x12);
    mmio.write(0x00, 2, 0x3456);
    mmio.write(0x00, 8, 0x1234_5678);
    assert_eq!(mmio.read(0x00, 4), 0);
}

#[test]
fn test_sifive_u_otp_pdout_no_enable_returns_ff() {
    let otp = Arc::new(SiFiveUOtp::new());
    let mmio = SiFiveUOtpMmio(Arc::clone(&otp));

    // Without PCE/PDSTB/PTRIM enables, PDOUT returns 0xFF
    assert_eq!(mmio.read(0x18, 4), 0xFF);
}

#[test]
fn test_sifive_u_otp_pdout_with_enables() {
    let otp = Arc::new(SiFiveUOtp::new());
    let mmio = SiFiveUOtpMmio(Arc::clone(&otp));

    // Enable reading
    mmio.write(0x0C, 4, 1); // PCE_EN
    mmio.write(0x1C, 4, 1); // PDSTB_EN
    mmio.write(0x34, 4, 1); // PTRIM_EN

    // Set address 0
    mmio.write(0x00, 4, 0);

    // Should read fuse[0] = 0xFFFF_FFFF
    assert_eq!(mmio.read(0x18, 4), 0xFFFF_FFFF);
}

#[test]
fn test_sifive_u_otp_serial_number() {
    let otp = Arc::new(SiFiveUOtp::with_serial(0x1234_5678));
    let mmio = SiFiveUOtpMmio(Arc::clone(&otp));

    // Enable reading
    mmio.write(0x0C, 4, 1); // PCE_EN
    mmio.write(0x1C, 4, 1); // PDSTB_EN
    mmio.write(0x34, 4, 1); // PTRIM_EN

    // Read serial address
    mmio.write(0x00, 4, 0xFC);
    assert_eq!(mmio.read(0x18, 4), 0x1234_5678);

    mmio.write(0x00, 4, 0xFD);
    assert_eq!(mmio.read(0x18, 4), u64::from(!(0x1234_5678u32)));
}

#[test]
fn test_sifive_u_otp_pwe_write() {
    let otp = Arc::new(SiFiveUOtp::new());
    let mmio = SiFiveUOtpMmio(Arc::clone(&otp));

    // Set up write
    mmio.write(0x00, 4, 0); // PA=0
    mmio.write(0x04, 4, 5); // PAIO=5 (bit 5)
    mmio.write(0x08, 4, 0); // PAS=0 (no redundancy)
    mmio.write(0x14, 4, 1); // PDIN=1 (set bit)

    // Enable reading for verification
    mmio.write(0x0C, 4, 1); // PCE_EN
    mmio.write(0x1C, 4, 1); // PDSTB_EN
    mmio.write(0x34, 4, 1); // PTRIM_EN

    // Before write, fuse[0] = 0xFFFF_FFFF
    assert_eq!(mmio.read(0x18, 4), 0xFFFF_FFFF);

    // Clear bit 5: PDIN=0, then PWE=1
    mmio.write(0x04, 4, 5); // PAIO=5
    mmio.write(0x14, 4, 0); // PDIN=0
    mmio.write(0x38, 4, 0x01);

    // Verify bit 5 cleared: 0xFFFF_FFFF & ~(1<<5) = 0xFFFF_FFDF
    assert_eq!(mmio.read(0x18, 4), 0xFFFF_FFDF);
}

#[test]
fn test_sifive_u_otp_pdin_value_is_shifted_into_fuse() {
    let otp = Arc::new(SiFiveUOtp::with_serial(1));
    let mmio = SiFiveUOtpMmio(Arc::clone(&otp));

    mmio.write(0x00, 4, 0xfc); // PA=serial fuse index
    mmio.write(0x04, 4, 5); // PAIO=5
    mmio.write(0x14, 4, 2); // PDIN=2
    mmio.write(0x38, 4, 1); // PWE_EN

    mmio.write(0x0c, 4, 1); // PCE_EN
    mmio.write(0x1c, 4, 1); // PDSTB_EN
    mmio.write(0x34, 4, 1); // PTRIM_EN

    assert_eq!(mmio.read(0x18, 4), 0x41);
}

#[test]
fn test_sifive_u_otp_pwe_write_once() {
    let otp = Arc::new(SiFiveUOtp::new());
    let mmio = SiFiveUOtpMmio(Arc::clone(&otp));

    // First write
    mmio.write(0x00, 4, 0); // PA=0
    mmio.write(0x04, 4, 3); // PAIO=3
    mmio.write(0x08, 4, 0); // PAS=0
    mmio.write(0x14, 4, 1); // PDIN=1
    mmio.write(0x38, 4, 0x01); // PWE=1

    // Try to write same bit again
    mmio.write(0x14, 4, 0); // PDIN=0 (try to clear)
    mmio.write(0x38, 4, 0x01); // PWE=1

    // Enable reading
    mmio.write(0x0C, 4, 1); // PCE_EN
    mmio.write(0x1C, 4, 1); // PDSTB_EN
    mmio.write(0x34, 4, 1); // PTRIM_EN

    // Bit should still be set (write-once protects)
    assert_eq!(mmio.read(0x18, 4) as u32 & (1 << 3), 1 << 3);
}

#[test]
fn test_sifive_u_otp_pdout_read_only() {
    let otp = Arc::new(SiFiveUOtp::new());
    let mmio = SiFiveUOtpMmio(Arc::clone(&otp));

    mmio.write(0x18, 4, 0xDEAD_BEEF);
    // PDOUT is read-only, value depends on enables
    assert_eq!(mmio.read(0x18, 4), 0xFF);
}
