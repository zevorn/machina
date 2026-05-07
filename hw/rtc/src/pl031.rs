use std::sync::Arc;

use machina_core::device_cell::DeviceRefCell;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_memory::region::MmioOps;

const RTC_DR: u64 = 0x00;
const RTC_MR: u64 = 0x04;
const RTC_LR: u64 = 0x08;
const RTC_CR: u64 = 0x0c;
const RTC_IMSC: u64 = 0x10;
const RTC_RIS: u64 = 0x14;
const RTC_MIS: u64 = 0x18;
const RTC_ICR: u64 = 0x1c;

const PL031_ID: [u8; 8] = [0x31, 0x10, 0x14, 0x00, 0x0d, 0xf0, 0x05, 0xb1];

fn access_mask(size: u32) -> u64 {
    match size {
        1 => 0xff,
        2 => 0xffff,
        4 => 0xffff_ffff,
        _ => u64::MAX,
    }
}

struct Pl031Regs {
    // DR: data register (current time in seconds)
    dr: u32,
    // MR: match register (alarm time)
    mr: u32,
    // LR: load register (sets time)
    lr: u32,
    // CR: control register (always reads 1)
    // IMSC: interrupt mask
    im: u32,
    // RIS: raw interrupt status
    is: u32,
}

impl Pl031Regs {
    fn new() -> Self {
        Self {
            dr: 0,
            mr: 0,
            lr: 0,
            im: 0,
            is: 0,
        }
    }

    fn reset(&mut self) {
        self.dr = 0;
        self.mr = 0;
        self.lr = 0;
        self.im = 0;
        self.is = 0;
    }

    fn update(&mut self) -> bool {
        let flags = self.is & self.im;
        flags != 0
    }

    fn set_alarm(&mut self) {
        if self.mr == self.dr {
            self.is = 1;
        }
    }

    fn tick(&mut self, seconds: u32) -> bool {
        self.dr = self.dr.wrapping_add(seconds);
        self.set_alarm();
        self.update()
    }
}

pub struct Pl031 {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<Pl031Regs>,
    output: parking_lot::Mutex<Option<InterruptSource>>,
}

impl Pl031 {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("pl031")
    }

    #[must_use]
    pub fn new_named(local_id: &str) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRefCell::new(Pl031Regs::new()),
            output: parking_lot::Mutex::new(None),
        }
    }

    machina_hw_core::machina_parking_lot_sysbus_accessors!(
        state,
        before_unrealize = lower_outputs
    );

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

    fn update_irq(&self, flags: bool) {
        if let Some(ref line) = *self.output.lock() {
            line.set(flags);
        }
    }

    /// Advance the RTC counter by `seconds` and check alarm.
    pub fn tick(&self, seconds: u32) {
        let flags = self.regs.borrow().tick(seconds);
        self.update_irq(flags);
    }

    /// Get current DR value (seconds counter).
    #[must_use]
    pub fn current_time(&self) -> u32 {
        self.regs.borrow().dr
    }
}

impl Default for Pl031 {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Pl031Mmio(pub Arc<Pl031>);

impl MmioOps for Pl031Mmio {
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
        let value = match offset {
            RTC_DR => u64::from(regs.dr),
            RTC_MR => u64::from(regs.mr),
            RTC_LR => u64::from(regs.lr),
            RTC_CR => 1,
            RTC_IMSC => u64::from(regs.im),
            RTC_RIS => u64::from(regs.is),
            RTC_MIS => u64::from(regs.is & regs.im),
            RTC_ICR => 0,
            idx @ 0xFE0..=0xFFF => {
                let off = ((idx - 0xFE0) >> 2) as usize;
                if off < PL031_ID.len() {
                    u64::from(PL031_ID[off])
                } else {
                    0
                }
            }
            _ => 0,
        };
        value & access_mask(size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        if size == 8 {
            self.write(offset, 4, val);
            self.write(offset.wrapping_add(4), 4, val >> 32);
            return;
        }

        let value = (val & access_mask(size)) as u32;
        match offset {
            RTC_LR => {
                let mut regs = self.0.regs.borrow();
                regs.lr = value;
                regs.dr = value;
                regs.set_alarm();
                let flags = regs.update();
                drop(regs);
                self.0.update_irq(flags);
            }
            RTC_MR => {
                let mut regs = self.0.regs.borrow();
                regs.mr = value;
                regs.set_alarm();
                let flags = regs.update();
                drop(regs);
                self.0.update_irq(flags);
            }
            RTC_IMSC => {
                let mut regs = self.0.regs.borrow();
                regs.im = value & 1;
                let flags = regs.update();
                drop(regs);
                self.0.update_irq(flags);
            }
            RTC_ICR => {
                let mut regs = self.0.regs.borrow();
                regs.is &= !value;
                let flags = regs.update();
                drop(regs);
                self.0.update_irq(flags);
            }
            RTC_CR => { /* writes ignored */ }
            // Read-only registers: DR, MIS, RIS — silently ignore
            _ => {}
        }
    }
}

fn read_unaligned(mmio: &Pl031Mmio, offset: u64, size: u32) -> Option<u64> {
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
