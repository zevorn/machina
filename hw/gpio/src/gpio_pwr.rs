// GPIO power controller.
//
// Two named GPIO inputs:
//   "reset"    — when asserted, triggers system reset
//   "shutdown" — when asserted, triggers system shutdown

use std::sync::Arc;

use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::mdev::MDevice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioPwrAction {
    Reset,
    Shutdown,
}

type ActionHandler =
    parking_lot::Mutex<Option<Box<dyn Fn(GpioPwrAction) + Send>>>;

pub struct GpioPwr {
    state: parking_lot::Mutex<SysBusDeviceState>,
    on_action: ActionHandler,
}

impl GpioPwr {
    pub fn new() -> Arc<Self> {
        Self::new_named("gpio_pwr")
    }

    pub fn new_named(local_id: &str) -> Arc<Self> {
        Arc::new(Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            on_action: parking_lot::Mutex::new(None),
        })
    }

    pub fn set_action_handler(
        &self,
        handler: Box<dyn Fn(GpioPwrAction) + Send>,
    ) {
        *self.on_action.lock() = Some(handler);
    }

    /// GPIO reset input: on rising edge, trigger reset.
    pub fn gpio_reset(&self, level: bool) {
        if level {
            if let Some(ref handler) = *self.on_action.lock() {
                handler(GpioPwrAction::Reset);
            }
        }
    }

    /// GPIO shutdown input: on rising edge, trigger shutdown.
    pub fn gpio_shutdown(&self, level: bool) {
        if level {
            if let Some(ref handler) = *self.on_action.lock() {
                handler(GpioPwrAction::Shutdown);
            }
        }
    }

    pub fn attach_to_bus(
        self: &Arc<Self>,
        bus: &mut SysBus,
    ) -> Result<(), SysBusError> {
        self.state.lock().attach_to_bus(bus)
    }

    pub fn realize(
        self: &Arc<Self>,
    ) -> Result<(), machina_hw_core::mdev::MDeviceError> {
        {
            let guard = self.state.lock();
            if guard.device().parent_bus().is_none() {
                return Err(machina_hw_core::mdev::MDeviceError::LateMutation(
                    "must attach to parent bus before realize",
                ));
            }
        }
        self.state.lock().device_mut().mark_realized()
    }

    pub fn unrealize(
        self: &Arc<Self>,
    ) -> Result<(), machina_hw_core::mdev::MDeviceError> {
        self.state.lock().device_mut().mark_unrealized()
    }

    pub fn realized(&self) -> bool {
        self.state.lock().device().is_realized()
    }

    pub fn reset_runtime(&self) {
        // No runtime state to reset.
    }

    pub fn with_mdevice<T>(&self, f: impl FnOnce(&dyn MDevice) -> T) -> T {
        let guard = self.state.lock();
        f(&*guard)
    }

    pub fn object_info(&self) -> MObjectInfo {
        self.state.lock().object_info()
    }
}
