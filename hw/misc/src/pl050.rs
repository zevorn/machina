use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::irq::InterruptSource;
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

const PL050_ID: [u8; 8] =
    [0x50, 0x10, 0x04, 0x00, 0x0d, 0xf0, 0x05, 0xb1];

const PL050_TXEMPTY: u32 = 1 << 6;
const PL050_TXBUSY: u32 = 1 << 5;
const PL050_RXFULL: u32 = 1 << 4;
const PL050_RXBUSY: u32 = 1 << 3;
const PL050_RXPARITY: u32 = 1 << 2;
const PL050_KMIC: u32 = 1 << 1;
const PL050_KMID: u32 = 1 << 0;

struct Pl050Regs {
    cr: u32,
    clk: u32,
    last: u32,
    pending: i32,
}

impl Pl050Regs {
    fn new() -> Self {
        Self {
            cr: 0,
            clk: 0,
            last: 0,
            pending: 0,
        }
    }

    fn reset(&mut self) {
        self.cr = 0;
        self.clk = 0;
        self.last = 0;
        self.pending = 0;
    }
}

pub struct Pl050 {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<Pl050Regs>,
    irq: parking_lot::Mutex<Option<InterruptSource>>,
}

impl Pl050 {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(
                "pl050",
            )),
            regs: DeviceRefCell::new(Pl050Regs::new()),
            irq: parking_lot::Mutex::new(None),
        }
    }

    pub fn connect_irq(&self, irq: InterruptSource) {
        *self.irq.lock() = Some(irq);
    }

    /// GPIO input: PS2 device IRQ signal.
    pub fn set_ps2_irq(&self, level: bool) {
        let mut regs = self.regs.borrow();
        regs.pending = i32::from(level);
        let irq_pending =
            (regs.pending != 0 && (regs.cr & 0x10) != 0)
                || (regs.cr & 0x08) != 0;
        drop(regs);
        if let Some(ref line) = *self.irq.lock() {
            line.set(irq_pending);
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
        Ok(())
    }

    pub fn unrealize_from(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.lower_irq();
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

    pub fn reset_runtime(&self) {
        self.regs.borrow().reset();
        self.lower_irq();
    }

    fn lower_irq(&self) {
        if let Some(ref line) = *self.irq.lock() {
            line.lower();
        }
    }
}

impl Default for Pl050 {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Pl050Mmio(pub Arc<Pl050>);

impl MmioOps for Pl050Mmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        if offset >= 0xFE0 && offset < 0x1000 {
            let idx = ((offset - 0xFE0) >> 2) as usize;
            if idx < PL050_ID.len() {
                return u64::from(PL050_ID[idx]);
            }
            return 0;
        }

        let regs = self.0.regs.borrow();
        match offset >> 2 {
            0 => u64::from(regs.cr),
            1 => {
                let val = regs.last;
                let parity =
                    ((val ^ (val >> 4)) ^ ((val ^ (val >> 4)) >> 2))
                        ^ (((val ^ (val >> 4)) ^ ((val ^ (val >> 4)) >> 2))
                            >> 1)
                        & 1;
                let mut stat = PL050_TXEMPTY;
                if parity != 0 {
                    stat |= PL050_RXPARITY;
                }
                if regs.pending != 0 {
                    stat |= PL050_RXFULL;
                }
                u64::from(stat)
            }
            2 => {
                // Data read returns last value (PS2 reads not modeled)
                u64::from(regs.last)
            }
            3 => u64::from(regs.clk),
            4 => u64::from((regs.pending | 2) as u32),
            _ => 0,
        }
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        let value = val as u32;
        let mut regs = self.0.regs.borrow();

        match offset >> 2 {
            0 => {
                regs.cr = value;
                let irq = (regs.pending != 0 && (regs.cr & 0x10) != 0)
                    || (regs.cr & 0x08) != 0;
                drop(regs);
                if let Some(ref line) = *self.0.irq.lock() {
                    line.set(irq);
                }
                return;
            }
            2 => {
                // PS2 keyboard/mouse write — data captured, PS2
                // device not modeled, just store
                regs.last = value;
            }
            3 => {
                regs.clk = value;
                return;
            }
            _ => {}
        }
    }
}
