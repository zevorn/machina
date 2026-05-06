use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use machina_accel::timer::VirtualClock;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_timer::policy;
use machina_hw_timer::sifive_pwm::{SiFivePwm, SiFivePwmMmio};
use machina_hw_timer::sse_counter::{
    SseCounter, SseCounterControlMmio, SseCounterStatusMmio,
};
use machina_hw_timer::sse_timer::{SseTimer, SseTimerMmio};
use machina_hw_timer::{self as timer, Ptimer};
use machina_memory::region::MmioOps;

/// Utility: create a timer with a callback that increments a counter.
fn counting_timer(policy_mask: u8) -> (Arc<Ptimer>, Arc<AtomicU64>) {
    let count = Arc::new(AtomicU64::new(0));
    let c = Arc::clone(&count);
    let timer = Ptimer::new(
        Some(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })),
        policy_mask,
    );
    (timer, count)
}

// -- Positive Tests --

#[test]
fn test_ptimer_new() {
    let (timer, _) = counting_timer(0);
    assert!(!timer.is_enabled());
    assert_eq!(timer.get_limit(), 0);
    assert_eq!(timer.get_count(), 0);
}

#[test]
fn test_ptimer_set_period() {
    let (timer, _) = counting_timer(0);
    timer.set_period(1000);
    // period is set; timer still disabled
    assert!(!timer.is_enabled());
}

#[test]
fn test_ptimer_set_freq() {
    let (timer, _) = counting_timer(0);
    // 1 MHz -> 1000 ns period
    timer.set_freq(1_000_000);
    timer.set_limit(5, true);
    assert_eq!(timer.get_limit(), 5);
    assert_eq!(timer.get_count(), 5);
}

#[test]
fn test_ptimer_set_freq_zero_disables_period() {
    let (timer, _) = counting_timer(0);
    timer.set_freq(0);
    timer.set_limit(3, true);
    timer.run(false);
    // Zero frequency -> timer can't tick
    assert!(!timer.tick());
}

#[test]
fn test_ptimer_set_limit_reload() {
    let (timer, _) = counting_timer(0);
    timer.set_limit(10, true);
    assert_eq!(timer.get_limit(), 10);
    assert_eq!(timer.get_count(), 10);
}

#[test]
fn test_ptimer_set_limit_no_reload() {
    let (timer, _) = counting_timer(0);
    timer.set_limit(10, true);
    timer.set_limit(5, false);
    assert_eq!(timer.get_limit(), 5);
    // count unchanged when reload=false
    assert_eq!(timer.get_count(), 10);
}

#[test]
fn test_ptimer_set_count() {
    let (timer, _) = counting_timer(0);
    timer.set_count(7);
    assert_eq!(timer.get_count(), 7);
}

#[test]
fn test_ptimer_run_periodic() {
    let (timer, count) = counting_timer(0);
    timer.set_freq(1_000_000); // 1 MHz
    timer.set_limit(3, true);
    timer.run(false); // periodic

    assert!(timer.is_enabled());

    // Tick 3 times to reach zero
    assert!(!timer.tick()); // 3 -> 2
    assert!(!timer.tick()); // 2 -> 1
    let triggered = timer.tick(); // 1 -> 0, fires
    assert!(triggered, "callback should fire at zero");

    assert_eq!(count.load(Ordering::SeqCst), 1);
    // Periodic: reloads to limit (3) after firing
    assert_eq!(timer.get_count(), 2); // NO_COUNTER_ROUND_DOWN -> actual 3, shown as 2 (legacy: delta-1)
    assert!(timer.is_enabled());
}

#[test]
fn test_ptimer_run_oneshot() {
    let (timer, count) = counting_timer(0);
    timer.set_freq(1_000_000);
    timer.set_limit(2, true);
    timer.run(true); // oneshot

    let _ = timer.tick(); // 2 -> 1
    let triggered = timer.tick(); // 1 -> 0
    assert!(triggered);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    // One-shot: stops after firing
    assert!(!timer.is_enabled());
}

#[test]
fn test_ptimer_stop() {
    let (timer, _) = counting_timer(0);
    timer.set_freq(1_000_000);
    timer.set_limit(5, true);
    timer.run(false);
    assert!(timer.is_enabled());

    timer.stop();
    assert!(!timer.is_enabled());
    // After stop, tick does nothing
    assert!(!timer.tick());
}

