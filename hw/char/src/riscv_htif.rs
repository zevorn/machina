use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::chardev::CharFrontend;
use machina_hw_core::mdev::{MDevice, MDeviceError};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

const HTIF_DEV_SHIFT: u64 = 56;
const HTIF_CMD_SHIFT: u64 = 48;

const HTIF_DEV_SYSTEM: u8 = 0;
const HTIF_DEV_CONSOLE: u8 = 1;

const HTIF_SYSTEM_CMD_SYSCALL: u8 = 0;
const HTIF_CONSOLE_CMD_GETC: u8 = 0;
const HTIF_CONSOLE_CMD_PUTC: u8 = 1;

#[allow(dead_code)]
const PK_SYS_WRITE: u64 = 64;

const TOHOST_OFFSET: u64 = 0;
const FROMHOST_OFFSET: u64 = 8;

struct HtifRegs {
    tohost: u64,
    fromhost: u64,
    allow_tohost: bool,
    fromhost_inprogress: bool,
    pending_read: u64,
}

impl HtifRegs {
    fn new() -> Self {
        Self {
            tohost: 0,
            fromhost: 0,
            allow_tohost: false,
            fromhost_inprogress: false,
            pending_read: 0,
        }
    }

    fn reset(&mut self) {
        self.tohost = 0;
        self.fromhost = 0;
        self.allow_tohost = false;
        self.fromhost_inprogress = false;
        self.pending_read = 0;
    }
}

pub type ExitCallback = Box<dyn Fn(i32) + Send + Sync>;

pub struct Htif {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<HtifRegs>,
    chardev: DeviceRefCell<Option<CharFrontend>>,
    configured_chardev: parking_lot::Mutex<Option<CharFrontend>>,
    exit_cb: parking_lot::Mutex<Option<ExitCallback>>,
}

impl Htif {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("riscv_htif")
    }

    #[must_use]
    pub fn new_named(local_id: &str) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRefCell::new(HtifRegs::new()),
            chardev: DeviceRefCell::new(None),
            configured_chardev: parking_lot::Mutex::new(None),
            exit_cb: parking_lot::Mutex::new(None),
        }
    }

    pub fn attach_to_bus(&self, bus: &mut SysBus) -> Result<(), SysBusError> {
        self.state.lock().attach_to_bus(bus)
    }

    pub fn register_mmio(
        &self,
        region: MemoryRegion,
        base: GPA,
    ) -> Result<(), SysBusError> {
        self.state.lock().register_mmio(region, base)
    }

    pub fn realize_onto(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.state.lock().realize_onto(bus, address_space)?;
        if let Some(fe) = self.configured_chardev.lock().take() {
            *self.chardev.borrow() = Some(fe);
        }
        Ok(())
    }

    pub fn unrealize_from(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.chardev.borrow().take();
        self.state.lock().unrealize_from(bus, address_space)?;
        Ok(())
    }

    #[must_use]
    pub fn realized(&self) -> bool {
        self.state.lock().device().is_realized()
    }

    #[must_use]
    pub fn object_info(&self) -> MObjectInfo {
        self.state.lock().object_info()
    }

    pub fn with_mdevice<T>(&self, f: impl FnOnce(&dyn MDevice) -> T) -> T {
        let guard = self.state.lock();
        f(&*guard)
    }

    pub fn attach_chardev(&self, fe: CharFrontend) -> Result<(), MDeviceError> {
        if self.state.lock().device().is_realized() {
            return Err(MDeviceError::LateMutation("chardev"));
        }
        *self.configured_chardev.lock() = Some(fe);
        Ok(())
    }

    pub fn set_exit_callback(&self, cb: ExitCallback) {
        *self.exit_cb.lock() = Some(cb);
    }

    pub fn reset_runtime(&self) {
        self.regs.borrow().reset();
    }

    /// Receive a byte from chardev (for GETC response).
    pub fn receive(&self, byte: u8) {
        let mut regs = self.regs.borrow();
        let val_written = regs.pending_read;
        let resp = 0x100u64 | u64::from(byte);
        regs.fromhost = (val_written & 0xFFFF_0000_0000_0000) | (resp & 0xFFFF);
        regs.pending_read = 0;
    }

    fn handle_tohost_write(&self, val_written: u64) {
        let device = (val_written >> HTIF_DEV_SHIFT) as u8;
        let cmd = (val_written >> HTIF_CMD_SHIFT) as u8;
        let payload = val_written & 0x0000_FFFF_FFFF_FFFF;
        let mut resp: u64 = 0;

        if device == HTIF_DEV_SYSTEM {
            if cmd == HTIF_SYSTEM_CMD_SYSCALL && payload & 0x1 != 0 {
                // Exit code
                let exit_code = (payload >> 1) as i32;
                if let Some(ref cb) = *self.exit_cb.lock() {
                    cb(exit_code);
                }
                return;
            }
            if cmd == HTIF_SYSTEM_CMD_SYSCALL {
                // Syscall: PK_SYS_WRITE to console is supported.
                // We don't do DMA; just ignore unsupported syscalls.
            }
        } else if device == HTIF_DEV_CONSOLE {
            if cmd == HTIF_CONSOLE_CMD_GETC {
                let mut regs = self.regs.borrow();
                regs.pending_read = val_written;
                regs.tohost = 0;
                // fromhost response will be set when receive() is called.
                return;
            } else if cmd == HTIF_CONSOLE_CMD_PUTC {
                let ch = payload as u8;
                if let Some(ref mut fe) = *self.chardev.borrow() {
                    fe.write(&[ch]);
                }
                resp = 0x100 | u64::from(ch);
            }
        }

        let mut regs = self.regs.borrow();
        regs.fromhost = (val_written & 0xFFFF_0000_0000_0000) | (resp & 0xFFFF);
        regs.tohost = 0;
    }
}

impl Default for Htif {
    fn default() -> Self {
        Self::new()
    }
}

pub struct HtifMmio(pub Arc<Htif>);

impl MmioOps for HtifMmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        let regs = self.0.regs.borrow();
        if offset == TOHOST_OFFSET {
            regs.tohost & 0xFFFF_FFFF
        } else if offset == TOHOST_OFFSET + 4 {
            (regs.tohost >> 32) & 0xFFFF_FFFF
        } else if offset == FROMHOST_OFFSET {
            regs.fromhost & 0xFFFF_FFFF
        } else if offset == FROMHOST_OFFSET + 4 {
            (regs.fromhost >> 32) & 0xFFFF_FFFF
        } else {
            0
        }
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        let value = val as u32;
        if offset == TOHOST_OFFSET {
            let mut regs = self.0.regs.borrow();
            if regs.tohost == 0 {
                regs.allow_tohost = true;
                regs.tohost = u64::from(value);
            } else {
                regs.allow_tohost = false;
            }
        } else if offset == TOHOST_OFFSET + 4 {
            let mut regs = self.0.regs.borrow();
            if regs.allow_tohost {
                regs.tohost |= u64::from(value) << 32;
                let tohost = regs.tohost;
                drop(regs);
                self.0.handle_tohost_write(tohost);
            }
        } else if offset == FROMHOST_OFFSET {
            let mut regs = self.0.regs.borrow();
            regs.fromhost_inprogress = true;
            regs.fromhost = u64::from(value);
        } else if offset == FROMHOST_OFFSET + 4 {
            let mut regs = self.0.regs.borrow();
            regs.fromhost |= u64::from(value) << 32;
            regs.fromhost_inprogress = false;
        }
    }
}
