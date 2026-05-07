use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_watchdog::{
    SbsaGwdt, SbsaGwdtControlMmio, SbsaGwdtRefreshMmio, SBSA_GWDT_CONTROL_SIZE,
    SBSA_GWDT_REFRESH_SIZE,
};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MemoryRegion;

const WRR: u64 = 0x000;
const WCS: u64 = 0x000;
const WOR: u64 = 0x008;
const WORU: u64 = 0x00c;
const WCV: u64 = 0x010;
const WCVU: u64 = 0x014;
const W_IIDR: u64 = 0xfcc;

const WCS_EN: u32 = 1 << 0;
const WCS_WS0: u32 = 1 << 1;
const WCS_WS1: u32 = 1 << 2;
const WATCHDOG_ID: u32 = 0x1043b;
const DEFAULT_CLOCK_FREQUENCY: u64 = 62_500_000;
const SBSA_REF_CLOCK_FREQUENCY: u64 = 1_000_000_000;

struct RecordingSink {
    level: Mutex<bool>,
}

impl RecordingSink {
    fn new() -> Self {
        Self {
            level: Mutex::new(false),
        }
    }

    fn level(&self) -> bool {
        *self.level.lock().unwrap()
    }
}

impl IrqSink for RecordingSink {
    fn set_irq(&self, _irq: u32, level: bool) {
        *self.level.lock().unwrap() = level;
    }
}

fn make_test_aspace() -> (AddressSpace, SysBus) {
    let root = MemoryRegion::container("root", 0x1_0000_0000);
    let aspace = AddressSpace::new(root);
    let bus = SysBus::new("sysbus");
    (aspace, bus)
}

#[test]
fn test_sbsa_gwdt_defaults_and_id() {
    let wdt = SbsaGwdt::new();

    assert_eq!(wdt.control_read(WCS, 4), 0);
    assert_eq!(wdt.control_read(WOR, 4), 0);
    assert_eq!(wdt.control_read(WORU, 4), 0);
    assert_eq!(wdt.control_read(WCV, 4), 0);
    assert_eq!(wdt.control_read(WCVU, 4), 0);
    assert_eq!(wdt.control_read(W_IIDR, 4) as u32, WATCHDOG_ID);
    assert_eq!(wdt.refresh_read(WRR, 4), 0);
    assert_eq!(wdt.refresh_read(W_IIDR, 4) as u32, WATCHDOG_ID);
}

#[test]
fn test_sbsa_gwdt_control_writes_mask_and_clear_status() {
    let wdt = SbsaGwdt::new();

    wdt.control_write(WCS, 4, 0xffff_ffff);
    assert_eq!(wdt.control_read(WCS, 4) as u32, WCS_EN);

    wdt.trigger_timeout();
    assert_eq!(wdt.control_read(WCS, 4) as u32, WCS_EN | WCS_WS0);

    wdt.control_write(WOR, 4, 0x1234_5678);
    assert_eq!(wdt.control_read(WOR, 4) as u32, 0x1234_5678);
    assert_eq!(wdt.control_read(WCS, 4) as u32, WCS_EN);

    wdt.trigger_timeout();
    wdt.control_write(WORU, 4, 0xaaaa_5555);
    assert_eq!(wdt.control_read(WORU, 4) as u32, 0x5555);
    assert_eq!(wdt.control_read(WCS, 4) as u32, WCS_EN);

    wdt.control_write(WCV, 4, 0xfeed_cafe);
    wdt.control_write(WCVU, 4, 0x1234_5678);
    assert_eq!(wdt.control_read(WCV, 4) as u32, 0xfeed_cafe);
    assert_eq!(wdt.control_read(WCVU, 4) as u32, 0x1234_5678);
}