#[test]
fn test_ptimer_get_count_round_down() {
    // Default: NO_COUNTER_ROUND_DOWN not set -> legacy round-down
    // Round-down only applies when timer is enabled
    let (timer, _) = counting_timer(0);
    timer.set_freq(1_000_000);
    timer.set_limit(5, true);
    timer.run(false); // enable to see round-down
    assert_eq!(timer.get_count(), 4); // delta 5 -> shown as 4
}

#[test]
fn test_ptimer_get_count_no_round_down() {
    let (timer, _) = counting_timer(policy::NO_COUNTER_ROUND_DOWN);
    timer.set_limit(5, true);
    assert_eq!(timer.get_count(), 5); // exact
}

#[test]
fn test_ptimer_step() {
    let (timer, count) = counting_timer(0);
    timer.set_freq(1_000_000);
    timer.set_limit(3, true);
    timer.run(false); // periodic

    // Step 5 ticks: fires at tick 3 (1x), reloads, fires at tick 6? No:
    // ticks: 3->2, 2->1, 1->0(fire+reload->3), 3->2, 2->1
    // 5 ticks = 1 fire
    let fires = timer.step(5);
    assert_eq!(fires, 1);
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn test_ptimer_step_multiple_fires() {
    let (timer, count) = counting_timer(0);
    timer.set_freq(1_000_000);
    timer.set_limit(1, true);
    timer.run(false); // periodic, limit=1 -> fires every tick

    let fires = timer.step(5);
    assert_eq!(fires, 5);
    assert_eq!(count.load(Ordering::SeqCst), 5);
}

#[test]
fn test_ptimer_step_stops_after_disabled() {
    let (timer, count) = counting_timer(0);
    timer.set_freq(1_000_000);
    timer.set_limit(1, true);
    timer.run(true); // oneshot

    let fires = timer.step(10);
    assert_eq!(fires, 1); // fires once then stops
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(!timer.is_enabled());
}

#[test]
fn test_ptimer_continuous_trigger() {
    // CONTINUOUS_TRIGGER keeps the timer alive when limit=0;
    // uses the transaction API for proper reload semantics.
    let (timer, count) = counting_timer(policy::CONTINUOUS_TRIGGER);
    timer.set_freq(1_000_000);
    timer.set_limit(0, true); // limit=0 with CONTINUOUS_TRIGGER

    timer.begin();
    timer.run(false); // periodic, delta=0, need_reload set
    timer.commit(); // reload sets delta=1 per CONTINUOUS_TRIGGER

    // First tick: delta 1 -> 0, fires (callback fired once in commit, once here)
    assert!(timer.tick());
    assert_eq!(count.load(Ordering::SeqCst), 2);
    // Periodic + limit=0 + CONTINUOUS_TRIGGER: reload sets delta=1, stays alive
    assert!(timer.is_enabled());
}

#[test]
fn test_ptimer_no_immediate_trigger() {
    let (timer, count) = counting_timer(policy::NO_IMMEDIATE_TRIGGER);
    timer.set_freq(1_000_000);
    timer.set_limit(0, true); // count=0, but NO_IMMEDIATE_TRIGGER set
    timer.run(false);

    // Should not trigger immediately
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn test_ptimer_trigger_only_on_decrement() {
    let (timer, count) = counting_timer(
        policy::NO_IMMEDIATE_TRIGGER | policy::TRIGGER_ONLY_ON_DECREMENT,
    );
    timer.set_freq(1_000_000);
    timer.set_limit(3, true);
    timer.run(false);

    // Writing 0 to counter should not trigger
    timer.set_count(0);
    assert_eq!(count.load(Ordering::SeqCst), 0);

    // But decrementing through tick DOES trigger
    timer.set_count(1);
    assert!(timer.tick());
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn test_ptimer_wrap_after_one_period() {
    let (timer, _) = counting_timer(policy::WRAP_AFTER_ONE_PERIOD);
    timer.set_freq(1_000_000);
    timer.set_limit(10, true);
    timer.run(false);

    // Tick 10 times to fire at 0
    for _ in 0..10 {
        let _ = timer.tick();
    }
    // After wrapping, counter should reflect wrap adjustment
    assert!(timer.is_enabled());
}

#[test]
fn test_ptimer_no_immediate_reload() {
    let (timer, count) = counting_timer(policy::NO_IMMEDIATE_RELOAD);
    timer.set_freq(1_000_000);
    timer.set_limit(5, true);
    timer.run(false);

    // Tick to 0
    for _ in 0..4 {
        let _ = timer.tick();
    }
    let triggered = timer.tick(); // reaches 0
    assert!(triggered);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    // With NO_IMMEDIATE_RELOAD and limit!=0, stays at 1
}

// -- Negative Tests --

#[test]
fn test_ptimer_tick_when_stopped() {
    let (timer, count) = counting_timer(0);
    timer.set_freq(1_000_000);
    timer.set_limit(3, true);
    // Not started -> tick does nothing
    assert!(!timer.tick());
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn test_ptimer_tick_zero_period() {
    let (timer, count) = counting_timer(0);
    timer.set_limit(3, true);
    timer.run(false);
    // No period set -> tick returns false
    assert!(!timer.tick());
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn test_ptimer_tick_at_zero() {
    let (timer, _) = counting_timer(0);
    timer.set_freq(1_000_000);
    timer.set_count(0);
    timer.run(false);
    // Already at 0, tick returns false early
    assert!(!timer.tick());
}

#[test]
fn test_ptimer_zero_limit_periodic_stops() {
    let (timer, _count) = counting_timer(0);
    timer.set_freq(1_000_000);
    timer.set_limit(0, true);
    timer.run(false);

    // With count=0 and no CONTINUOUS_TRIGGER, tick returns false
    assert!(!timer.tick());
}

#[test]
fn test_ptimer_get_count_disabled() {
    let (timer, _) = counting_timer(0);
    timer.set_count(5);
    assert_eq!(timer.get_count(), 5); // disabled: exact count
}

#[test]
fn test_ptimer_transaction_begin_commit() {
    let (timer, _count) = counting_timer(0);
    timer.set_freq(1_000_000);
    timer.set_limit(5, true);

    timer.begin();
    timer.set_count(0);
    timer.run(false);
    timer.commit();

    // Transaction completed, timer should be enabled
    assert!(timer.is_enabled());
}

#[test]
fn test_ptimer_callback_none() {
    let timer = Ptimer::new(None::<Arc<dyn Fn() + Send + Sync>>, 0);
    timer.set_freq(1_000_000);
    timer.set_limit(1, true);
    timer.run(true); // oneshot

    // Should not crash with no callback; fires and stops
    assert!(timer.tick());
    assert!(!timer.is_enabled());
}

// -- VirtualClock integration tests --

#[test]
fn test_drive_ptimer_periodic() {
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let timer = Ptimer::new(
        Some(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })),
        0,
    );
    timer.set_freq(1_000_000); // 1 MHz → 1000 ns period
    timer.set_limit(1, true);
    timer.run(false); // periodic

    let clock = VirtualClock::new(machina_accel::timer::ClockType::Virtual);

    // Advance clock by 10,000 ns → 10 periods elapsed
    let fired = timer::drive_ptimer(&timer, &clock, 10_000);
    assert_eq!(fired, 10);
    assert_eq!(count.load(Ordering::SeqCst), 10);
    assert!(timer.is_enabled());
}

#[test]
fn test_drive_ptimer_oneshot() {
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let timer = Ptimer::new(
        Some(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })),
        0,
    );
    timer.set_freq(1_000_000);
    timer.set_limit(1, true);
    timer.run(true); // oneshot

    let clock = VirtualClock::new(machina_accel::timer::ClockType::Virtual);

    // 10,000 ns → 10 periods, but oneshot stops after first
    let fired = timer::drive_ptimer(&timer, &clock, 10_000);
    assert_eq!(fired, 1);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(!timer.is_enabled());
}

#[test]
fn test_drive_ptimer_stopped_no_fire() {
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let timer = Ptimer::new(
        Some(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })),
        0,
    );
    timer.set_freq(1_000_000);
    timer.set_limit(1, true);
    // Not started — timer is disabled

    let clock = VirtualClock::new(machina_accel::timer::ClockType::Virtual);
    let fired = timer::drive_ptimer(&timer, &clock, 10_000);
    assert_eq!(fired, 0);
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn test_drive_ptimer_sub_period_no_fire() {
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let timer = Ptimer::new(
        Some(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })),
        0,
    );
    timer.set_freq(1_000_000); // 1000 ns
    timer.set_limit(1, true);
    timer.run(false);

    let clock = VirtualClock::new(machina_accel::timer::ClockType::Virtual);
    // 500 ns < 1000 ns period → no full period elapsed yet
    let fired = timer::drive_ptimer(&timer, &clock, 500);
    assert_eq!(fired, 0);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(timer.is_enabled());
}

