use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::irq::InterruptSource;
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

const IMSIC_MMIO_PAGE_SZ: u64 = 0x1000;
const IMSIC_MMIO_PAGE_LE: u64 = 0x00;

const IMSIC_EIPX_BITS: u32 = 32;
const IMSIC_MIN_ID: u32 = (IMSIC_EIPX_BITS * 2) - 1;
const IMSIC_MAX_ID: u32 = 0x7ff;

const IMSIC_TOPEI_IID_SHIFT: u32 = 16;
const IMSIC_TOPEI_IID_MASK: u32 = 0x7ff;

const IMSIC_EISTATE_PENDING: u32 = 1 << 0;
const IMSIC_EISTATE_ENABLED: u32 = 1 << 1;
const IMSIC_EISTATE_ENPEND: u32 = IMSIC_EISTATE_ENABLED | IMSIC_EISTATE_PENDING;

// ISELECT register offsets for CSR-level RMW operations
const ISELECT_IMSIC_EIDELIVERY: u32 = 0x70;
const ISELECT_IMSIC_EITHRESHOLD: u32 = 0x72;
const ISELECT_IMSIC_TOPEI: u32 = 0x1ff + 1;
const ISELECT_IMSIC_EIP0: u32 = 0x80;
const ISELECT_IMSIC_EIE0: u32 = 0xc0;

// AIA register field extraction
fn aia_ireg_isel(ireg: u64) -> u32 {
    (ireg & 0xffff) as u32
}
fn aia_ireg_priv(ireg: u64) -> u32 {
    ((ireg >> 16) & 0x3) as u32
}
fn aia_ireg_virt(ireg: u64) -> u32 {
    ((ireg >> 18) & 0x1) as u32
}
fn aia_ireg_vgein(ireg: u64) -> u32 {
    ((ireg >> 20) & 0x3f) as u32
}
fn aia_ireg_xlen(ireg: u64) -> u32 {
    ((ireg >> 24) & 0xff) as u32
}

pub struct RiscvImsic {
    state: parking_lot::Mutex<SysBusDeviceState>,
    #[allow(dead_code)]
    mmode: bool,
    #[allow(dead_code)]
    hartid: u32,
    num_pages: u32,
    num_irqs: u32,
    eidelivery: DeviceRefCell<Vec<u32>>,
    eithreshold: DeviceRefCell<Vec<u32>>,
    eistate: DeviceRefCell<Vec<u32>>,
    outputs: parking_lot::Mutex<Vec<Option<InterruptSource>>>,
}