#[test]
fn test_sbsa_gwdt_timeout_irq_and_refresh_status() {
    let wdt = SbsaGwdt::new();
    let sink = Arc::new(RecordingSink::new());
    wdt.connect_irq(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    wdt.control_write(WCS, 4, u64::from(WCS_EN));
    assert!(!sink.level());

    wdt.trigger_timeout();
    assert_eq!(wdt.control_read(WCS, 4) as u32, WCS_EN | WCS_WS0);
    assert!(sink.level());

    wdt.refresh_write(WRR, 4, 0);
    assert_eq!(wdt.control_read(WCS, 4) as u32, WCS_EN);
    assert!(sink.level());

    wdt.control_write(WCS, 4, u64::from(WCS_EN));
    assert!(!sink.level());

    wdt.trigger_timeout();
    wdt.trigger_timeout();
    assert_eq!(wdt.control_read(WCS, 4) as u32, WCS_EN | WCS_WS0 | WCS_WS1);
}

#[test]
fn test_sbsa_gwdt_ptimer_steps_to_warning_and_second_stage() {
    let wdt = SbsaGwdt::new();
    let sink = Arc::new(RecordingSink::new());
    wdt.connect_irq(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    wdt.control_write(WOR, 4, 2);
    wdt.control_write(WCS, 4, u64::from(WCS_EN));

    assert_eq!(wdt.control_read(WCV, 4), 32);
    assert_eq!(wdt.control_read(WCVU, 4), 0);
    assert_eq!(wdt.step_timer(1), 0);
    assert_eq!(wdt.control_read(WCS, 4) as u32, WCS_EN);
    assert!(!sink.level());

    assert_eq!(wdt.step_timer(1), 1);
    assert_eq!(wdt.control_read(WCS, 4) as u32, WCS_EN | WCS_WS0);
    assert!(sink.level());

    assert_eq!(wdt.step_timer(2), 1);
    assert_eq!(wdt.control_read(WCS, 4) as u32, WCS_EN | WCS_WS0 | WCS_WS1);
    assert!(sink.level());
    assert_eq!(wdt.step_timer(2), 0);
}

#[test]
fn test_sbsa_gwdt_enable_stores_compare_value_in_virtual_ns() {
    let wdt = SbsaGwdt::new();

    wdt.control_write(WOR, 4, DEFAULT_CLOCK_FREQUENCY);
    wdt.control_write(WCS, 4, u64::from(WCS_EN));

    assert_eq!(wdt.control_read(WCV, 4), 1_000_000_000);
    assert_eq!(wdt.control_read(WCVU, 4), 0);
}

#[test]
fn test_sbsa_gwdt_clock_frequency_controls_compare_value() {
    let wdt =
        SbsaGwdt::new_with_clock_frequency(SBSA_REF_CLOCK_FREQUENCY as u32);

    wdt.control_write(WOR, 4, DEFAULT_CLOCK_FREQUENCY);
    wdt.control_write(WCS, 4, u64::from(WCS_EN));

    assert_eq!(wdt.control_read(WCV, 4), DEFAULT_CLOCK_FREQUENCY);
    assert_eq!(wdt.control_read(WCVU, 4), 0);
}

#[test]
fn test_sbsa_gwdt_reset_runtime_restores_defaults_and_lowers_irq() {
    let wdt = SbsaGwdt::new();
    let sink = Arc::new(RecordingSink::new());
    wdt.connect_irq(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    wdt.control_write(WCS, 4, u64::from(WCS_EN));
    wdt.trigger_timeout();
    assert!(sink.level());

    wdt.reset_runtime();

    assert_eq!(wdt.control_read(WCS, 4), 0);
    assert_eq!(wdt.control_read(WOR, 4), 0);
    assert_eq!(wdt.control_read(WORU, 4), 0);
    assert_eq!(wdt.control_read(W_IIDR, 4) as u32, WATCHDOG_ID);
    assert!(!sink.level());
}

#[test]
fn test_sbsa_gwdt_lifecycle_and_mom_identity() {
    let wdt = SbsaGwdt::new();
    assert!(!wdt.realized());
    wdt.with_mdevice(|device| assert_eq!(device.local_id(), "sbsa-gwdt"));
    assert_eq!(wdt.object_info().local_id, "sbsa-gwdt");

    let (mut aspace, mut bus) = make_test_aspace();
    let refresh_base = GPA(0x1000_0000);
    let control_base = GPA(0x1001_0000);

    wdt.attach_to_bus(&mut bus).unwrap();
    wdt.register_mmio(
        MemoryRegion::io(
            "sbsa-gwdt-refresh",
            SBSA_GWDT_REFRESH_SIZE,
            Arc::new(SbsaGwdtRefreshMmio(wdt.clone())),
        ),
        refresh_base,
    )
    .unwrap();
    wdt.register_mmio(
        MemoryRegion::io(
            "sbsa-gwdt-control",
            SBSA_GWDT_CONTROL_SIZE,
            Arc::new(SbsaGwdtControlMmio(wdt.clone())),
        ),
        control_base,
    )
    .unwrap();
    wdt.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(wdt.realized());

    assert_eq!(
        aspace.read(GPA(control_base.0 + W_IIDR), 4) as u32,
        WATCHDOG_ID
    );
    aspace.write(GPA(control_base.0 + WOR), 4, 0x1234);
    assert_eq!(aspace.read(GPA(control_base.0 + WOR), 4), 0x1234);
    assert_eq!(aspace.read(refresh_base, 4), 0);

    let err = wdt.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

    wdt.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!wdt.realized());
    assert_eq!(aspace.read(GPA(control_base.0 + W_IIDR), 4), 0);
}
