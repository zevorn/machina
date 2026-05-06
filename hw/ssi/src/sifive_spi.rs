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

// Register indices (offset >> 2)
const R_SCKDIV: usize = 0;
const R_CSID: usize = 0x10 / 4;
const R_CSDEF: usize = 0x14 / 4;
const R_CSMODE: usize = 0x18 / 4;
const R_DELAY0: usize = 0x28 / 4;
const R_DELAY1: usize = 0x2C / 4;
const R_FMT: usize = 0x40 / 4;
const R_TXDATA: usize = 0x48 / 4;
const R_RXDATA: usize = 0x4C / 4;
const R_TXMARK: usize = 0x50 / 4;
const R_RXMARK: usize = 0x54 / 4;
const R_FCTRL: usize = 0x60 / 4;
const R_FFMT: usize = 0x64 / 4;
const R_IE: usize = 0x70 / 4;
const R_IP: usize = 0x74 / 4;

const SIFIVE_SPI_REG_NUM: usize = 0x78 / 4;

const TXDATA_FULL: u32 = 1 << 31;
const RXDATA_EMPTY: u32 = 1 << 31;

const IP_TXWM: u32 = 1 << 0;
const IP_RXWM: u32 = 1 << 1;

const FMT_DIR: u32 = 1 << 3;

const FIFO_CAPACITY: usize = 8;

struct Fifo8 {
    data: [u8; FIFO_CAPACITY],
    head: usize,
    num: usize,
}

impl Fifo8 {
    fn new() -> Self {
        Self {
            data: [0; FIFO_CAPACITY],
            head: 0,
            num: 0,
        }
    }

    fn reset(&mut self) {
        self.head = 0;
        self.num = 0;
    }

    fn is_empty(&self) -> bool {
        self.num == 0
    }

    fn is_full(&self) -> bool {
        self.num == FIFO_CAPACITY
    }

    fn num_used(&self) -> usize {
        self.num
    }

    fn push(&mut self, val: u8) {
        if !self.is_full() {
            let idx = (self.head + self.num) % FIFO_CAPACITY;
            self.data[idx] = val;
            self.num += 1;
        }
    }

    fn pop(&mut self) -> u8 {
        if self.is_empty() {
            return 0;
        }
        let val = self.data[self.head];
        self.head = (self.head + 1) % FIFO_CAPACITY;
        self.num -= 1;
        val
    }
}

struct SiFiveSpiRegs {
    regs: [u32; SIFIVE_SPI_REG_NUM],
    tx_fifo: Fifo8,
    rx_fifo: Fifo8,
}

impl SiFiveSpiRegs {
    fn new(num_cs: u32) -> Self {
        let mut regs = SiFiveSpiRegs {
            regs: [0u32; SIFIVE_SPI_REG_NUM],
            tx_fifo: Fifo8::new(),
            rx_fifo: Fifo8::new(),
        };
        regs.regs[R_CSDEF] = (1u32 << num_cs) - 1;
        regs.regs[R_SCKDIV] = 0x03;
        regs.regs[R_DELAY0] = 0x1001;
        regs.regs[R_DELAY1] = 0x01;
        regs
    }

    fn reset(&mut self, num_cs: u32) {
        self.regs = [0u32; SIFIVE_SPI_REG_NUM];
        self.regs[R_CSDEF] = (1u32 << num_cs) - 1;
        self.regs[R_SCKDIV] = 0x03;
        self.regs[R_DELAY0] = 0x1001;
        self.regs[R_DELAY1] = 0x01;
        self.tx_fifo.reset();
        self.rx_fifo.reset();
    }
}

fn is_bad_reg(addr: u64, allow_reserved: bool) -> bool {
    match addr {
        0x08 | 0x0C | 0x1C | 0x20 | 0x24 | 0x30 | 0x34 | 0x38 | 0x3C | 0x44
        | 0x58 | 0x5C | 0x68 | 0x6C => allow_reserved,
        _ => {
            if addr >= (SIFIVE_SPI_REG_NUM as u64) << 2 {
                return true;
            }
            false
        }
    }
}

