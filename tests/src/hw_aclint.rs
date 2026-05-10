use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use machina_core::address::GPA;
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{IrqLine, IrqSink};
use machina_hw_intc::aclint::{Aclint, AclintMmio};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MemoryRegion;

struct TestIrqSink {
    levels: Vec<AtomicBool>,
}

/// Wait for a condition to become true with a deadline.
/// Returns true if the condition was met within the timeout.
fn wait_with_deadline<F>(condition: F, timeout: Duration) -> bool
where
    F: Fn() -> bool,
{
    let start = Instant::now();
    let deadline = start + timeout;

    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        // Small sleep to avoid busy-waiting
        std::thread::sleep(Duration::from_millis(1));
    }

    // Final check at timeout boundary
    condition()
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
fn test_aclint_mtime_wall_clock() {
    let aclint = Aclint::new(2);

    // Poll with a deadline rather than sleep-then-check so this
    // test cannot fail simply because a slow CI scheduler stole
    // CPU between the sleep and the assertion. We accept any
    // diff that crosses the 500_000-tick (50ms @ 10 MHz) lower
    // bound within a generous 2-second deadline; the upper
    // bound caps how far the wall clock is allowed to outrun
    // the lower bound to catch egregious drift.
    let t0 = aclint.read(0xBFF8, 8);
    let started = Instant::now();
    let advanced = wait_with_deadline(
        || aclint.read(0xBFF8, 8).saturating_sub(t0) > 500_000,
        Duration::from_secs(2),
    );
    let waited = started.elapsed();
    let t1 = aclint.read(0xBFF8, 8);
    let diff = t1.saturating_sub(t0);

    assert!(
        advanced,
        "mtime never crossed +500_000 ticks. \
         t0={t0}, t1={t1}, diff={diff}, waited={waited:?}",
    );
    // Once it crosses, the diff should not greatly exceed
    // 2 seconds * 10 MHz = 20_000_000 ticks plus the polling
    // tail, which is the loose upper bound.
    assert!(
        diff < 25_000_000,
        "mtime diff {diff} grew implausibly large \
         ({waited:?} elapsed)",
    );
}

#[test]
fn test_aclint_mtime_write_resets_epoch() {
    let aclint = Aclint::new(2);

    aclint.write(0xBFF8, 8, 1_000_000);
    let t = aclint.read(0xBFF8, 8);
    // Should be close to 1_000_000 (just written).
    assert!(
        (1_000_000..1_100_000).contains(&t),
        "mtime {} not near written value 1_000_000",
        t
    );
}

#[test]
fn test_aclint_mtime_low_write_preserves_live_high_half() {
    let aclint = Aclint::new(1);

    aclint.write(0xBFF8, 8, 0xffff_fff0);
    assert!(
        wait_with_deadline(
            || (aclint.read(0xBFF8, 8) >> 32) != 0,
            Duration::from_millis(50),
        ),
        "mtime should cross the 32-bit boundary"
    );
    let high = aclint.read(0xBFF8, 8) & 0xffff_ffff_0000_0000;

    aclint.write(0xBFF8, 4, 0x1234);

    let mtime = aclint.read(0xBFF8, 8);
    assert_eq!(mtime & 0xffff_ffff_0000_0000, high);
    assert!(
        (0x1234..0x1234 + 100_000).contains(&(mtime & 0xffff_ffff)),
        "mtime low half should restart near the written value: {mtime:#x}"
    );
}

