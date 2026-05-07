use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_watchdog::k230::{
    K230Wdt, K230WdtMmio, CCVR, COMP_PARAM_1, COMP_PARAM_1_VAL, COMP_PARAM_2,
    COMP_TYPE, COMP_TYPE_VAL, COMP_VERSION, COMP_VERSION_VAL, CR, CRR,
    CRR_RESTART, CR_RMOD, CR_RPL_MASK, CR_RPL_SHIFT, CR_WDT_EN, EOI, MMIO_SIZE,
    PROT_LEVEL, STAT, STAT_INT, TORR, TORR_TOP_MASK,
};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

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
fn k230_wdt_masks_writable_registers_and_reports_ids() {
    let wdt = K230Wdt::new_named("k230-wdt0");
    let mmio = K230WdtMmio(wdt);

    mmio.write(CR, 4, u64::MAX);
    assert_eq!(
        mmio.read(CR, 4),
        ((CR_RPL_MASK << CR_RPL_SHIFT) | CR_RMOD | CR_WDT_EN) as u64
    );

    mmio.write(TORR, 4, u64::MAX);
    assert_eq!(mmio.read(TORR, 4), TORR_TOP_MASK as u64);

    mmio.write(PROT_LEVEL, 4, u64::MAX);
    assert_eq!(mmio.read(PROT_LEVEL, 4), 0x7);

    assert_eq!(COMP_PARAM_1_VAL, 0x2000_0e40);
    assert_eq!(mmio.read(COMP_PARAM_2, 4), u64::from(u32::MAX));
    assert_eq!(mmio.read(COMP_PARAM_1, 4), u64::from(COMP_PARAM_1_VAL));
    assert_eq!(mmio.read(COMP_VERSION, 4), u64::from(COMP_VERSION_VAL));
    assert_eq!(mmio.read(COMP_TYPE, 4), u64::from(COMP_TYPE_VAL));
    assert_eq!(mmio.read(CR, 2), 0);
}

#[test]
fn k230_wdt_interrupt_mode_sets_and_clears_status() {
    let wdt = K230Wdt::new_named("k230-wdt0");
    let sink = Arc::new(RecordingSink::new());
    wdt.connect_irq(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));
    let mmio = K230WdtMmio(wdt.clone());

    mmio.write(TORR, 4, 1);
    mmio.write(CR, 4, u64::from(CR_RMOD | CR_WDT_EN));
    assert_eq!(mmio.read(CCVR, 4), 1 << 17);

    assert_eq!(wdt.step_timer(1 << 17), 1);
    assert_eq!(
        mmio.read(STAT, 4) & u64::from(STAT_INT),
        u64::from(STAT_INT)
    );
    assert!(sink.level());

    mmio.write(EOI, 4, 1);
    assert_eq!(mmio.read(STAT, 4) & u64::from(STAT_INT), 0);
    assert!(!sink.level());
}

#[test]
fn k230_wdt_restart_magic_clears_pending_interrupt() {
    let wdt = K230Wdt::new_named("k230-wdt0");
    let sink = Arc::new(RecordingSink::new());
    wdt.connect_irq(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));
    let mmio = K230WdtMmio(wdt.clone());

    mmio.write(TORR, 4, 1);
    mmio.write(CR, 4, u64::from(CR_RMOD | CR_WDT_EN));
    assert_eq!(wdt.step_timer(1 << 17), 1);
    assert!(sink.level());

    mmio.write(CRR, 4, u64::from(CRR_RESTART));
    assert_eq!(mmio.read(STAT, 4) & u64::from(STAT_INT), 0);
    assert!(!sink.level());
}

#[test]
fn k230_wdt_lifecycle_and_mmio_region() {
    let wdt = K230Wdt::new_named("k230-wdt0");
    wdt.with_mdevice(|device| assert_eq!(device.local_id(), "k230-wdt0"));
    assert_eq!(wdt.object_info().local_id, "k230-wdt0");

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA(0x9110_0000);

    wdt.attach_to_bus(&mut bus).unwrap();
    wdt.register_mmio(
        MemoryRegion::io(
            "k230-wdt0-mmio",
            MMIO_SIZE,
            Arc::new(K230WdtMmio(wdt.clone())),
        ),
        base,
    )
    .unwrap();
    wdt.realize_onto(&mut bus, &mut aspace).unwrap();

    assert_eq!(
        aspace.read(GPA(base.0 + COMP_TYPE), 4),
        u64::from(COMP_TYPE_VAL)
    );
    aspace.write(GPA(base.0 + TORR), 4, 0xffff_ffff);
    assert_eq!(aspace.read(GPA(base.0 + TORR), 4), u64::from(TORR_TOP_MASK));

    wdt.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert_eq!(aspace.read(GPA(base.0 + COMP_TYPE), 4), 0);
}
