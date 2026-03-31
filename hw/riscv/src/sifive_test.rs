// SiFive Test device for system reset/shutdown.
//
// MMIO register at base address (single 32-bit word):
//   Write 0x5555 -> PASS  (clean shutdown)
//   Write 0x3333 -> RESET (system reboot)
//   Other values  -> FAIL  (error exit)
//   Read          -> 0     (no side effects)
//
// DTB compatible: "sifive,test0"

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use machina_memory::region::MmioOps;

const FINISHER_PASS: u32 = 0x5555;
const FINISHER_RESET: u32 = 0x3333;

/// Shutdown reason reported by the SiFive Test device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    Pass,
    Reset,
    Fail(u32),
}

type ShutdownFn = Mutex<Option<Box<dyn Fn(ShutdownReason) + Send>>>;

/// SiFive Test device state.
pub struct SifiveTest {
    triggered: Arc<AtomicBool>,
    on_shutdown: ShutdownFn,
}

impl SifiveTest {
    pub fn new() -> Self {
        Self {
            triggered: Arc::new(AtomicBool::new(false)),
            on_shutdown: Mutex::new(None),
        }
    }

    /// Install a shutdown callback.
    pub fn set_shutdown_handler(
        &self,
        handler: Box<dyn Fn(ShutdownReason) + Send>,
    ) {
        *self.on_shutdown.lock().unwrap() = Some(handler);
    }

    /// Whether a shutdown/reset has been triggered.
    pub fn is_triggered(&self) -> bool {
        self.triggered.load(Ordering::Relaxed)
    }
}

impl Default for SifiveTest {
    fn default() -> Self {
        Self::new()
    }
}

impl MmioOps for SifiveTest {
    fn read(&self, _offset: u64, _size: u32) -> u64 {
        0
    }

    fn write(&self, _offset: u64, _size: u32, val: u64) {
        let val32 = val as u32;
        let reason = match val32 {
            FINISHER_PASS => ShutdownReason::Pass,
            FINISHER_RESET => ShutdownReason::Reset,
            _ => ShutdownReason::Fail(val32),
        };
        self.triggered.store(true, Ordering::SeqCst);
        if let Some(ref handler) = *self.on_shutdown.lock().unwrap() {
            handler(reason);
        }
    }
}