#[test]
fn test_aclint_mtime_high_write_preserves_live_low_half() {
    let aclint = Aclint::new(1);

    aclint.write(0xBFF8, 8, 0);
    // Poll until the low half has advanced past the threshold
    // we'll snapshot. Loaded CI may need more than 5ms to
    // accumulate 10_000 ticks at 10 MHz, but a 200ms deadline
    // is still effectively instant — and the polling avoids
    // any "sleep-then-snapshot" race.
    assert!(
        wait_with_deadline(
            || (aclint.read(0xBFF8, 8) & 0xffff_ffff) > 10_000,
            Duration::from_millis(200),
        ),
        "mtime low half never advanced past 10_000; \
         current mtime = {:#x}",
        aclint.read(0xBFF8, 8),
    );
    let low_before = aclint.read(0xBFF8, 8) & 0xffff_ffff;

    aclint.write(0xBFFC, 4, 2);

    let mtime = aclint.read(0xBFF8, 8);
    assert_eq!(
        mtime >> 32,
        2,
        "high half should be the value just written; mtime={mtime:#x}",
    );
    assert!(
        (low_before..low_before + 100_000).contains(&(mtime & 0xffff_ffff)),
        "mtime low half should preserve the live value: \
         before={low_before:#x}, after={mtime:#x}"
    );
}

#[test]
fn test_aclint_mtimecmp_set() {
    let aclint = Aclint::new(2);

    aclint.write(0x4000, 8, 500);
    assert_eq!(aclint.read(0x4000, 8), 500);

    aclint.write(0x4008, 8, 1000);
    assert_eq!(aclint.read(0x4008, 8), 1000);
}

#[test]
fn test_aclint_mtimecmp_rejects_invalid_sizes_and_offsets() {
    let aclint = Aclint::new(1);

    aclint.write(0x4000, 8, 0x1122_3344_5566_7788);
    assert_eq!(aclint.read(0x4000, 8), 0x1122_3344_5566_7788);

    assert_eq!(aclint.read(0x4000, 1), 0);
    assert_eq!(aclint.read(0x4000, 2), 0);
    assert_eq!(aclint.read(0x4002, 4), 0);
    assert_eq!(aclint.read(0x4004, 8), 0);

    for size in [1_u32, 2] {
        aclint.write(0x4000, size, 0);
        assert_eq!(aclint.read(0x4000, 8), 0x1122_3344_5566_7788);
    }

    aclint.write(0x4002, 4, 0);
    assert_eq!(aclint.read(0x4000, 8), 0x1122_3344_5566_7788);

    aclint.write(0x4004, 8, 0);
    assert_eq!(aclint.read(0x4000, 8), 0x1122_3344_5566_7788);
}

#[test]
fn test_aclint_mtime_rejects_invalid_sizes_and_high_64_bit_access() {
    let aclint = Aclint::new(1);

    aclint.write(0xBFF8, 8, 0x0000_0002_0000_1000);
    assert_eq!(aclint.read(0xBFF8, 1), 0);
    assert_eq!(aclint.read(0xBFF8, 2), 0);
    assert_eq!(aclint.read(0xBFFA, 4), 0);
    assert_eq!(aclint.read(0xBFFC, 8), 0);

    let before = aclint.read(0xBFF8, 8);
    aclint.write(0xBFF8, 1, 0);
    aclint.write(0xBFF8, 2, 0);
    aclint.write(0xBFFA, 4, 0);
    aclint.write(0xBFFC, 8, 0);
    let after = aclint.read(0xBFF8, 8);

    assert!(
        after >= before,
        "invalid mtime writes must not move time backwards: before={before:#x}, after={after:#x}"
    );
    assert_eq!(after >> 32, before >> 32);
}

#[test]
fn test_aclint_msip_set_clear() {
    let aclint = Aclint::new(2);

    aclint.write(0x0000, 4, 1);
    assert_eq!(aclint.read(0x0000, 4), 1);

    aclint.write(0x0000, 4, 0);
    assert_eq!(aclint.read(0x0000, 4), 0);

    // Only bit 0 is writable.
    aclint.write(0x0000, 4, 0xFF);
    assert_eq!(aclint.read(0x0000, 4), 1);
}

