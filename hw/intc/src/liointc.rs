use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

const NUM_IRQS: usize = 32;
const NUM_CORES: usize = 4;
const NUM_IPS: usize = 4;
const NUM_PARENTS: usize = NUM_CORES * NUM_IPS;

const R_MAPPER_END: u64 = 0x20;
const R_ISR: u64 = 0x20;
const R_IEN: u64 = 0x24;
const R_IEN_SET: u64 = 0x28;
const R_IEN_CLR: u64 = 0x2c;
const R_PER_CORE_ISR: u64 = 0x40;
const R_ISR_SIZE: u64 = 0x8;
const R_END: u64 = 0x60;

struct LiointcRegs {
    mapper: [u8; NUM_IRQS],
    isr: u32,
    ien: u32,
    per_core_isr: [u32; NUM_CORES],
    pin_state: u32,
    parent_state: [bool; NUM_PARENTS],
}

impl LiointcRegs {
    fn new() -> Self {
        Self {
            mapper: [0; NUM_IRQS],
            isr: 0,
            ien: 0,
            per_core_isr: [0; NUM_CORES],
            pin_state: 0,
            parent_state: [false; NUM_PARENTS],
        }
    }
}

fn parent_index(core: usize, ip: usize) -> usize {
    NUM_IPS * core + ip
}

pub struct Liointc {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<LiointcRegs>,
    outputs: parking_lot::Mutex<Vec<Option<InterruptSource>>>,
}

impl Liointc {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("liointc")
    }

    #[must_use]
    pub fn new_named(local_id: &str) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRefCell::new(LiointcRegs::new()),
            outputs: parking_lot::Mutex::new({
                let mut v = Vec::with_capacity(NUM_PARENTS);
                v.resize_with(NUM_PARENTS, || None);
                v
            }),
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

    pub fn connect_output(&self, parent_idx: u32, irq: InterruptSource) {
        let mut outputs = self.outputs.lock();
        if (parent_idx as usize) < outputs.len() {
            outputs[parent_idx as usize] = Some(irq);
        }
        drop(outputs);
        self.update_outputs();
    }

    pub fn connect_output_xy(&self, core: u32, ip: u32, irq: InterruptSource) {
        let idx = parent_index(core as usize, ip as usize) as u32;
        self.connect_output(idx, irq);
    }

    pub fn set_irq(&self, irq: u32, level: bool) {
        if irq >= NUM_IRQS as u32 {
            return;
        }
        {
            let mut regs = self.regs.borrow();
            if level {
                regs.pin_state |= 1u32 << irq;
            } else {
                regs.pin_state &= !(1u32 << irq);
            }
        }
        self.update_outputs();
    }

    pub fn reset_runtime(&self) {
        {
            let mut regs = self.regs.borrow();
            regs.isr = 0;
            regs.ien = 0;
            regs.per_core_isr = [0; NUM_CORES];
            regs.pin_state = 0;
            regs.parent_state = [false; NUM_PARENTS];
        }
        self.lower_outputs();
    }

    fn lower_outputs(&self) {
        let outputs = self.outputs.lock();
        for line in outputs.iter().flatten() {
            line.lower();
        }
    }

    fn update_outputs(&self) {
        let mut regs = self.regs.borrow();
        let mut per_ip_isr = [0u32; NUM_IPS];

        regs.isr = regs.pin_state & regs.ien;

        for core in 0..NUM_CORES {
            regs.per_core_isr[core] = 0;
        }

        for irq in 0..NUM_IRQS {
            if regs.isr & (1 << irq) == 0 {
                continue;
            }
            for core in 0..NUM_CORES {
                if regs.mapper[irq] & (1 << core) != 0 {
                    regs.per_core_isr[core] |= 1 << irq;
                }
            }
            for (ip, item) in per_ip_isr.iter_mut().enumerate().take(NUM_IPS) {
                if regs.mapper[irq] & (1 << (ip + 4)) != 0 {
                    *item |= 1 << irq;
                }
            }
        }

        let outputs = self.outputs.lock();
        for core in 0..NUM_CORES {
            for (ip, &ip_isr) in per_ip_isr.iter().enumerate().take(NUM_IPS) {
                let parent = parent_index(core, ip);
                let new_state = regs.per_core_isr[core] != 0 && ip_isr != 0;
                if regs.parent_state[parent] != new_state {
                    regs.parent_state[parent] = new_state;
                    if let Some(Some(line)) = outputs.get(parent) {
                        line.set(new_state);
                    }
                }
            }
        }
    }
}

impl Default for Liointc {
    fn default() -> Self {
        Self::new()
    }
}

pub struct LiointcMmio(pub Arc<Liointc>);

impl MmioOps for LiointcMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        let regs = self.0.regs.borrow();

        if size == 1 && offset < R_MAPPER_END {
            return u64::from(regs.mapper[offset as usize]);
        }

        if size != 4 || !offset.is_multiple_of(4) {
            return 0;
        }

        if (R_PER_CORE_ISR..R_END).contains(&offset) {
            let rel = offset - R_PER_CORE_ISR;
            if !rel.is_multiple_of(R_ISR_SIZE) {
                return 0;
            }
            let core = rel / R_ISR_SIZE;
            return u64::from(regs.per_core_isr[core as usize]);
        }

        match offset {
            R_ISR => u64::from(regs.isr),
            R_IEN => u64::from(regs.ien),
            _ => 0,
        }
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        let value = val as u32;

        if size == 1 && offset < R_MAPPER_END {
            self.0.regs.borrow().mapper[offset as usize] = value as u8;
            self.0.update_outputs();
            return;
        }

        if size != 4 || !offset.is_multiple_of(4) {
            return;
        }

        if (R_PER_CORE_ISR..R_END).contains(&offset) {
            let rel = offset - R_PER_CORE_ISR;
            if !rel.is_multiple_of(R_ISR_SIZE) {
                return;
            }
            // Reference recomputes per-core ISR from pin/enable/mapper.
            // Guest writes to per-core ISR are ignored; the value is
            // derived state.
            return;
        }

        {
            let mut regs = self.0.regs.borrow();
            match offset {
                R_IEN_SET => regs.ien |= value,
                R_IEN_CLR => regs.ien &= !value,
                _ => {}
            }
        }

        self.0.update_outputs();
    }
}

pub struct LiointcIrqSink(pub Arc<Liointc>);

impl IrqSink for LiointcIrqSink {
    fn set_irq(&self, irq: u32, level: bool) {
        self.0.set_irq(irq, level);
    }
}
