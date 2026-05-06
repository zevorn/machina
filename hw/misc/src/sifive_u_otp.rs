use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

// Register offsets
const SIFIVE_U_OTP_PA: u64 = 0x00;
const SIFIVE_U_OTP_PAIO: u64 = 0x04;
const SIFIVE_U_OTP_PAS: u64 = 0x08;
const SIFIVE_U_OTP_PCE: u64 = 0x0C;
const SIFIVE_U_OTP_PCLK: u64 = 0x10;
const SIFIVE_U_OTP_PDIN: u64 = 0x14;
const SIFIVE_U_OTP_PDOUT: u64 = 0x18;
const SIFIVE_U_OTP_PDSTB: u64 = 0x1C;
const SIFIVE_U_OTP_PPROG: u64 = 0x20;
const SIFIVE_U_OTP_PTC: u64 = 0x24;
const SIFIVE_U_OTP_PTM: u64 = 0x28;
const SIFIVE_U_OTP_PTM_REP: u64 = 0x2C;
const SIFIVE_U_OTP_PTR: u64 = 0x30;
const SIFIVE_U_OTP_PTRIM: u64 = 0x34;
const SIFIVE_U_OTP_PWE: u64 = 0x38;

const PWE_EN: u32 = 1 << 0;
const PCE_EN: u32 = 1 << 0;
const PDSTB_EN: u32 = 1 << 0;
const PTRIM_EN: u32 = 1 << 0;

const PA_MASK: usize = 0xFFF;
const NUM_FUSES: usize = 0x1000;
const SERIAL_ADDR: usize = 0xFC;
const WRITTEN_BIT_ON: u32 = 0x1;

struct SiFiveUOtpRegs {
    pa: u32,
    paio: u32,
    pas: u32,
    pce: u32,
    pclk: u32,
    pdin: u32,
    pdstb: u32,
    pprog: u32,
    ptc: u32,
    ptm: u32,
    ptm_rep: u32,
    ptr: u32,
    ptrim: u32,
    pwe: u32,
    fuse: [u32; NUM_FUSES],
    fuse_wo: [u32; NUM_FUSES],
}

impl SiFiveUOtpRegs {
    fn new(serial: u32) -> Self {
        let mut regs = Self {
            pa: 0,
            paio: 0,
            pas: 0,
            pce: 0,
            pclk: 0,
            pdin: 0,
            pdstb: 0,
            pprog: 0,
            ptc: 0,
            ptm: 0,
            ptm_rep: 0,
            ptr: 0,
            ptrim: 0,
            pwe: 0,
            fuse: [0xFFFF_FFFF; NUM_FUSES],
            fuse_wo: [0; NUM_FUSES],
        };
        regs.fuse[SERIAL_ADDR] = serial;
        regs.fuse[SERIAL_ADDR + 1] = !serial;
        regs
    }

    #[expect(dead_code)]
    fn get_fusearray_bit(&self, idx: usize, offset: u32) -> u32 {
        (self.fuse[idx] >> offset) & 0x1
    }

    fn set_fusearray_bit(&mut self, idx: usize, offset: u32, bit: u32) {
        if bit != 0 {
            self.fuse[idx] |= 1u32 << offset;
        } else {
            self.fuse[idx] &= !(1u32 << offset);
        }
    }

    fn get_wo_bit(&self, idx: usize, offset: u32) -> u32 {
        (self.fuse_wo[idx] >> offset) & 0x1
    }

    fn set_wo_bit(&mut self, idx: usize, offset: u32, bit: u32) {
        if bit != 0 {
            self.fuse_wo[idx] |= 1u32 << offset;
        } else {
            self.fuse_wo[idx] &= !(1u32 << offset);
        }
    }
}

pub struct SiFiveUOtp {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<SiFiveUOtpRegs>,
}

impl SiFiveUOtp {
    #[must_use]
    pub fn new() -> Self {
        Self::with_serial(0)
    }

