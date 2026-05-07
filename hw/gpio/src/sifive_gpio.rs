use std::sync::Arc;

use machina_core::device_cell::DeviceRegs;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_memory::region::MmioOps;

const SIFIVE_GPIO_PINS: usize = 32;

// Register offsets
const REG_VALUE: u64 = 0x000;
const REG_INPUT_EN: u64 = 0x004;
const REG_OUTPUT_EN: u64 = 0x008;
const REG_PORT: u64 = 0x00C;
const REG_PUE: u64 = 0x010;
const REG_DS: u64 = 0x014;
const REG_RISE_IE: u64 = 0x018;
const REG_RISE_IP: u64 = 0x01C;
const REG_FALL_IE: u64 = 0x020;
const REG_FALL_IP: u64 = 0x024;
const REG_HIGH_IE: u64 = 0x028;
const REG_HIGH_IP: u64 = 0x02C;
const REG_LOW_IE: u64 = 0x030;
const REG_LOW_IP: u64 = 0x034;
const REG_IOF_EN: u64 = 0x038;
const REG_IOF_SEL: u64 = 0x03C;
const REG_OUT_XOR: u64 = 0x040;

fn access_mask(size: u32) -> u64 {
    match size {
        1 => 0xff,
        2 => 0xffff,
        4 => 0xffff_ffff,
        _ => u64::MAX,
    }
}

fn read_unaligned(
    mmio: &SiFiveGpioMmio,
    offset: u64,
    size: u32,
) -> Option<u64> {
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

fn write_unaligned(
    mmio: &SiFiveGpioMmio,
    offset: u64,
    size: u32,
    val: u64,
) -> bool {
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

struct SiFiveGpioRegs {
    value: u32,
    input_en: u32,
    output_en: u32,
    port: u32,
    pue: u32,
    ds: u32,
    rise_ie: u32,
    rise_ip: u32,
    fall_ie: u32,
    fall_ip: u32,
    high_ie: u32,
    high_ip: u32,
    low_ie: u32,
    low_ip: u32,
    iof_en: u32,
    iof_sel: u32,
    out_xor: u32,
    // External input
    input_mask: u32,
    ext_input: u32,
}

impl SiFiveGpioRegs {
    fn new() -> Self {
        Self {
            value: 0,
            input_en: 0,
            output_en: 0,
            port: 0,
            pue: 0,
            ds: 0,
            rise_ie: 0,
            rise_ip: 0,
            fall_ie: 0,
            fall_ip: 0,
            high_ie: 0,
            high_ip: 0,
            low_ie: 0,
            low_ip: 0,
            iof_en: 0,
            iof_sel: 0,
            out_xor: 0,
            input_mask: 0,
            ext_input: 0,
        }
    }

    fn reset(&mut self) {
        self.value = 0;
        self.input_en = 0;
        self.output_en = 0;
        self.port = 0;
        self.pue = 0;
        self.ds = 0;
        self.rise_ie = 0;
        self.rise_ip = 0;
        self.fall_ie = 0;
        self.fall_ip = 0;
        self.high_ie = 0;
        self.high_ip = 0;
        self.low_ie = 0;
        self.low_ip = 0;
        self.iof_en = 0;
        self.iof_sel = 0;
        self.out_xor = 0;
        self.input_mask = 0;
        self.ext_input = 0;
    }

    fn extract_bit(val: u32, i: usize) -> bool {
        ((val >> i) & 1) != 0
    }

    fn deposit_bit(val: u32, i: usize, bit: bool) -> u32 {
        if bit {
            val | (1 << i)
        } else {
            val & !(1 << i)
        }
    }

    fn update(&mut self) {
        for i in 0..SIFIVE_GPIO_PINS {
            let prev_ival = Self::extract_bit(self.value, i);
            let input = Self::extract_bit(self.ext_input, i);
            let in_mask = Self::extract_bit(self.input_mask, i);
            let port = Self::extract_bit(self.port, i);
            let out_xor = Self::extract_bit(self.out_xor, i);
            let pull = Self::extract_bit(self.pue, i);
            let output_en = Self::extract_bit(self.output_en, i);
            let input_en = Self::extract_bit(self.input_en, i);

            let oval = output_en && (port ^ out_xor);
            let actual_value = if in_mask {
                input
            } else if output_en {
                oval
            } else {
                pull
            };

            let ival = input_en && actual_value;

            // Edge/level interrupts
            let high_ip = Self::extract_bit(self.high_ip, i) || ival;
            self.high_ip = Self::deposit_bit(self.high_ip, i, high_ip);

            let low_ip = Self::extract_bit(self.low_ip, i) || !ival;
            self.low_ip = Self::deposit_bit(self.low_ip, i, low_ip);

            let rise_ip =
                Self::extract_bit(self.rise_ip, i) || (ival && !prev_ival);
            self.rise_ip = Self::deposit_bit(self.rise_ip, i, rise_ip);

            let fall_ip =
                Self::extract_bit(self.fall_ip, i) || (!ival && prev_ival);
            self.fall_ip = Self::deposit_bit(self.fall_ip, i, fall_ip);

            self.value = Self::deposit_bit(self.value, i, ival);
        }
    }

    fn irq_pending(&self, i: usize) -> bool {
        let pin = 1u32 << i;
        let mut pending = self.high_ip & self.high_ie;
        pending |= self.low_ip & self.low_ie;
        pending |= self.rise_ip & self.rise_ie;
        pending |= self.fall_ip & self.fall_ie;
        (pending & pin) != 0
    }
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot", before_unrealize = lower_outputs)]
pub struct SiFiveGpio {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRegs<SiFiveGpioRegs>,
    outputs: parking_lot::Mutex<[Option<InterruptSource>; SIFIVE_GPIO_PINS]>,
}

impl SiFiveGpio {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(
                "sifive_gpio",
            )),
            regs: DeviceRegs::new(SiFiveGpioRegs::new()),
            outputs: parking_lot::Mutex::new(
                [const { None }; SIFIVE_GPIO_PINS],
            ),
        }
    }

    pub fn connect_output(&self, pin: usize, irq: InterruptSource) {
        self.outputs.lock()[pin] = Some(irq);
    }

    pub fn reset_runtime(&self) {
        self.regs.borrow().reset();
        self.lower_outputs();
    }

    fn lower_outputs(&self) {
        for l in self.outputs.lock().iter().flatten() {
            l.lower();
        }
    }

    fn update_irqs(&self) {
        let regs = self.regs.borrow();
        for (i, line) in self.outputs.lock().iter().enumerate() {
            if let Some(ref l) = line {
                l.set(regs.irq_pending(i));
            }
        }
    }

    /// Set GPIO input pin from external source.
    pub fn set_input(&self, pin: usize, level: bool) {
        let mut regs = self.regs.borrow();
        regs.input_mask =
            SiFiveGpioRegs::deposit_bit(regs.input_mask, pin, true);
        regs.ext_input =
            SiFiveGpioRegs::deposit_bit(regs.ext_input, pin, level);
        regs.update();
        drop(regs);
        self.update_irqs();
    }
}

