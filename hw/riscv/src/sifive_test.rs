// SiFive Test device for system reset/shutdown.
//
// MMIO register at base address (single 32-bit word):
//   Write 0x5555 -> PASS  (clean shutdown)
//   Write 0x3333 -> FAIL  (error exit)
//   Write 0x7777 -> RESET (system reboot)
//   Read          -> 0     (no side effects)
//
// DTB compatible: "sifive,test0"

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

const FINISHER_FAIL: u32 = 0x3333;
const FINISHER_PASS: u32 = 0x5555;
const FINISHER_RESET: u32 = 0x7777;

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
    state: Mutex<SysBusDeviceState>,
    triggered: Arc<AtomicBool>,
    on_shutdown: ShutdownFn,
}

impl SifiveTest {
    pub fn new() -> Self {
        Self::new_named("sifive-test")
    }

    pub fn new_named(local_id: &str) -> Self {
        Self {
            state: Mutex::new(SysBusDeviceState::new(local_id)),
            triggered: Arc::new(AtomicBool::new(false)),
            on_shutdown: Mutex::new(None),
        }
    }

    pub fn attach_to_bus(&self, bus: &mut SysBus) -> Result<(), SysBusError> {
        self.state.lock().unwrap().attach_to_bus(bus)
    }

    pub fn register_mmio(
        &self,
        region: MemoryRegion,
        base: GPA,
    ) -> Result<(), SysBusError> {
        self.state.lock().unwrap().register_mmio(region, base)
    }

    pub fn realize_onto(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.state.lock().unwrap().realize_onto(bus, address_space)
    }

    pub fn unrealize_from(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.state
            .lock()
            .unwrap()
            .unrealize_from(bus, address_space)
    }

    pub fn reset_runtime(&self) {
        self.triggered.store(false, Ordering::SeqCst);
    }

    pub fn realized(&self) -> bool {
        self.state.lock().unwrap().is_realized()
    }

    pub fn with_mdevice<T>(&self, f: impl FnOnce(&dyn MDevice) -> T) -> T {
        let guard = self.state.lock().unwrap();
        f(&*guard)
    }

    pub fn object_info(&self) -> MObjectInfo {
        self.state.lock().unwrap().object_info()
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

    fn write(&self, offset: u64, size: u32, val: u64) {
        if offset != 0 || (size != 2 && size != 4) {
            return;
        }
        let status = (val & 0xffff) as u32;
        let code = ((val >> 16) & 0xffff) as u32;
        let reason = match status {
            FINISHER_FAIL => ShutdownReason::Fail(code),
            FINISHER_PASS => ShutdownReason::Pass,
            FINISHER_RESET => ShutdownReason::Reset,
            _ => return,
        };
        self.triggered.store(true, Ordering::SeqCst);
        if let Some(ref handler) = *self.on_shutdown.lock().unwrap() {
            handler(reason);
        }
    }
}