    #[must_use]
    pub fn with_serial(serial: u32) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(
                "sifive_u_otp",
            )),
            regs: DeviceRefCell::new(SiFiveUOtpRegs::new(serial)),
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
        // OTP has no runtime reset state
    }
}

impl Default for SiFiveUOtp {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SiFiveUOtpMmio(pub Arc<SiFiveUOtp>);

impl MmioOps for SiFiveUOtpMmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        let regs = self.0.regs.borrow();
        match offset {
            SIFIVE_U_OTP_PA => u64::from(regs.pa),
            SIFIVE_U_OTP_PAIO => u64::from(regs.paio),
            SIFIVE_U_OTP_PAS => u64::from(regs.pas),
            SIFIVE_U_OTP_PCE => u64::from(regs.pce),
            SIFIVE_U_OTP_PCLK => u64::from(regs.pclk),
            SIFIVE_U_OTP_PDIN => u64::from(regs.pdin),
            SIFIVE_U_OTP_PDOUT => {
                if (regs.pce & PCE_EN) != 0
                    && (regs.pdstb & PDSTB_EN) != 0
                    && (regs.ptrim & PTRIM_EN) != 0
                {
                    let idx = regs.pa as usize & PA_MASK;
                    u64::from(regs.fuse[idx])
                } else {
                    0xFF
                }
            }
            SIFIVE_U_OTP_PDSTB => u64::from(regs.pdstb),
            SIFIVE_U_OTP_PPROG => u64::from(regs.pprog),
            SIFIVE_U_OTP_PTC => u64::from(regs.ptc),
            SIFIVE_U_OTP_PTM => u64::from(regs.ptm),
            SIFIVE_U_OTP_PTM_REP => u64::from(regs.ptm_rep),
            SIFIVE_U_OTP_PTR => u64::from(regs.ptr),
            SIFIVE_U_OTP_PTRIM => u64::from(regs.ptrim),
            SIFIVE_U_OTP_PWE => u64::from(regs.pwe),
            _ => 0,
        }
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        let value = val as u32;
        let mut regs = self.0.regs.borrow();

        match offset {
            SIFIVE_U_OTP_PA => {
                regs.pa = value & PA_MASK as u32;
            }
            SIFIVE_U_OTP_PAIO => {
                regs.paio = value;
            }
            SIFIVE_U_OTP_PAS => {
                regs.pas = value;
            }
            SIFIVE_U_OTP_PCE => {
                regs.pce = value;
            }
            SIFIVE_U_OTP_PCLK => {
                regs.pclk = value;
            }
            SIFIVE_U_OTP_PDIN => {
                regs.pdin = value;
            }
            SIFIVE_U_OTP_PDOUT => {
                // read-only
            }
            SIFIVE_U_OTP_PDSTB => {
                regs.pdstb = value;
            }
            SIFIVE_U_OTP_PPROG => {
                regs.pprog = value;
            }
            SIFIVE_U_OTP_PTC => {
                regs.ptc = value;
            }
            SIFIVE_U_OTP_PTM => {
                regs.ptm = value;
            }
            SIFIVE_U_OTP_PTM_REP => {
                regs.ptm_rep = value;
            }
            SIFIVE_U_OTP_PTR => {
                regs.ptr = value;
            }
            SIFIVE_U_OTP_PTRIM => {
                regs.ptrim = value;
            }
            SIFIVE_U_OTP_PWE => {
                regs.pwe = value & PWE_EN;
                if regs.pwe != 0 && regs.pas == 0 {
                    let idx = regs.pa as usize & PA_MASK;
                    let bit = regs.paio;
                    let din = regs.pdin;
                    // Check write-once
                    if regs.get_wo_bit(idx, bit) != 0 {
                        // Already written, ignore
                        drop(regs);
                        return;
                    }
                    regs.set_fusearray_bit(idx, bit, din);
                    regs.set_wo_bit(idx, bit, WRITTEN_BIT_ON);
                }
            }
            _ => {}
        }
    }
}
