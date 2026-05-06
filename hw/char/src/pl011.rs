use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::chardev::CharFrontend;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_core::mdev::{MDevice, MDeviceError};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

const PL011_FIFO_DEPTH: usize = 16;

const PL011_FLAG_RI: u32 = 0x100;
const PL011_FLAG_TXFE: u32 = 0x80;
const PL011_FLAG_RXFF: u32 = 0x40;
const PL011_FLAG_TXFF: u32 = 0x20;
const PL011_FLAG_RXFE: u32 = 0x10;
const PL011_FLAG_DCD: u32 = 0x04;
const PL011_FLAG_DSR: u32 = 0x02;
const PL011_FLAG_CTS: u32 = 0x01;

const DR_BE: u32 = 1 << 10;

const INT_OE: u32 = 1 << 10;
const INT_BE: u32 = 1 << 9;
const INT_PE: u32 = 1 << 8;
const INT_FE: u32 = 1 << 7;
const INT_RT: u32 = 1 << 6;
const INT_TX: u32 = 1 << 5;
const INT_RX: u32 = 1 << 4;
const INT_DSR: u32 = 1 << 3;
const INT_DCD: u32 = 1 << 2;
const INT_CTS: u32 = 1 << 1;
const INT_RI: u32 = 1 << 0;
const INT_E: u32 = INT_OE | INT_BE | INT_PE | INT_FE;
const INT_MS: u32 = INT_RI | INT_DSR | INT_DCD | INT_CTS;

const LCR_FEN: u32 = 1 << 4;
#[allow(dead_code)]
const LCR_BRK: u32 = 1 << 0;

const CR_OUT2: u32 = 1 << 13;
const CR_OUT1: u32 = 1 << 12;
const CR_RTS: u32 = 1 << 11;
const CR_DTR: u32 = 1 << 10;
#[allow(dead_code)]
const CR_RXE: u32 = 1 << 9;
const CR_TXE: u32 = 1 << 8;
const CR_LBE: u32 = 1 << 7;
#[allow(dead_code)]
const CR_UARTEN: u32 = 1 << 0;

const IBRD_MASK: u32 = 0xffff;
const FBRD_MASK: u32 = 0x3f;

const PL011_ID: [u8; 8] = [0x11, 0x10, 0x14, 0x00, 0x0d, 0xf0, 0x05, 0xb1];

/// IRQ output indices.
pub const PL011_IRQ_COMBINED: u32 = 0;
pub const PL011_IRQ_RX: u32 = 1;
pub const PL011_IRQ_TX: u32 = 2;
pub const PL011_IRQ_RT: u32 = 3;
pub const PL011_IRQ_MS: u32 = 4;
pub const PL011_IRQ_E: u32 = 5;
const PL011_NUM_IRQS: usize = 6;

const IRQ_MASK: [u32; PL011_NUM_IRQS] = [
    INT_E | INT_MS | INT_RT | INT_TX | INT_RX,
    INT_RX,
    INT_TX,
    INT_RT,
    INT_MS,
    INT_E,
];

struct Pl011Regs {
    flags: u32,
    lcr: u32,
    rsr: u32,
    cr: u32,
    dmacr: u32,
    int_enabled: u32,
    int_level: u32,
    read_fifo: [u32; PL011_FIFO_DEPTH],
    ilpr: u32,
    ibrd: u32,
    fbrd: u32,
    ifl: u32,
    read_pos: usize,
    read_count: usize,
    read_trigger: usize,
    logged_disabled_uart: bool,
}

impl Pl011Regs {
    fn new() -> Self {
        Self {
            flags: PL011_FLAG_RXFE | PL011_FLAG_TXFE,
            lcr: 0,
            rsr: 0,
            cr: 0x300,
            dmacr: 0,
            int_enabled: 0,
            int_level: 0,
            read_fifo: [0; PL011_FIFO_DEPTH],
            ilpr: 0,
            ibrd: 0,
            fbrd: 0,
            ifl: 0x12,
            read_pos: 0,
            read_count: 0,
            read_trigger: 1,
            logged_disabled_uart: false,
        }
    }

    fn reset(&mut self) {
        self.lcr = 0;
        self.rsr = 0;
        self.dmacr = 0;
        self.int_enabled = 0;
        self.int_level = 0;
        self.ilpr = 0;
        self.ibrd = 0;
        self.fbrd = 0;
        self.read_trigger = 1;
        self.ifl = 0x12;
        self.cr = 0x300;
        self.flags = 0;
        self.logged_disabled_uart = false;
        self.flags |= PL011_FLAG_RXFE;
        self.flags |= PL011_FLAG_TXFE;
        self.read_pos = 0;
        self.read_count = 0;
    }

    fn fifo_enabled(&self) -> bool {
        (self.lcr & LCR_FEN) != 0
    }

