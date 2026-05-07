use std::sync::Arc;

use machina_core::device_cell::DeviceRegs;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_memory::region::MmioOps;

const PCH_MSI_MAX_IRQ_NUM: u32 = 224;

struct PchMsiRegs {
    irq_base: u32,
    irq_num: u32,
}

impl PchMsiRegs {
    fn validate(&self) -> Result<(), String> {
        if self.irq_num == 0 {
            return Err("pch_msi: irq_num must be > 0".to_string());
        }
        if self.irq_num > PCH_MSI_MAX_IRQ_NUM {
            return Err(format!(
                "pch_msi: irq_num {} exceeds max {}",
                self.irq_num, PCH_MSI_MAX_IRQ_NUM
            ));
        }
        Ok(())
    }
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot", before_unrealize = lower_outputs)]
pub struct PchMsi {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRegs<PchMsiRegs>,
    outputs: parking_lot::Mutex<Vec<Option<InterruptSource>>>,
}

impl PchMsi {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("pch_msi", 0, 0)
    }

    #[must_use]
    pub fn new_named(local_id: &str, irq_base: u32, irq_num: u32) -> Self {
        let irq_count = irq_num as usize;
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRegs::new(PchMsiRegs { irq_base, irq_num }),
            outputs: parking_lot::Mutex::new({
                let mut v = Vec::with_capacity(irq_count);
                v.resize_with(irq_count, || None);
                v
            }),
        }
    }

    pub fn validate_properties(&self) -> Result<(), String> {
        self.regs.borrow().validate()
    }

    pub fn connect_output(&self, irq: u32, line: InterruptSource) {
        let mut outputs = self.outputs.lock();
        if (irq as usize) < outputs.len() {
            outputs[irq as usize] = Some(line);
        }
    }

    pub fn reset_runtime(&self) {
        self.lower_outputs();
    }

    fn lower_outputs(&self) {
        let outputs = self.outputs.lock();
        for line in outputs.iter().flatten() {
            line.lower();
        }
    }
}

impl Default for PchMsi {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PchMsiMmio(pub Arc<PchMsi>);

impl MmioOps for PchMsiMmio {
    fn read(&self, _offset: u64, _size: u32) -> u64 {
        0
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        if size == 8 {
            self.write(offset, 4, val);
            self.write(offset.wrapping_add(4), 4, val >> 32);
            return;
        }

        if !matches!(size, 1 | 2 | 4) {
            return;
        }
        if offset >= 8 {
            return;
        }
        let regs = self.0.regs.borrow();
        let vector = (val & 0xff) as u32;
        if vector < regs.irq_base {
            return;
        }
        let irq = u64::from(vector - regs.irq_base);
        let irq_num = regs.irq_num as u64;
        drop(regs);
        if irq < irq_num {
            let outputs = self.0.outputs.lock();
            if let Some(Some(line)) = outputs.get(irq as usize) {
                line.set(true);
            }
        }
    }
}
