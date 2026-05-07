// SiFive U PRCI (Power, Reset, Clock, Interrupt).
//
// Simple register model for the FU540-C000 PRCI. All PLLs stay
// locked on write with internal feedback enabled.
//
// DTB compatible: "sifive,fu540-c000-prci"

use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

// Register offsets
const HFXOSCCFG: u64 = 0x00;
const COREPLLCFG0: u64 = 0x04;
const DDRPLLCFG0: u64 = 0x0C;
const DDRPLLCFG1: u64 = 0x10;
const GEMGXLPLLCFG0: u64 = 0x1C;
const GEMGXLPLLCFG1: u64 = 0x20;
const CORECLKSEL: u64 = 0x24;
const DEVICESRESET: u64 = 0x28;
const CLKMUXSTATUS: u64 = 0x2C;

// Bit definitions
const HFXOSCCFG_EN: u32 = 1 << 30;
const HFXOSCCFG_RDY: u32 = 1 << 31;

const PLLCFG0_DIVR: u32 = 1 << 0;
const PLLCFG0_DIVF: u32 = 31 << 6;
const PLLCFG0_DIVQ: u32 = 3 << 15;
const PLLCFG0_FSE: u32 = 1 << 25;
const PLLCFG0_LOCK: u32 = 1 << 31;

const CORECLKSEL_HFCLK: u32 = 1 << 0;

const PLLCFG0_DEFAULT: u32 =
    PLLCFG0_DIVR | PLLCFG0_DIVF | PLLCFG0_DIVQ | PLLCFG0_FSE | PLLCFG0_LOCK;

pub const SIFIVE_U_PRCI_REG_SIZE: u64 = 0x1000;

pub struct SifiveUPRCI {
    state: parking_lot::Mutex<SysBusDeviceState>,
    hfxosccfg: DeviceCell<u32>,
    corepllcfg0: DeviceCell<u32>,
    ddrpllcfg0: DeviceCell<u32>,
    ddrpllcfg1: DeviceCell<u32>,
    gemgxlpllcfg0: DeviceCell<u32>,
    gemgxlpllcfg1: DeviceCell<u32>,
    coreclksel: DeviceCell<u32>,
    devicesreset: DeviceCell<u32>,
    clkmuxstatus: DeviceCell<u32>,
}

impl SifiveUPRCI {
    pub fn new() -> Arc<Self> {
        Self::new_named("sifive_u_prci")
    }

    pub fn new_named(local_id: &str) -> Arc<Self> {
        Arc::new(Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            hfxosccfg: DeviceCell::new(HFXOSCCFG_RDY | HFXOSCCFG_EN),
            corepllcfg0: DeviceCell::new(PLLCFG0_DEFAULT),
            ddrpllcfg0: DeviceCell::new(PLLCFG0_DEFAULT),
            ddrpllcfg1: DeviceCell::new(0),
            gemgxlpllcfg0: DeviceCell::new(PLLCFG0_DEFAULT),
            gemgxlpllcfg1: DeviceCell::new(0),
            coreclksel: DeviceCell::new(CORECLKSEL_HFCLK),
            devicesreset: DeviceCell::new(0),
            clkmuxstatus: DeviceCell::new(0),
        })
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
        // Reference reset only assigns these five registers;
        // ddrpllcfg1, gemgxlpllcfg1, devicesreset, and
        // clkmuxstatus are left untouched.
        self.hfxosccfg.set(HFXOSCCFG_RDY | HFXOSCCFG_EN);
        self.corepllcfg0.set(PLLCFG0_DEFAULT);
        self.ddrpllcfg0.set(PLLCFG0_DEFAULT);
        self.gemgxlpllcfg0.set(PLLCFG0_DEFAULT);
        self.coreclksel.set(CORECLKSEL_HFCLK);
    }

    pub fn with_mdevice<T>(&self, f: impl FnOnce(&dyn MDevice) -> T) -> T {
        let guard = self.state.lock();
        f(&*guard)
    }

    pub fn object_info(&self) -> MObjectInfo {
        self.state.lock().object_info()
    }

    pub fn do_read(&self, offset: u64, size: u32) -> u64 {
        if size != 4 {
            return 0;
        }
        match offset {
            HFXOSCCFG => u64::from(self.hfxosccfg.get()),
            COREPLLCFG0 => u64::from(self.corepllcfg0.get()),
            DDRPLLCFG0 => u64::from(self.ddrpllcfg0.get()),
            DDRPLLCFG1 => u64::from(self.ddrpllcfg1.get()),
            GEMGXLPLLCFG0 => u64::from(self.gemgxlpllcfg0.get()),
            GEMGXLPLLCFG1 => u64::from(self.gemgxlpllcfg1.get()),
            CORECLKSEL => u64::from(self.coreclksel.get()),
            DEVICESRESET => u64::from(self.devicesreset.get()),
            CLKMUXSTATUS => u64::from(self.clkmuxstatus.get()),
            _ => 0,
        }
    }

    pub fn do_write(&self, offset: u64, size: u32, val: u64) {
        if size != 4 {
            return;
        }
        let val32 = val as u32;
        match offset {
            HFXOSCCFG => {
                self.hfxosccfg.set(val32 | HFXOSCCFG_RDY);
            }
            COREPLLCFG0 => {
                self.corepllcfg0.set(val32 | PLLCFG0_FSE | PLLCFG0_LOCK);
            }
            DDRPLLCFG0 => {
                self.ddrpllcfg0.set(val32 | PLLCFG0_FSE | PLLCFG0_LOCK);
            }
            DDRPLLCFG1 => {
                self.ddrpllcfg1.set(val32);
            }
            GEMGXLPLLCFG0 => {
                self.gemgxlpllcfg0.set(val32 | PLLCFG0_FSE | PLLCFG0_LOCK);
            }
            GEMGXLPLLCFG1 => {
                self.gemgxlpllcfg1.set(val32);
            }
            CORECLKSEL => {
                self.coreclksel.set(val32);
            }
            DEVICESRESET => {
                self.devicesreset.set(val32);
            }
            CLKMUXSTATUS => {
                self.clkmuxstatus.set(val32);
            }
            _ => {}
        }
    }
}

pub struct SifiveUPRCIMmio(pub Arc<SifiveUPRCI>);

impl MmioOps for SifiveUPRCIMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.do_read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.do_write(offset, size, val);
    }
}