    fn fifo_depth(&self) -> usize {
        if self.fifo_enabled() {
            PL011_FIFO_DEPTH
        } else {
            1
        }
    }

    fn loopback_enabled(&self) -> bool {
        (self.cr & CR_LBE) != 0
    }

    fn rx_fifo_put(&mut self, value: u32) {
        let depth = self.fifo_depth();
        let slot = (self.read_pos + self.read_count) & (depth - 1);
        self.read_fifo[slot] = value;
        self.read_count += 1;
        self.flags &= !PL011_FLAG_RXFE;
        if self.read_count == depth {
            self.flags |= PL011_FLAG_RXFF;
        }
        if self.read_count == self.read_trigger {
            self.int_level |= INT_RX;
        }
    }

    fn rx_fifo_get(&mut self) -> u32 {
        let depth = self.fifo_depth();
        self.flags &= !PL011_FLAG_RXFF;
        let c = self.read_fifo[self.read_pos];
        if self.read_count > 0 {
            self.read_count -= 1;
            self.read_pos = (self.read_pos + 1) & (depth - 1);
        }
        if self.read_count == 0 {
            self.flags |= PL011_FLAG_RXFE;
        }
        if self.read_count == self.read_trigger.saturating_sub(1) {
            self.int_level &= !INT_RX;
        }
        c
    }

    fn set_read_trigger(&mut self) {
        self.read_trigger = 1;
    }

    fn loopback_mdmctrl(&mut self) {
        if !self.loopback_enabled() {
            return;
        }
        let cr = self.cr;
        let mut fr = self.flags
            & !(PL011_FLAG_RI
                | PL011_FLAG_DCD
                | PL011_FLAG_DSR
                | PL011_FLAG_CTS);
        if cr & CR_OUT2 != 0 {
            fr |= PL011_FLAG_RI;
        }
        if cr & CR_OUT1 != 0 {
            fr |= PL011_FLAG_DCD;
        }
        if cr & CR_RTS != 0 {
            fr |= PL011_FLAG_CTS;
        }
        if cr & CR_DTR != 0 {
            fr |= PL011_FLAG_DSR;
        }

        let mut il = self.int_level & !(INT_DSR | INT_DCD | INT_CTS | INT_RI);
        if fr & PL011_FLAG_DSR != 0 {
            il |= INT_DSR;
        }
        if fr & PL011_FLAG_DCD != 0 {
            il |= INT_DCD;
        }
        if fr & PL011_FLAG_CTS != 0 {
            il |= INT_CTS;
        }
        if fr & PL011_FLAG_RI != 0 {
            il |= INT_RI;
        }

        self.flags = fr;
        self.int_level = il;
    }
}

pub struct Pl011 {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<Pl011Regs>,
    outputs: parking_lot::Mutex<Vec<Option<InterruptSource>>>,
    chardev: DeviceRefCell<Option<CharFrontend>>,
    configured_chardev: parking_lot::Mutex<Option<CharFrontend>>,
}

impl Pl011 {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("pl011")
    }

    #[must_use]
    pub fn new_named(local_id: &str) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRefCell::new(Pl011Regs::new()),
            outputs: parking_lot::Mutex::new({
                let mut v = Vec::with_capacity(PL011_NUM_IRQS);
                v.resize_with(PL011_NUM_IRQS, || None);
                v
            }),
            chardev: DeviceRefCell::new(None),
            configured_chardev: parking_lot::Mutex::new(None),
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
        if let Some(frontend) = self.chardev.borrow().take() {
            drop(frontend);
        }
        self.state.lock().unrealize_from(bus, address_space)?;
        Ok(())
    }

    pub fn attach_chardev(&self, fe: CharFrontend) -> Result<(), MDeviceError> {
        if self.state.lock().device().is_realized() {
            return Err(MDeviceError::LateMutation("chardev"));
        }
        *self.configured_chardev.lock() = Some(fe);
        Ok(())
    }

    pub fn realize_with_chardev(&self) {
        if let Some(fe) = self.configured_chardev.lock().take() {
            *self.chardev.borrow() = Some(fe);
        }
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

    pub fn connect_output(&self, idx: u32, irq: InterruptSource) {
        let mut outputs = self.outputs.lock();
        if (idx as usize) < outputs.len() {
            outputs[idx as usize] = Some(irq);
        }
    }

    pub fn reset_runtime(&self) {
        self.regs.borrow().reset();
        self.lower_outputs();
    }

    fn lower_outputs(&self) {
        let outputs = self.outputs.lock();
        for line in outputs.iter().flatten() {
            line.lower();
        }
    }

    fn update_irqs(&self) {
        let regs = self.regs.borrow();
        let flags = regs.int_level & regs.int_enabled;
        let outputs = self.outputs.lock();
        for i in 0..PL011_NUM_IRQS {
            if let Some(line) = &outputs[i] {
                line.set((flags & IRQ_MASK[i]) != 0);
            }
        }
    }

    /// Receive a byte from the chardev backend.
    pub fn receive(&self, ch: u8) {
        if self.regs.borrow().loopback_enabled() {
            return;
        }
        self.regs.borrow().rx_fifo_put(u32::from(ch));
        self.update_irqs();
    }

    /// Receive a BREAK event.
    pub fn receive_break(&self) {
        if self.regs.borrow().loopback_enabled() {
            return;
        }
        self.regs.borrow().rx_fifo_put(DR_BE);
        self.update_irqs();
    }

    /// Non-blocking RX read (returns Some(byte) or None if empty).
    pub fn read_rx_nonblocking(&self) -> Option<u8> {
        let mut regs = self.regs.borrow();
        if regs.read_count > 0 {
            let c = regs.rx_fifo_get() as u8;
            let has_more = regs.read_count > 0;
            drop(regs);
            // Accept more input after draining.
            if has_more {
                // Will be handled by chardev callback.
            }
            self.update_irqs();
            Some(c)
        } else {
            None
        }
    }
}

