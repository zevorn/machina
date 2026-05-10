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

// ===== PLIC boundary / multi-context regressions (#70) =====
//
// SiFive PLIC MMIO map (constants from hw/intc/src/plic.rs):
//   0x00_0000 priority[]
//   0x00_1000 pending bitmap (read-only)
//   0x00_2000 enable bitmap, stride 0x80 per context
//   0x20_0000 context, stride 0x1000 per context (0:thresh,4:claim)
//
// These tests pin down out-of-range MMIO behaviour, per-context
// independence, the priority-0 claim rule, and the invalid-IRQ
// guard rails — all without booting a guest.

#[test]
fn test_plic_priority_oob_read_returns_zero_no_panic() {
    let plic = Plic::new(32, 1);

    // priority[32] is the first index past the configured source
    // count and the pending region begins at 0x1000, so anything in
    // (32*4 .. 0x1000) is out-of-range for priority.
    let oob_offset = 32u64 * 4;
    assert_eq!(plic.read(oob_offset, 4), 0);
    plic.write(oob_offset, 4, 0xdead_beef);
    // Re-read still returns 0; the write was silently dropped.
    assert_eq!(plic.read(oob_offset, 4), 0);
    // A neighbouring valid priority still behaves normally.
    plic.write(0x04, 4, 5);
    assert_eq!(plic.read(0x04, 4), 5);
}

#[test]
fn test_plic_pending_oob_read_returns_zero_no_panic() {
    // pending bitmap covers ceil(num_sources / 32) words. With
    // num_sources=32 only word 0 (offset 0x1000) is valid; reading
    // word 1 at 0x1004 must return 0.
    let plic = Plic::new(32, 1);
    assert_eq!(plic.read(0x1004, 4), 0);
}

#[test]
fn test_plic_enable_oob_context_returns_zero_no_panic() {
    // num_contexts=2 -> only ctx 0/1 valid. Reading enable for
    // ctx 5 must not panic and must return 0.
    let plic = Plic::new(64, 2);
    let stride = 0x80u64;
    let bogus_ctx = 5u64;
    let off = 0x2000 + bogus_ctx * stride;
    assert_eq!(plic.read(off, 4), 0);
    plic.write(off, 4, 0xffff_ffff);
    assert_eq!(plic.read(off, 4), 0);
}

#[test]
fn test_plic_context_register_oob_returns_zero_no_panic() {
    // Threshold/claim window for ctx 5 with num_contexts=2.
    let plic = Plic::new(64, 2);
    let off = 0x20_0000 + 5 * 0x1000;
    assert_eq!(plic.read(off, 4), 0); // threshold
    assert_eq!(plic.read(off + 4, 4), 0); // claim
    plic.write(off, 4, 0xabcd);
    plic.write(off + 4, 4, 1);
    // Real ctx 0 threshold remains unaffected.
    assert_eq!(plic.read(0x20_0000, 4), 0);
}

#[test]
fn test_plic_far_offset_returns_zero_no_panic() {
    // An offset far beyond every known region must not panic and
    // must read 0 / silently drop writes.
    let plic = Plic::new(64, 2);
    let far = 0x1000_0000u64;
    assert_eq!(plic.read(far, 4), 0);
    plic.write(far, 4, 0x12345);
    assert_eq!(plic.read(far, 4), 0);
}

#[test]
fn test_plic_enable_and_threshold_are_per_context() {
    let plic = Plic::new(64, 2);

    // Configure context 0: enable IRQ 1 (bit 1 of word 0), threshold = 3.
    plic.write(0x2000, 4, 1u64 << 1); // enable[0][0] = 0b10
    plic.write(0x20_0000, 4, 3); // threshold[0] = 3

    // Context 1 must remain at zeros — even reading uses a separate
    // window per context.
    let enable1 = plic.read(0x2000 + 0x80, 4);
    assert_eq!(enable1, 0, "ctx 1 enable must not see ctx 0 writes");
    let thresh1 = plic.read(0x20_0000 + 0x1000, 4);
    assert_eq!(thresh1, 0, "ctx 1 threshold must not see ctx 0 writes");

    // Reverse: writing ctx 1 must not bleed into ctx 0.
    plic.write(0x2000 + 0x80, 4, 1u64 << 5); // enable[1][0] = 0x20
    plic.write(0x20_0000 + 0x1000, 4, 7); // threshold[1] = 7
    assert_eq!(plic.read(0x2000, 4), 1u64 << 1);
    assert_eq!(plic.read(0x20_0000, 4), 3);
}

#[test]
fn test_plic_pending_irq_with_priority_zero_cannot_be_claimed() {
    let plic = Plic::new(64, 1);

    // Enable IRQ 1 for ctx 0, leave its priority at 0.
    plic.write(0x2000, 4, 1u64 << 1);
    plic.set_pending(1, true);

    // Claim port (offset 4) should return 0 because priority 0 is
    // never above the (default zero) threshold.
    assert_eq!(plic.read(0x20_0004, 4), 0);

    // After raising priority above the threshold, the same claim
    // succeeds — confirming the only thing blocking it was priority.
    plic.write(0x04, 4, 5);
    plic.set_pending(1, true);
    assert_eq!(plic.read(0x20_0004, 4), 1);
}

#[test]
fn test_plic_invalid_source_does_not_disturb_valid_irqs() {
    let plic = Plic::new(32, 1);

    // Configure IRQ 1 fully so it is claimable.
    plic.write(0x04, 4, 5);
    plic.write(0x2000, 4, 1u64 << 1);

    // Invalid source 0 (reserved) and source 32 (== num_sources)
    // must be ignored by both set_irq and set_pending.
    plic.set_irq(0, true);
    plic.set_irq(32, true);
    plic.set_pending(0, true);
    plic.set_pending(32, true);

    // Pending bitmap word 0 must NOT have bit 0 set (IRQ 0 reserved)
    // and the lone valid IRQ 1 still works after we deliberately
    // drive it.
    assert_eq!(plic.read(0x1000, 4) & 1, 0, "IRQ 0 must never go pending");
    plic.set_irq(1, true);
    assert_eq!(plic.read(0x20_0004, 4), 1, "valid IRQ 1 still claimable");
}
