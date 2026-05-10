use std::sync::{Arc, Mutex};

use machina_hw_core::clock::DeviceClock;

#[test]
fn test_clock_period() {
    // 1 GHz clock -> 1 ns period.
    let clk = DeviceClock::new(1_000_000_000);
    assert_eq!(clk.period_ns(), 1);
}

#[test]
fn test_clock_disabled() {
    let mut clk = DeviceClock::new(100_000_000);
    clk.set_enabled(false);
    assert!(!clk.enabled());
    // Frequency is still reported even when disabled.
    assert_eq!(clk.freq_hz(), 100_000_000);
}

#[test]
fn test_clock_zero_freq() {
    let clk = DeviceClock::new(0);
    assert_eq!(clk.period_ns(), 0);
}

// -- Propagation tests --

#[test]
fn test_clock_propagation() {
    let mut parent = DeviceClock::new(100_000_000);
    let child = Arc::new(Mutex::new(DeviceClock::new(0)));
    parent.add_child(&child);

    // Propagate 100 MHz to child (1:1 ratio).
    parent.propagate();
    assert_eq!(child.lock().unwrap().freq_hz(), 100_000_000);

    // Change parent and propagate again.
    parent.set_freq_and_propagate(200_000_000);
    assert_eq!(child.lock().unwrap().freq_hz(), 200_000_000);
}

#[test]
fn test_clock_multiplier() {
    let mut parent = DeviceClock::new(50_000_000);
    let child = Arc::new(Mutex::new(DeviceClock::new(0)));
    child.lock().unwrap().multiplier = 2;

    parent.add_child(&child);
    parent.propagate();

    // Child should get 2x the parent frequency.
    assert_eq!(child.lock().unwrap().freq_hz(), 100_000_000);
}

#[test]
fn test_clock_dead_child() {
    let mut parent = DeviceClock::new(100_000_000);
    {
        let child = Arc::new(Mutex::new(DeviceClock::new(0)));
        parent.add_child(&child);
        // child dropped here
    }
    // Propagating with a dead weak ref must not panic.
    parent.propagate();
}

// -- Issue #81: extra property and boundary coverage --

#[test]
fn test_clock_freq_roundtrip() {
    let mut clk = DeviceClock::new(0);
    clk.set_freq(123_456_789);
    assert_eq!(clk.freq_hz(), 123_456_789);
    clk.set_freq(0);
    assert_eq!(clk.freq_hz(), 0);
}

#[test]
fn test_clock_enabled_by_default_then_toggle() {
    let mut clk = DeviceClock::new(50_000_000);
    assert!(
        clk.enabled(),
        "new DeviceClock should be enabled by default"
    );
    clk.set_enabled(false);
    assert!(!clk.enabled());
    clk.set_enabled(true);
    assert!(clk.enabled(), "set_enabled(true) must re-enable the clock");
}

#[test]
fn test_clock_period_100mhz_is_10ns() {
    // Spec example from issue #81: 100 MHz -> 10 ns period.
    let clk = DeviceClock::new(100_000_000);
    assert_eq!(clk.period_ns(), 10);
}

#[test]
fn test_clock_period_large_frequency_truncates_to_zero() {
    // Frequencies above 1 GHz produce sub-nanosecond periods, which
    // truncate to zero under integer division. This is the documented
    // behaviour and must not panic on near-u64::MAX inputs.
    let clk = DeviceClock::new(u64::MAX);
    assert_eq!(clk.period_ns(), 0);
    let clk = DeviceClock::new(2_000_000_000);
    assert_eq!(clk.period_ns(), 0);
}

#[test]
fn test_clock_propagate_with_divider() {
    let mut parent = DeviceClock::new(200_000_000);
    let child = Arc::new(Mutex::new(DeviceClock::new(0)));
    child.lock().unwrap().divider = 4;
    parent.add_child(&child);

    parent.propagate();
    assert_eq!(
        child.lock().unwrap().freq_hz(),
        50_000_000,
        "divider=4 must quarter the parent frequency",
    );
}

#[test]
fn test_clock_propagate_with_combined_multiplier_and_divider() {
    let mut parent = DeviceClock::new(48_000_000);
    let child = Arc::new(Mutex::new(DeviceClock::new(0)));
    {
        let mut c = child.lock().unwrap();
        c.multiplier = 5;
        c.divider = 3;
    }
    parent.add_child(&child);
    parent.propagate();
    assert_eq!(
        child.lock().unwrap().freq_hz(),
        80_000_000,
        "child freq = parent * mul / div",
    );
}

#[test]
fn test_clock_propagate_recurses_into_grandchildren() {
    // parent (50 MHz) -> child (x2 = 100 MHz) -> grandchild (/5 = 20 MHz)
    let mut parent = DeviceClock::new(50_000_000);
    let child = Arc::new(Mutex::new(DeviceClock::new(0)));
    let grandchild = Arc::new(Mutex::new(DeviceClock::new(0)));
    {
        let mut c = child.lock().unwrap();
        c.multiplier = 2;
        c.add_child(&grandchild);
    }
    grandchild.lock().unwrap().divider = 5;
    parent.add_child(&child);

    parent.set_freq_and_propagate(50_000_000);

    assert_eq!(child.lock().unwrap().freq_hz(), 100_000_000);
    assert_eq!(
        grandchild.lock().unwrap().freq_hz(),
        20_000_000,
        "set_freq_and_propagate must reach grandchildren",
    );
}

#[test]
fn test_clock_add_child_prunes_dead_refs_on_insert() {
    let mut parent = DeviceClock::new(100_000_000);

    // Insert and immediately drop a child — leaves a dead Weak in
    // parent.children.
    {
        let stale = Arc::new(Mutex::new(DeviceClock::new(0)));
        parent.add_child(&stale);
    }

    // Inserting a fresh child must prune the stale entry; the only
    // observable signal is that propagation still updates the live
    // child correctly without panicking on the dead weak ref.
    let live = Arc::new(Mutex::new(DeviceClock::new(0)));
    parent.add_child(&live);

    parent.propagate();
    assert_eq!(live.lock().unwrap().freq_hz(), 100_000_000);
}
