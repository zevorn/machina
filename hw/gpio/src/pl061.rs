use std::sync::Arc;

use machina_core::device_cell::DeviceRefCell;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_memory::region::MmioOps;

const N_GPIOS: usize = 8;

const PL061_ID: [u8; 12] = [
    0x00, 0x00, 0x00, 0x00, 0x61, 0x10, 0x04, 0x00, 0x0d, 0xf0, 0x05, 0xb1,
];
const PL061_ID_LUMINARY: [u8; 12] = [
    0x00, 0x00, 0x00, 0x00, 0x61, 0x00, 0x18, 0x01, 0x0d, 0xf0, 0x05, 0xb1,
];

fn access_mask(size: u32) -> u64 {
    match size {
        1 => 0xff,
        2 => 0xffff,
        4 => 0xffff_ffff,
        _ => u64::MAX,
    }
}

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
    luminary: bool,
}

impl Pl061Regs {
    fn new(pullups: u8, pulldowns: u8, luminary: bool) -> Self {
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
            luminary,
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
        let pullups = if self.luminary {
            self.pur as u8
        } else {
            self.pullups
        };
        pullups & !self.dir as u8
    }

    fn floating_mask(&self) -> u8 {
        let fixed = if self.luminary {
            (self.pur | self.pdr) as u8
        } else {
            self.pullups | self.pulldowns
        };
        !fixed & !self.dir as u8
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
            if changed & mask != 0 && self.isense & mask == 0 {
                // Edge interrupt
                if self.ibe & mask != 0 {
                    self.istate |= mask;
                } else {
                    self.istate |= !(self.data ^ self.iev) & mask;
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

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot", before_unrealize = lower_outputs)]
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
        Self::new_variant(pullups, pulldowns, false)
    }

    #[must_use]
    pub fn new_luminary() -> Self {
        Self::new_variant(0xFF, 0x00, true)
    }

    fn new_variant(pullups: u8, pulldowns: u8, luminary: bool) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new("pl061")),
            regs: DeviceRefCell::new(Pl061Regs::new(
                pullups, pulldowns, luminary,
            )),
            output: parking_lot::Mutex::new(None),
            gpio_outputs: parking_lot::Mutex::new([
                None, None, None, None, None, None, None, None,
            ]),
        }
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
        for line in self.gpio_outputs.lock().iter().flatten() {
            line.lower();
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
    fn read(&self, offset: u64, size: u32) -> u64 {
        if let Some(value) = read_unaligned(self, offset, size) {
            return value;
        }

        if size == 8 {
            let lo = self.read(offset, 4);
            let hi = self.read(offset.wrapping_add(4), 4);
            return lo | (hi << 32);
        }

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
            0x500 if regs.luminary => u64::from(regs.dr2r),
            0x504 if regs.luminary => u64::from(regs.dr4r),
            0x508 if regs.luminary => u64::from(regs.dr8r),
            0x50C if regs.luminary => u64::from(regs.odr),
            0x510 if regs.luminary => u64::from(regs.pur),
            0x514 if regs.luminary => u64::from(regs.pdr),
            0x518 if regs.luminary => u64::from(regs.slr),
            0x51C if regs.luminary => u64::from(regs.den),
            0x520 if regs.luminary => u64::from(u32::from(regs.locked)),
            0x524 if regs.luminary => u64::from(regs.cr),
            0x528 if regs.luminary => u64::from(regs.amsel),
            0x500 | 0x504 | 0x508 | 0x50C | 0x510 | 0x514 | 0x518 | 0x51C
            | 0x520 | 0x524 | 0x528 => 0,
            0xFD0..=0xFFF => {
                let idx = ((offset - 0xFD0) >> 2) as usize;
                let id = if regs.luminary {
                    &PL061_ID_LUMINARY
                } else {
                    &PL061_ID
                };
                if idx < id.len() {
                    u64::from(id[idx])
                } else {
                    0
                }
            }
            _ => 0,
        }
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
            0x500 if regs.luminary => regs.dr2r = value & 0xFF,
            0x504 if regs.luminary => regs.dr4r = value & 0xFF,
            0x508 if regs.luminary => regs.dr8r = value & 0xFF,
            0x50C if regs.luminary => regs.odr = value & 0xFF,
            0x510 if regs.luminary => regs.pur = value & 0xFF,
            0x514 if regs.luminary => regs.pdr = value & 0xFF,
            0x518 if regs.luminary => regs.slr = value & 0xFF,
            0x51C if regs.luminary => regs.den = value & 0xFF,
            0x520 if regs.luminary => regs.locked = value != 0x0ACC_E551,
            0x524 if regs.luminary => {
                if !regs.locked {
                    regs.cr = value & 0xFF;
                }
            }
            0x528 if regs.luminary => regs.amsel = value & 0xFF,
            0x500 | 0x504 | 0x508 | 0x50C | 0x510 | 0x514 | 0x518 | 0x51C
            | 0x520 | 0x524 | 0x528 => return,
            _ => return,
        }
        regs.update();
        drop(regs);
        self.0.update_irq();
    }
}

fn read_unaligned(mmio: &Pl061Mmio, offset: u64, size: u32) -> Option<u64> {
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

fn write_unaligned(mmio: &Pl061Mmio, offset: u64, size: u32, val: u64) -> bool {
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
