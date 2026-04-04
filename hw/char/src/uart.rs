// NS16550A UART emulation.
//
// Register map (offsets 0-7):
//   0: RBR(R)/THR(W) when DLAB=0, DLL when DLAB=1
//   1: IER when DLAB=0, DLM when DLAB=1
//   2: IIR(R) / FCR(W)
//   3: LCR  (bit 7 = DLAB)
//   4: MCR
//   5: LSR  (bit0=DR, bit5=THRE, bit6=TEMT)
//   6: MSR
//   7: SCR

use std::any::Any;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::chardev::{
    ByteCb, CharFrontend, ChardevResolveError, ChardevResolver,
};
use machina_hw_core::irq::IrqLine;
use machina_hw_core::mdev::{MDevice, MDeviceError, MDeviceState};
use machina_hw_core::property::{MPropertySpec, MPropertyType, MPropertyValue};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

// IER bits
const IER_RX_AVAIL: u8 = 1 << 0;

// IIR values
const IIR_NONE: u8 = 0x01; // no interrupt pending
const IIR_RX_AVAIL: u8 = 0x04; // rx data available
const IIR_THR_EMPTY: u8 = 0x02; // THR empty

// LSR bits
const LSR_DR: u8 = 1 << 0; // data ready
const LSR_THRE: u8 = 1 << 5; // THR empty
const LSR_TEMT: u8 = 1 << 6; // transmitter empty

// LCR bits
const LCR_DLAB: u8 = 1 << 7;

const FIFO_SIZE: usize = 16;

#[derive(Debug)]
pub enum UartError {
    Device(MDeviceError),
    SysBus(SysBusError),
    Resolve(ChardevResolveError),
}

