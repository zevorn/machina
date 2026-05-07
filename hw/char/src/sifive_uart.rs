use std::sync::Arc;

use machina_core::device_cell::DeviceRefCell;
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::chardev::CharFrontend;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MmioOps;

// Register offsets
const SIFIVE_UART_TXFIFO: u64 = 0;
const SIFIVE_UART_RXFIFO: u64 = 4;
const SIFIVE_UART_TXCTRL: u64 = 8;
const SIFIVE_UART_RXCTRL: u64 = 12;
const SIFIVE_UART_IE: u64 = 16;
const SIFIVE_UART_IP: u64 = 20;
const SIFIVE_UART_DIV: u64 = 24;

const SIFIVE_UART_IE_TXWM: u32 = 1;
const SIFIVE_UART_IE_RXWM: u32 = 2;

const SIFIVE_UART_IP_TXWM: u32 = 1;
const SIFIVE_UART_IP_RXWM: u32 = 2;

const SIFIVE_UART_TXFIFO_FULL: u32 = 0x8000_0000;

const SIFIVE_UART_RX_FIFO_SIZE: usize = 8;
const SIFIVE_UART_TX_FIFO_SIZE: usize = 8;

fn sifive_uart_txen(txctrl: u32) -> bool {
    (txctrl & 0x1) != 0
}

fn sifive_uart_rxen(rxctrl: u32) -> bool {
    (rxctrl & 0x1) != 0
}

fn sifive_uart_get_txcnt(txctrl: u32) -> usize {
    ((txctrl >> 16) & 0x7) as usize
}

fn sifive_uart_get_rxcnt(rxctrl: u32) -> usize {
    ((rxctrl >> 16) & 0x7) as usize
}

struct SiFiveUartRegs {
    txfifo: u32,
    ie: u32,
    txctrl: u32,
    rxctrl: u32,
    div: u32,
    rx_fifo: [u8; SIFIVE_UART_RX_FIFO_SIZE],
    rx_fifo_len: usize,
    tx_fifo: [u8; SIFIVE_UART_TX_FIFO_SIZE],
    tx_fifo_head: usize,
    tx_fifo_num: usize,
}

impl SiFiveUartRegs {
    fn new() -> Self {
        Self {
            txfifo: 0,
            ie: 0,
            txctrl: 0,
            rxctrl: 0,
            div: 0,
            rx_fifo: [0; SIFIVE_UART_RX_FIFO_SIZE],
            rx_fifo_len: 0,
            tx_fifo: [0; SIFIVE_UART_TX_FIFO_SIZE],
            tx_fifo_head: 0,
            tx_fifo_num: 0,
        }
    }

    fn reset(&mut self) {
        self.txfifo = 0;
        self.ie = 0;
        self.txctrl = 0;
        self.rxctrl = 0;
        self.div = 0;
        self.rx_fifo_len = 0;
        self.tx_fifo_head = 0;
        self.tx_fifo_num = 0;
    }

    fn ip(&self) -> u32 {
        let mut ret = 0;
        let txcnt = sifive_uart_get_txcnt(self.txctrl);
        let rxcnt = sifive_uart_get_rxcnt(self.rxctrl);

        if self.tx_fifo_num < txcnt {
            ret |= SIFIVE_UART_IP_TXWM;
        }
        if self.rx_fifo_len > rxcnt {
            ret |= SIFIVE_UART_IP_RXWM;
        }
        ret
    }

    fn tx_fifo_full(&self) -> bool {
        self.tx_fifo_num >= SIFIVE_UART_TX_FIFO_SIZE
    }

    fn tx_fifo_empty(&self) -> bool {
        self.tx_fifo_num == 0
    }

    fn tx_fifo_push(&mut self, byte: u8) -> bool {
        if self.tx_fifo_full() {
            return false;
        }
        let idx =
            (self.tx_fifo_head + self.tx_fifo_num) % SIFIVE_UART_TX_FIFO_SIZE;
        self.tx_fifo[idx] = byte;
        self.tx_fifo_num += 1;
        if self.tx_fifo_full() {
            self.txfifo |= SIFIVE_UART_TXFIFO_FULL;
        }
        true
    }

    fn tx_fifo_pop(&mut self) -> Option<u8> {
        if self.tx_fifo_empty() {
            return None;
        }
        let byte = self.tx_fifo[self.tx_fifo_head];
        self.tx_fifo_head = (self.tx_fifo_head + 1) % SIFIVE_UART_TX_FIFO_SIZE;
        self.tx_fifo_num -= 1;
        self.txfifo &= !SIFIVE_UART_TXFIFO_FULL;
        Some(byte)
    }
}

pub struct SiFiveUart {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<SiFiveUartRegs>,
    output: parking_lot::Mutex<Option<InterruptSource>>,
    chardev: DeviceRefCell<Option<CharFrontend>>,
    configured_chardev: parking_lot::Mutex<Option<CharFrontend>>,
}