#[test]
fn test_drive_ptimer_cumulative_sub_period() {
    let count = Arc::new(AtomicU64::new(0));
    let c = count.clone();
    let timer = Ptimer::new(
        Some(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })),
        0,
    );
    timer.set_freq(1_000_000); // 1000 ns
    timer.set_limit(1, true);
    timer.run(false);

    let clock = VirtualClock::new(machina_accel::timer::ClockType::Virtual);
    // 500 ns + 500 ns = 1000 ns → one full period, fires once
    let fired1 = timer::drive_ptimer(&timer, &clock, 500);
    assert_eq!(fired1, 0, "no fire at 500/1000 ns");
    let fired2 = timer::drive_ptimer(&timer, &clock, 500);
    assert_eq!(fired2, 1, "fire at 1000/1000 ns");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(timer.is_enabled());
}

// -- schedule_ptimer tests --

#[test]
fn test_schedule_ptimer_periodic() {
    let count = Arc::new(AtomicU64::new(0));
    let c = count.clone();
    let timer = Ptimer::new(
        Some(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })),
        0,
    );
    timer.set_freq(1_000_000); // 1000 ns
    timer.set_limit(1, true);
    timer.run(false);

    let clock =
        Arc::new(VirtualClock::new(machina_accel::timer::ClockType::Virtual));
    let _handle = timer::schedule_ptimer(timer, clock.clone());

    // Step in loop: VirtualClock::step() collects expired timers
    // at call time and does not chain-fire callbacks added during
    // callback execution. Looping 5 × 1000 ns gives 5 fires.
    for _ in 0..5 {
        clock.step(1000);
    }
    assert_eq!(count.load(Ordering::SeqCst), 5);
}