impl std::fmt::Display for UartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Device(err) => write!(f, "{err}"),
            Self::SysBus(err) => write!(f, "{err}"),
            Self::Resolve(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for UartError {}

impl From<MDeviceError> for UartError {
    fn from(value: MDeviceError) -> Self {
        Self::Device(value)
    }
}

impl From<SysBusError> for UartError {
    fn from(value: SysBusError) -> Self {
        Self::SysBus(value)
    }
}

impl From<ChardevResolveError> for UartError {
    fn from(value: ChardevResolveError) -> Self {
        Self::Resolve(value)
    }
}

pub struct Uart16550 {
    state: SysBusDeviceState,
    rbr: u8,
    thr: u8,
    ier: u8,
    iir: u8,
    fcr: u8,
    lcr: u8,
    mcr: u8,
    lsr: u8,
    msr: u8,
    scr: u8,
    dll: u8,
    dlm: u8,
    rx_fifo: VecDeque<u8>,
    irq_pending: bool,
    irq_line: Option<IrqLine>,
    chardev: Option<CharFrontend>,
    configured_chardev: Option<CharFrontend>,
    resolved_chardev_path: Option<String>,
}

impl Uart16550 {
    pub fn new() -> Self {
        Self::new_named("uart")
    }

    pub fn new_named(local_id: &str) -> Self {
        let mut state = SysBusDeviceState::new(local_id);
        state
            .device_mut()
            .define_property(MPropertySpec::new("chardev", MPropertyType::Link))
            .expect("UART chardev property schema must be valid");

        Self {
            state,
            rbr: 0,
            thr: 0,
            ier: 0,
            iir: IIR_NONE,
            fcr: 0,
            lcr: 0,
            mcr: 0,
            lsr: LSR_THRE | LSR_TEMT,
            msr: 0,
            scr: 0,
            dll: 0,
            dlm: 0,
            rx_fifo: VecDeque::with_capacity(FIFO_SIZE),
            irq_pending: false,
            irq_line: None,
            chardev: None,
            configured_chardev: None,
            resolved_chardev_path: None,
        }
    }

    pub fn set_chardev_property(
        &mut self,
        path: &str,
    ) -> Result<(), MDeviceError> {
        self.state
            .device_mut()
            .set_property("chardev", MPropertyValue::Link(path.to_string()))
    }

    pub fn chardev_property(&self) -> Option<&str> {
        match self.state.device().property("chardev") {
            Some(MPropertyValue::Link(path)) => Some(path.as_str()),
            _ => None,
        }
    }

    pub fn realized(&self) -> bool {
        self.state.device().is_realized()
    }

    pub fn attach_to_bus(&mut self, bus: &mut SysBus) -> Result<(), SysBusError> {
        self.state.attach_to_bus(bus)
    }

    pub fn register_mmio(
        &mut self,
        region: MemoryRegion,
        base: GPA,
    ) -> Result<(), SysBusError> {
        self.state.register_mmio(region, base)
    }

    pub fn attach_irq(&mut self, irq: IrqLine) -> Result<(), SysBusError> {
        self.state.register_irq(irq)
    }

    pub fn attach_chardev(
        &mut self,
        fe: CharFrontend,
    ) -> Result<(), MDeviceError> {
        if self.state.device().is_realized() {
            return Err(MDeviceError::LateMutation("chardev_frontend"));
        }
        self.configured_chardev = Some(fe);
        Ok(())
    }

    pub fn realize_onto(
        &mut self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
        rx_cb: ByteCb,
    ) -> Result<(), UartError> {
        self.realize_onto_with_resolver(bus, address_space, rx_cb, None)
    }

    pub fn realize_onto_with_resolver(
        &mut self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
        rx_cb: ByteCb,
        resolver: Option<&dyn ChardevResolver>,
    ) -> Result<(), UartError> {
        self.state.realize_onto(bus, address_space)?;
        self.irq_line = self.state.irq_outputs().first().cloned();

        if let Some((path, mut fe)) = self.resolve_chardev(resolver)? {
            fe.start_input(rx_cb);
            self.chardev = Some(fe);
            self.resolved_chardev_path = path;
        }

        Ok(())
    }

    pub fn unrealize_from(
        &mut self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), UartError> {
        self.unrealize_from_with_resolver(bus, address_space, None)
    }

    pub fn unrealize_from_with_resolver(
        &mut self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
        resolver: Option<&dyn ChardevResolver>,
    ) -> Result<(), UartError> {
        if let Some(frontend) = self.chardev.take() {
            if let (Some(path), Some(resolver)) =
                (self.resolved_chardev_path.take(), resolver)
            {
                resolver.put_frontend(&path, frontend)?;
            }
        }
        self.irq_line = None;
        self.state.unrealize_from(bus, address_space)?;
        Ok(())
    }

    fn resolve_chardev(
        &mut self,
        resolver: Option<&dyn ChardevResolver>,
    ) -> Result<Option<(Option<String>, CharFrontend)>, UartError> {
        if let Some(frontend) = self.configured_chardev.take() {
            return Ok(Some((None, frontend)));
        }

        let Some(MPropertyValue::Link(path)) = self.state.device().property("chardev")
        else {
            return Ok(None);
        };

        let Some(resolver) = resolver else {
            return Ok(None);
        };
        let frontend = resolver.take_frontend(path)?;
        Ok(Some((Some(path.clone()), frontend)))
    }

    pub fn object_info(&self) -> MObjectInfo {
        self.state.object_info()
    }

    pub fn reset_runtime(&mut self) {
        self.rbr = 0;
        self.thr = 0;
        self.ier = 0;
        self.iir = IIR_NONE;
        self.fcr = 0;
        self.lcr = 0;
        self.mcr = 0;
        self.lsr = LSR_THRE | LSR_TEMT;
        self.msr = 0;
        self.scr = 0;
        self.dll = 0;
        self.dlm = 0;
        self.rx_fifo.clear();
        self.irq_pending = false;
        if let Some(ref line) = self.irq_line {
            line.lower();
        }
    }

    /// Push a byte into the receive FIFO.
    pub fn receive(&mut self, ch: u8) {
        if self.rx_fifo.len() < FIFO_SIZE {
            self.rx_fifo.push_back(ch);
        }
        self.lsr |= LSR_DR;
        self.update_irq();
    }

    pub fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    pub fn update_irq(&mut self) {
        let mut iir = IIR_NONE;

        // RX data available has higher priority.
        if (self.ier & IER_RX_AVAIL) != 0 && (self.lsr & LSR_DR) != 0 {
            iir = IIR_RX_AVAIL;
        } else if (self.ier & 0x02) != 0 && (self.lsr & LSR_THRE) != 0 {
            iir = IIR_THR_EMPTY;
        }

        self.iir = iir;
        self.irq_pending = iir != IIR_NONE;

        if let Some(ref line) = self.irq_line {
            line.set(self.irq_pending);
        }
    }

    pub fn read(&mut self, offset: u64) -> u8 {
        match offset & 0x7 {
            0 => {
                if self.lcr & LCR_DLAB != 0 {
                    self.dll
                } else {
                    self.read_rbr()
                }
            }
            1 => {
                if self.lcr & LCR_DLAB != 0 {
                    self.dlm
                } else {
                    self.ier
                }
            }
            2 => self.iir,
            3 => self.lcr,
            4 => self.mcr,
            5 => self.lsr,
            6 => self.msr,
            7 => self.scr,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u64, val: u8) {
        match offset & 0x7 {
            0 => {
                if self.lcr & LCR_DLAB != 0 {
                    self.dll = val;
                } else {
                    self.write_thr(val);
                }
            }
            1 => {
                if self.lcr & LCR_DLAB != 0 {
                    self.dlm = val;
                } else {
                    self.ier = val & 0x0F;
                    self.update_irq();
                }
            }
            2 => {
                self.fcr = val;
                if val & 0x02 != 0 {
                    // Clear RX FIFO.
                    self.rx_fifo.clear();
                    self.lsr &= !LSR_DR;
                    self.update_irq();
                }
            }
            3 => self.lcr = val,
            4 => self.mcr = val,
            5 => {} // LSR is read-only
            6 => {} // MSR is read-only
            7 => self.scr = val,
            _ => {}
        }
    }

    fn read_rbr(&mut self) -> u8 {
        if let Some(ch) = self.rx_fifo.pop_front() {
            self.rbr = ch;
            if self.rx_fifo.is_empty() {
                self.lsr &= !LSR_DR;
            }
            self.update_irq();
            ch
        } else {
            self.rbr
        }
    }

    fn write_thr(&mut self, val: u8) {
        self.thr = val;
        // Forward to chardev frontend if attached.
        if let Some(ref mut fe) = self.chardev {
            fe.write(&[val]);
        }
        // In emulation the byte is "transmitted"
        // instantly, so THRE stays set.
        self.lsr |= LSR_THRE | LSR_TEMT;
        self.update_irq();
    }
}

impl MObject for Uart16550 {
    fn mobject_state(&self) -> &machina_core::mobject::MObjectState {
        self.state.mobject_state()
    }

    fn mobject_state_mut(
        &mut self,
    ) -> &mut machina_core::mobject::MObjectState {
        self.state.mobject_state_mut()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl MDevice for Uart16550 {
    fn mdevice_state(&self) -> &MDeviceState {
        self.state.device()
    }

    fn mdevice_state_mut(&mut self) -> &mut MDeviceState {
        self.state.device_mut()
    }
}

impl Default for Uart16550 {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Uart16550Mmio(pub Arc<Mutex<Uart16550>>);

impl MmioOps for Uart16550Mmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        self.0.lock().unwrap().read(offset) as u64
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        self.0.lock().unwrap().write(offset, val as u8);
    }
}
