use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

pub type CpcMtimeCb = Box<dyn Fn() -> u64 + Send + Sync>;
pub type CpcVpActionCb = Box<dyn Fn(u64, bool) + Send + Sync>;

const CPC_MTIME_REG_OFS: u64 = 0x50;
const CPC_CM_STAT_CONF_OFS: u64 = 0x1008;
const CPC_CL_BASE_OFS: u64 = 0x2000;
const CPC_CORE_REG_STRIDE: u64 = 0x100;

const CPC_STAT_CONF_OFS: u64 = 0x08;
const CPC_VP_STOP_OFS: u64 = 0x20;
const CPC_VP_RUN_OFS: u64 = 0x28;

const SEQ_STATE_BIT: u64 = 19;
const SEQ_STATE_U5: u64 = 0x6;
const SEQ_STATE_U6: u64 = 0x7;
#[allow(non_upper_case_globals)]
const CPC_Cx_STAT_CONF_SEQ_STATE_U5: u64 = SEQ_STATE_U5 << SEQ_STATE_BIT;
#[allow(non_upper_case_globals)]
const CPC_Cx_STAT_CONF_SEQ_STATE_U6: u64 = SEQ_STATE_U6 << SEQ_STATE_BIT;

struct CpcRegs {
    cluster_id: u32,
    num_vp: u32,
    num_hart: u32,
    num_core: u32,
    vps_start_running_mask: u64,
    vps_running_mask: u64,
}

impl CpcRegs {
    fn new(
        cluster_id: u32,
        num_vp: u32,
        num_hart: u32,
        num_core: u32,
        vps_start_running_mask: u64,
    ) -> Self {
        Self {
            cluster_id,
            num_vp,
            num_hart,
            num_core,
            vps_start_running_mask,
            vps_running_mask: 0,
        }
    }
}

pub struct Cpc {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<CpcRegs>,
    mtime_cb: Mutex<Option<CpcMtimeCb>>,
    vp_action_cb: Mutex<Option<CpcVpActionCb>>,
}

impl Cpc {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("cpc", 0, 1, 1, 1, 1)
    }

    #[must_use]
    pub fn new_named(
        local_id: &str,
        cluster_id: u32,
        num_vp: u32,
        num_hart: u32,
        num_core: u32,
        vps_start_running_mask: u64,
    ) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRefCell::new(CpcRegs::new(
                cluster_id,
                num_vp,
                num_hart,
                num_core,
                vps_start_running_mask,
            )),
            mtime_cb: Mutex::new(None),
            vp_action_cb: Mutex::new(None),
        }
    }

    pub fn validate_properties(&self) -> Result<(), String> {
        let regs = self.regs.borrow();
        let max_mask = if regs.num_vp >= 64 {
            u64::MAX
        } else {
            (1u64 << regs.num_vp) - 1
        };
        if regs.vps_start_running_mask & !max_mask != 0 {
            return Err(format!(
                "cpc: vps_start_running_mask 0x{:x} exceeds num_vp {}",
                regs.vps_start_running_mask, regs.num_vp
            ));
        }
        Ok(())
    }

    pub fn set_mtime_cb(&self, cb: CpcMtimeCb) {
        *self.mtime_cb.lock().unwrap() = Some(cb);
    }

    pub fn set_vp_action_cb(&self, cb: CpcVpActionCb) {
        *self.vp_action_cb.lock().unwrap() = Some(cb);
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

    pub fn reset_runtime(&self) {
        let mut regs = self.regs.borrow();
        regs.vps_running_mask = 0;
        regs.vps_running_mask |= regs.vps_start_running_mask;
    }
}

impl Default for Cpc {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CpcMmio(pub Arc<Cpc>);

impl MmioOps for CpcMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        let regs = self.0.regs.borrow();

        let mut value = 0;
        for c in 0..regs.num_core as u64 {
            let addr =
                CPC_CL_BASE_OFS + CPC_STAT_CONF_OFS + c * CPC_CORE_REG_STRIDE;
            if offset == addr {
                value = CPC_Cx_STAT_CONF_SEQ_STATE_U6;
                break;
            }
        }

        if value == 0 {
            value = match offset {
                CPC_CM_STAT_CONF_OFS => CPC_Cx_STAT_CONF_SEQ_STATE_U5,
                CPC_MTIME_REG_OFS => {
                    let cb = self.0.mtime_cb.lock().unwrap();
                    match *cb {
                        Some(ref f) => f(),
                        None => 0,
                    }
                }
                _ => 0,
            };
        }

        match size {
            1 => value & 0xff,
            2 => value & 0xffff,
            4 => value & 0xffff_ffff,
            _ => value,
        }
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        let value = match size {
            1 => val & 0xff,
            2 => val & 0xffff,
            4 => val & 0xffff_ffff,
            _ => val,
        };
        let mut regs = self.0.regs.borrow();
        let vp_run_mask = if regs.num_vp >= 64 {
            u64::MAX
        } else {
            (1u64 << regs.num_vp) - 1
        };

        for c in 0..regs.num_core as u64 {
            let cpu_index = c * regs.num_hart as u64
                + regs.cluster_id as u64
                    * regs.num_core as u64
                    * regs.num_hart as u64;
            if offset
                == CPC_CL_BASE_OFS + CPC_VP_RUN_OFS + c * CPC_CORE_REG_STRIDE
            {
                let mask = (value << cpu_index) & vp_run_mask;
                regs.vps_running_mask |= mask;
                let action_cb = self.0.vp_action_cb.lock().unwrap();
                if let Some(ref cb) = *action_cb {
                    for bit in 0..64 {
                        if mask & (1u64 << bit) != 0 {
                            cb(bit, true);
                        }
                    }
                }
                return;
            }
            if offset
                == CPC_CL_BASE_OFS + CPC_VP_STOP_OFS + c * CPC_CORE_REG_STRIDE
            {
                let mask = (value << cpu_index) & vp_run_mask;
                regs.vps_running_mask &= !mask;
                let action_cb = self.0.vp_action_cb.lock().unwrap();
                if let Some(ref cb) = *action_cb {
                    for bit in 0..64 {
                        if mask & (1u64 << bit) != 0 {
                            cb(bit, false);
                        }
                    }
                }
                return;
            }
        }
    }
}
