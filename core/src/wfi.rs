// WFI wakeup primitive: Condvar-based notification for
// halted CPU wakeup by device IRQ or manager stop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};

/// Wakeup signal for WFI (Wait For Interrupt).
///
/// - Device IRQ sinks call `wake()` to unblock WFI.
/// - CpuManager calls `stop()` to force-unblock WFI
///   for safe shutdown.
pub struct WfiWaker {
    notified: Mutex<bool>,
    stopped: AtomicBool,
    cv: Condvar,
}

impl WfiWaker {
    pub fn new() -> Self {
        Self {
            notified: Mutex::new(false),
            stopped: AtomicBool::new(false),
            cv: Condvar::new(),
        }
    }

    /// Wake halted CPU (device IRQ arrived).
    pub fn wake(&self) {
        let mut n = self.notified.lock().unwrap();
        *n = true;
        self.cv.notify_all();
    }

    /// Force-unblock any waiting CPU (manager stop).
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.cv.notify_all();
    }

    /// Block until woken by `wake()` or `stop()`.
    /// Returns true if woken by IRQ, false if stopped.
    pub fn wait(&self) -> bool {
        let mut n = self.notified.lock().unwrap();
        loop {
            if *n {
                *n = false;
                return true;
            }
            if self.stopped.load(Ordering::SeqCst) {
                return false;
            }
            n = self.cv.wait(n).unwrap();
        }
    }
}

impl Default for WfiWaker {
    fn default() -> Self {
        Self::new()
    }
}
