// Device clock model with parent-child propagation.

use std::sync::{Arc, Mutex, Weak};

/// A device clock with configurable frequency and
/// parent-child propagation.
pub struct DeviceClock {
    freq_hz: u64,
    enabled: bool,
    children: Vec<Weak<Mutex<DeviceClock>>>,
    /// Child frequency = parent_freq * multiplier / divider.
    pub multiplier: u64,
    /// Child frequency = parent_freq * multiplier / divider.
    pub divider: u64,
}

impl DeviceClock {
    /// Create a new enabled clock at `freq_hz`. The clock has no
    /// children and a 1:1 multiplier/divider, so by default it
    /// propagates the parent frequency unchanged.
    pub fn new(freq_hz: u64) -> Self {
        Self {
            freq_hz,
            enabled: true,
            children: Vec::new(),
            multiplier: 1,
            divider: 1,
        }
    }

    /// Current frequency in Hz.
    pub fn freq_hz(&self) -> u64 {
        self.freq_hz
    }

    /// Change the frequency.
    pub fn set_freq(&mut self, freq_hz: u64) {
        self.freq_hz = freq_hz;
    }

    /// Whether the clock is enabled.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable the clock.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Period in nanoseconds (integer division).
    /// Returns 0 when `freq_hz` is 0.
    pub fn period_ns(&self) -> u64 {
        if self.freq_hz == 0 {
            return 0;
        }
        1_000_000_000 / self.freq_hz
    }

    /// Subscribe a child clock to this parent.
    /// Dead weak refs are pruned on each call.
    pub fn add_child(&mut self, child: &Arc<Mutex<DeviceClock>>) {
        self.children.retain(|w| w.strong_count() > 0);
        self.children.push(Arc::downgrade(child));
    }

    /// Propagate this clock's frequency to all children.
    /// Each child's effective frequency is
    /// `self.freq_hz * child.multiplier / child.divider`.
    /// Dead weak refs are silently skipped.
    pub fn propagate(&self) {
        for weak in &self.children {
            if let Some(arc) = weak.upgrade() {
                let mut child = arc.lock().unwrap();
                child.freq_hz = self.freq_hz * child.multiplier / child.divider;
                // Recurse into grandchildren.
                child.propagate();
            }
        }
    }

    /// Set this clock's frequency and propagate to all
    /// descendant clocks.
    pub fn set_freq_and_propagate(&mut self, freq: u64) {
        self.freq_hz = freq;
        self.propagate();
    }
}
