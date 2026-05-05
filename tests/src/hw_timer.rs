use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use machina_accel::timer::VirtualClock;
use machina_hw_timer::policy;
use machina_hw_timer::{self as timer, Ptimer};

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
    // 500 ns < 1000 ns period → no full period elapsed
    let fired = timer::drive_ptimer(&timer, &clock, 500);
    assert_eq!(fired, 0);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(timer.is_enabled());
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
