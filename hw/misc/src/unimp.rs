// "Unimplemented" dummy device.
//
// Accepts all MMIO accesses and returns 0 for reads. Used
// to stub out SoC regions for unimplemented peripherals
// during bring-up. Configurable size and name.

use std::sync::Arc;

use machina_hw_core::bus::{SysBusDeviceState, SysBusError};
use machina_hw_core::mdev::MDeviceError;
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

    machina_hw_core::machina_parking_lot_sysbus_accessors!(
        state,
        before_register_mmio = validate_mmio_region,
        before_realize = validate_realize
    );

    fn validate_mmio_region(
        &self,
        region: &MemoryRegion,
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
        Ok(())
    }

    fn validate_realize(&self) -> Result<(), SysBusError> {
        if self.size == 0 {
            return Err(SysBusError::Device(MDeviceError::LateMutation(
                "unimp size must be non-zero",
            )));
        }
        Ok(())
    }

    pub fn reset_runtime(&self) {
        // No runtime state to reset.
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
