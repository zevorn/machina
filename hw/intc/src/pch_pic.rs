use std::sync::Arc;

use machina_core::device_cell::DeviceRefCell;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_memory::region::MmioOps;

const NUM_IRQS: usize = 64;
const NUM_OUTPUTS: usize = 256;
const INT_ID_BASE: u64 = 0x000;
const INT_MASK_BASE: u64 = 0x020;
const HTMSI_EN_BASE: u64 = 0x040;
const INT_EDGE_BASE: u64 = 0x060;
const INT_CLEAR_BASE: u64 = 0x080;
const ROUTE_ENTRY_BASE: u64 = 0x100;
const HTMSI_VEC_BASE: u64 = 0x200;
const INT_STATUS_BASE: u64 = 0x3a0;
const INT_POL_BASE: u64 = 0x3e0;

struct PchPicRegs {
    int_mask: u64,
    htmsi_en: u64,
    intedge: u64,
    intirr: u64,
    intisr: u64,
    last_intirr: u64,
    int_polarity: u64,
    route_entry: [u8; NUM_IRQS],
    htmsi_vector: [u8; NUM_IRQS],
}

impl PchPicRegs {
    fn new() -> Self {
        Self {
            int_mask: u64::MAX,
            htmsi_en: 0,
            intedge: 0,
            intirr: 0,
            intisr: 0,
            last_intirr: 0,
            int_polarity: 0,
            route_entry: [1; NUM_IRQS],
            htmsi_vector: [0; NUM_IRQS],
        }
    }
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot", before_unrealize = lower_outputs)]
pub struct PchPic {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<PchPicRegs>,
    outputs: parking_lot::Mutex<Vec<Option<InterruptSource>>>,
    irq_num: usize,
}

impl PchPic {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("pch_pic", 32)
    }

    #[must_use]
    pub fn new_named(local_id: &str, irq_num: u32) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRefCell::new(PchPicRegs::new()),
            outputs: parking_lot::Mutex::new(empty_outputs()),
            irq_num: (irq_num as usize).clamp(1, NUM_IRQS),
        }
    }

    pub fn connect_output(&self, output_irq: u32, irq: InterruptSource) {
        if output_irq as usize >= NUM_OUTPUTS {
            return;
        }
        self.outputs.lock()[output_irq as usize] = Some(irq);
        self.update_outputs();
    }

    pub fn set_irq(&self, irq: u32, level: bool) {
        if irq as usize >= self.irq_num {
            return;
        }
        {
            let mut regs = self.regs.borrow();
            set_input_level(&mut regs, irq as usize, level);
            pch_pic_update_irq(&mut regs, 1u64 << irq, level, self.irq_num);
        }
        self.update_outputs();
    }

    #[must_use]
    pub fn mmio_read(&self, offset: u64) -> u64 {
        self.mmio_read_sized(offset, 4)
    }

    pub fn mmio_write(&self, offset: u64, val: u64) {
        self.mmio_write_sized(offset, 4, val);
    }

    #[must_use]
    pub fn mmio_read_sized(&self, offset: u64, size: u32) -> u64 {
        if !valid_mmio_size(size) {
            return 0;
        }
        let regs = self.regs.borrow();
        let mut val = 0u64;
        for byte in 0..size {
            val |= u64::from(read_reg_byte(
                &regs,
                self.irq_num,
                offset + u64::from(byte),
            )) << (byte * 8);
        }
        val
    }

    pub fn mmio_write_sized(&self, offset: u64, size: u32, val: u64) {
        if !valid_mmio_size(size) {
            return;
        }
        let mut needs_update = false;
        {
            let mut regs = self.regs.borrow();
            let old_mask = regs.int_mask;
            for byte in 0..size {
                needs_update |= write_reg_byte(
                    &mut regs,
                    self.irq_num,
                    offset + u64::from(byte),
                    ((val >> (byte * 8)) & 0xff) as u8,
                );
            }
            let new_mask = regs.int_mask;
            if new_mask != old_mask {
                pch_pic_update_irq(
                    &mut regs,
                    old_mask & !new_mask,
                    true,
                    self.irq_num,
                );
                pch_pic_update_irq(
                    &mut regs,
                    !old_mask & new_mask,
                    false,
                    self.irq_num,
                );
                needs_update = true;
            }
        }
        if needs_update {
            self.update_outputs();
        }
    }

    fn lower_outputs(&self) {
        let outputs = self.outputs.lock();
        for line in outputs.iter().flatten() {
            line.lower();
        }
    }

    fn update_outputs(&self) {
        let regs = self.regs.borrow();
        let mut levels = [false; NUM_OUTPUTS];
        // Output is driven by intisr directly; the mask gates
        // ISR acceptance, not ongoing output. Masking does not
        // retroactively drop active IRQs.
        let active = regs.intisr;
        for irq in 0..self.irq_num {
            if active & (1u64 << irq) == 0 {
                continue;
            }
            let output = regs.htmsi_vector[irq] as usize;
            levels[output] = true;
        }
        let outputs = self.outputs.lock();
        for (output, line) in outputs.iter().enumerate() {
            if let Some(line) = line {
                line.set(levels[output]);
            }
        }
    }
}

impl Default for PchPic {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PchPicMmio(pub Arc<PchPic>);

impl MmioOps for PchPicMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.mmio_read_sized(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.mmio_write_sized(offset, size, val);
    }
}

pub struct PchPicIrqSink(pub Arc<PchPic>);

impl IrqSink for PchPicIrqSink {
    fn set_irq(&self, irq: u32, level: bool) {
        self.0.set_irq(irq, level);
    }
}