#[test]
fn test_aclint_msip_rejects_non_32_bit_accesses() {
    let aclint = Aclint::new(1);

    aclint.write(0x0000, 4, 1);
    assert_eq!(aclint.read(0x0000, 4), 1);

    assert_eq!(aclint.read(0x0000, 1), 0);
    assert_eq!(aclint.read(0x0000, 2), 0);
    assert_eq!(aclint.read(0x0000, 8), 0);
    assert_eq!(aclint.read(0x0002, 4), 0);

    for size in [1_u32, 2, 8] {
        aclint.write(0x0000, size, 0);
        assert_eq!(aclint.read(0x0000, 4), 1);
    }

    aclint.write(0x0002, 4, 0);
    assert_eq!(aclint.read(0x0000, 4), 1);
}

#[test]
fn test_aclint_timer_compare() {
    let aclint = Aclint::new(2);
    aclint.set_virtual_clock(true);

    // Set mtimecmp[0] = 100.
    aclint.write(0x4000, 8, 100);

    // Set mtime to 50 (below mtimecmp).
    aclint.write(0xBFF8, 8, 50);
    assert!(!aclint.timer_irq_pending(0));

    // Set mtime to 100 (at threshold).
    aclint.write(0xBFF8, 8, 100);
    assert!(aclint.timer_irq_pending(0));

    // Hart 1 has mtimecmp = u64::MAX, not pending.
    assert!(!aclint.timer_irq_pending(1));
}

#[test]
fn test_aclint_virtual_clock_advances_only_by_tick() {
    let aclint = Aclint::new(1);
    let sink = Arc::new(TestIrqSink::new(16));
    let mti_irq = 7u32;
    let exit_requested = Arc::new(AtomicBool::new(false));
    let exit_seen = Arc::clone(&exit_requested);
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, mti_irq);
    aclint.connect_mti(0, line);
    aclint.connect_exit_request(
        0,
        Arc::new(move || {
            exit_seen.store(true, Ordering::Release);
        }),
    );
    aclint.set_virtual_clock(true);

    aclint.write(0xBFF8, 8, 10);
    let t0 = aclint.read(0xBFF8, 8);
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(aclint.read(0xBFF8, 8), t0);

    aclint.write(0x4000, 8, 20);
    aclint.tick(9);
    assert_eq!(aclint.read(0xBFF8, 8), 19);
    assert!(!sink.level(mti_irq));

    aclint.tick(1);
    assert_eq!(aclint.read(0xBFF8, 8), 20);
    assert!(sink.level(mti_irq));
    assert!(
        exit_requested.load(Ordering::Acquire),
        "virtual timer expiry should request an exec-loop exit"
    );
}

#[test]
fn test_aclint_mti_output() {
    let aclint = Aclint::new(2);

    let sink = Arc::new(TestIrqSink::new(16));
    let mti_irq = 7u32;
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, mti_irq);
    aclint.connect_mti(0, line);

    // Set mtimecmp[0] = 100.
    aclint.write(0x4000, 8, 100);

    // Set mtime = 50 -> MTI low.
    aclint.write(0xBFF8, 8, 50);
    assert!(
        !sink.level(mti_irq),
        "MTI should be low when mtime < mtimecmp"
    );

    // Set mtime = 100 -> MTI high.
    aclint.write(0xBFF8, 8, 100);
    assert!(
        sink.level(mti_irq),
        "MTI should be high when mtime >= mtimecmp"
    );

    // Re-anchor mtime to 50 before the final check so that
    // wall-clock drift between the previous write and the
    // mtimecmp write cannot push mtime past 200 on slow CI.
    // update_mti() sees mtime=50 < mtimecmp=100, so MTI stays
    // low after this; the HIGH assertion is already done above.
    aclint.write(0xBFF8, 8, 50);

    // Raise mtimecmp to 200 -> MTI low (mtime=50 < 200).
    aclint.write(0x4000, 8, 200);
    assert!(
        !sink.level(mti_irq),
        "MTI should go low after raising mtimecmp"
    );
}

