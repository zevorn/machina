use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::irq::InterruptSource;
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

const N_GPIOS: usize = 8;

const PL061_ID: [u8; 12] = [
    0x00, 0x00, 0x00, 0x00, 0x61, 0x10, 0x04, 0x00, 0x0d, 0xf0, 0x05, 0xb1,
];

struct Pl061Regs {
    data: u32,
    old_out_data: u32,
    old_in_data: u32,
    dir: u32,
    isense: u32,
    ibe: u32,
    iev: u32,
    im: u32,
    istate: u32,
    afsel: u32,
    // Luminary registers
    dr2r: u32,
    dr4r: u32,
    dr8r: u32,
    odr: u32,
    pur: u32,
    pdr: u32,
    slr: u32,
    den: u32,
    locked: bool,
    cr: u32,
    amsel: u32,
    // Properties
    pullups: u8,
    pulldowns: u8,
}

impl Pl061Regs {
    fn new(pullups: u8, pulldowns: u8) -> Self {
        Self {
            data: 0,
            old_out_data: 0,
            old_in_data: 0,
            dir: 0,
            isense: 0,
            ibe: 0,
            iev: 0,
            im: 0,
            istate: 0,
            afsel: 0,
            dr2r: 0xFF,
            dr4r: 0,
            dr8r: 0,
            odr: 0,
            pur: 0,
            pdr: 0,
            slr: 0,
            den: 0,
            locked: true,
            cr: 0xFF,
            amsel: 0,
            pullups,
            pulldowns,
        }
    }

    fn reset(&mut self) {
        self.data = 0;
        self.old_in_data = 0;
        self.old_out_data = 0;
        self.dir = 0;
        self.isense = 0;
        self.ibe = 0;
        self.iev = 0;
        self.im = 0;
        self.istate = 0;
        self.afsel = 0;
        self.dr2r = 0xFF;
        self.dr4r = 0;
        self.dr8r = 0;
        self.odr = 0;
        self.pur = 0;
        self.pdr = 0;
        self.slr = 0;
        self.den = 0;
        self.locked = true;
        self.cr = 0xFF;
        self.amsel = 0;
    }

    fn pullups_mask(&self) -> u8 {
        (self.pullups & !self.dir as u8) as u8
    }

    fn floating_mask(&self) -> u8 {
        (!(self.pullups | self.pulldowns) & !self.dir as u8) as u8
    }

    fn update(&mut self) {
        let pullups = self.pullups_mask();
        let floating = self.floating_mask();

        // Outputs
        let out = (self.data & self.dir)
            | u32::from(pullups)
            | (self.old_out_data & u32::from(floating));

        // Input changes
        let changed = (self.old_in_data ^ self.data) & !self.dir;
        for i in 0..N_GPIOS {
            let mask = 1u32 << i;
            if changed & mask != 0 {
                if self.isense & mask == 0 {
                    // Edge interrupt
                    if self.ibe & mask != 0 {
                        self.istate |= mask;
                    } else {
                        self.istate |= !(self.data ^ self.iev) & mask;
                    }
                }
            }
        }

        // Level interrupt
        self.istate |= !(self.data ^ self.iev) & self.isense;

        self.old_out_data = out;
        self.old_in_data = self.data;
    }

    fn irq_level(&self) -> bool {
        (self.istate & self.im) != 0
    }
}

pub struct Pl061 {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<Pl061Regs>,
    output: parking_lot::Mutex<Option<InterruptSource>>,
    gpio_outputs: parking_lot::Mutex<[Option<InterruptSource>; N_GPIOS]>,
}

