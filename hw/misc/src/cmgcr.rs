use std::sync::{Arc, Mutex};

use machina_core::device_cell::DeviceRegs;
use machina_hw_core::bus::SysBusDeviceState;
use machina_memory::region::MmioOps;

pub type CpuResetBaseCb = Box<dyn Fn(usize, u64) + Send + Sync>;

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
        reset_base.fill(CM_RESET_VEC & GCR_CL_RESET_BASE_RESETBASE_MSK);
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

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot")]
pub struct Cmgcr {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRegs<CmgcrRegs>,
    vp_reset_base_cb: Mutex<Option<CpuResetBaseCb>>,
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
        num_vps_val: u32,
        num_hart_val: u32,
        num_core_val: u32,
        gcr_base: u64,
    ) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRegs::new(CmgcrRegs::new(
                gcr_rev,
                cluster_id,
                num_vps_val,
                num_hart_val,
                num_core_val,
                gcr_base,
            )),
            vp_reset_base_cb: Mutex::new(None),
        }
    }

    pub fn validate_properties(&self) -> Result<(), String> {
        let regs = self.regs.borrow();
        if regs.num_vps == 0 {
            return Err("cmgcr: num_vps must be > 0".to_string());
        }
        if regs.num_vps as usize > GCR_MAX_VPS {
            return Err(format!(
                "cmgcr: num_vps {} exceeds max {}",
                regs.num_vps, GCR_MAX_VPS
            ));
        }
        Ok(())
    }

    pub fn set_vp_reset_base_cb(&self, cb: CpuResetBaseCb) {
        *self.vp_reset_base_cb.lock().unwrap() = Some(cb);
    }

    pub fn set_cpc_connected(&self, connected: bool) {
        self.regs.borrow().has_cpc = connected;
    }

    pub fn reset_runtime(&self) {
        let mut regs = self.regs.borrow();
        regs.cpc_base = (regs.gcr_base + 0x8001) & GCR_CPC_BASE_MSK;
        let num_vps = regs.num_vps as usize;
        let cb = self.vp_reset_base_cb.lock().unwrap();
        let val = CM_RESET_VEC & GCR_CL_RESET_BASE_RESETBASE_MSK;
        for vp in 0..num_vps {
            regs.reset_base[vp] = val;
            if let Some(ref cb) = *cb {
                cb(vp, val);
            }
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
    fn read(&self, offset: u64, size: u32) -> u64 {
        let regs = self.0.regs.borrow();
        let value = match offset {
            GCR_CONFIG_OFS => 0,
            GCR_BASE_OFS => regs.gcr_base,
            GCR_REV_OFS => regs.gcr_rev as u64,
            GCR_CPC_STATUS_OFS => u64::from(regs.has_cpc),
            GCR_L2_CONFIG_OFS => GCR_L2_CONFIG_BYPASS_MSK,
            _ => 0,
        };

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

        for c in 0..regs.num_core as u64 {
            for h in 0..regs.num_hart as u64 {
                let addr = CLCB_OFS + c * CORE_REG_STRIDE + h * 8;
                if offset == addr {
                    let cpu_index = (c * regs.num_hart as u64 + h) as usize;
                    if cpu_index < GCR_MAX_VPS {
                        let masked = value & GCR_CL_RESET_BASE_MSK;
                        regs.reset_base[cpu_index] = masked;
                        let cb = self.0.vp_reset_base_cb.lock().unwrap();
                        if let Some(ref cb) = *cb {
                            cb(cpu_index, masked);
                        }
                    }
                    return;
                }
            }
        }

        if offset == GCR_BASE_OFS {
            regs.gcr_base = value & GCR_BASE_GCRBASE_MSK;
            let new_cpc = (regs.gcr_base + 0x8001) & GCR_CPC_BASE_MSK;
            regs.cpc_base = new_cpc;
        }
    }
}