#[test]
fn test_aclint_msi_output() {
    let aclint = Aclint::new(2);

    let sink = Arc::new(TestIrqSink::new(16));
    let msi_irq = 3u32;
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, msi_irq);
    aclint.connect_msi(0, line);

    assert!(!sink.level(msi_irq), "MSI should be low");

    aclint.write(0x0000, 4, 1);
    assert!(sink.level(msi_irq), "MSI should go high after msip=1");

    aclint.write(0x0000, 4, 0);
    assert!(!sink.level(msi_irq), "MSI should go low after msip=0");
}

#[test]
fn test_aclint_clint_layout() {
    let aclint = Aclint::new(2);

    aclint.write(0x0000, 4, 1);
    assert_eq!(aclint.read(0x0000, 4), 1);

    aclint.write(0x4000, 8, 42);
    assert_eq!(aclint.read(0x4000, 8), 42);

    aclint.write(0xBFF8, 8, 999);
    let t = aclint.read(0xBFF8, 8);
    assert!(
        (999..999 + 100_000).contains(&t),
        "mtime {} not near written value 999",
        t
    );
}

#[test]
fn test_aclint_mtimecmp_disable() {
    let aclint = Aclint::new(1);
    let sink = Arc::new(TestIrqSink::new(16));
    let mti = 7u32;
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, mti);
    aclint.connect_mti(0, line);

    // Set mtimecmp to a near-future value, then disable.
    aclint.write(0xBFF8, 8, 0);
    aclint.write(0x4000, 8, 10);
    aclint.write(0xBFF8, 8, 100);
    assert!(sink.level(mti), "MTI should be high");

    // Disable timer by setting mtimecmp to u64::MAX.
    aclint.write(0x4000, 8, u64::MAX);
    assert!(
        !sink.level(mti),
        "MTI should go low after mtimecmp=u64::MAX"
    );
}

#[test]
fn test_aclint_mtimecmp_disable_cancels_stale_timer_worker() {
    let aclint = Aclint::new(1);
    let sink = Arc::new(TestIrqSink::new(16));
    let mti = 7u32;
    let exits = Arc::new(AtomicUsize::new(0));
    let exits_seen = Arc::clone(&exits);
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, mti);
    aclint.connect_mti(0, line);
    aclint.connect_exit_request(
        0,
        Arc::new(move || {
            exits_seen.fetch_add(1, Ordering::AcqRel);
        }),
    );

    let now = aclint.read(0xBFF8, 8);
    let near_future = now + 100_000;
    aclint.write(0x4000, 8, near_future);
    aclint.write(0x4000, 8, u64::MAX);

    // Negative check: nothing should fire after the disable.
    // Sleep 50ms (the worker would have fired well within
    // 10ms), then sample state with diagnostics in the
    // failure paths.
    std::thread::sleep(Duration::from_millis(50));
    let final_mtime = aclint.read(0xBFF8, 8);
    let final_mtimecmp = aclint.read(0x4000, 8);
    let exit_count = exits.load(Ordering::Acquire);

    assert!(
        !sink.level(mti),
        "disabled timer must not assert MTI. \
         scheduled_at={near_future} (now+100_000), \
         disabled_at_mtimecmp={final_mtimecmp:#x}, \
         current_mtime={final_mtime}, exits={exit_count}",
    );
    assert_eq!(
        exit_count, 0,
        "disabled stale timer worker must not request exit. \
         scheduled_at={near_future}, current_mtime={final_mtime}",
    );
}

#[test]
fn test_aclint_mtimecmp_retarget_past() {
    let aclint = Aclint::new(1);
    let sink = Arc::new(TestIrqSink::new(16));
    let mti = 7u32;
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, mti);
    aclint.connect_mti(0, line);

    // Set mtime high, then set mtimecmp to past value.
    aclint.write(0xBFF8, 8, 1000);
    aclint.write(0x4000, 8, 500);
    assert!(sink.level(mti), "MTI should be high when mtimecmp < mtime");
}

