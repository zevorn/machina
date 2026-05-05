use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::irq::InterruptSource;
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

const VIRT_DINTC_BASE: u64 = 0x2FE0_0000;

pub struct Dintc {
    state: parking_lot::Mutex<SysBusDeviceState>,
    #[allow(dead_code)]
    num_cpus: u32,
    outputs: parking_lot::Mutex<Vec<Option<InterruptSource>>>,
    pending_vectors: parking_lot::Mutex<Vec<u64>>,
}

impl Dintc {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("dintc", 1)
    }

    #[must_use]
    pub fn new_named(local_id: &str, num_cpus: u32) -> Self {
        let count = num_cpus.max(1) as usize;
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            num_cpus: num_cpus.max(1),
            outputs: parking_lot::Mutex::new({
                let mut v = Vec::with_capacity(count);
                v.resize_with(count, || None);
                v
            }),
            pending_vectors: parking_lot::Mutex::new(vec![0u64; count]),
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

    pub fn connect_output(&self, cpu_id: u32, irq: InterruptSource) {
        let mut outputs = self.outputs.lock();
        while outputs.len() <= cpu_id as usize {
            outputs.push(None);
        }
        outputs[cpu_id as usize] = Some(irq);
    }

    // Observable per-CPU pending vector. Returns the pending bitmap for a
    // given CPU so tests can verify which IRQ vectors were delivered.
    #[must_use]
    pub fn pending_vector(&self, cpu_id: u32) -> u64 {
        self.pending_vectors
            .lock()
            .get(cpu_id as usize)
            .copied()
            .unwrap_or(0)
    }

    pub fn reset_runtime(&self) {
        self.lower_outputs();
        let mut pending = self.pending_vectors.lock();
        for v in pending.iter_mut() {
            *v = 0;
        }
    }

    fn lower_outputs(&self) {
        let outputs = self.outputs.lock();
        for line in outputs.iter().flatten() {
            line.lower();
        }
    }
}

impl Default for Dintc {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DintcMmio(pub Arc<Dintc>);

impl MmioOps for DintcMmio {
    fn read(&self, _offset: u64, _size: u32) -> u64 {
        0
    }

    fn write(&self, offset: u64, _size: u32, _val: u64) {
        let msg_addr = offset + VIRT_DINTC_BASE;
        let cpu_num = ((msg_addr >> 12) & 0xff) as u32;
        let irq_num = ((msg_addr >> 4) & 0xff) as u32;
        // Set the pending vector bit for this CPU.
        {
            let mut pending = self.0.pending_vectors.lock();
            if (cpu_num as usize) < pending.len() {
                pending[cpu_num as usize] |= 1u64 << irq_num;
            }
        }
        let outputs = self.0.outputs.lock();
        if let Some(Some(line)) = outputs.get(cpu_num as usize) {
            line.set(true);
        }
    }
}
