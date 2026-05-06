use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::irq::InterruptSource;
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

use crate::SpiBus;

const PL022_ID: [u8; 8] = [0x22, 0x10, 0x04, 0x00, 0x0d, 0xf0, 0x05, 0xb1];

const PL022_CR1_LBM: u32 = 1 << 0;
const PL022_CR1_SSE: u32 = 1 << 1;
const PL022_CR1_MS: u32 = 1 << 2;
const PL022_CR1_SDO: u32 = 1 << 3;

const PL022_SR_TFE: u32 = 1 << 0;
const PL022_SR_TNF: u32 = 1 << 1;
const PL022_SR_RNE: u32 = 1 << 2;
const PL022_SR_RFF: u32 = 1 << 3;
const PL022_SR_BSY: u32 = 1 << 4;

const PL022_INT_ROR: u32 = 1 << 0;
const PL022_INT_RT: u32 = 1 << 1;
const PL022_INT_RX: u32 = 1 << 2;
const PL022_INT_TX: u32 = 1 << 3;

const FIFO_SIZE: usize = 8;

struct Pl022Regs {
    cr0: u32,
    cr1: u32,
    bitmask: u32,
    sr: u32,
    cpsr: u32,
    is: u32,
    im: u32,
    tx_fifo: [u16; FIFO_SIZE],
    rx_fifo: [u16; FIFO_SIZE],
    tx_fifo_head: i32,
    rx_fifo_head: i32,
    tx_fifo_len: i32,
    rx_fifo_len: i32,
}

impl Pl022Regs {
    fn new() -> Self {
        Self {
            cr0: 0,
            cr1: 0,
            bitmask: 0,
            sr: PL022_SR_TFE | PL022_SR_TNF,
            cpsr: 0,
            is: PL022_INT_TX,
            im: 0,
            tx_fifo: [0; FIFO_SIZE],
            rx_fifo: [0; FIFO_SIZE],
            tx_fifo_head: 0,
            rx_fifo_head: 0,
            tx_fifo_len: 0,
            rx_fifo_len: 0,
        }
    }

    fn reset(&mut self) {
        self.rx_fifo_len = 0;
        self.tx_fifo_len = 0;
        self.im = 0;
        self.is = PL022_INT_TX;
        self.sr = PL022_SR_TFE | PL022_SR_TNF;
    }

    fn update(&mut self) {
        self.sr = 0;
        if self.tx_fifo_len == 0 {
            self.sr |= PL022_SR_TFE;
        }
        if self.tx_fifo_len != 8 {
            self.sr |= PL022_SR_TNF;
        }
        if self.rx_fifo_len != 0 {
            self.sr |= PL022_SR_RNE;
        }
        if self.rx_fifo_len == 8 {
            self.sr |= PL022_SR_RFF;
        }
        if self.tx_fifo_len != 0 {
            self.sr |= PL022_SR_BSY;
        }
        self.is = 0;
        if self.rx_fifo_len >= 4 {
            self.is |= PL022_INT_RX;
        }
        if self.tx_fifo_len <= 4 {
            self.is |= PL022_INT_TX;
        }
    }
}

pub struct Pl022 {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<Pl022Regs>,
    irq: parking_lot::Mutex<Option<InterruptSource>>,
    ssi_bus: parking_lot::Mutex<Option<Arc<SpiBus>>>,
}

