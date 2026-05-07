// "Unimplemented" dummy device.
//
// Accepts all MMIO accesses and returns 0 for reads. Used
// to stub out SoC regions for unimplemented peripherals
// during bring-up. Configurable size and name.

use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::mdev::{MDevice, MDeviceError};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

pub struct Unimp {
    state: parking_lot::Mutex<SysBusDeviceState>,
    name: String,
    size: u64,
}

impl Unimp {
    pub fn new(name: &str, size: u64) -> Arc<Self> {
        Arc::new(Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(name)),
            name: name.to_string(),
            size,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn attach_to_bus(
        self: &Arc<Self>,
        bus: &mut SysBus,
    ) -> Result<(), SysBusError> {
        self.state.lock().attach_to_bus(bus)
    }

    pub fn register_mmio(
        self: &Arc<Self>,
        region: MemoryRegion,
        base: GPA,
    ) -> Result<(), SysBusError> {
        if region.name != self.name {
            return Err(SysBusError::Device(MDeviceError::LateMutation(
                "unimp region name must match configured name",
            )));
        }
        if region.size != self.size {
            return Err(SysBusError::Device(MDeviceError::LateMutation(
                "unimp region size must match configured size",
            )));
        }
        self.state.lock().register_mmio(region, base)
    }

    pub fn realize_onto(
        self: &Arc<Self>,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        if self.size == 0 {
            return Err(SysBusError::Device(MDeviceError::LateMutation(
                "unimp size must be non-zero",
            )));
        }
        self.state.lock().realize_onto(bus, address_space)
    }

    pub fn unrealize_from(
        self: &Arc<Self>,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.state.lock().unrealize_from(bus, address_space)
    }

    pub fn realized(&self) -> bool {
        self.state.lock().device().is_realized()
    }

    pub fn reset_runtime(&self) {
        // No runtime state to reset.
    }

    pub fn with_mdevice<T>(&self, f: impl FnOnce(&dyn MDevice) -> T) -> T {
        let guard = self.state.lock();
        f(&*guard)
    }

    pub fn object_info(&self) -> MObjectInfo {
        self.state.lock().object_info()
    }

    pub fn do_read(&self, _offset: u64, _size: u32) -> u64 {
        0
    }

    pub fn do_write(&self, _offset: u64, _size: u32, _val: u64) {}
}

pub struct UnimpMmio(pub Arc<Unimp>);

impl MmioOps for UnimpMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.do_read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.do_write(offset, size, val);
    }
}