#[test]
fn test_schedule_ptimer_oneshot() {
    let count = Arc::new(AtomicU64::new(0));
    let c = count.clone();
    let timer = Ptimer::new(
        Some(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })),
        0,
    );
    timer.set_freq(1_000_000);
    timer.set_limit(1, true);
    timer.run(true); // oneshot

    let clock =
        Arc::new(VirtualClock::new(machina_accel::timer::ClockType::Virtual));
    let _handle = timer::schedule_ptimer(timer, clock.clone());

    // Step 5000 ns → only first period fires (oneshot), then stops
    clock.step(5000);
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn test_schedule_ptimer_cancel() {
    let count = Arc::new(AtomicU64::new(0));
    let c = count.clone();
    let timer = Ptimer::new(
        Some(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })),
        0,
    );
    timer.set_freq(1_000_000);
    timer.set_limit(1, true);
    timer.run(false);

    let clock =
        Arc::new(VirtualClock::new(machina_accel::timer::ClockType::Virtual));
    let handle = timer::schedule_ptimer(timer, clock.clone());

    // Cancel before any fires
    assert!(handle.cancel());

    // Step well past one period — should fire nothing
    clock.step(5000);
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn test_schedule_ptimer_drop_cancels() {
    let count = Arc::new(AtomicU64::new(0));
    let c = count.clone();
    let timer = Ptimer::new(
        Some(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })),
        0,
    );
    timer.set_freq(1_000_000);
    timer.set_limit(1, true);
    timer.run(false);

    let clock =
        Arc::new(VirtualClock::new(machina_accel::timer::ClockType::Virtual));
    {
        let _handle = timer::schedule_ptimer(timer, clock.clone());
        // handle dropped here → cancels scheduling
    }

    clock.step(5000);
    // Drop destroyed the chain before the first fire
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn test_drive_ptimer_clock_advances() {
    let timer = Ptimer::new(None::<Arc<dyn Fn() + Send + Sync>>, 0);
    timer.set_freq(1_000_000);
    timer.set_limit(1, true);
    timer.run(false);

    let clock = VirtualClock::new(machina_accel::timer::ClockType::Virtual);
    assert_eq!(clock.get_ns(), 0);

    timer::drive_ptimer(&timer, &clock, 5_000_000);
    assert_eq!(clock.get_ns(), 5_000_000);
}

// -- Test helper for IRQ devices --

struct TestIrqSink {
    level: AtomicBool,
}

impl TestIrqSink {
    fn new() -> Self {
        Self {
            level: AtomicBool::new(false),
        }
    }

    fn level(&self) -> bool {
        self.level.load(Ordering::Relaxed)
    }
}

