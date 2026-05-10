use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use machina_hw_core::irq::{
    InterruptSource, IrqLine, IrqSink, OrIrq, SplitIrq,
};

/// Simple test sink that records the last level and
/// the number of set_irq calls.
struct TestSink {
    level: AtomicBool,
    call_count: AtomicUsize,
}

impl TestSink {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            level: AtomicBool::new(false),
            call_count: AtomicUsize::new(0),
        })
    }

    fn level(&self) -> bool {
        self.level.load(Ordering::SeqCst)
    }

    fn calls(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

impl IrqSink for TestSink {
    fn set_irq(&self, _irq: u32, level: bool) {
        self.level.store(level, Ordering::SeqCst);
        self.call_count.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn test_irq_set_clear() {
    let sink = TestSink::new();
    let line = IrqLine::new(sink.clone(), 0);

    line.raise();
    assert!(sink.level());

    line.lower();
    assert!(!sink.level());
}

#[test]
fn test_or_irq() {
    let sink = TestSink::new();
    let output = IrqLine::new(sink.clone(), 0);
    let or_gate = Arc::new(OrIrq::new(output, 3));

    // Raise input 0 -> output high.
    or_gate.set_irq(0, true);
    assert!(sink.level());

    // Raise input 1 -> still high.
    or_gate.set_irq(1, true);
    assert!(sink.level());

    // Lower input 0 -> still high (input 1 holds).
    or_gate.set_irq(0, false);
    assert!(sink.level());

    // Lower input 1 -> all low, output low.
    or_gate.set_irq(1, false);
    assert!(!sink.level());
}

#[test]
fn test_or_irq_all_low() {
    let sink = TestSink::new();
    let output = IrqLine::new(sink.clone(), 0);
    let or_gate = Arc::new(OrIrq::new(output, 4));

    // All inputs default low; output must be low.
    or_gate.set_irq(0, false);
    assert!(!sink.level());

    or_gate.set_irq(1, false);
    assert!(!sink.level());

    or_gate.set_irq(2, false);
    assert!(!sink.level());

    or_gate.set_irq(3, false);
    assert!(!sink.level());
}

#[test]
fn test_split_irq() {
    let s0 = TestSink::new();
    let s1 = TestSink::new();
    let s2 = TestSink::new();

    let outputs = vec![
        IrqLine::new(s0.clone(), 0),
        IrqLine::new(s1.clone(), 1),
        IrqLine::new(s2.clone(), 2),
    ];
    let split = Arc::new(SplitIrq::new(outputs));

    // Raise -> all outputs high.
    split.set_irq(0, true);
    assert!(s0.level());
    assert!(s1.level());
    assert!(s2.level());

    // Lower -> all outputs low.
    split.set_irq(0, false);
    assert!(!s0.level());
    assert!(!s1.level());
    assert!(!s2.level());
}

#[test]
fn test_irq_no_spurious() {
    let sink = TestSink::new();
    let line = IrqLine::new(sink.clone(), 0);

    // Line starts low. Lowering it again should not
    // change the level.
    assert!(!sink.level());
    let before = sink.calls();
    line.lower();
    assert!(!sink.level());

    // The sink was called, but level stays false.
    assert_eq!(sink.calls(), before + 1);
    assert!(!sink.level());
}

// -- InterruptSource tests --

/// Bitfield sink for InterruptSource tests.
struct BitSink(AtomicU64);

impl IrqSink for BitSink {
    fn set_irq(&self, irq: u32, level: bool) {
        let bit = 1u64 << irq;
        if level {
            self.0.fetch_or(bit, Ordering::SeqCst);
        } else {
            self.0.fetch_and(!bit, Ordering::SeqCst);
        }
    }
}

#[test]
fn test_interrupt_source_raise_lower() {
    let sink = Arc::new(BitSink(AtomicU64::new(0)));
    let src = InterruptSource::new(sink.clone() as Arc<dyn IrqSink>, 5);
    assert_eq!(sink.0.load(Ordering::SeqCst), 0);
    src.raise();
    assert_eq!(sink.0.load(Ordering::SeqCst), 1 << 5);
    src.lower();
    assert_eq!(sink.0.load(Ordering::SeqCst), 0);
}

#[test]
fn test_interrupt_source_set() {
    let sink = Arc::new(BitSink(AtomicU64::new(0)));
    let src = InterruptSource::new(sink.clone() as Arc<dyn IrqSink>, 3);
    src.set(true);
    assert_eq!(sink.0.load(Ordering::SeqCst), 1 << 3);
    src.set(false);
    assert_eq!(sink.0.load(Ordering::SeqCst), 0);
}

// ===== OrIrq / SplitIrq additional coverage (#89) =====

#[test]
fn test_or_irq_full_truth_table_three_inputs() {
    let sink = TestSink::new();
    let or_gate = Arc::new(OrIrq::new(IrqLine::new(sink.clone(), 0), 3));

    // Iterate all 8 input combinations and verify the output level
    // matches the OR of the three inputs.
    for combo in 0u8..8 {
        let bits = [combo & 1 != 0, combo & 2 != 0, combo & 4 != 0];
        // Drive each input to its configured level.
        for (idx, &lvl) in bits.iter().enumerate() {
            or_gate.set_irq(idx as u32, lvl);
        }
        let any = bits.iter().any(|&b| b);
        assert_eq!(
            sink.level(),
            any,
            "OrIrq output mismatch for input pattern {bits:?}",
        );
    }
}

#[test]
fn test_or_irq_idempotent_writes_do_not_drop_holding_inputs() {
    let sink = TestSink::new();
    let or_gate = Arc::new(OrIrq::new(IrqLine::new(sink.clone(), 0), 2));

    or_gate.set_irq(0, true);
    assert!(sink.level());

    // Re-asserting input 0 must not lower the output, regardless of
    // whether input 1 ever toggled.
    or_gate.set_irq(0, true);
    or_gate.set_irq(0, true);
    assert!(sink.level(), "repeated raises must keep output high");

    // Lower a never-raised input — output stays high because input 0
    // still holds it.
    or_gate.set_irq(1, false);
    assert!(sink.level(), "lowering an idle input must not drop output");
}

#[test]
fn test_split_irq_preserves_per_output_irq_numbers() {
    // A single BitSink covers all three outputs. SplitIrq should
    // forward to each IrqLine's own irq_num so each output bit ends
    // up set.
    let sink = Arc::new(BitSink(AtomicU64::new(0)));
    let split = Arc::new(SplitIrq::new(vec![
        IrqLine::new(sink.clone(), 1),
        IrqLine::new(sink.clone(), 4),
        IrqLine::new(sink.clone(), 7),
    ]));

    split.set_irq(0, true);
    let raised = sink.0.load(Ordering::SeqCst);
    assert_eq!(
        raised,
        (1 << 1) | (1 << 4) | (1 << 7),
        "each output line must drive its own irq_num",
    );

    split.set_irq(0, false);
    assert_eq!(
        sink.0.load(Ordering::SeqCst),
        0,
        "lowering the input must clear all output bits",
    );
}

#[test]
fn test_split_irq_with_no_outputs_is_safe() {
    // Degenerate fan-out with zero sinks must accept set_irq
    // without panicking.
    let split = Arc::new(SplitIrq::new(Vec::new()));
    split.set_irq(0, true);
    split.set_irq(0, false);
}

#[test]
fn test_interrupt_source_irq_num_getter() {
    let sink = Arc::new(BitSink(AtomicU64::new(0)));
    let src = InterruptSource::new(sink as Arc<dyn IrqSink>, 17);
    assert_eq!(src.irq_num(), 17);
}
