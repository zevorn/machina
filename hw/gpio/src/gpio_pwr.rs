// GPIO power controller.
//
// Two named GPIO inputs:
//   "reset"    — when asserted, triggers system reset
//   "shutdown" — when asserted, triggers system shutdown

use std::sync::Arc;

use machina_hw_core::bus::SysBusDeviceState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioPwrAction {
    Reset,
    Shutdown,
}

type ActionHandler =
    parking_lot::Mutex<Option<Box<dyn Fn(GpioPwrAction) + Send>>>;

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot_child")]
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

    pub fn reset_runtime(&self) {
        // No runtime state to reset.
    }
}
