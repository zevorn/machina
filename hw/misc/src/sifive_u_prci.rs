// SiFive U PRCI (Power, Reset, Clock, Interrupt).
//
// Simple register model for the FU540-C000 PRCI. All PLLs stay
// locked on write with internal feedback enabled.
//
// DTB compatible: "sifive,fu540-c000-prci"

use machina_core::device_cell::DeviceCell;
use machina_memory::region::MmioOps;

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

// Default PLLCFG0 value on reset
const PLLCFG0_DEFAULT: u32 =
    PLLCFG0_DIVR | PLLCFG0_DIVF | PLLCFG0_DIVQ | PLLCFG0_FSE | PLLCFG0_LOCK;

pub const SIFIVE_U_PRCI_REG_SIZE: u64 = 0x1000;

pub struct SifiveUPRCI {
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
    pub fn new() -> Self {
        Self {
            hfxosccfg: DeviceCell::new(HFXOSCCFG_RDY | HFXOSCCFG_EN),
            corepllcfg0: DeviceCell::new(PLLCFG0_DEFAULT),
            ddrpllcfg0: DeviceCell::new(PLLCFG0_DEFAULT),
            ddrpllcfg1: DeviceCell::new(0),
            gemgxlpllcfg0: DeviceCell::new(PLLCFG0_DEFAULT),
            gemgxlpllcfg1: DeviceCell::new(0),
            coreclksel: DeviceCell::new(CORECLKSEL_HFCLK),
            devicesreset: DeviceCell::new(0),
            clkmuxstatus: DeviceCell::new(0),
        }
    }
}

impl Default for SifiveUPRCI {
    fn default() -> Self {
        Self::new()
    }
}

impl MmioOps for SifiveUPRCI {
    fn read(&self, offset: u64, _size: u32) -> u64 {
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

    fn write(&self, offset: u64, _size: u32, val: u64) {
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