impl RiscvImsic {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("riscv_imsic", false, 0, 1, 64)
    }

    #[must_use]
    pub fn new_named(
        local_id: &str,
        mmode: bool,
        hartid: u32,
        num_pages: u32,
        num_irqs: u32,
    ) -> Self {
        let np = num_pages.max(1);
        let ni = num_irqs.max(IMSIC_MIN_ID + 1);
        let num_eistate = (np * ni) as usize;
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            mmode,
            num_pages: np,
            num_irqs: ni,
            eidelivery: DeviceRefCell::new(vec![0u32; np as usize]),
            eithreshold: DeviceRefCell::new(vec![0u32; np as usize]),
            eistate: DeviceRefCell::new(vec![0u32; num_eistate]),
            outputs: parking_lot::Mutex::new({
                let mut v = Vec::with_capacity(np as usize);
                v.resize_with(np as usize, || None);
                v
            }),
            hartid,
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
        self.state.lock().realize_onto(bus, address_space)
    }

    pub fn unrealize_from(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.lower_outputs();
        self.state.lock().unrealize_from(bus, address_space)
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

    pub fn connect_output(&self, page: u32, irq: InterruptSource) {
        let mut outputs = self.outputs.lock();
        while outputs.len() <= page as usize {
            outputs.push(None);
        }
        outputs[page as usize] = Some(irq);
        drop(outputs);
        self.update_outputs(page);
    }

    pub fn reset_runtime(&self) {
        self.lower_outputs();
        let mut eidelivery = self.eidelivery.borrow();
        for v in eidelivery.iter_mut() {
            *v = 0;
        }
        drop(eidelivery);
        let mut eithreshold = self.eithreshold.borrow();
        for v in eithreshold.iter_mut() {
            *v = 0;
        }
        drop(eithreshold);
        let mut eistate = self.eistate.borrow();
        for v in eistate.iter_mut() {
            *v = 0;
        }
    }

    /// CSR-level RMW operation. Decodes `reg` using AIA IREG fields,
    /// routes to the appropriate internal RMW handler.
    /// Returns 0 on success, -1 on invalid register.
    pub fn rmw(
        &self,
        reg: u64,
        val: &mut u64,
        new_val: u64,
        wr_mask: u64,
    ) -> i32 {
        let priv_ = aia_ireg_priv(reg);
        let virt = aia_ireg_virt(reg);
        let isel = aia_ireg_isel(reg);
        let vgein = aia_ireg_vgein(reg);
        let xlen = aia_ireg_xlen(reg);

        let page = self.resolve_page(priv_, virt, vgein);
        if page >= self.num_pages {
            return -1;
        }

        match isel {
            ISELECT_IMSIC_EIDELIVERY => {
                self.eidelivery_rmw(page, val, new_val, wr_mask)
            }
            ISELECT_IMSIC_EITHRESHOLD => {
                self.eithreshold_rmw(page, val, new_val, wr_mask)
            }
            ISELECT_IMSIC_TOPEI => self.topei_rmw(page, val, new_val, wr_mask),
            ISELECT_IMSIC_EIP0..=0xbf => {
                let num = isel - ISELECT_IMSIC_EIP0;
                self.eix_rmw(xlen, page, num, true, val, new_val, wr_mask)
            }
            ISELECT_IMSIC_EIE0..=0xff => {
                let num = isel - ISELECT_IMSIC_EIE0;
                self.eix_rmw(xlen, page, num, false, val, new_val, wr_mask)
            }
            _ => -1,
        }
    }

    // --- observable state for tests ---

    #[must_use]
    pub fn eidelivery_val(&self, page: u32) -> u32 {
        self.eidelivery
            .borrow()
            .get(page as usize)
            .copied()
            .unwrap_or(0)
    }

    #[must_use]
    pub fn eithreshold_val(&self, page: u32) -> u32 {
        self.eithreshold
            .borrow()
            .get(page as usize)
            .copied()
            .unwrap_or(0)
    }

    #[must_use]
    pub fn eistate_val(&self, idx: u32) -> u32 {
        self.eistate
            .borrow()
            .get(idx as usize)
            .copied()
            .unwrap_or(0)
    }

    // --- private helpers ---

    fn resolve_page(&self, priv_: u32, virt: u32, vgein: u32) -> u32 {
        if self.mmode {
            if priv_ == 3 && virt == 0 {
                return 0;
            }
            return self.num_pages; // invalid
        }
        if priv_ == 1 {
            if virt != 0 {
                if vgein != 0 && vgein < self.num_pages {
                    return vgein;
                }
                return self.num_pages;
            }
            return 0;
        }
        self.num_pages
    }

    fn topei(&self, page: u32) -> u32 {
        let base = page * self.num_irqs;
        let max_irq = {
            let eth = self.eithreshold.borrow();
            let t = eth[page as usize];
            if t != 0 && t <= self.num_irqs {
                t
            } else {
                self.num_irqs
            }
        };
        let eistate = self.eistate.borrow();
        for i in 1..max_irq {
            if (eistate[(base + i) as usize] & IMSIC_EISTATE_ENPEND)
                == IMSIC_EISTATE_ENPEND
            {
                return (i << IMSIC_TOPEI_IID_SHIFT) | i;
            }
        }
        0
    }

    fn update_outputs(&self, page: u32) {
        let base = page * self.num_irqs;
        {
            let mut eistate = self.eistate.borrow();
            eistate[base as usize] &= !IMSIC_EISTATE_ENPEND;
        }
        let outputs = self.outputs.lock();
        if let Some(Some(line)) = outputs.get(page as usize) {
            line.lower();
        }
        let eidelivery = self.eidelivery.borrow();
        if eidelivery[page as usize] & 0x1 != 0 && self.topei(page) != 0 {
            if let Some(Some(line)) = outputs.get(page as usize) {
                line.raise();
            }
            let mut eistate = self.eistate.borrow();
            eistate[base as usize] |= IMSIC_EISTATE_ENPEND;
        }
    }

    fn eidelivery_rmw(
        &self,
        page: u32,
        val: &mut u64,
        new_val: u64,
        wr_mask: u64,
    ) -> i32 {
        let mut eidelivery = self.eidelivery.borrow();
        let old = eidelivery[page as usize] as u64;
        *val = old;
        let mask = wr_mask & 0x1;
        eidelivery[page as usize] = ((old & !mask) | (new_val & mask)) as u32;
        drop(eidelivery);
        self.update_outputs(page);
        0
    }

    fn eithreshold_rmw(
        &self,
        page: u32,
        val: &mut u64,
        new_val: u64,
        wr_mask: u64,
    ) -> i32 {
        let mut eithreshold = self.eithreshold.borrow();
        let old = eithreshold[page as usize] as u64;
        *val = old;
        let mask = wr_mask & IMSIC_MAX_ID as u64;
        eithreshold[page as usize] = ((old & !mask) | (new_val & mask)) as u32;
        drop(eithreshold);
        self.update_outputs(page);
        0
    }

    fn topei_rmw(
        &self,
        page: u32,
        val: &mut u64,
        _new_val: u64,
        wr_mask: u64,
    ) -> i32 {
        let topei = self.topei(page);
        *val = topei as u64;
        if topei != 0 && wr_mask != 0 {
            let iid = (topei >> IMSIC_TOPEI_IID_SHIFT) & IMSIC_TOPEI_IID_MASK;
            if iid != 0 {
                let base = page * self.num_irqs;
                let mut eistate = self.eistate.borrow();
                eistate[(base + iid) as usize] &= !IMSIC_EISTATE_PENDING;
            }
        }
        self.update_outputs(page);
        0
    }

    #[allow(clippy::too_many_arguments)]
    fn eix_rmw(
        &self,
        xlen: u32,
        page: u32,
        num: u32,
        pend: bool,
        val: &mut u64,
        new_val: u64,
        wr_mask: u64,
    ) -> i32 {
        let state_bit = if pend {
            IMSIC_EISTATE_PENDING
        } else {
            IMSIC_EISTATE_ENABLED
        };

        let num = if xlen != 32 {
            if num & 0x1 != 0 {
                return -1;
            }
            num >> 1
        } else {
            num
        };

        if num >= self.num_irqs / xlen {
            return -1;
        }

        let base = (page * self.num_irqs) + (num * xlen);
        let mut eistate = self.eistate.borrow();
        *val = 0;

        for i in 0..xlen {
            if num == 0 && i == 0 {
                continue;
            }
            let mask = 1u64 << i;
            let idx = (base + i) as usize;
            let prev = if wr_mask & mask != 0 {
                let old = eistate[idx];
                if new_val & mask != 0 {
                    eistate[idx] = old | state_bit;
                } else {
                    eistate[idx] = old & !state_bit;
                }
                old
            } else {
                eistate[idx]
            };
            if prev & state_bit != 0 {
                *val |= mask;
            }
        }
        drop(eistate);
        self.update_outputs(page);
        0
    }

    fn lower_outputs(&self) {
        let outputs = self.outputs.lock();
        for line in outputs.iter().flatten() {
            line.lower();
        }
    }
}

impl Default for RiscvImsic {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RiscvImsicMmio(pub Arc<RiscvImsic>);

impl MmioOps for RiscvImsicMmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        if (offset & 0x3) != 0 {
            return 0;
        }
        if offset >= IMSIC_MMIO_PAGE_SZ * self.0.num_pages as u64 {
            return 0;
        }
        0
    }

    fn write(&self, offset: u64, size: u32, value: u64) {
        if size != 4 {
            return;
        }
        if (offset & 0x3) != 0 {
            return;
        }
        if offset >= IMSIC_MMIO_PAGE_SZ * self.0.num_pages as u64 {
            return;
        }

        let page = (offset >> 12) as u32;
        let page_off = offset & (IMSIC_MMIO_PAGE_SZ - 1);

        if page_off == IMSIC_MMIO_PAGE_LE
            && value != 0
            && (value as u32) < self.0.num_irqs
        {
            let base = page * self.0.num_irqs;
            let mut eistate = self.0.eistate.borrow();
            eistate[(base + value as u32) as usize] |= IMSIC_EISTATE_PENDING;
            drop(eistate);
            self.0.update_outputs(page);
        }
    }
}