impl IrqSink for TestIrqSink {
    fn set_irq(&self, _irq: u32, level: bool) {
        self.level.store(level, Ordering::Relaxed);
    }
}

// -- SiFive PWM tests --

#[test]
fn test_sifive_pwm_defaults() {
    let pwm = Arc::new(SiFivePwm::new_with_freq(500_000_000));
    let mmio = SiFivePwmMmio(Arc::clone(&pwm));

    assert_eq!(mmio.read(0x00, 4), 0); // CONFIG
    assert_eq!(mmio.read(0x08, 4), 0); // COUNT
    assert_eq!(mmio.read(0x10, 4), 0); // PWMS
    assert_eq!(mmio.read(0x20, 4), 0); // PWMCMP0
    assert_eq!(mmio.read(0x24, 4), 0); // PWMCMP1
    assert_eq!(mmio.read(0x28, 4), 0); // PWMCMP2
    assert_eq!(mmio.read(0x2C, 4), 0); // PWMCMP3
}

#[test]
fn test_sifive_pwm_config_write() {
    let pwm = Arc::new(SiFivePwm::new_with_freq(500_000_000));
    let mmio = SiFivePwmMmio(Arc::clone(&pwm));

    // Write scale=3, enalways=1
    let val: u32 = 3 | (1 << 12);
    mmio.write(0x00, 4, u64::from(val));
    assert_eq!(mmio.read(0x00, 4), u64::from(val));
}

#[test]
fn test_sifive_pwm_count_write() {
    let pwm = Arc::new(SiFivePwm::new_with_freq(500_000_000));
    let mmio = SiFivePwmMmio(Arc::clone(&pwm));

    mmio.write(0x08, 4, 100);
    assert_eq!(mmio.read(0x08, 4), 100);
}

#[test]
fn test_sifive_pwm_pwmcmp_write() {
    let pwm = Arc::new(SiFivePwm::new_with_freq(500_000_000));
    let mmio = SiFivePwmMmio(Arc::clone(&pwm));

    mmio.write(0x20, 4, 0xABCD);
    assert_eq!(mmio.read(0x20, 4), 0xABCD);

    mmio.write(0x24, 4, 0x1234);
    assert_eq!(mmio.read(0x24, 4), 0x1234);
}

#[test]
fn test_sifive_pwm_pwms_scaled() {
    let pwm = Arc::new(SiFivePwm::new_with_freq(500_000_000));
    let mmio = SiFivePwmMmio(Arc::clone(&pwm));

    // Write count = 0x100 (shifted by scale=0 gives pwms=0x100)
    mmio.write(0x08, 4, 0x100);
    // PWMS = count >> scale & PWMCMP_MASK
    assert_eq!(mmio.read(0x10, 4), 0x100 & 0xFFFF);
}

#[test]
fn test_sifive_pwm_irq_fires() {
    let pwm = Arc::new(SiFivePwm::new_with_freq(500_000_000));
    let mmio = SiFivePwmMmio(Arc::clone(&pwm));
    let sink = Arc::new(TestIrqSink::new());
    pwm.connect_output(
        0,
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0),
    );

    assert!(!sink.level());

    // Set pwmcmp0 = 0 (matches pwms=0) and enable
    let cfg = (1u32 << 12); // ENALWAYS
    mmio.write(0x00, 4, u64::from(cfg));
    // IRQ should fire since pwms(0) >= pwmcmp0(0)
    assert!(sink.level());
}

#[test]
fn test_sifive_pwm_reset_runtime() {
    let pwm = Arc::new(SiFivePwm::new_with_freq(500_000_000));
    let mmio = SiFivePwmMmio(Arc::clone(&pwm));

    mmio.write(0x00, 4, 0xDEAD);
    mmio.write(0x20, 4, 1234);

    pwm.reset_runtime();

    assert_eq!(mmio.read(0x00, 4), 0);
    assert_eq!(mmio.read(0x20, 4), 0);
}

#[test]
fn test_sifive_pwm_pwmcmp_mask() {
    let pwm = Arc::new(SiFivePwm::new_with_freq(500_000_000));
    let mmio = SiFivePwmMmio(Arc::clone(&pwm));

    // Write value larger than 16-bit mask
    mmio.write(0x20, 4, 0x1ABCD);
    // Should be masked to 16 bits
    assert_eq!(mmio.read(0x20, 4), 0xABCD);
}

// -- SSE Counter tests --