fn set_input_level(regs: &mut PchPicRegs, irq: usize, level: bool) {
    let bit = 1u64 << irq;
    if regs.intedge & bit != 0 {
        if level {
            if regs.last_intirr & bit == 0 {
                regs.intirr |= bit;
            }
            regs.last_intirr |= bit;
        } else {
            regs.last_intirr &= !bit;
        }
        return;
    }

    if level {
        regs.intirr |= bit;
        regs.last_intirr |= bit;
    } else {
        regs.intirr &= !bit;
        regs.last_intirr &= !bit;
    }
}

fn pch_pic_update_irq(
    regs: &mut PchPicRegs,
    mask: u64,
    level: bool,
    irq_num: usize,
) {
    let valid_mask = valid_irq_mask(irq_num);
    regs.intirr &= valid_mask;
    regs.intisr &= valid_mask;
    let mask = mask & valid_mask;
    let val = if level {
        mask & regs.intirr & !regs.int_mask
    } else {
        mask & regs.intisr & !regs.intirr
    };
    if val == 0 {
        return;
    }

    if level {
        regs.intisr |= val;
    } else {
        regs.intisr &= !val;
    }
}

fn read_reg_byte(regs: &PchPicRegs, irq_num: usize, offset: u64) -> u8 {
    match offset {
        INT_ID_BASE..=0x007 => {
            read_u64_byte(id_value(irq_num), (offset - INT_ID_BASE) as usize)
        }
        INT_MASK_BASE..=0x027 => {
            read_u64_byte(regs.int_mask, (offset - INT_MASK_BASE) as usize)
        }
        HTMSI_EN_BASE..=0x047 => {
            read_u64_byte(regs.htmsi_en, (offset - HTMSI_EN_BASE) as usize)
        }
        INT_EDGE_BASE..=0x067 => {
            read_u64_byte(regs.intedge, (offset - INT_EDGE_BASE) as usize)
        }
        ROUTE_ENTRY_BASE..=0x13f => {
            regs.route_entry[(offset - ROUTE_ENTRY_BASE) as usize]
        }
        HTMSI_VEC_BASE..=0x23f => {
            regs.htmsi_vector[(offset - HTMSI_VEC_BASE) as usize]
        }
        INT_STATUS_BASE..=0x3a7 => {
            let status = regs.intisr & !regs.int_mask;
            read_u64_byte(status, (offset - INT_STATUS_BASE) as usize)
        }
        INT_POL_BASE..=0x3e7 => {
            read_u64_byte(regs.int_polarity, (offset - INT_POL_BASE) as usize)
        }
        _ => 0,
    }
}

fn write_reg_byte(
    regs: &mut PchPicRegs,
    irq_num: usize,
    offset: u64,
    val: u8,
) -> bool {
    match offset {
        INT_MASK_BASE..=0x027 => {
            write_u64_byte(
                &mut regs.int_mask,
                (offset - INT_MASK_BASE) as usize,
                val,
            );
            false
        }
        HTMSI_EN_BASE..=0x047 => {
            write_u64_byte(
                &mut regs.htmsi_en,
                (offset - HTMSI_EN_BASE) as usize,
                val,
            );
            false
        }
        INT_EDGE_BASE..=0x067 => {
            write_u64_byte(
                &mut regs.intedge,
                (offset - INT_EDGE_BASE) as usize,
                val,
            );
            true
        }
        INT_CLEAR_BASE..=0x087 => {
            clear_edge_irqs(regs, irq_num, offset, val);
            true
        }
        ROUTE_ENTRY_BASE..=0x13f => {
            regs.route_entry[(offset - ROUTE_ENTRY_BASE) as usize] = val;
            false
        }
        HTMSI_VEC_BASE..=0x23f => {
            regs.htmsi_vector[(offset - HTMSI_VEC_BASE) as usize] = val;
            true
        }
        INT_POL_BASE..=0x3e7 => {
            write_u64_byte(
                &mut regs.int_polarity,
                (offset - INT_POL_BASE) as usize,
                val,
            );
            false
        }
        _ => false,
    }
}

fn clear_edge_irqs(
    regs: &mut PchPicRegs,
    irq_num: usize,
    offset: u64,
    val: u8,
) {
    let first_irq = ((offset - INT_CLEAR_BASE) as usize) * 8;
    for bit_idx in 0..8 {
        if val & (1 << bit_idx) == 0 {
            continue;
        }
        let irq = first_irq + bit_idx;
        if irq >= irq_num {
            continue;
        }
        let bit = 1u64 << irq;
        if regs.intedge & bit == 0 {
            continue;
        }
        regs.intirr &= !bit;
        regs.intisr &= !bit;
    }
}

fn read_u64_byte(word: u64, byte: usize) -> u8 {
    ((word >> (byte * 8)) & 0xff) as u8
}

fn write_u64_byte(word: &mut u64, byte: usize, val: u8) {
    let shift = byte * 8;
    let mask = 0xffu64 << shift;
    *word = (*word & !mask) | (u64::from(val) << shift);
}

fn id_value(irq_num: usize) -> u64 {
    (0x7 << 24) | (0x1 << 32) | (((irq_num as u64).saturating_sub(1)) << 48)
}

fn valid_irq_mask(irq_num: usize) -> u64 {
    if irq_num >= NUM_IRQS {
        u64::MAX
    } else {
        (1u64 << irq_num) - 1
    }
}

fn valid_mmio_size(size: u32) -> bool {
    matches!(size, 1 | 2 | 4 | 8)
}

fn empty_outputs() -> Vec<Option<InterruptSource>> {
    let mut outputs = Vec::with_capacity(NUM_OUTPUTS);
    outputs.resize_with(NUM_OUTPUTS, || None);
    outputs
}