impl Default for Pl011 {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Pl011Mmio(pub Arc<Pl011>);

impl MmioOps for Pl011Mmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        let mut regs = self.0.regs.borrow();
        match offset >> 2 {
            0 => {
                // DR: read RX FIFO
                let c = regs.rx_fifo_get();
                let rsr = c >> 8;
                regs.rsr = rsr;
                drop(regs);
                self.0.update_irqs();
                u64::from(c)
            }
            1 => u64::from(regs.rsr),
            6 => u64::from(regs.flags),
            8 => u64::from(regs.ilpr),
            9 => u64::from(regs.ibrd),
            10 => u64::from(regs.fbrd),
            11 => u64::from(regs.lcr),
            12 => u64::from(regs.cr),
            13 => u64::from(regs.ifl),
            14 => u64::from(regs.int_enabled),
            15 => u64::from(regs.int_level),
            16 => u64::from(regs.int_level & regs.int_enabled),
            18 => u64::from(regs.dmacr),
            idx @ 0x3f8..=0x400 => {
                let off = (idx - 0x3f8) as usize;
                u64::from(PL011_ID[off])
            }
            _ => 0,
        }
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        let value = val as u32;
        match offset >> 2 {
            0 => {
                // DR: write TX
                let ch = value as u8;
                let tx_en = self.0.regs.borrow().cr & CR_TXE != 0;
                if tx_en {
                    if let Some(ref mut fe) = *self.0.chardev.borrow() {
                        fe.write(&[ch]);
                    }
                }
                // Loopback
                {
                    let mut regs = self.0.regs.borrow();
                    if regs.loopback_enabled() {
                        regs.rx_fifo_put(u32::from(ch));
                    }
                    regs.int_level |= INT_TX;
                }
                self.0.update_irqs();
            }
            1 => self.0.regs.borrow().rsr = 0,
            6 => { /* Flag register — writes ignored */ }
            8 => self.0.regs.borrow().ilpr = value,
            9 => {
                let mut regs = self.0.regs.borrow();
                regs.ibrd = value & IBRD_MASK;
            }
            10 => {
                let mut regs = self.0.regs.borrow();
                regs.fbrd = value & FBRD_MASK;
            }
            11 => {
                let mut regs = self.0.regs.borrow();
                let old_lcr = regs.lcr;
                // FIFO toggle resets RX/TX FIFOs
                if (old_lcr ^ value) & LCR_FEN != 0 {
                    regs.flags |= PL011_FLAG_RXFE | PL011_FLAG_TXFE;
                    regs.flags &= !(PL011_FLAG_RXFF | PL011_FLAG_TXFF);
                    regs.read_count = 0;
                    regs.read_pos = 0;
                }
                regs.lcr = value;
                regs.set_read_trigger();
            }
            12 => {
                let mut regs = self.0.regs.borrow();
                regs.cr = value;
                regs.loopback_mdmctrl();
                drop(regs);
                self.0.update_irqs();
            }
            13 => {
                let mut regs = self.0.regs.borrow();
                regs.ifl = value;
                regs.set_read_trigger();
            }
            14 => {
                self.0.regs.borrow().int_enabled = value;
                self.0.update_irqs();
            }
            17 => {
                self.0.regs.borrow().int_level &= !value;
                self.0.update_irqs();
            }
            18 => self.0.regs.borrow().dmacr = value,
            _ => {}
        }
    }
}

pub struct Pl011IrqSink {
    pub dev: Arc<Pl011>,
    pub irq: u32,
}

impl IrqSink for Pl011IrqSink {
    fn set_irq(&self, _irq: u32, level: bool) {
        // Passthrough to chardev input would go here.
        // PL011 doesn't have incoming IRQ in standard use.
        let _ = level;
    }
}
