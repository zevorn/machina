use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

const GCR_MAX_VPS: usize = 256;

const CM_RESET_VEC: u64 = 0x1FC0_0000;

const CLCB_OFS: u64 = 0x2000;
const CORE_REG_STRIDE: u64 = 0x100;

const GCR_CONFIG_OFS: u64 = 0x0000;
const GCR_BASE_OFS: u64 = 0x0008;
const GCR_REV_OFS: u64 = 0x0030;
const GCR_CPC_STATUS_OFS: u64 = 0x00F0;
const GCR_L2_CONFIG_OFS: u64 = 0x0130;

const GCR_L2_CONFIG_BYPASS_MSK: u64 = 1 << 20;
const GCR_BASE_GCRBASE_MSK: u64 = 0xFFFF_FFFF_8000;
const GCR_CPC_BASE_CPCEN_MSK: u64 = 1;
const GCR_CPC_BASE_CPCBASE_MSK: u64 = 0xFFFF_FFFF_8000;
const GCR_CPC_BASE_MSK: u64 = GCR_CPC_BASE_CPCEN_MSK | GCR_CPC_BASE_CPCBASE_MSK;
const GCR_CL_RESET_BASE_RESETBASE_MSK: u64 = 0xFFFF_FFFF_FFFF_F000;
const GCR_CL_RESET_BASE_MSK: u64 = GCR_CL_RESET_BASE_RESETBASE_MSK;

#[allow(dead_code)]
struct CmgcrRegs {
    gcr_rev: i32,
    cluster_id: u32,
    num_vps: u32,
    num_hart: u32,
    num_core: u32,
    gcr_base: u64,
    cpc_base: u64,
    has_cpc: bool,
    reset_base: [u64; GCR_MAX_VPS],
}

impl CmgcrRegs {
    fn new(
        gcr_rev: i32,
        cluster_id: u32,
        num_vps: u32,
        num_hart: u32,
        num_core: u32,
        gcr_base: u64,
    ) -> Self {
        let mut reset_base =
            [CM_RESET_VEC & GCR_CL_RESET_BASE_RESETBASE_MSK; GCR_MAX_VPS];
        for entry in &mut reset_base {
            *entry = CM_RESET_VEC & GCR_CL_RESET_BASE_RESETBASE_MSK;
        }
        Self {
            gcr_rev,
            cluster_id,
            num_vps,
            num_hart,
            num_core,
            gcr_base,
            cpc_base: 0,
            has_cpc: false,
            reset_base,
        }
    }
}

pub struct Cmgcr {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<CmgcrRegs>,
}

impl Cmgcr {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("cmgcr", 0xa00, 0, 1, 1, 1, 0x1FB8_0000)
    }

    #[must_use]
    pub fn new_named(
        local_id: &str,
        gcr_rev: i32,
        cluster_id: u32,
        num_vps: u32,
        num_hart: u32,
        num_core: u32,
        gcr_base: u64,
    ) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRefCell::new(CmgcrRegs::new(
                gcr_rev,
                cluster_id,
                num_vps.max(1),
                num_hart.max(1),
                num_core.max(1),
                gcr_base,
            )),
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

    pub fn set_cpc_connected(&self, connected: bool) {
        self.regs.borrow().has_cpc = connected;
    }

    pub fn reset_runtime(&self) {
        let mut regs = self.regs.borrow();
        regs.cpc_base = (regs.gcr_base + 0x8001) & GCR_CPC_BASE_MSK;
        for entry in &mut regs.reset_base {
            *entry = CM_RESET_VEC & GCR_CL_RESET_BASE_RESETBASE_MSK;
        }
    }
}

impl Default for Cmgcr {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CmgcrMmio(pub Arc<Cmgcr>);

impl MmioOps for CmgcrMmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        let regs = self.0.regs.borrow();
        match offset {
            GCR_CONFIG_OFS => 0,
            GCR_BASE_OFS => regs.gcr_base,
            GCR_REV_OFS => regs.gcr_rev as u64,
            GCR_CPC_STATUS_OFS => u64::from(regs.has_cpc),
            GCR_L2_CONFIG_OFS => GCR_L2_CONFIG_BYPASS_MSK,
            _ => 0,
        }
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        let mut regs = self.0.regs.borrow();

        for c in 0..regs.num_core as u64 {
            for h in 0..regs.num_hart as u64 {
                let addr = CLCB_OFS + c * CORE_REG_STRIDE + h * 8;
                if offset == addr {
                    let cpu_index = (c * regs.num_hart as u64 + h) as usize;
                    if cpu_index < GCR_MAX_VPS {
                        regs.reset_base[cpu_index] =
                            val & GCR_CL_RESET_BASE_MSK;
                    }
                    return;
                }
            }
        }

        match offset {
            GCR_BASE_OFS => {
                regs.gcr_base = val & GCR_BASE_GCRBASE_MSK;
                let new_cpc = (regs.gcr_base + 0x8001) & GCR_CPC_BASE_MSK;
                regs.cpc_base = new_cpc;
            }
            _ => {}
        }
    }
}
