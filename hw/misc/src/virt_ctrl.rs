// Virt system controller.
//
// MMIO device with two registers:
//   FEATURES (0x00) — read-only, reports supported features
//   CMD      (0x04) — write-only, triggers reset/halt/panic

use std::sync::Arc;

use machina_hw_core::bus::SysBusDeviceState;
use machina_memory::region::MmioOps;

const REG_FEATURES: u64 = 0x00;
const REG_CMD: u64 = 0x04;

const FEAT_POWER_CTRL: u32 = 0x0000_0001;

const CMD_RESET: u32 = 1;
const CMD_HALT: u32 = 2;
const CMD_PANIC: u32 = 3;

pub const VIRT_CTRL_REG_SIZE: u64 = 0x100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtCtrlAction {
    Reset,
    Halt,
    Panic,
}

type ActionHandler =
    parking_lot::Mutex<Option<Box<dyn Fn(VirtCtrlAction) + Send>>>;

pub struct VirtCtrl {
    state: parking_lot::Mutex<SysBusDeviceState>,
    on_action: ActionHandler,
}

impl VirtCtrl {
    pub fn new() -> Arc<Self> {
        Self::new_named("virt_ctrl")
    }

    pub fn new_named(local_id: &str) -> Arc<Self> {
        Arc::new(Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            on_action: parking_lot::Mutex::new(None),
        })
    }

    pub fn set_action_handler(
        &self,
        handler: Box<dyn Fn(VirtCtrlAction) + Send>,
    ) {
        *self.on_action.lock() = Some(handler);
    }

    machina_hw_core::machina_parking_lot_sysbus_accessors!(state);

    pub fn reset_runtime(&self) {
        // No runtime state to reset.
    }

    pub fn do_read(&self, offset: u64, size: u32) -> u64 {
        if size > 4 {
            return 0;
        }
        match offset {
            REG_FEATURES => u64::from(FEAT_POWER_CTRL),
            _ => 0,
        }
    }

    pub fn do_write(&self, offset: u64, size: u32, val: u64) {
        if size > 4 {
            return;
        }
        if offset == REG_CMD {
            let cmd = val as u32;
            let action = match cmd {
                CMD_RESET => Some(VirtCtrlAction::Reset),
                CMD_HALT => Some(VirtCtrlAction::Halt),
                CMD_PANIC => Some(VirtCtrlAction::Panic),
                _ => None,
            };
            if let Some(action) = action {
                if let Some(ref handler) = *self.on_action.lock() {
                    handler(action);
                }
            }
        }
    }
}

pub struct VirtCtrlMmio(pub Arc<VirtCtrl>);

impl MmioOps for VirtCtrlMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.do_read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.do_write(offset, size, val);
    }
}
