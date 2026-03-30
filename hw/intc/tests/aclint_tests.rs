use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use machina_hw_core::irq::{IrqLine, IrqSink};
use machina_hw_intc::aclint::Aclint;

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
fn test_aclint_mtime_read_write() {
    let mut aclint = Aclint::new(2);

    // mtime at MTIMER offset 0xBFF8, initially 0.
    assert_eq!(aclint.mtimer_read(0xBFF8, 8), 0);

    // Write mtime.
    aclint.mtimer_write(0xBFF8, 8, 1000);
    assert_eq!(aclint.mtimer_read(0xBFF8, 8), 1000);
}

#[test]
fn test_aclint_mtimecmp_set() {
    let mut aclint = Aclint::new(2);

    // mtimecmp[0] at offset 0x0000.
    aclint.mtimer_write(0x0000, 8, 500);
    assert_eq!(aclint.mtimer_read(0x0000, 8), 500);

    // mtimecmp[1] at offset 0x0008.
    aclint.mtimer_write(0x0008, 8, 1000);
    assert_eq!(aclint.mtimer_read(0x0008, 8), 1000);
}

#[test]
fn test_aclint_msip_set_clear() {
    let mut aclint = Aclint::new(2);

    // msip[0] at MSWI offset 0x0000.
    aclint.mswi_write(0x0000, 4, 1);
    assert_eq!(aclint.mswi_read(0x0000, 4), 1);

    // Clear.
    aclint.mswi_write(0x0000, 4, 0);
    assert_eq!(aclint.mswi_read(0x0000, 4), 0);

    // Only bit 0 is writable.
    aclint.mswi_write(0x0000, 4, 0xFF);
    assert_eq!(aclint.mswi_read(0x0000, 4), 1);
}

#[test]
fn test_aclint_timer_compare() {
    let mut aclint = Aclint::new(2);

    // Set mtimecmp[0] = 5.
    aclint.mtimer_write(0x0000, 8, 5);

    // Initially no pending.
    assert!(!aclint.timer_irq_pending(0));

    // Tick 4 times -> mtime = 4, still < 5.
    for _ in 0..4 {
        aclint.tick();
    }
    assert!(!aclint.timer_irq_pending(0));

    // Tick once more -> mtime = 5, now >= mtimecmp.
    aclint.tick();
    assert!(aclint.timer_irq_pending(0));

    // Hart 1 has mtimecmp = u64::MAX, still not pending.
    assert!(!aclint.timer_irq_pending(1));
}

#[test]
fn test_aclint_tick_mti() {
    let mut aclint = Aclint::new(2);

    // Connect MTI output for hart 0.
    let sink = Arc::new(TestIrqSink::new(16));
    let mti_irq = 7u32;
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, mti_irq);
    aclint.connect_mti(0, line);

    // Set mtimecmp[0] = 3.
    aclint.mtimer_write(0x0000, 8, 3);

    // MTI should be low.
    assert!(!sink.level(mti_irq), "MTI should be low before threshold");

    // Tick twice: mtime = 2, still < 3.
    aclint.tick();
    aclint.tick();
    assert!(!sink.level(mti_irq), "MTI should be low at mtime=2");

    // Tick once more: mtime = 3, now >= mtimecmp.
    aclint.tick();
    assert!(sink.level(mti_irq), "MTI should be high at mtime=3");

    // Raise mtimecmp to clear: set mtimecmp[0] = 100.
    aclint.mtimer_write(0x0000, 8, 100);
    assert!(
        !sink.level(mti_irq),
        "MTI should go low after raising mtimecmp"
    );
}

#[test]
fn test_aclint_msi_output() {
    let mut aclint = Aclint::new(2);

    // Connect MSI output for hart 0.
    let sink = Arc::new(TestIrqSink::new(16));
    let msi_irq = 3u32;
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, msi_irq);
    aclint.connect_msi(0, line);

    // Initially low.
    assert!(!sink.level(msi_irq), "MSI should be low");

    // Write msip[0] = 1.
    aclint.mswi_write(0x0000, 4, 1);
    assert!(sink.level(msi_irq), "MSI should go high after msip=1");

    // Clear msip[0].
    aclint.mswi_write(0x0000, 4, 0);
    assert!(!sink.level(msi_irq), "MSI should go low after msip=0");
}
