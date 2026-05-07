// Panic device (paravirtualized).
//
// Reports guest panic/crash/shutdown events through MMIO.
// Read returns the supported events mask; write dispatches
// the first recognized event by priority:
//   PANICKED > CRASH_LOADED > SHUTDOWN
//
// The MMIO variant (PvpanicMmio) wraps the core Pvpanic
// device and maps it into a MMIO region.

use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

/// Bitmask of supported/recognized events.
pub mod event {
    pub const PANICKED: u8 = 1 << 0;
    pub const CRASH_LOADED: u8 = 1 << 1;
    pub const SHUTDOWN: u8 = 1 << 2;
}

pub use event as PvpanicEvent;

pub const PVPANIC_MMIO_SIZE: u64 = 0x2;

type EventHandler = parking_lot::Mutex<Option<Box<dyn Fn(u8) + Send>>>;

/// Core pvpanic device — handles event dispatch and lifecycle.
pub struct Pvpanic {
    state: parking_lot::Mutex<SysBusDeviceState>,
    events: u8,
    on_event: EventHandler,
}

impl Pvpanic {
    pub fn new(events: u8) -> Arc<Self> {
        Self::new_named("pvpanic", events)
    }

    pub fn new_named(local_id: &str, events: u8) -> Arc<Self> {
        Arc::new(Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            events,
            on_event: parking_lot::Mutex::new(None),
        })
    }

    pub fn set_event_handler(&self, handler: Box<dyn Fn(u8) + Send>) {
        *self.on_event.lock() = Some(handler);
    }

    pub fn attach_to_bus(
        self: &Arc<Self>,
        bus: &mut SysBus,
    ) -> Result<(), SysBusError> {
        self.state.lock().attach_to_bus(bus)
    }

    pub fn register_mmio(
        self: &Arc<Self>,
        region: MemoryRegion,
        base: GPA,
    ) -> Result<(), SysBusError> {
        self.state.lock().register_mmio(region, base)
    }

    pub fn realize_onto(
        self: &Arc<Self>,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.state.lock().realize_onto(bus, address_space)
    }

    pub fn unrealize_from(
        self: &Arc<Self>,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.state.lock().unrealize_from(bus, address_space)
    }

    pub fn realized(&self) -> bool {
        self.state.lock().device().is_realized()
    }

    pub fn reset_runtime(&self) {
        // Runtime reset: no mutable runtime state to clear.
    }

    pub fn with_mdevice<T>(&self, f: impl FnOnce(&dyn MDevice) -> T) -> T {
        let guard = self.state.lock();
        f(&*guard)
    }

    pub fn object_info(&self) -> MObjectInfo {
        self.state.lock().object_info()
    }

    fn do_read(&self, _offset: u64, size: u32) -> u64 {
        let event_byte = u64::from(self.events);
        match size {
            1 => event_byte,
            2 => event_byte | (event_byte << 8),
            4 => {
                event_byte
                    | (event_byte << 8)
                    | (event_byte << 16)
                    | (event_byte << 24)
            }
            _ => 0,
        }
    }

    fn dispatch_event(&self, event: u8) {
        if let Some(ref handler) = *self.on_event.lock() {
            if event & event::PANICKED != 0 {
                handler(event::PANICKED);
            } else if event & event::CRASH_LOADED != 0 {
                handler(event::CRASH_LOADED);
            } else if event & event::SHUTDOWN != 0 {
                handler(event::SHUTDOWN);
            }
        }
    }

    fn do_write(&self, _offset: u64, size: u32, val: u64) {
        match size {
            1 | 2 | 4 => {
                for byte in 0..size {
                    let event = ((val >> (byte * 8)) & 0xFF) as u8;
                    self.dispatch_event(event);
                }
            }
            _ => {}
        }
    }
}

pub struct PvpanicMmio(pub Arc<Pvpanic>);

impl MmioOps for PvpanicMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.do_read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.do_write(offset, size, val);
    }
}
