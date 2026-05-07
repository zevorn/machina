use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::irq::InterruptSource;
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

const PL050_ID: [u8; 8] = [0x50, 0x10, 0x04, 0x00, 0x0d, 0xf0, 0x05, 0xb1];

const PL050_TXEMPTY: u32 = 1 << 6;
const PL050_RXFULL: u32 = 1 << 4;
const PL050_RXPARITY: u32 = 1 << 2;
const PS2_RESEND: u32 = 0xfe;

fn access_mask(size: u32) -> u64 {
    match size {
        1 => 0xff,
        2 => 0xffff,
        4 => 0xffff_ffff,
        _ => u64::MAX,
    }
}

struct Pl050Regs {
    cr: u32,
    clk: u32,
    last: u32,
    pending: i32,
    queued_response: Option<u32>,
}

impl Pl050Regs {
    fn new() -> Self {
        Self {
            cr: 0,
            clk: 0,
            last: 0,
            pending: 0,
            queued_response: None,
        }
    }

    fn reset(&mut self) {
        self.cr = 0;
        self.clk = 0;
        self.last = 0;
        self.pending = 0;
        self.queued_response = None;
    }
}

pub struct Pl050 {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<Pl050Regs>,
    irq: parking_lot::Mutex<Option<InterruptSource>>,
}

impl Pl050 {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new("pl050")),
            regs: DeviceRefCell::new(Pl050Regs::new()),
            irq: parking_lot::Mutex::new(None),
        }
    }

    pub fn connect_irq(&self, irq: InterruptSource) {
        *self.irq.lock() = Some(irq);
    }

    /// GPIO input: PS2 device IRQ signal.
    pub fn set_ps2_irq(&self, level: bool) {
        let mut regs = self.regs.borrow();
        regs.pending = i32::from(level);
        let irq_pending = Self::irq_level(regs.pending, regs.cr);
        drop(regs);
        if let Some(ref line) = *self.irq.lock() {
            line.set(irq_pending);
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
        self.state.lock().realize_onto(bus, address_space)?;
        Ok(())
    }

    pub fn unrealize_from(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.lower_irq();
        self.state.lock().unrealize_from(bus, address_space)?;
        Ok(())
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
        self.regs.borrow().reset();
        self.lower_irq();
    }

    fn lower_irq(&self) {
        if let Some(ref line) = *self.irq.lock() {
            line.lower();
        }
    }

    fn irq_level(pending: i32, cr: u32) -> bool {
        (pending != 0 && (cr & 0x10) != 0) || (cr & 0x08) != 0
    }

    fn update_irq(&self, pending: i32, cr: u32) {
        if let Some(ref line) = *self.irq.lock() {
            line.set(Self::irq_level(pending, cr));
        }
    }
}

impl Default for Pl050 {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Pl050Mmio(pub Arc<Pl050>);

impl MmioOps for Pl050Mmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        if let Some(value) = read_unaligned(self, offset, size) {
            return value;
        }

        if size == 8 {
            let lo = self.read(offset, 4);
            let hi = self.read(offset.wrapping_add(4), 4);
            return lo | (hi << 32);
        }

        if (0xFE0..0x1000).contains(&offset) {
            let idx = ((offset - 0xFE0) >> 2) as usize;
            if idx < PL050_ID.len() {
                return u64::from(PL050_ID[idx]) & access_mask(size);
            }
            return 0;
        }

        let mut irq_update = None;
        let result = {
            let mut regs = self.0.regs.borrow();
            match offset >> 2 {
                0 => u64::from(regs.cr),
                1 => {
                    let mut val = regs.last;
                    val ^= val >> 4;
                    val ^= val >> 2;
                    let parity = (val ^ (val >> 1)) & 1;
                    let mut stat = PL050_TXEMPTY;
                    if parity != 0 {
                        stat |= PL050_RXPARITY;
                    }
                    if regs.pending != 0 {
                        stat |= PL050_RXFULL;
                    }
                    u64::from(stat)
                }
                2 => {
                    if regs.pending != 0 {
                        if let Some(response) = regs.queued_response.take() {
                            regs.last = response;
                        }
                        regs.pending = 0;
                        irq_update = Some((regs.pending, regs.cr));
                    }
                    u64::from(regs.last)
                }
                3 => u64::from(regs.clk),
                4 => u64::from((regs.pending | 2) as u32),
                _ => 0,
            }
        };
        if let Some((pending, cr)) = irq_update {
            self.0.update_irq(pending, cr);
        }
        result & access_mask(size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        if write_unaligned(self, offset, size, val) {
            return;
        }

        if size == 8 {
            self.write(offset, 4, val);
            self.write(offset.wrapping_add(4), 4, val >> 32);
            return;
        }

        let value = (val & access_mask(size)) as u32;
        let mut irq_update = None;

        {
            let mut regs = self.0.regs.borrow();
            match offset >> 2 {
                0 => {
                    regs.cr = value;
                    irq_update = Some((regs.pending, regs.cr));
                }
                2 => {
                    regs.queued_response = Some(PS2_RESEND);
                    regs.pending = 1;
                    irq_update = Some((regs.pending, regs.cr));
                }
                3 => {
                    regs.clk = value;
                }
                _ => {}
            }
        }

        if let Some((pending, cr)) = irq_update {
            self.0.update_irq(pending, cr);
        }
    }
}

fn read_unaligned(mmio: &Pl050Mmio, offset: u64, size: u32) -> Option<u64> {
    if !needs_unaligned_split(offset, size) {
        return None;
    }

    let mut value = 0u64;
    let mut done = 0u32;
    while done < size {
        let cur = offset + u64::from(done);
        let chunk = aligned_chunk_size(cur, size - done);
        value |= (mmio.read(cur, chunk) & access_mask(chunk)) << (done * 8);
        done += chunk;
    }
    Some(value)
}

fn write_unaligned(mmio: &Pl050Mmio, offset: u64, size: u32, val: u64) -> bool {
    if !needs_unaligned_split(offset, size) {
        return false;
    }

    let mut done = 0u32;
    while done < size {
        let cur = offset + u64::from(done);
        let chunk = aligned_chunk_size(cur, size - done);
        let chunk_value = (val >> (done * 8)) & access_mask(chunk);
        mmio.write(cur, chunk, chunk_value);
        done += chunk;
    }
    true
}

fn needs_unaligned_split(offset: u64, size: u32) -> bool {
    matches!(size, 2 | 4 | 8) && !offset.is_multiple_of(u64::from(size))
}

fn aligned_chunk_size(offset: u64, remaining: u32) -> u32 {
    for size in [8u32, 4, 2, 1] {
        if remaining >= size && offset.is_multiple_of(u64::from(size)) {
            return size;
        }
    }
    1
}