pub struct SiFiveSpi {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<SiFiveSpiRegs>,
    irq: parking_lot::Mutex<Option<InterruptSource>>,
    cs_lines: parking_lot::Mutex<Vec<Option<InterruptSource>>>,
    num_cs: u32,
    ssi_bus: parking_lot::Mutex<Option<Arc<SpiBus>>>,
}

impl SiFiveSpi {
    #[must_use]
    pub fn new() -> Self {
        Self::with_num_cs(1)
    }

    #[must_use]
    pub fn with_num_cs(num_cs: u32) -> Self {
        let mut cs_lines = Vec::with_capacity(num_cs as usize);
        for _ in 0..num_cs {
            cs_lines.push(None);
        }
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(
                "sifive_spi",
            )),
            regs: DeviceRefCell::new(SiFiveSpiRegs::new(num_cs)),
            irq: parking_lot::Mutex::new(None),
            cs_lines: parking_lot::Mutex::new(cs_lines),
            num_cs,
            ssi_bus: parking_lot::Mutex::new(None),
        }
    }

    pub fn connect_ssi_bus(&self, bus: Arc<SpiBus>) {
        *self.ssi_bus.lock() = Some(bus);
    }

    pub fn connect_irq(&self, irq: InterruptSource) {
        *self.irq.lock() = Some(irq);
    }

    pub fn connect_cs(&self, idx: usize, irq: InterruptSource) {
        let mut lines = self.cs_lines.lock();
        if idx < lines.len() {
            lines[idx] = Some(irq);
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
        self.lower_outputs();
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
        self.lower_outputs();
        self.regs.borrow().reset(self.num_cs);
        self.update_cs();
    }

    fn lower_outputs(&self) {
        if let Some(ref line) = *self.irq.lock() {
            line.lower();
        }
        for l in self.cs_lines.lock().iter().flatten() {
            l.lower();
        }
        // Deassert all CS lines: drive each line to its default
        // (idle) level per CSDEF. CSDEF bit = 1 means active-high,
        // so idle = low = false; CSDEF bit = 0 means active-low,
        // so idle = high = true.
        if let Some(ref bus) = *self.ssi_bus.lock() {
            let regs = self.regs.borrow();
            for i in 0..self.num_cs {
                let csdef_bit = (regs.regs[R_CSDEF] >> i) & 1;
                let default_level = csdef_bit == 0;
                bus.set_cs(i as u8, default_level);
            }
        }
    }

    fn update_cs(&self) {
        let regs = self.regs.borrow();
        let cs_lines = self.cs_lines.lock();
        let ssi = self.ssi_bus.lock();
        for i in 0..self.num_cs as usize {
            let csdef_bit = (regs.regs[R_CSDEF] >> i) & 1;
            // CSDEF=1 → active-high → assert=true,  idle=false
            // CSDEF=0 → active-low  → assert=false, idle=true
            let level = match regs.regs[R_CSMODE] {
                0 /* AUTO */ => csdef_bit == 0, // idle = deasserted
                2 /* HOLD */ => {
                    if i == regs.regs[R_CSID] as usize {
                        csdef_bit != 0 // selected = asserted
                    } else {
                        csdef_bit == 0 // idle = deasserted
                    }
                }
                _ => csdef_bit == 0, /* OFF or invalid: idle */
            };
            if i < cs_lines.len() {
                if let Some(ref line) = cs_lines[i] {
                    line.set(level);
                }
            }
            if let Some(ref bus) = *ssi {
                bus.set_cs(i as u8, level);
            }
        }
    }

    fn update_irq(&self) {
        let mut regs = self.regs.borrow();
        // Recompute IP flags from current FIFO levels
        if regs.tx_fifo.num_used() < regs.regs[R_TXMARK] as usize {
            regs.regs[R_IP] |= IP_TXWM;
        } else {
            regs.regs[R_IP] &= !IP_TXWM;
        }
        if regs.rx_fifo.num_used() > regs.regs[R_RXMARK] as usize {
            regs.regs[R_IP] |= IP_RXWM;
        } else {
            regs.regs[R_IP] &= !IP_RXWM;
        }
        let irq = (regs.regs[R_IP] & regs.regs[R_IE]) != 0;
        drop(regs);
        if let Some(ref line) = *self.irq.lock() {
            line.set(irq);
        }
    }

    fn flush_txfifo(&self, ssi: Option<&SpiBus>) {
        let mut regs = self.regs.borrow();
        let auto_mode = regs.regs[R_CSMODE] == 0;
        let cs_id = regs.regs[R_CSID] as u8;
        let csdef_assert = if auto_mode {
            (regs.regs[R_CSDEF] >> cs_id) & 1 != 0
        } else {
            false
        };
        while !regs.tx_fifo.is_empty() {
            let tx = regs.tx_fifo.pop();
            if auto_mode {
                if let Some(bus) = ssi {
                    bus.set_cs(cs_id, csdef_assert);
                }
            }
            let rx = if let Some(bus) = ssi {
                bus.transfer(u32::from(tx))
            } else {
                0xFF
            };
            if auto_mode {
                if let Some(bus) = ssi {
                    bus.set_cs(cs_id, !csdef_assert);
                }
            }
            if !regs.rx_fifo.is_full() && regs.regs[R_FMT] & FMT_DIR == 0 {
                regs.rx_fifo.push(rx as u8);
            }
        }
    }
}