impl SiFiveUart {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("sifive_uart")
    }

    #[must_use]
    pub fn new_named(local_id: &str) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRefCell::new(SiFiveUartRegs::new()),
            output: parking_lot::Mutex::new(None),
            chardev: DeviceRefCell::new(None),
            configured_chardev: parking_lot::Mutex::new(None),
        }
    }

    machina_hw_core::machina_parking_lot_sysbus_accessors!(
        state,
        lifecycle = manual
    );

    pub fn realize_onto(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.state.lock().realize_onto(bus, address_space)?;
        if let Some(fe) = self.configured_chardev.lock().take() {
            *self.chardev.borrow() = Some(fe);
        }
        Ok(())
    }

    pub fn unrealize_from(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.chardev.borrow().take();
        self.lower_outputs();
        self.state.lock().unrealize_from(bus, address_space)?;
        Ok(())
    }

    pub fn connect_output(&self, irq: InterruptSource) {
        *self.output.lock() = Some(irq);
    }

    pub fn reset_runtime(&self) {
        self.regs.borrow().reset();
        self.lower_outputs();
    }

    fn lower_outputs(&self) {
        if let Some(ref line) = *self.output.lock() {
            line.lower();
        }
    }

    fn update_irq(&self) {
        let regs = self.regs.borrow();
        let ip = regs.ip();
        let cond = ((ip & SIFIVE_UART_IP_TXWM) != 0
            && (regs.ie & SIFIVE_UART_IE_TXWM) != 0)
            || ((ip & SIFIVE_UART_IP_RXWM) != 0
                && (regs.ie & SIFIVE_UART_IE_RXWM) != 0);
        drop(regs);
        if let Some(ref line) = *self.output.lock() {
            line.set(cond);
        }
    }

    /// Flush TX FIFO to chardev.
    pub fn flush_tx(&self) {
        if !sifive_uart_txen(self.regs.borrow().txctrl) {
            self.update_irq();
            return;
        }

        if let Some(ref mut fe) = *self.chardev.borrow() {
            let mut regs = self.regs.borrow();
            while !regs.tx_fifo_empty() {
                // Write one byte at a time.
                if let Some(byte) = regs.tx_fifo_pop() {
                    fe.write(&[byte]);
                }
            }
        } else {
            // No backend: drain instantly.
            let mut regs = self.regs.borrow();
            regs.tx_fifo_head = 0;
            regs.tx_fifo_num = 0;
            regs.txfifo &= !SIFIVE_UART_TXFIFO_FULL;
        }
        self.update_irq();
    }

    /// Receive a byte from chardev into RX FIFO.
    pub fn receive(&self, byte: u8) {
        let mut regs = self.regs.borrow();
        let rxen = sifive_uart_rxen(regs.rxctrl);
        let space = regs.rx_fifo_len < SIFIVE_UART_RX_FIFO_SIZE;
        if rxen && space {
            let idx = regs.rx_fifo_len;
            regs.rx_fifo[idx] = byte;
            regs.rx_fifo_len = idx + 1;
        }
        drop(regs);
        self.update_irq();
    }
}

impl Default for SiFiveUart {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SiFiveUartMmio(pub Arc<SiFiveUart>);

impl MmioOps for SiFiveUartMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        if size != 4 {
            return 0;
        }

        match offset {
            SIFIVE_UART_RXFIFO => {
                let mut regs = self.0.regs.borrow();
                if regs.rx_fifo_len > 0 {
                    let r = regs.rx_fifo[0];
                    for i in 1..regs.rx_fifo_len {
                        regs.rx_fifo[i - 1] = regs.rx_fifo[i];
                    }
                    regs.rx_fifo_len -= 1;
                    drop(regs);
                    self.0.update_irq();
                    u64::from(r)
                } else {
                    0x8000_0000
                }
            }
            SIFIVE_UART_TXFIFO => u64::from(self.0.regs.borrow().txfifo),
            SIFIVE_UART_IE => u64::from(self.0.regs.borrow().ie),
            SIFIVE_UART_IP => u64::from(self.0.regs.borrow().ip()),
            SIFIVE_UART_TXCTRL => u64::from(self.0.regs.borrow().txctrl),
            SIFIVE_UART_RXCTRL => u64::from(self.0.regs.borrow().rxctrl),
            SIFIVE_UART_DIV => u64::from(self.0.regs.borrow().div),
            _ => 0,
        }
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        if size != 4 {
            return;
        }

        let value = val as u32;
        match offset {
            SIFIVE_UART_TXFIFO => {
                let mut regs = self.0.regs.borrow();
                regs.tx_fifo_push(value as u8);
                drop(regs);
                self.0.flush_tx();
            }
            SIFIVE_UART_IE => {
                self.0.regs.borrow().ie = value;
                self.0.update_irq();
            }
            SIFIVE_UART_TXCTRL => {
                let was_disabled =
                    !sifive_uart_txen(self.0.regs.borrow().txctrl);
                self.0.regs.borrow().txctrl = value;
                if was_disabled && sifive_uart_txen(value) {
                    self.0.flush_tx();
                }
            }
            SIFIVE_UART_RXCTRL => {
                self.0.regs.borrow().rxctrl = value;
                self.0.update_irq();
            }
            SIFIVE_UART_DIV => self.0.regs.borrow().div = value,
            _ => {}
        }
    }
}

pub struct SiFiveUartIrqSink(pub Arc<SiFiveUart>);

impl IrqSink for SiFiveUartIrqSink {
    fn set_irq(&self, _irq: u32, level: bool) {
        if level {
            self.0.flush_tx();
        }
    }
}
