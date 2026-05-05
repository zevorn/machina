// Virt system controller.
//
// MMIO device with two registers:
//   FEATURES (0x00) — read-only, reports supported features
//   CMD      (0x04) — write-only, triggers reset/halt/panic

use std::sync::Mutex;

use machina_memory::region::MmioOps;

// Register offsets
const REG_FEATURES: u64 = 0x00;
const REG_CMD: u64 = 0x04;

// Features
const FEAT_POWER_CTRL: u32 = 0x0000_0001;

// Commands
const CMD_RESET: u32 = 1;
const CMD_HALT: u32 = 2;
const CMD_PANIC: u32 = 3;

pub const VIRT_CTRL_REG_SIZE: u64 = 0x100;

/// Shutdown action requested via virt_ctrl CMD register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtCtrlAction {
    Reset,
    Halt,
    Panic,
}

type ActionHandler = Mutex<Option<Box<dyn Fn(VirtCtrlAction) + Send>>>;

pub struct VirtCtrl {
    on_action: ActionHandler,
}

impl VirtCtrl {
    pub fn new() -> Self {
        Self {
            on_action: Mutex::new(None),
        }
    }

    pub fn set_action_handler(
        &self,
        handler: Box<dyn Fn(VirtCtrlAction) + Send>,
    ) {
        *self.on_action.lock().unwrap() = Some(handler);
    }
}

impl Default for VirtCtrl {
    fn default() -> Self {
        Self::new()
    }
}

impl MmioOps for VirtCtrl {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        match offset {
            REG_FEATURES => u64::from(FEAT_POWER_CTRL),
            _ => 0,
        }
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        if offset == REG_CMD {
            let cmd = val as u32;
            let action = match cmd {
                CMD_RESET => Some(VirtCtrlAction::Reset),
                CMD_HALT => Some(VirtCtrlAction::Halt),
                CMD_PANIC => Some(VirtCtrlAction::Panic),
                _ => None,
            };
            if let Some(action) = action {
                if let Some(ref handler) = *self.on_action.lock().unwrap() {
                    handler(action);
                }
            }
        }
    }
}