impl Default for SiFiveSpi {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SiFiveSpiMmio(pub Arc<SiFiveSpi>);

impl MmioOps for SiFiveSpiMmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        if is_bad_reg(offset, true) {
            return 0;
        }

        let addr = (offset >> 2) as usize;
        match addr {
            R_TXDATA => {
                let regs = self.0.regs.borrow();
                if regs.tx_fifo.is_full() {
                    return u64::from(TXDATA_FULL);
                }
                0
            }
            R_RXDATA => {
                let mut regs = self.0.regs.borrow();
                if regs.rx_fifo.is_empty() {
                    return u64::from(RXDATA_EMPTY);
                }
                let val = regs.rx_fifo.pop();
                drop(regs);
                self.0.update_irq();
                u64::from(val)
            }
            _ => {
                let regs = self.0.regs.borrow();
                u64::from(regs.regs[addr])
            }
        }
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        if is_bad_reg(offset, false) {
            return;
        }

        let value = val as u32;
        let addr = (offset >> 2) as usize;

        match addr {
            R_CSID => {
                let mut regs = self.0.regs.borrow();
                if value >= self.0.num_cs {
                    drop(regs);
                    return;
                }
                regs.regs[R_CSID] = value;
                drop(regs);
                self.0.update_cs();
            }
            R_CSDEF => {
                let mut regs = self.0.regs.borrow();
                if value >= (1u32 << self.0.num_cs) {
                    drop(regs);
                    return;
                }
                regs.regs[R_CSDEF] = value;
                drop(regs);
                self.0.update_cs();
            }
            R_CSMODE => {
                let mut regs = self.0.regs.borrow();
                if value > 3 {
                    drop(regs);
                    return;
                }
                regs.regs[R_CSMODE] = value;
                drop(regs);
                self.0.update_cs();
            }
            R_TXDATA => {
                let mut regs = self.0.regs.borrow();
                if !regs.tx_fifo.is_full() {
                    regs.tx_fifo.push(value as u8);
                    let ssi = self.0.ssi_bus.lock();
                    let bus_ref = ssi.as_ref().map(|b| b.as_ref());
                    drop(regs);
                    self.0.flush_txfifo(bus_ref);
                }
            }
            R_RXDATA | R_IP => {
                // Read-only registers, ignore writes
                return;
            }
            R_TXMARK | R_RXMARK => {
                let mut regs = self.0.regs.borrow();
                if value >= FIFO_CAPACITY as u32 {
                    drop(regs);
                    return;
                }
                regs.regs[addr] = value;
            }
            R_FCTRL | R_FFMT => {
                // Direct-map flash interface not implemented
                return;
            }
            _ => {
                let mut regs = self.0.regs.borrow();
                regs.regs[addr] = value;
            }
        }

        self.0.update_irq();
    }
}
