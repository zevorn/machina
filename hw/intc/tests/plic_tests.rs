use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use machina_hw_core::irq::{IrqLine, IrqSink};
use machina_hw_intc::plic::Plic;

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

#[test]
fn test_plic_set_priority() {
    let mut plic = Plic::new(64, 2);

    // Write priority for IRQ 1 via MMIO (offset 4).
    plic.write(0x04, 4, 7);
    assert_eq!(plic.read(0x04, 4), 7);

    // Write priority for IRQ 10 (offset 0x28).
    plic.write(0x28, 4, 3);
    assert_eq!(plic.read(0x28, 4), 3);
}

#[test]
fn test_plic_claim_highest() {
    let mut plic = Plic::new(64, 2);

    // Set priorities: IRQ 1 = 2, IRQ 2 = 5.
    plic.write(0x04, 4, 2); // priority[1] = 2
    plic.write(0x08, 4, 5); // priority[2] = 5

    // Enable IRQ 1 and IRQ 2 for context 0.
    // Enable bitmap at 0x2000 + 0x80*0 = 0x2000.
    // Both IRQs are in word 0: bits 1 and 2.
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
    let mut plic = Plic::new(64, 1);

    plic.write(0x04, 4, 1); // priority[1] = 1
    plic.write(0x2000, 4, 0x02); // enable IRQ 1, ctx 0
    plic.set_pending(1, true);

    let claimed = plic.claim_irq(0);
    assert_eq!(claimed, Some(1));

    // Complete via MMIO: write IRQ number to
    // claim/complete register at 0x200004.
    plic.write(0x200004, 4, 1);

    // After completion, claim register should be 0.
    assert_eq!(plic.read(0x200004, 4), 0);
}

#[test]
fn test_plic_threshold() {
    let mut plic = Plic::new(64, 1);

    plic.write(0x04, 4, 3); // priority[1] = 3
    plic.write(0x2000, 4, 0x02); // enable IRQ 1, ctx 0
    plic.set_pending(1, true);

    // Set threshold for context 0 to 5 (above priority).
    plic.write(0x200000, 4, 5);

    // IRQ 1 has priority 3 which is <= threshold 5,
    // so it should not be claimable.
    assert_eq!(plic.claim_irq(0), None);
}

#[test]
fn test_plic_no_pending() {
    let mut plic = Plic::new(64, 1);

    plic.write(0x04, 4, 1); // priority[1] = 1
    plic.write(0x2000, 4, 0x02); // enable IRQ 1, ctx 0

    // Nothing pending.
    assert_eq!(plic.claim_irq(0), None);
}

#[test]
fn test_plic_set_irq_propagates() {
    let mut plic = Plic::new(64, 2);

    // Connect output for context 0.
    let sink = Arc::new(TestIrqSink::new(16));
    let out_irq = 11u32; // MEI
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, out_irq);
    plic.connect_context_output(0, line);

    // Set priority[1] = 1, enable IRQ 1 for ctx 0.
    plic.write(0x04, 4, 1);
    plic.write(0x2000, 4, 0x02);

    // No interrupt yet.
    assert!(!sink.level(out_irq), "output should be low before set_irq");

    // Assert source 1.
    plic.set_irq(1, true);
    assert!(sink.level(out_irq), "output should go high after set_irq");

    // Deassert source 1.
    plic.set_irq(1, false);
    assert!(
        !sink.level(out_irq),
        "output should go low after clearing IRQ"
    );
}

#[test]
fn test_plic_claim_on_read() {
    let mut plic = Plic::new(64, 1);

    // priority[1] = 1, enable IRQ 1 for ctx 0.
    plic.write(0x04, 4, 1);
    plic.write(0x2000, 4, 0x02);
    plic.set_pending(1, true);

    // MMIO read at claim offset (0x200004) should
    // perform the claim and return IRQ 1.
    let claimed = plic.read(0x200004, 4);
    assert_eq!(claimed, 1, "MMIO claim read should return IRQ 1");

    // Pending bit should now be cleared.
    let pending = plic.read(0x1000, 4);
    assert_eq!(
        pending & (1 << 1),
        0,
        "pending bit should be cleared after claim"
    );

    // Second claim read should return 0 (nothing pending).
    let claimed2 = plic.read(0x200004, 4);
    assert_eq!(claimed2, 0, "second claim should return 0");
}
