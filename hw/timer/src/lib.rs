//! General-purpose periodic/one-shot countdown timer.
//!
//! Provides [`Ptimer`], a virtual-clock-driven countdown timer with
//! policy flags, limit-reload semantics, and a transaction-based API
//! for batched state modifications.
//!
//! ## Integration with VirtualClock
//!
//! Ptimer can be driven by a [`VirtualClock`] via [`Ptimer::schedule_on`].
//! The clock steps virtual nanoseconds forward and fires expired
//! callbacks; each callback calls `Ptimer::tick()`. For periodic
//! timers the callback re-schedules itself.

use std::sync::Arc;

use machina_accel::timer::VirtualClock;
use machina_core::device_cell::DeviceRefCell;

/// Policy flags controlling ptimer trigger/reload behavior.
pub mod policy {
    /// Counter wraps after one full period at zero.
    pub const WRAP_AFTER_ONE_PERIOD: u8 = 1 << 0;
    /// Periodic timer with limit=0 triggers continuously.
    pub const CONTINUOUS_TRIGGER: u8 = 1 << 1;
    /// Setting counter to 0 does not immediately trigger.
    pub const NO_IMMEDIATE_TRIGGER: u8 = 1 << 2;
    /// Setting counter to 0 does not immediately reload.
    pub const NO_IMMEDIATE_RELOAD: u8 = 1 << 3;
    /// Counter value reflects actual value, not value-1.
    pub const NO_COUNTER_ROUND_DOWN: u8 = 1 << 4;
    /// Only a counter decrement to 0 triggers; a write of 0 does not.
    /// Incompatible with NO_IMMEDIATE_TRIGGER.
    pub const TRIGGER_ONLY_ON_DECREMENT: u8 = 1 << 5;
}

/// Timer enabled state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Enabled {
    Disabled = 0,
    Periodic = 1,
    OneShot = 2,
}

/// Callback invoked when the timer expires.
pub type PtimerCallback = Arc<dyn Fn() + Send + Sync>;

/// Ptimer internal state.
struct PtimerState {
    enabled: Enabled,
    limit: u64,
    delta: u64,
    period_ns: u64,
    period_frac: u32,
    policy_mask: u8,
    callback: Option<PtimerCallback>,
    in_transaction: bool,
    need_reload: bool,
}

/// A general-purpose periodic/one-shot countdown timer.
///
/// Driven by a virtual clock — the caller must call [`tick`](Ptimer::tick)
/// or the timer can be advanced externally via [`step`](Ptimer::step).
///
/// Uses a transaction-based API: all state-modifying calls
/// (`set_limit`, `set_count`, `set_period`, `run`, `stop`)
/// must be wrapped in [`begin`](Ptimer::begin) /
/// [`commit`](Ptimer::commit).
pub struct Ptimer {
    inner: DeviceRefCell<PtimerState>,
}

impl Ptimer {
    /// Create a new ptimer with the given callback and policy mask.
    #[must_use]
    pub fn new(callback: Option<PtimerCallback>, policy_mask: u8) -> Arc<Self> {
        Arc::new(Self {
            inner: DeviceRefCell::new(PtimerState {
                enabled: Enabled::Disabled,
                limit: 0,
                delta: 0,
                period_ns: 0,
                period_frac: 0,
                policy_mask,
                callback,
                in_transaction: false,
                need_reload: false,
            }),
        })
    }

    /// Begin a modification transaction.
    ///
    /// Must be paired with [`commit`](Self::commit).
    pub fn begin(&self) {
        let mut s = self.inner.borrow();
        s.in_transaction = true;
    }

    /// Commit a modification transaction.
    ///
    /// Evaluates the timer state after all changes in the transaction
    /// and calls the callback if necessary.
    pub fn commit(&self) {
        let mut s = self.inner.borrow();
        s.in_transaction = false;
        if s.need_reload {
            s.need_reload = false;
            drop(s);
            self.reload(0);
        }
    }

    /// Set the timer period in nanoseconds.
    pub fn set_period(&self, period_ns: u64) {
        let mut s = self.inner.borrow();
        s.period_ns = period_ns;
        s.period_frac = 0;
    }

    /// Set the timer frequency in Hz.
    pub fn set_freq(&self, freq_hz: u32) {
        let mut s = self.inner.borrow();
        if freq_hz == 0 {
            s.period_ns = 0;
            s.period_frac = 0;
        } else {
            // period_ns = 1_000_000_000 / freq_hz
            // period_frac handles the remainder for sub-ns precision
            s.period_ns = 1_000_000_000u64 / u64::from(freq_hz);
            let rem = 1_000_000_000u64 % u64::from(freq_hz);
            let f = u64::from(freq_hz);
            s.period_frac = (((rem << 32) / f) & 0xFFFF_FFFF) as u32;
        }
    }

    /// Return the current limit (reload value).
    #[must_use]
    pub fn get_limit(&self) -> u64 {
        self.inner.borrow().limit
    }

    /// Set the limit.
    ///
    /// If `reload` is true, also resets the counter to the new limit.
    pub fn set_limit(&self, limit: u64, reload: bool) {
        let mut s = self.inner.borrow();
        s.limit = limit;
        if reload {
            s.delta = limit;
        }
        if s.delta == 0 && s.need_reload_if_enabled() {
            s.need_reload = true;
        }
    }

    /// Return the current counter value.
    #[must_use]
    pub fn get_count(&self) -> u64 {
        let s = self.inner.borrow();
        if s.enabled != Enabled::Disabled
            && s.policy_mask & policy::NO_COUNTER_ROUND_DOWN == 0
        {
            // In legacy mode, counter is one less than actual
            s.delta.saturating_sub(1)
        } else {
            s.delta
        }
    }