impl Pl061 {
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_pull(0xFF, 0x00)
    }

    #[must_use]
    pub fn new_with_pull(pullups: u8, pulldowns: u8) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new("pl061")),
            regs: DeviceRefCell::new(Pl061Regs::new(pullups, pulldowns)),
            output: parking_lot::Mutex::new(None),
            gpio_outputs: parking_lot::Mutex::new([
                None, None, None, None, None, None, None, None,
            ]),
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

    pub fn connect_output(&self, irq: InterruptSource) {
        *self.output.lock() = Some(irq);
    }

    pub fn connect_gpio_output(&self, pin: usize, irq: InterruptSource) {
        self.gpio_outputs.lock()[pin] = Some(irq);
    }

    pub fn reset_runtime(&self) {
        self.regs.borrow().reset();
        self.lower_outputs();
    }

    fn lower_outputs(&self) {
        if let Some(ref line) = *self.output.lock() {
            line.lower();
        }
        for out in self.gpio_outputs.lock().iter() {
            if let Some(ref line) = out {
                line.lower();
            }
        }
    }

    fn update_irq(&self) {
        let irq = self.regs.borrow().irq_level();
        if let Some(ref line) = *self.output.lock() {
            line.set(irq);
        }
    }

    /// Set GPIO input pin level (external signal).
    pub fn set_gpio_input(&self, pin: usize, level: bool) {
        let mask = 1u32 << pin;
        let mut regs = self.regs.borrow();
        if regs.dir & mask == 0 {
            regs.data &= !mask;
            if level {
                regs.data |= mask;
            }
            regs.update();
            let pullups = regs.pullups_mask();
            let floating = regs.floating_mask();
            let out = (regs.data & regs.dir)
                | u32::from(pullups)
                | (regs.old_out_data & u32::from(floating));
            let out_levels = out as u8;
            drop(regs);
            // Propagate GPIO outputs
            for (i, line) in self.gpio_outputs.lock().iter().enumerate() {
                if let Some(ref line) = line {
                    let lvl = (out_levels >> i) & 1 != 0;
                    line.set(lvl);
                }
            }
            self.update_irq();
        }
    }
}

impl Default for Pl061 {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Pl061Mmio(pub Arc<Pl061>);

impl MmioOps for Pl061Mmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        let regs = self.0.regs.borrow();
        match offset {
            0x000..=0x3FF => {
                let idx = (offset >> 2) as u32;
                u64::from(regs.data & idx)
            }
            0x400 => u64::from(regs.dir),
            0x404 => u64::from(regs.isense),
            0x408 => u64::from(regs.ibe),
            0x40C => u64::from(regs.iev),
            0x410 => u64::from(regs.im),
            0x414 => u64::from(regs.istate),
            0x418 => u64::from(regs.istate & regs.im),
            0x420 => u64::from(regs.afsel),
            0x500 => u64::from(regs.dr2r),
            0x504 => u64::from(regs.dr4r),
            0x508 => u64::from(regs.dr8r),
            0x50C => u64::from(regs.odr),
            0x510 => u64::from(regs.pur),
            0x514 => u64::from(regs.pdr),
            0x518 => u64::from(regs.slr),
            0x51C => u64::from(regs.den),
            0x520 => u64::from(u32::from(regs.locked)),
            0x524 => u64::from(regs.cr),
            0x528 => u64::from(regs.amsel),
            0xFD0..=0xFFF => {
                let idx = ((offset - 0xFD0) >> 2) as usize;
                if idx < PL061_ID.len() {
                    u64::from(PL061_ID[idx])
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        let value = val as u32;
        let mut regs = self.0.regs.borrow();
        match offset {
            0x000..=0x3FF => {
                let mask = (offset >> 2) as u32 & regs.dir;
                regs.data = (regs.data & !mask) | (value & mask);
                regs.update();
                drop(regs);
                self.0.update_irq();
                return;
            }
            0x400 => regs.dir = value & 0xFF,
            0x404 => regs.isense = value & 0xFF,
            0x408 => regs.ibe = value & 0xFF,
            0x40C => regs.iev = value & 0xFF,
            0x410 => regs.im = value & 0xFF,
            0x41C => {
                regs.istate &= !value;
                drop(regs);
                self.0.update_irq();
                return;
            }
            0x420 => {
                let mask = regs.cr;
                regs.afsel = (regs.afsel & !mask) | (value & mask);
            }
            0x500 => regs.dr2r = value & 0xFF,
            0x504 => regs.dr4r = value & 0xFF,
            0x508 => regs.dr8r = value & 0xFF,
            0x50C => regs.odr = value & 0xFF,
            0x510 => regs.pur = value & 0xFF,
            0x514 => regs.pdr = value & 0xFF,
            0x518 => regs.slr = value & 0xFF,
            0x51C => regs.den = value & 0xFF,
            0x520 => regs.locked = value != 0xACCE_551,
            0x524 => {
                if !regs.locked {
                    regs.cr = value & 0xFF;
                }
            }
            0x528 => regs.amsel = value & 0xFF,
            _ => return,
        }
        regs.update();
        drop(regs);
        self.0.update_irq();
    }
}