#[test]
fn test_aclint_mtimecmp_past_retarget_cancels_stale_timer_worker() {
    let aclint = Aclint::new(1);
    let sink = Arc::new(TestIrqSink::new(16));
    let mti = 7u32;
    let exits = Arc::new(AtomicUsize::new(0));
    let exits_seen = Arc::clone(&exits);
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, mti);
    aclint.connect_mti(0, line);
    aclint.connect_exit_request(
        0,
        Arc::new(move || {
            exits_seen.fetch_add(1, Ordering::AcqRel);
        }),
    );

    let now = aclint.read(0xBFF8, 8);
    let future = now + 300_000;
    aclint.write(0x4000, 8, future);
    let past = aclint.read(0xBFF8, 8).saturating_sub(1);
    aclint.write(0x4000, 8, past);

    let mtime_after_retarget = aclint.read(0xBFF8, 8);
    assert!(
        sink.level(mti),
        "past retarget should assert MTI now. \
         future={future}, past={past}, \
         mtime={mtime_after_retarget}",
    );
    assert_eq!(
        exits.load(Ordering::Acquire),
        1,
        "past retarget should request exactly one immediate exit. \
         future={future}, past={past}, \
         mtime={mtime_after_retarget}",
    );

    // Wait long enough for the old future-timer worker to have
    // run, then assert it did not request a second exit. 60ms
    // is well past the 30µs wait the future timer was armed for.
    std::thread::sleep(Duration::from_millis(60));
    let final_mtime = aclint.read(0xBFF8, 8);
    let final_mtimecmp = aclint.read(0x4000, 8);
    let final_exits = exits.load(Ordering::Acquire);
    assert_eq!(
        final_exits, 1,
        "old future timer worker must not request another exit. \
         original_future={future}, retarget_to={past}, \
         mtime={final_mtime}, mtimecmp={final_mtimecmp:#x}, \
         exits={final_exits}",
    );
}

#[test]
fn test_aclint_timer_thread_asserts_mti() {
    let aclint = Aclint::new(1);
    let sink = Arc::new(TestIrqSink::new(16));
    let mti = 7u32;
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, mti);
    aclint.connect_mti(0, line);

    // Set mtime near current wall clock, mtimecmp 10ms
    // in the future. The timer thread should assert MTI.
    let now = aclint.read(0xBFF8, 8);
    let mtimecmp = now + 100_000; // 10ms
    aclint.write(0x4000, 8, mtimecmp);

    assert!(!sink.level(mti), "MTI should be low before deadline");

    // Wait for timer thread to fire with deadline
    let timeout = Duration::from_millis(100);
    let success = wait_with_deadline(|| sink.level(mti), timeout);

    // Get current state for diagnostics
    let current_mtime = aclint.read(0xBFF8, 8);
    let current_mtimecmp = aclint.read(0x4000, 8);

    assert!(
        success,
        "MTI should be high after timer deadline. Current state: mtime={}, mtimecmp={}, waited for {:?}",
        current_mtime,
        current_mtimecmp,
        timeout
    );
}

#[test]
fn test_aclint_timer_thread_requests_exit() {
    let aclint = Aclint::new(1);
    let sink = Arc::new(TestIrqSink::new(16));
    let mti = 7u32;
    let exit_requested = Arc::new(AtomicBool::new(false));
    let exit_seen = Arc::clone(&exit_requested);
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, mti);
    aclint.connect_mti(0, line);
    aclint.connect_exit_request(
        0,
        Arc::new(move || {
            exit_seen.store(true, Ordering::Release);
        }),
    );

    let now = aclint.read(0xBFF8, 8);
    aclint.write(0x4000, 8, now + 100_000);

    assert!(
        wait_with_deadline(
            || exit_requested.load(Ordering::Acquire),
            Duration::from_millis(100),
        ),
        "timer thread should request an exec-loop exit"
    );
    assert!(sink.level(mti), "timer thread should still assert MTI");
}