    /// Set the counter value.
    pub fn set_count(&self, count: u64) {
        let mut s = self.inner.borrow();
        s.delta = count;
        if s.delta == 0 && s.need_reload_if_enabled() {
            s.need_reload = true;
        }
    }

    /// Start the timer.
    ///
    /// `oneshot` — if true, timer stops after one expiry;
    /// if false, timer reloads and continues.
    pub fn run(&self, oneshot: bool) {
        let mut s = self.inner.borrow();
        s.enabled = if oneshot {
            Enabled::OneShot
        } else {
            Enabled::Periodic
        };
        if s.delta == 0 && s.need_reload_if_enabled() {
            s.need_reload = true;
        }
    }

    /// Stop the timer.
    pub fn stop(&self) {
        let mut s = self.inner.borrow();
        s.enabled = Enabled::Disabled;
    }

    /// Return true if the timer is currently running.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.inner.borrow().enabled != Enabled::Disabled
    }

    /// Step the timer forward by one tick (one period).
    ///
    /// Decrements the counter and triggers the callback if it reaches
    /// zero. Returns true if the callback was invoked.
    #[must_use]
    pub fn tick(&self) -> bool {
        let s = self.inner.borrow();
        if s.enabled == Enabled::Disabled || s.period_ns == 0 {
            return false;
        }
        if s.delta == 0 {
            return false;
        }
        drop(s);

        let mut s = self.inner.borrow();
        s.delta = s.delta.saturating_sub(1);

        if s.delta == 0 {
            let was_oneshot = s.enabled == Enabled::OneShot;
            let callback = s.callback.clone();
            drop(s);

            // Invoke callback outside the borrow
            if let Some(ref cb) = callback {
                cb();
            }

            let mut s = self.inner.borrow();
            if was_oneshot {
                s.enabled = Enabled::Disabled;
            } else {
                // Reload for periodic
                s.delta = s.limit;
                if s.delta == 0
                    && s.policy_mask & policy::CONTINUOUS_TRIGGER == 0
                {
                    s.enabled = Enabled::Disabled;
                }
            }
            return true;
        }
        false
    }

    /// Step the timer forward by `ticks` periods.
    ///
    /// Returns the number of times the callback was invoked.
    #[must_use]
    pub fn step(&self, ticks: u64) -> u64 {
        let mut count = 0;
        for _ in 0..ticks {
            if self.tick() {
                count += 1;
            }
            if !self.is_enabled() {
                break;
            }
        }
        count
    }
}

impl PtimerState {
    fn need_reload_if_enabled(&self) -> bool {
        self.enabled != Enabled::Disabled
            && self.policy_mask & policy::NO_IMMEDIATE_TRIGGER == 0
    }
}

/// Drive a Ptimer from a VirtualClock step.
///
/// Advances the clock by `delta_ns`, then ticks the ptimer once
/// for each full period elapsed. Returns the number of callback
/// invocations.
///
/// This is the canonical event-loop integration pattern:
///
/// ```text
/// loop {
///     let delta = compute_time_slice();
///     drive_ptimer(&ptimer, &clock, delta);
///     // ... other event loop work
/// }
/// ```
pub fn drive_ptimer(
    ptimer: &Ptimer,
    clock: &VirtualClock,
    delta_ns: i64,
) -> u64 {
    clock.step(delta_ns);
    let period_ns = ptimer.period_ns() as i64;
    if period_ns <= 0 || !ptimer.is_enabled() {
        return 0;
    }
    let elapsed = (delta_ns / period_ns) as u64;
    if elapsed == 0 {
        return 0;
    }
    ptimer.step(elapsed)
}

impl Ptimer {
    /// Return the current period in nanoseconds.
    #[must_use]
    pub fn period_ns(&self) -> u64 {
        self.inner.borrow().period_ns
    }

    /// Internal reload logic.
    fn reload(&self, delta_adjust: i32) {
        let mut s = self.inner.borrow();

        let suppress_trigger = delta_adjust == 0
            && s.policy_mask & policy::TRIGGER_ONLY_ON_DECREMENT != 0;

        if s.delta == 0
            && s.policy_mask & policy::NO_IMMEDIATE_TRIGGER == 0
            && !suppress_trigger
        {
            let callback = s.callback.clone();
            drop(s);
            if let Some(ref cb) = callback {
                cb();
            }
            s = self.inner.borrow();
        }

        let delta = s.delta;
        let period_ns = s.period_ns;
        let period_frac = s.period_frac;

        if delta == 0 && s.policy_mask & policy::NO_IMMEDIATE_RELOAD == 0 {
            s.delta = s.limit;
        }

        if period_ns == 0 && period_frac == 0 {
            s.enabled = Enabled::Disabled;
            return;
        }

        if s.policy_mask & policy::WRAP_AFTER_ONE_PERIOD != 0
            && delta_adjust >= 0
        {
            s.delta += delta_adjust as u64;
        }

        if s.delta == 0
            && s.policy_mask & policy::CONTINUOUS_TRIGGER != 0
            && s.enabled == Enabled::Periodic
            && s.limit == 0
        {
            s.delta = 1;
        }

        if s.delta == 0
            && s.policy_mask & policy::NO_IMMEDIATE_TRIGGER != 0
            && delta_adjust >= 0
        {
            s.delta = 1;
        }

        if s.delta == 0
            && s.policy_mask & policy::NO_IMMEDIATE_RELOAD != 0
            && s.enabled == Enabled::Periodic
            && s.limit != 0
        {
            s.delta = 1;
        }

        if s.delta == 0 {
            s.enabled = Enabled::Disabled;
        }
    }
}