#[test]
fn test_sse_counter_control_defaults() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let mmio = SseCounterControlMmio(Arc::clone(&counter));

    assert_eq!(mmio.read(0x00, 4), 0); // CNTCR
    assert_eq!(mmio.read(0x04, 4), 0); // CNTSR
    assert_eq!(mmio.read(0x08, 4), 0); // CNTCV_LO
    assert_eq!(mmio.read(0x0C, 4), 0); // CNTCV_HI
}

#[test]
fn test_sse_counter_status_defaults() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let mmio = SseCounterStatusMmio(Arc::clone(&counter));

    assert_eq!(mmio.read(0x00, 4), 0); // CNTCV_LO
    assert_eq!(mmio.read(0x04, 4), 0); // CNTCV_HI
}

#[test]
fn test_sse_counter_control_id_registers() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let mmio = SseCounterControlMmio(Arc::clone(&counter));

    assert_eq!(mmio.read(0xFD0, 4), 0x04); // PID4
    assert_eq!(mmio.read(0xFE0, 4), 0xBA); // PID0
    assert_eq!(mmio.read(0xFF0, 4), 0x0D); // CID0
    assert_eq!(mmio.read(0xFFC, 4), 0xB1); // CID3
}

#[test]
fn test_sse_counter_status_id_registers() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let mmio = SseCounterStatusMmio(Arc::clone(&counter));

    assert_eq!(mmio.read(0xFD0, 4), 0x04); // PID4
    assert_eq!(mmio.read(0xFE0, 4), 0xBB); // PID0
    assert_eq!(mmio.read(0xFFC, 4), 0xB1); // CID3
}

#[test]
fn test_sse_counter_cntid() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let mmio = SseCounterControlMmio(Arc::clone(&counter));

    // CNTSC=1, CNTSELCLK=1
    let id = mmio.read(0x1C, 4);
    assert_eq!(id & 1, 1); // CNTSC bit 0
}

#[test]
fn test_sse_counter_enable_disable() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let mmio = SseCounterControlMmio(Arc::clone(&counter));

    // Enable counter
    mmio.write(0x00, 4, 1); // CNTCR.EN = 1
    assert_eq!(mmio.read(0x00, 4), 1);

    // Disable
    mmio.write(0x00, 4, 0);
    assert_eq!(mmio.read(0x00, 4), 0);
}

#[test]
fn test_sse_counter_write_cntcv() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let mmio = SseCounterControlMmio(Arc::clone(&counter));

    mmio.write(0x08, 4, 0x12345678); // CNTCV_LO
    assert_eq!(mmio.read(0x08, 4), 0x12345678);

    mmio.write(0x0C, 4, 0x9ABCDEF0); // CNTCV_HI
    assert_eq!(mmio.read(0x0C, 4), 0x9ABCDEF0);
    // CNTCV_LO should be 0 (HI write clears LO)
    assert_eq!(mmio.read(0x08, 4), 0);
}

#[test]
fn test_sse_counter_write_cntscr() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let mmio = SseCounterControlMmio(Arc::clone(&counter));

    // Default CNTSCR0 = 0x0100_0000
    assert_eq!(mmio.read(0x10, 4), 0x0100_0000);

    mmio.write(0x10, 4, 0x0200_0000);
    assert_eq!(mmio.read(0x10, 4), 0x0200_0000);
}

#[test]
fn test_sse_counter_tick_advances() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let mmio = SseCounterControlMmio(Arc::clone(&counter));

    // Enable counter
    mmio.write(0x00, 4, 1);
    // Tick 1 second = 1_000_000_000 ns
    counter.tick(1_000_000_000);

    // At 1MHz, 1 second = 1_000_000 ticks
    let lo = mmio.read(0x08, 4);
    assert_eq!(lo, 1_000_000);
}

#[test]
fn test_sse_counter_tick_disabled() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let mmio = SseCounterControlMmio(Arc::clone(&counter));

    // Counter disabled by default
    counter.tick(1_000_000_000);
    assert_eq!(mmio.read(0x08, 4), 0);
}

#[test]
fn test_sse_counter_reset_runtime() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let mmio = SseCounterControlMmio(Arc::clone(&counter));

    mmio.write(0x00, 4, 1);
    mmio.write(0x08, 4, 1234);
    mmio.write(0x10, 4, 0xDEAD);

    counter.reset_runtime();

    assert_eq!(mmio.read(0x00, 4), 0);
    assert_eq!(mmio.read(0x10, 4), 0x0100_0000); // default CNTSCR0
}

