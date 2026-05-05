// GPIO power controller.
//
// Two named GPIO inputs:
//   "reset"    — when asserted, triggers system reset
//   "shutdown" — when asserted, triggers system shutdown

use std::sync::Mutex;

/// Action requested via GPIO power control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioPwrAction {
    Reset,
    Shutdown,
}

type ActionHandler = Mutex<Option<Box<dyn Fn(GpioPwrAction) + Send>>>;

pub struct GpioPwr {
    on_action: ActionHandler,
}

impl GpioPwr {
    pub fn new() -> Self {
        Self {
            on_action: Mutex::new(None),
        }
    }

    pub fn set_action_handler(
        &self,
        handler: Box<dyn Fn(GpioPwrAction) + Send>,
    ) {
        *self.on_action.lock().unwrap() = Some(handler);
    }

    /// GPIO reset input: on rising edge, trigger reset.
    pub fn gpio_reset(&self, level: bool) {
        if level {
            if let Some(ref handler) = *self.on_action.lock().unwrap() {
                handler(GpioPwrAction::Reset);
            }
        }
    }

    /// GPIO shutdown input: on rising edge, trigger shutdown.
    pub fn gpio_shutdown(&self, level: bool) {
        if level {
            if let Some(ref handler) = *self.on_action.lock().unwrap() {
                handler(GpioPwrAction::Shutdown);
            }
        }
    }
}

impl Default for GpioPwr {
    fn default() -> Self {
        Self::new()
    }
}