impl Default for SiFiveGpio {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SiFiveGpioMmio(pub Arc<SiFiveGpio>);

impl MmioOps for SiFiveGpioMmio {
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
            REG_VALUE => u64::from(regs.value),
            REG_INPUT_EN => u64::from(regs.input_en),
            REG_OUTPUT_EN => u64::from(regs.output_en),
            REG_PORT => u64::from(regs.port),
            REG_PUE => u64::from(regs.pue),
            REG_DS => u64::from(regs.ds),
            REG_RISE_IE => u64::from(regs.rise_ie),
            REG_RISE_IP => u64::from(regs.rise_ip),
            REG_FALL_IE => u64::from(regs.fall_ie),
            REG_FALL_IP => u64::from(regs.fall_ip),
            REG_HIGH_IE => u64::from(regs.high_ie),
            REG_HIGH_IP => u64::from(regs.high_ip),
            REG_LOW_IE => u64::from(regs.low_ie),
            REG_LOW_IP => u64::from(regs.low_ip),
            REG_IOF_EN => u64::from(regs.iof_en),
            REG_IOF_SEL => u64::from(regs.iof_sel),
            REG_OUT_XOR => u64::from(regs.out_xor),
            _ => 0,
        };
        value & access_mask(size)
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
        let mut regs = self.0.regs.borrow();
        match offset {
            REG_INPUT_EN => regs.input_en = value,
            REG_OUTPUT_EN => regs.output_en = value,
            REG_PORT => regs.port = value,
            REG_PUE => regs.pue = value,
            REG_DS => regs.ds = value,
            REG_RISE_IE => regs.rise_ie = value,
            REG_RISE_IP => regs.rise_ip &= !value,
            REG_FALL_IE => regs.fall_ie = value,
            REG_FALL_IP => regs.fall_ip &= !value,
            REG_HIGH_IE => regs.high_ie = value,
            REG_HIGH_IP => regs.high_ip &= !value,
            REG_LOW_IE => regs.low_ie = value,
            REG_LOW_IP => regs.low_ip &= !value,
            REG_IOF_EN => regs.iof_en = value,
            REG_IOF_SEL => regs.iof_sel = value,
            REG_OUT_XOR => regs.out_xor = value,
            _ => return,
        }
        regs.update();
        drop(regs);
        self.0.update_irqs();
    }
}