impl Pl022 {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new("pl022")),
            regs: DeviceRefCell::new(Pl022Regs::new()),
            irq: parking_lot::Mutex::new(None),
            ssi_bus: parking_lot::Mutex::new(None),
        }
    }

    pub fn connect_ssi_bus(&self, bus: Arc<SpiBus>) {
        *self.ssi_bus.lock() = Some(bus);
    }

    pub fn connect_irq(&self, irq: InterruptSource) {
        *self.irq.lock() = Some(irq);
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

    /// Run the transfer engine and update IRQ output.
    fn xfer_and_update_irq(&self) {
        let mut regs = self.regs.borrow();
        if regs.cr1 & PL022_CR1_SSE == 0 {
            regs.update();
            let irq = (regs.is & regs.im) != 0;
            drop(regs);
            if let Some(ref line) = *self.irq.lock() {
                line.set(irq);
            }
            return;
        }

        let ssi = self.ssi_bus.lock();
        let mut i = (regs.tx_fifo_head - regs.tx_fifo_len) & 7;
        let mut o = regs.rx_fifo_head;

        while regs.tx_fifo_len > 0 && regs.rx_fifo_len < 8 {
            let val = regs.tx_fifo[i as usize];
            let result = if regs.cr1 & PL022_CR1_LBM != 0 {
                u32::from(val)
            } else if let Some(ref bus) = *ssi {
                bus.transfer(u32::from(val))
            } else {
                0
            };
            regs.rx_fifo[o as usize] = (result & regs.bitmask) as u16;
            i = (i + 1) & 7;
            o = (o + 1) & 7;
            regs.tx_fifo_len -= 1;
            regs.rx_fifo_len += 1;
        }
        regs.rx_fifo_head = o;
        regs.update();
        let irq = (regs.is & regs.im) != 0;
        drop(regs);
        // Drop ssi bus lock before setting IRQ
        drop(ssi);
        if let Some(ref line) = *self.irq.lock() {
            line.set(irq);
        }
    }
}

impl Default for Pl022 {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Pl022Mmio(pub Arc<Pl022>);

impl MmioOps for Pl022Mmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        if offset >= 0xFE0 && offset < 0x1000 {
            let idx = ((offset - 0xFE0) >> 2) as usize;
            if idx < PL022_ID.len() {
                return u64::from(PL022_ID[idx]);
            }
            return 0;
        }

        if offset == 0x08 {
            let mut regs = self.0.regs.borrow();
            if regs.rx_fifo_len > 0 {
                let idx = ((regs.rx_fifo_head - regs.rx_fifo_len) & 7) as usize;
                let val = regs.rx_fifo[idx];
                regs.rx_fifo_len -= 1;
                drop(regs);
                self.0.xfer_and_update_irq();
                return u64::from(val);
            }
            return 0;
        }

        let regs = self.0.regs.borrow();
        match offset {
            0x00 => u64::from(regs.cr0),
            0x04 => u64::from(regs.cr1),
            0x0C => u64::from(regs.sr),
            0x10 => u64::from(regs.cpsr),
            0x14 => u64::from(regs.im),
            0x18 => u64::from(regs.is),
            0x1C => u64::from(regs.im & regs.is),
            0x24 => 0,
            _ => 0,
        }
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        let value = val as u32;
        let mut regs = self.0.regs.borrow();

        match offset {
            0x00 => {
                regs.cr0 = value;
                regs.bitmask = (1u32 << ((value & 0xF) + 1)) - 1;
                drop(regs);
                return;
            }
            0x04 => {
                regs.cr1 = value;
                drop(regs);
                self.0.xfer_and_update_irq();
                return;
            }
            0x08 => {
                if regs.tx_fifo_len < 8 {
                    let head = regs.tx_fifo_head as usize;
                    regs.tx_fifo[head] = (value & regs.bitmask) as u16;
                    regs.tx_fifo_head = (regs.tx_fifo_head + 1) & 7;
                    regs.tx_fifo_len += 1;
                    drop(regs);
                    self.0.xfer_and_update_irq();
                    return;
                }
                drop(regs);
                return;
            }
            0x10 => {
                regs.cpsr = value & 0xFF;
            }
            0x14 => {
                regs.im = value;
                regs.update();
            }
            0x20 => {
                let clear = value & (PL022_INT_ROR | PL022_INT_RT);
                regs.is &= !clear;
            }
            0x24 => {}
            _ => {
                drop(regs);
                return;
            }
        }

        let irq = (regs.is & regs.im) != 0;
        drop(regs);
        if let Some(ref line) = *self.0.irq.lock() {
            line.set(irq);
        }
    }
}