#[test]
fn test_sse_counter_status_frame_read_only() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let ctrl_mmio = SseCounterControlMmio(Arc::clone(&counter));
    let status_mmio = SseCounterStatusMmio(Arc::clone(&counter));

    ctrl_mmio.write(0x08, 4, 0xABCD);
    // Status frame should see the same value
    assert_eq!(status_mmio.read(0x00, 4), 0xABCD);

    // Write to status frame is ignored
    status_mmio.write(0x00, 4, 0xFFFF);
    assert_eq!(status_mmio.read(0x00, 4), 0xABCD); // unchanged
}

// -- SSE Timer tests --

#[test]
fn test_sse_timer_defaults() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let timer = Arc::new(SseTimer::new(Arc::clone(&counter)));
    let mmio = SseTimerMmio(Arc::clone(&timer));

    assert_eq!(mmio.read(0x00, 4), 0); // CNTPCT_LO
    assert_eq!(mmio.read(0x04, 4), 0); // CNTPCT_HI
    assert_eq!(mmio.read(0x10, 4), 0); // CNTFRQ
    assert_eq!(mmio.read(0x20, 4), 0); // CNTP_CVAL_LO
    assert_eq!(mmio.read(0x24, 4), 0); // CNTP_CVAL_HI
    assert_eq!(mmio.read(0x28, 4), 0); // CNTP_TVAL
    assert_eq!(mmio.read(0x2C, 4), 0); // CNTP_CTL
    assert_eq!(mmio.read(0x40, 4), 0); // CNTP_AIVAL_LO
    assert_eq!(mmio.read(0x44, 4), 0); // CNTP_AIVAL_HI
    assert_eq!(mmio.read(0x48, 4), 0); // CNTP_AIVAL_RELOAD
    assert_eq!(mmio.read(0x4C, 4), 0); // CNTP_AIVAL_CTL
    assert_eq!(mmio.read(0x50, 4), 1); // CNTP_CFG
}

#[test]
fn test_sse_timer_id_registers() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let timer = Arc::new(SseTimer::new(Arc::clone(&counter)));
    let mmio = SseTimerMmio(Arc::clone(&timer));

    assert_eq!(mmio.read(0xFD0, 4), 0x04); // PID4
    assert_eq!(mmio.read(0xFE0, 4), 0xB7); // PID0
    assert_eq!(mmio.read(0xFF0, 4), 0x0D); // CID0
    assert_eq!(mmio.read(0xFFC, 4), 0xB1); // CID3
}

#[test]
fn test_sse_timer_write_cntfrq() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let timer = Arc::new(SseTimer::new(Arc::clone(&counter)));
    let mmio = SseTimerMmio(Arc::clone(&timer));

    mmio.write(0x10, 4, 100_000_000);
    assert_eq!(mmio.read(0x10, 4), 100_000_000);
}

#[test]
fn test_sse_timer_write_cval() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let timer = Arc::new(SseTimer::new(Arc::clone(&counter)));
    let mmio = SseTimerMmio(Arc::clone(&timer));

    mmio.write(0x20, 4, 0x12345678); // CNTP_CVAL_LO
    assert_eq!(mmio.read(0x20, 4), 0x12345678);

    mmio.write(0x24, 4, 0x9ABC); // CNTP_CVAL_HI
    assert_eq!(mmio.read(0x24, 4), 0x9ABC);
}

#[test]
fn test_sse_timer_tval_reflects_cval_minus_counter() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let ctrl_mmio = SseCounterControlMmio(Arc::clone(&counter));
    let timer = Arc::new(SseTimer::new(Arc::clone(&counter)));
    let mmio = SseTimerMmio(Arc::clone(&timer));

    // Enable counter and advance to 100
    ctrl_mmio.write(0x00, 4, 1);
    counter.tick(100_000); // At 1MHz, 100000ns = 100 ticks
    assert_eq!(ctrl_mmio.read(0x08, 4), 100);

    // Set cval to 200
    mmio.write(0x20, 4, 200);
    // TVAL = cval - counter = 200 - 100 = 100
    assert_eq!(mmio.read(0x28, 4), 100);
}

