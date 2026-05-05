// Panic device (paravirtualized).
//
// Reports guest panic/crash/shutdown events through MMIO.
// Read returns the supported events mask; write triggers
// event handling.
//
// The MMIO variant (PvpanicMmio) wraps the core Pvpanic
// device and maps it into a MMIO region.

use std::sync::Mutex;

use machina_memory::region::MmioOps;

/// Bitmask of supported/recognized events.
pub mod event {
    pub const PANICKED: u8 = 1 << 0;
    pub const CRASH_LOADED: u8 = 1 << 1;
    pub const SHUTDOWN: u8 = 1 << 2;
}

pub use event as PvpanicEvent;

pub const PVPANIC_MMIO_SIZE: u64 = 0x2;

type EventHandler = Mutex<Option<Box<dyn Fn(u8) + Send>>>;

/// Core pvpanic device — handles event dispatch.
pub struct Pvpanic {
    events: u8,
    on_event: EventHandler,
}

impl Pvpanic {
    pub fn new(events: u8) -> Self {
        Self {
            events,
            on_event: Mutex::new(None),
        }
    }

    pub fn set_event_handler(&self, handler: Box<dyn Fn(u8) + Send>) {
        *self.on_event.lock().unwrap() = Some(handler);
    }

    fn handle_event(&self, event: u8) {
        let valid = event & self.events;
        if let Some(ref handler) = *self.on_event.lock().unwrap() {
            if valid != 0 {
                handler(valid);
            }
        }
    }
}

impl MmioOps for Pvpanic {
    fn read(&self, _offset: u64, _size: u32) -> u64 {
        u64::from(self.events)
    }

    fn write(&self, _offset: u64, _size: u32, val: u64) {
        self.handle_event(val as u8);
    }
}

/// MMIO front-end for the pvpanic device.
pub struct PvpanicMmio {
    pvpanic: Pvpanic,
}

impl PvpanicMmio {
    pub fn new(events: u8) -> Self {
        Self {
            pvpanic: Pvpanic::new(events),
        }
    }

    pub fn set_event_handler(&self, handler: Box<dyn Fn(u8) + Send>) {
        self.pvpanic.set_event_handler(handler);
    }
}

impl MmioOps for PvpanicMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.pvpanic.read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.pvpanic.write(offset, size, val);
    }
}
