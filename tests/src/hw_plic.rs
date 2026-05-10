use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use machina_core::address::GPA;
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_intc::plic::{Plic, PlicMmio};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MemoryRegion;

struct TestIrqSink {
    levels: Vec<AtomicBool>,
}

impl TestIrqSink {
    fn new(n: usize) -> Self {
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(AtomicBool::new(false));
        }
        Self { levels: v }
    }

    fn level(&self, irq: u32) -> bool {
        self.levels[irq as usize].load(Ordering::Relaxed)
    }
}

impl IrqSink for TestIrqSink {
    fn set_irq(&self, irq: u32, level: bool) {
        if let Some(f) = self.levels.get(irq as usize) {
            f.store(level, Ordering::Relaxed);
        }
    }
}

fn make_address_space() -> AddressSpace {
    AddressSpace::new(MemoryRegion::container("system", u64::MAX))
}

#[test]
fn test_plic_set_priority() {
    let plic = Plic::new(64, 2);

    // Write priority for IRQ 1 via MMIO (offset 4).
    plic.write(0x04, 4, 7);
    assert_eq!(plic.read(0x04, 4), 7);

    // Write priority for IRQ 10 (offset 0x28).
    plic.write(0x28, 4, 3);
    assert_eq!(plic.read(0x28, 4), 3);
}

#[test]
fn test_plic_ignores_reserved_priority_zero_write() {
    let plic = Plic::new(64, 2);

    plic.write(0x00, 4, 7);

    assert_eq!(plic.read(0x00, 4), 0);
}

#[test]
fn test_plic_rejects_non_32_bit_mmio_accesses() {
    let plic = Plic::new(64, 2);

    plic.write(0x04, 4, 7);
    assert_eq!(plic.read(0x04, 4), 7);

    assert_eq!(plic.read(0x04, 1), 0);
    assert_eq!(plic.read(0x04, 2), 0);
    assert_eq!(plic.read(0x04, 8), 0);

    plic.write(0x04, 1, 0x55);
    plic.write(0x04, 2, 0x6666);
    plic.write(0x04, 8, 0x7777_7777);

    assert_eq!(plic.read(0x04, 4), 7);
}

#[test]
fn test_plic_rejects_unaligned_mmio_accesses() {
    let plic = Plic::new(64, 2);

    plic.write(0x14, 4, 7);
    assert_eq!(plic.read(0x14, 4), 7);

    assert_eq!(plic.read(0x15, 4), 0);

    plic.write(0x15, 4, 3);
    assert_eq!(plic.read(0x14, 4), 7);
}

#[test]
fn test_plic_claim_highest() {
    let plic = Plic::new(64, 2);

    // Set priorities: IRQ 1 = 2, IRQ 2 = 5.
    plic.write(0x04, 4, 2);
    plic.write(0x08, 4, 5);

    // Enable IRQ 1 and IRQ 2 for context 0.
    plic.write(0x2000, 4, 0x06);

    // Set both pending.
    plic.set_pending(1, true);
    plic.set_pending(2, true);

    // Claim should return IRQ 2 (higher priority).
    let claimed = plic.claim_irq(0);
    assert_eq!(claimed, Some(2));
}

#[test]
fn test_plic_complete() {
    let plic = Plic::new(64, 1);

    plic.write(0x04, 4, 1);
    plic.write(0x2000, 4, 0x02);
    plic.set_pending(1, true);

    let claimed = plic.claim_irq(0);
    assert_eq!(claimed, Some(1));

    // Complete via MMIO.
    plic.write(0x200004, 4, 1);

    // After completion, claim register should be 0.
    assert_eq!(plic.read(0x200004, 4), 0);
}

#[test]
fn test_plic_completion_clears_claimed_bitmap_for_valid_irq() {
    let plic = Plic::new(64, 2);

    plic.write(0x04, 4, 1);
    plic.write(0x2000, 4, 0x02);
    plic.set_pending(1, true);

    assert_eq!(plic.read(0x200004, 4), 1);

    plic.write(0x201004, 4, 1);
    plic.set_pending(1, true);

    assert_eq!(
        plic.read(0x200004, 4),
        1,
        "valid completion must clear claimed state even from another context"
    );
}

#[test]
fn test_plic_threshold() {
    let plic = Plic::new(64, 1);

    plic.write(0x04, 4, 3);
    plic.write(0x2000, 4, 0x02);
    plic.set_pending(1, true);

    // Set threshold for context 0 to 5.
    plic.write(0x200000, 4, 5);

    // IRQ 1 priority 3 <= threshold 5 -> not claimable.
    assert_eq!(plic.claim_irq(0), None);
}

#[test]
fn test_plic_no_pending() {
    let plic = Plic::new(64, 1);

    plic.write(0x04, 4, 1);
    plic.write(0x2000, 4, 0x02);

    // Nothing pending.
    assert_eq!(plic.claim_irq(0), None);
}

