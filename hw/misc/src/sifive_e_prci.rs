// SiFive E PRCI (Power, Reset, Clock, Interrupt).
//
// Simple register model that emulates reads made by SDK BSP code.
// All oscillators report ready and PLL stays locked on write.
//
// DTB compatible: "sifive,e-prci0"

use machina_core::device_cell::DeviceCell;
use machina_memory::region::MmioOps;

// Register offsets
const HFROSCCFG: u64 = 0x00;
const HFXOSCCFG: u64 = 0x04;
const PLLCFG: u64 = 0x08;
const PLLOUTDIV: u64 = 0x0C;

// Bit definitions
const HFROSCCFG_RDY: u32 = 1 << 31;
const HFROSCCFG_EN: u32 = 1 << 30;
const HFXOSCCFG_RDY: u32 = 1 << 31;
const HFXOSCCFG_EN: u32 = 1 << 30;
const PLLCFG_REFSEL: u32 = 1 << 17;
const PLLCFG_BYPASS: u32 = 1 << 18;
const PLLCFG_LOCK: u32 = 1 << 31;
const PLLOUTDIV_DIV1: u32 = 1 << 8;

pub const SIFIVE_E_PRCI_REG_SIZE: u64 = 0x1000;

pub struct SifiveEPRCI {
    hfrosccfg: DeviceCell<u32>,
    hfxosccfg: DeviceCell<u32>,
    pllcfg: DeviceCell<u32>,
    plloutdiv: DeviceCell<u32>,
}

impl SifiveEPRCI {
    pub fn new() -> Self {
        Self {
            hfrosccfg: DeviceCell::new(HFROSCCFG_RDY | HFROSCCFG_EN),
            hfxosccfg: DeviceCell::new(HFXOSCCFG_RDY | HFXOSCCFG_EN),
            pllcfg: DeviceCell::new(
                PLLCFG_REFSEL | PLLCFG_BYPASS | PLLCFG_LOCK,
            ),
            plloutdiv: DeviceCell::new(PLLOUTDIV_DIV1),
        }
    }
}

impl Default for SifiveEPRCI {
    fn default() -> Self {
        Self::new()
    }
}

impl MmioOps for SifiveEPRCI {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        match offset {
            HFROSCCFG => u64::from(self.hfrosccfg.get()),
            HFXOSCCFG => u64::from(self.hfxosccfg.get()),
            PLLCFG => u64::from(self.pllcfg.get()),
            PLLOUTDIV => u64::from(self.plloutdiv.get()),
            _ => 0,
        }
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        let val32 = val as u32;
        match offset {
            HFROSCCFG => {
                self.hfrosccfg.set(val32 | HFROSCCFG_RDY);
            }
            HFXOSCCFG => {
                self.hfxosccfg.set(val32 | HFXOSCCFG_RDY);
            }
            PLLCFG => {
                self.pllcfg.set(val32 | PLLCFG_LOCK);
            }
            PLLOUTDIV => {
                self.plloutdiv.set(val32);
            }
            _ => {}
        }
    }
}
