// WFI wakeup primitive: Condvar-based notification for
// halted CPU wakeup by device IRQ, timer deadline, or
// manager stop.
//
// All state is protected by a single mutex to prevent
// lost-wakeup races between wake/stop and wait.

use std::sync::{Condvar, Mutex};
use std::time::Instant;

struct WfiState {
    irq_pending: bool,
    stopped: bool,
    /// Nearest timer deadline (if any). wait() uses
    /// condvar::wait_timeout when this is set.
    deadline: Option<Instant>,
}

/// Wakeup signal for WFI (Wait For Interrupt).
///
/// - Device IRQ sinks call `wake()` to unblock WFI.
/// - Timer code calls `set_deadline()` + `wake()` when
///   mtimecmp changes.
/// - CpuManager calls `stop()` to force-unblock WFI.
/// - `wait()` blocks until IRQ, deadline, or stop.
pub struct WfiWaker {
    state: Mutex<WfiState>,
    cv: Condvar,
}

impl WfiWaker {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(WfiState {
                irq_pending: false,
                stopped: false,
                deadline: None,
            }),
            cv: Condvar::new(),
        }
    }

    /// Wake halted CPU (device IRQ arrived).
    pub fn wake(&self) {
        let mut s = self.state.lock().unwrap();
        s.irq_pending = true;
        self.cv.notify_all();
    }

    /// Force-unblock any waiting CPU (manager stop).
    pub fn stop(&self) {
        let mut s = self.state.lock().unwrap();
        s.stopped = true;
        self.cv.notify_all();
    }

    /// Set the nearest timer deadline. The next wait()
    /// call will use wait_timeout with this deadline.
    /// Call wake() after this to re-evaluate an ongoing
    /// wait.
    pub fn set_deadline(&self, deadline: Instant) {
        let mut s = self.state.lock().unwrap();
        s.deadline = Some(deadline);
    }

    /// Clear the timer deadline.
    pub fn clear_deadline(&self) {
        let mut s = self.state.lock().unwrap();
        s.deadline = None;
    }

    /// Block until woken by `wake()`, `stop()`, or timer
    /// deadline expiry.
    /// Returns true if woken by IRQ or timer, false if
    /// stopped.
    pub fn wait(&self) -> bool {
        let mut s = self.state.lock().unwrap();
        loop {
            if s.irq_pending {
                s.irq_pending = false;
                return true;
            }
            if s.stopped {
                return false;
            }
            if let Some(deadline) = s.deadline {
                let now = Instant::now();
                if now >= deadline {
                    // Deadline already passed.
                    s.deadline = None;
                    return true;
                }
                let remaining = deadline - now;
                let (new_s, result) =
                    self.cv.wait_timeout(s, remaining).unwrap();
                s = new_s;
                if result.timed_out() {
                    s.deadline = None;
                    return true;
                }
            } else {
                s = self.cv.wait(s).unwrap();
            }
        }
    }
}

impl Default for WfiWaker {
    fn default() -> Self {
        Self::new()
    }
}