#[test]
fn test_aclint_retarget_future_cancels_stale_timer() {
    let aclint = Aclint::new(1);
    let sink = Arc::new(TestIrqSink::new(16));
    let mti = 7u32;
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, mti);
    aclint.connect_mti(0, line);

    let now = aclint.read(0xBFF8, 8);
    let original_target = now + 200_000; // 20ms
    let retarget_target = now + 10_000_000; // 1s

    // Set mtimecmp to now+20ms, then immediately retarget
    // to now+1s. The old 20ms timer must be cancelled.
    aclint.write(0x4000, 8, original_target);
    aclint.write(0x4000, 8, retarget_target);

    // After 50ms, the original 20ms timer would have fired —
    // MTI should still be low because the retarget cancelled
    // it. Use a busy-poll to detect a regression early; only
    // run the full deadline if the cancel was honoured.
    let timeout1 = Duration::from_millis(50);
    let bad = wait_with_deadline(|| sink.level(mti), timeout1);
    let current_mtime1 = aclint.read(0xBFF8, 8);
    let current_mtimecmp1 = aclint.read(0x4000, 8);

    assert!(
        !bad,
        "MTI must stay low after retarget cancelled the old \
         20ms timer. \
         original_target={original_target}, \
         retarget_target={retarget_target}, \
         mtime={current_mtime1}, mtimecmp={current_mtimecmp1}, \
         waited up to {timeout1:?}",
    );

    // Now retarget to near future (10ms from current).
    let now2 = aclint.read(0xBFF8, 8);
    let mtimecmp2 = now2 + 100_000; // 10ms
    aclint.write(0x4000, 8, mtimecmp2);

    // Wait for timer thread to fire with deadline
    let timeout2 = Duration::from_millis(100);
    let success = wait_with_deadline(|| sink.level(mti), timeout2);

    // Get current state for diagnostics
    let current_mtime2 = aclint.read(0xBFF8, 8);
    let current_mtimecmp2 = aclint.read(0x4000, 8);

    assert!(
        success,
        "MTI should be high after retarget to near future. Current state: mtime={}, mtimecmp={}, waited for {:?}",
        current_mtime2,
        current_mtimecmp2,
        timeout2
    );
}

#[test]
fn test_aclint_lifecycle_and_mom_identity() {
    let mut bus = SysBus::new("sysbus0");
    let aclint = Arc::new(Aclint::new_named("aclint0", 2));
    assert!(!aclint.realized());
    aclint.with_mdevice(|device| assert_eq!(device.local_id(), "aclint0"));
    assert_eq!(aclint.object_info().local_id, "aclint0");

    aclint.attach_to_bus(&mut bus).unwrap();
    aclint
        .register_mmio(
            MemoryRegion::io(
                "clint",
                0x1_0000,
                Arc::new(AclintMmio(Arc::clone(&aclint))),
            ),
            GPA::new(0x0200_0000),
        )
        .unwrap();

    let mut address_space = make_address_space();
    aclint.realize_onto(&mut bus, &mut address_space).unwrap();

    assert!(aclint.realized());
    assert!(address_space.is_mapped(GPA::new(0x0200_0000), 8));
    address_space.write(GPA::new(0x0200_4000), 8, 0x1234);
    assert_eq!(address_space.read(GPA::new(0x0200_4000), 8), 0x1234);
    assert_eq!(bus.mappings().len(), 1);
    assert_eq!(bus.mappings()[0].owner, "aclint0");

    let err = aclint
        .realize_onto(&mut bus, &mut address_space)
        .unwrap_err();
    assert!(err.to_string().contains("already realized"));

    aclint.unrealize_from(&mut bus, &mut address_space).unwrap();
    assert!(!aclint.realized());

    let err = aclint
        .unrealize_from(&mut bus, &mut address_space)
        .unwrap_err();
    assert!(err.to_string().contains("not realized"));
}
