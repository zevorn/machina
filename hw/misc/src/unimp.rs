// "Unimplemented" dummy device.
//
// Accepts all MMIO accesses and returns 0 for reads. Used
// to stub out SoC regions for unimplemented peripherals
// during bring-up. Configurable size and name.

use machina_memory::region::MmioOps;

pub struct Unimp {
    name: String,
    size: u64,
}

impl Unimp {
    pub fn new(name: &str, size: u64) -> Self {
        Self {
            name: name.to_string(),
            size,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}

impl MmioOps for Unimp {
    fn read(&self, _offset: u64, _size: u32) -> u64 {
        0
    }

    fn write(&self, _offset: u64, _size: u32, _val: u64) {}
}