#[test]
fn test_sse_timer_irq_fires_when_counter_reaches_cval() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let ctrl_mmio = SseCounterControlMmio(Arc::clone(&counter));
    let timer = Arc::new(SseTimer::new(Arc::clone(&counter)));
    let mmio = SseTimerMmio(Arc::clone(&timer));
    let sink = Arc::new(TestIrqSink::new());
    timer.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    // Enable counter
    ctrl_mmio.write(0x00, 4, 1);

    // Set cval to 1000 and enable timer
    mmio.write(0x20, 4, 1000);
    mmio.write(0x2C, 4, 1); // CTL.ENABLE = 1

    assert!(!sink.level());

    // Advance counter past cval
    counter.tick(2_000_000); // 2000 ticks (2ms at 1MHz)

    // Timer tick checks condition
    timer.tick();
    assert!(sink.level());
    assert_eq!(mmio.read(0x2C, 4) & 4, 4); // ISTATUS set
}

#[test]
fn test_sse_timer_imask_blocks_irq() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let ctrl_mmio = SseCounterControlMmio(Arc::clone(&counter));
    let timer = Arc::new(SseTimer::new(Arc::clone(&counter)));
    let mmio = SseTimerMmio(Arc::clone(&timer));
    let sink = Arc::new(TestIrqSink::new());
    timer.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    // Enable with IMASK set
    mmio.write(0x20, 4, 100);
    mmio.write(0x2C, 4, 3); // ENABLE | IMASK
    ctrl_mmio.write(0x00, 4, 1);
    counter.tick(200_000);
    timer.tick();

    assert!(!sink.level(), "IRQ should be blocked when IMASK set");
}

#[test]
fn test_sse_timer_autoinc_mode() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let ctrl_mmio = SseCounterControlMmio(Arc::clone(&counter));
    let timer = Arc::new(SseTimer::new(Arc::clone(&counter)));
    let mmio = SseTimerMmio(Arc::clone(&timer));
    let sink = Arc::new(TestIrqSink::new());
    timer.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    // Set reload and enable auto-inc
    mmio.write(0x48, 4, 1000); // AIVAL_RELOAD
    mmio.write(0x4C, 4, 1); // AIVAL_CTL.EN
    mmio.write(0x2C, 4, 1); // CTL.ENABLE
    ctrl_mmio.write(0x00, 4, 1);

    // Advance counter
    counter.tick(2_000_000); // past AIVAL
    timer.tick();

    // CLR should be set (auto-increment fired)
    assert_eq!(mmio.read(0x4C, 4) & 2, 2);
    assert!(sink.level());
}

#[test]
fn test_sse_timer_aival_ctl_clr_write_zero_clears() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let ctrl_mmio = SseCounterControlMmio(Arc::clone(&counter));
    let timer = Arc::new(SseTimer::new(Arc::clone(&counter)));
    let mmio = SseTimerMmio(Arc::clone(&timer));

    // Set reload and enable auto-inc
    mmio.write(0x48, 4, 100);
    mmio.write(0x4C, 4, 1); // EN
    mmio.write(0x2C, 4, 1); // CTL.ENABLE
    ctrl_mmio.write(0x00, 4, 1);

    // Fire auto-increment
    counter.tick(200_000);
    timer.tick();
    assert_eq!(mmio.read(0x4C, 4) & 2, 2); // CLR set

    // Write 0 to CLR bit to clear
    mmio.write(0x4C, 4, 1); // keep EN, clear CLR
    assert_eq!(mmio.read(0x4C, 4) & 2, 0); // CLR cleared
}

#[test]
fn test_sse_timer_tval_write_sets_cval() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let ctrl_mmio = SseCounterControlMmio(Arc::clone(&counter));
    let timer = Arc::new(SseTimer::new(Arc::clone(&counter)));
    let mmio = SseTimerMmio(Arc::clone(&timer));

    ctrl_mmio.write(0x00, 4, 1);
    counter.tick(100_000); // counter = 100

    // Write TVAL = 500 → cval = counter + 500 = 600
    mmio.write(0x28, 4, 500);
    assert_eq!(mmio.read(0x20, 4), 600); // CNTP_CVAL_LO
}

#[test]
fn test_sse_timer_reset_runtime() {
    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let timer = Arc::new(SseTimer::new(Arc::clone(&counter)));
    let mmio = SseTimerMmio(Arc::clone(&timer));

    mmio.write(0x10, 4, 1234);
    mmio.write(0x20, 4, 5678);
    mmio.write(0x2C, 4, 1);

    timer.reset_runtime();

    assert_eq!(mmio.read(0x10, 4), 0);
    assert_eq!(mmio.read(0x20, 4), 0);
    assert_eq!(mmio.read(0x2C, 4), 0);
}