#[test]
fn test_plic_set_irq_propagates() {
    let plic = Plic::new(64, 2);

    // Connect output for context 0.
    let sink = Arc::new(TestIrqSink::new(16));
    let out_irq = 11u32; // MEI
    let isrc =
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, out_irq);
    plic.connect_context_output(0, isrc);

    // Set priority[1] = 1, enable IRQ 1 for ctx 0.
    plic.write(0x04, 4, 1);
    plic.write(0x2000, 4, 0x02);

    // No interrupt yet.
    assert!(!sink.level(out_irq), "output should be low before set_irq");

    // Assert source 1 (rising edge → pending).
    plic.set_irq(1, true);
    assert!(sink.level(out_irq), "output should go high after set_irq");

    // Deassert source 1. QEMU's SiFive PLIC ignores low input updates;
    // pending remains latched until software claims it.
    plic.set_irq(1, false);
    assert!(
        sink.level(out_irq),
        "output should stay high until pending is claimed"
    );

    let claimed = plic.read(0x200004, 4);
    assert_eq!(claimed, 1);
    plic.write(0x200004, 4, 1); // complete
    assert!(
        !sink.level(out_irq),
        "output should go low after claim+complete"
    );
}

#[test]
fn test_plic_claim_on_read() {
    let plic = Plic::new(64, 1);

    plic.write(0x04, 4, 1);
    plic.write(0x2000, 4, 0x02);
    plic.set_pending(1, true);

    // MMIO read at claim offset should perform the claim.
    let claimed = plic.read(0x200004, 4);
    assert_eq!(claimed, 1, "MMIO claim read should return IRQ 1");

    // Pending bit should now be cleared.
    let pending = plic.read(0x1000, 4);
    assert_eq!(
        pending & (1 << 1),
        0,
        "pending bit should be cleared after claim"
    );

    // Second claim read should return 0.
    let claimed2 = plic.read(0x200004, 4);
    assert_eq!(claimed2, 0, "second claim should return 0");
}

/// QEMU's SiFive PLIC input path treats each device `set_irq`
/// update as authoritative level state. Calling `set_irq(true)`
/// again must refresh pending even if the source had not been
/// lowered between claim and the next device-side update.
#[test]
fn test_plic_level_high_update_refreshes_pending_after_claim() {
    let plic = Plic::new(64, 1);

    let sink = Arc::new(TestIrqSink::new(16));
    let out_irq = 11u32;
    let isrc =
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, out_irq);
    plic.connect_context_output(0, isrc);

    plic.write(0x04, 4, 1); // priority[1] = 1
    plic.write(0x2000, 4, 0x02); // enable IRQ 1

    // Assert source 1 → pending.
    plic.set_irq(1, true);
    assert!(sink.level(out_irq));

    // Claim.
    let claimed = plic.read(0x200004, 4);
    assert_eq!(claimed, 1);

    // Device reports the line high again while the previous claim is still
    // outstanding. This is how UART THR-empty can feed the next byte without
    // needing an artificial low pulse at the PLIC input.
    plic.set_irq(1, true);
    let pending = plic.read(0x1000, 4);
    assert_ne!(
        pending & (1 << 1),
        0,
        "pending should refresh on repeated high level updates"
    );

    plic.write(0x200004, 4, 1);
    assert!(sink.level(out_irq));

    assert_eq!(plic.read(0x200004, 4), 1);
    plic.set_irq(1, false);
    plic.write(0x200004, 4, 1);
    assert!(!sink.level(out_irq));
}

#[test]
fn test_plic_lifecycle_and_mom_identity() {
    let mut bus = SysBus::new("sysbus0");
    let plic = Arc::new(Plic::new_named("plic0", 64, 2));
    assert!(!plic.realized());
    plic.with_mdevice(|device| assert_eq!(device.local_id(), "plic0"));
    assert_eq!(plic.object_info().local_id, "plic0");

    plic.attach_to_bus(&mut bus).unwrap();
    plic.register_mmio(
        MemoryRegion::io(
            "plic",
            0x0400_0000,
            Arc::new(PlicMmio(Arc::clone(&plic))),
        ),
        GPA::new(0x0C00_0000),
    )
    .unwrap();

    let mut address_space = make_address_space();
    plic.realize_onto(&mut bus, &mut address_space).unwrap();

    assert!(plic.realized());
    assert!(address_space.is_mapped(GPA::new(0x0C00_0000), 4));
    address_space.write(GPA::new(0x0C00_0004), 4, 7);
    assert_eq!(address_space.read(GPA::new(0x0C00_0004), 4), 7);
    assert_eq!(bus.mappings().len(), 1);
    assert_eq!(bus.mappings()[0].owner, "plic0");

    let err = plic.realize_onto(&mut bus, &mut address_space).unwrap_err();
    assert!(err.to_string().contains("already realized"));

    plic.unrealize_from(&mut bus, &mut address_space).unwrap();
    assert!(!plic.realized());

    let err = plic
        .unrealize_from(&mut bus, &mut address_space)
        .unwrap_err();
    assert!(err.to_string().contains("not realized"));
}
