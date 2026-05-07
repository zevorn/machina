use std::sync::Arc;

use machina_core::device_cell::DeviceRefCell;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_memory::region::MmioOps;

const NUM_IRQS: usize = 256;
const NUM_U32: usize = NUM_IRQS / 32;
const NUM_HWI: usize = 8;
const NUM_IPMAP_HWI: u32 = 4;
const NODEMAP_BASE: u64 = 0x00A0;
const IPMAP_BASE: u64 = 0x00C0;
const ENABLE_BASE: u64 = 0x0200;
const BOUNCE_BASE: u64 = 0x0280;
const ISR_BASE: u64 = 0x0300;
const CORE_ISR_BASE: u64 = 0x0400;
const COREMAP_BASE: u64 = 0x0800;

struct EiointcRegs {
    nodemap: [u32; NUM_U32],
    enable: [u32; NUM_U32],
    isr: [u32; NUM_U32],
    /// Per-CPU core ISR — cleared on CORE_ISR write (ack) without
    /// affecting global ISR.
    core_isr: Vec<[u32; NUM_U32]>,
    coremap: [u8; NUM_IRQS],
    ipmap: [u8; 8],
    bounce: [u32; NUM_U32],
}

impl EiointcRegs {
    fn new(num_cpus: usize) -> Self {
        let nc = num_cpus.max(1);
        let mut core_isr = Vec::with_capacity(nc);
        for _ in 0..nc {
            core_isr.push([0u32; NUM_U32]);
        }
        Self {
            nodemap: [0; NUM_U32],
            enable: [0; NUM_U32],
            isr: [0; NUM_U32],
            core_isr,
            coremap: [0; NUM_IRQS],
            ipmap: [0; 8],
            bounce: [0; NUM_U32],
        }
    }
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot", before_unrealize = lower_outputs)]
pub struct Eiointc {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<EiointcRegs>,
    hwi_outputs: parking_lot::Mutex<Vec<Vec<Option<InterruptSource>>>>,
}

impl Eiointc {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("eiointc", 1)
    }

    #[must_use]
    pub fn new_named(local_id: &str, num_cpus: u32) -> Self {
        let nc = num_cpus.max(1) as usize;
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRefCell::new(EiointcRegs::new(nc)),
            hwi_outputs: parking_lot::Mutex::new(empty_hwi_outputs(num_cpus)),
        }
    }

    pub fn connect_hwi_output(
        &self,
        cpu_id: u32,
        hwi: u8,
        irq: InterruptSource,
    ) {
        if hwi as usize >= NUM_HWI {
            return;
        }
        let mut outputs = self.hwi_outputs.lock();
        while outputs.len() <= cpu_id as usize {
            outputs.push(empty_hwi_output_row());
        }
        outputs[cpu_id as usize][hwi as usize] = Some(irq);
        let route_cpu_count = outputs.len().max(1) as u32;
        drop(outputs);
        {
            let mut regs = self.regs.borrow();
            rebuild_core_isr(&mut regs, route_cpu_count);
        }
        self.update_outputs();
    }

    pub fn set_irq(&self, irq: u32, level: bool) {
        if irq >= NUM_IRQS as u32 {
            return;
        }
        let route_cpu_count = self.route_cpu_count_for(0);
        {
            let mut regs = self.regs.borrow();
            let idx = (irq / 32) as usize;
            let bit = 1u32 << (irq % 32);
            if level {
                regs.isr[idx] |= bit;
            } else {
                regs.isr[idx] &= !bit;
            }
            rebuild_core_isr(&mut regs, route_cpu_count);
        }
        self.update_outputs();
    }

    pub fn ack(&self, irq: u32) {
        if irq >= NUM_IRQS as u32 {
            return;
        }
        {
            let mut regs = self.regs.borrow();
            let idx = (irq / 32) as usize;
            let bit = 1u32 << (irq % 32);
            // Clear per-CPU core_isr; global ISR tracks source
            // assertion state.
            for core in &mut regs.core_isr {
                core[idx] &= !bit;
            }
        }
        self.update_outputs();
    }

    #[must_use]
    pub fn pending_for_cpu(&self, cpu_id: u32) -> u8 {
        let route_cpu_count = self.route_cpu_count_for(cpu_id);
        let regs = self.regs.borrow();
        hwi_bits_for_cpu(&regs, cpu_id, route_cpu_count)
    }

    #[must_use]
    pub fn mmio_read(&self, offset: u64) -> u64 {
        self.mmio_read_sized(0, offset, 4)
    }

    pub fn mmio_write(&self, offset: u64, val: u64) {
        self.mmio_write_sized(0, offset, 4, val);
    }

    #[must_use]
    pub fn mmio_read_sized(&self, cpu_id: u32, offset: u64, size: u32) -> u64 {
        let size = normalize_mmio_size(size);
        let route_cpu_count = self.route_cpu_count_for(cpu_id);
        let regs = self.regs.borrow();
        let mut val = 0u64;
        for byte in 0..size {
            val |= u64::from(read_reg_byte(
                &regs,
                cpu_id,
                route_cpu_count,
                offset + u64::from(byte),
            )) << (byte * 8);
        }
        val
    }

    pub fn mmio_write_sized(
        &self,
        cpu_id: u32,
        offset: u64,
        size: u32,
        val: u64,
    ) {
        let size = normalize_mmio_size(size);
        let route_cpu_count = self.route_cpu_count_for(cpu_id);
        let mut needs_update = false;
        {
            let mut regs = self.regs.borrow();
            for byte in 0..size {
                needs_update |= write_reg_byte(
                    &mut regs,
                    cpu_id,
                    route_cpu_count,
                    offset + u64::from(byte),
                    ((val >> (byte * 8)) & 0xff) as u8,
                );
            }
        }
        if needs_update {
            self.update_outputs();
        }
    }

    fn lower_outputs(&self) {
        let outputs = self.hwi_outputs.lock();
        for line in outputs
            .iter()
            .flat_map(|cpu_outputs| cpu_outputs.iter())
            .flatten()
        {
            line.lower();
        }
    }

    fn update_outputs(&self) {
        let outputs = self.hwi_outputs.lock();
        let regs = self.regs.borrow();
        let route_cpu_count = outputs.len().max(1) as u32;
        for (cpu_id, cpu_outputs) in outputs.iter().enumerate() {
            let bits = hwi_bits_for_cpu(&regs, cpu_id as u32, route_cpu_count);
            for (hwi, line) in cpu_outputs.iter().enumerate() {
                if let Some(line) = line {
                    line.set(bits & (1 << hwi) != 0);
                }
            }
        }
    }

    fn route_cpu_count_for(&self, cpu_id: u32) -> u32 {
        let outputs_len = self.hwi_outputs.lock().len() as u32;
        outputs_len.max(cpu_id.saturating_add(1)).max(1)
    }
}

impl Default for Eiointc {
    fn default() -> Self {
        Self::new()
    }
}

pub struct EiointcMmio(pub Arc<Eiointc>);

impl MmioOps for EiointcMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.mmio_read_sized(0, offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.mmio_write_sized(0, offset, size, val);
    }
}

pub struct EiointcIrqSink(pub Arc<Eiointc>);

impl IrqSink for EiointcIrqSink {
    fn set_irq(&self, irq: u32, level: bool) {
        self.0.set_irq(irq, level);
    }
}

fn rebuild_core_isr(regs: &mut EiointcRegs, route_cpu_count: u32) {
    resize_core_isr(regs, route_cpu_count);
    for word in regs.core_isr.iter_mut().flatten() {
        *word = 0;
    }
    let ncpus = regs.core_isr.len();
    for irq in 0..NUM_IRQS {
        let idx = irq / 32;
        let bit = 1u32 << (irq % 32);
        if regs.isr[idx] & bit == 0 {
            continue;
        }
        if regs.enable[idx] & bit == 0 {
            continue;
        }
        let cpu = decoded_cpu_for_irq(regs, irq, route_cpu_count) as usize;
        if cpu < ncpus {
            regs.core_isr[cpu][idx] |= bit;
        }
    }
}

fn resize_core_isr(regs: &mut EiointcRegs, route_cpu_count: u32) {
    regs.core_isr
        .resize(route_cpu_count.max(1) as usize, [0u32; NUM_U32]);
}

fn hwi_bits_for_cpu(
    regs: &EiointcRegs,
    cpu_id: u32,
    _route_cpu_count: u32,
) -> u8 {
    let cpu = cpu_id as usize;
    if cpu >= regs.core_isr.len() {
        return 0;
    }
    let mut hwi_bits: u8 = 0;
    for irq in 0..NUM_IRQS {
        let idx = irq / 32;
        let bit = 1u32 << (irq % 32);
        if regs.core_isr[cpu][idx] & bit == 0 {
            continue;
        }
        let hwi = decoded_hwi_for_irq(regs, irq);
        hwi_bits |= 1 << hwi;
    }
    hwi_bits
}

fn read_reg_byte(
    regs: &EiointcRegs,
    cpu_id: u32,
    route_cpu_count: u32,
    offset: u64,
) -> u8 {
    match offset {
        NODEMAP_BASE..=0x00BF => {
            let rel = (offset - NODEMAP_BASE) as usize;
            read_u32_byte(regs.nodemap[rel / 4], rel % 4)
        }
        IPMAP_BASE..=0x00C7 => regs.ipmap[(offset - IPMAP_BASE) as usize],
        ENABLE_BASE..=0x021F => {
            let rel = (offset - ENABLE_BASE) as usize;
            read_u32_byte(regs.enable[rel / 4], rel % 4)
        }
        BOUNCE_BASE..=0x029F => {
            let rel = (offset - BOUNCE_BASE) as usize;
            read_u32_byte(regs.bounce[rel / 4], rel % 4)
        }
        ISR_BASE..=0x031F => {
            let rel = (offset - ISR_BASE) as usize;
            read_u32_byte(regs.isr[rel / 4], rel % 4)
        }
        CORE_ISR_BASE..=0x041F => {
            let rel = (offset - CORE_ISR_BASE) as usize;
            read_u32_byte(
                core_isr_word_for_cpu(regs, cpu_id, route_cpu_count, rel / 4),
                rel % 4,
            )
        }
        COREMAP_BASE..=0x08FF => regs.coremap[(offset - COREMAP_BASE) as usize],
        _ => 0,
    }
}

fn write_reg_byte(
    regs: &mut EiointcRegs,
    cpu_id: u32,
    route_cpu_count: u32,
    offset: u64,
    val: u8,
) -> bool {
    match offset {
        NODEMAP_BASE..=0x00BF => {
            let rel = (offset - NODEMAP_BASE) as usize;
            write_u32_byte(&mut regs.nodemap[rel / 4], rel % 4, val);
            rebuild_core_isr(regs, route_cpu_count);
            true
        }
        IPMAP_BASE..=0x00C7 => {
            regs.ipmap[(offset - IPMAP_BASE) as usize] = val;
            rebuild_core_isr(regs, route_cpu_count);
            true
        }
        ENABLE_BASE..=0x021F => {
            let rel = (offset - ENABLE_BASE) as usize;
            write_u32_byte(&mut regs.enable[rel / 4], rel % 4, val);
            rebuild_core_isr(regs, route_cpu_count);
            true
        }
        BOUNCE_BASE..=0x029F => {
            let rel = (offset - BOUNCE_BASE) as usize;
            write_u32_byte(&mut regs.bounce[rel / 4], rel % 4, val);
            false
        }
        CORE_ISR_BASE..=0x041F => {
            clear_core_isr_byte(regs, cpu_id, route_cpu_count, offset, val);
            true
        }
        COREMAP_BASE..=0x08FF => {
            regs.coremap[(offset - COREMAP_BASE) as usize] = val;
            rebuild_core_isr(regs, route_cpu_count);
            true
        }
        _ => false,
    }
}

fn read_u32_byte(word: u32, byte: usize) -> u8 {
    ((word >> (byte * 8)) & 0xff) as u8
}

fn write_u32_byte(word: &mut u32, byte: usize, val: u8) {
    let shift = byte * 8;
    let mask = 0xffu32 << shift;
    *word = (*word & !mask) | (u32::from(val) << shift);
}

fn core_isr_word_for_cpu(
    regs: &EiointcRegs,
    cpu_id: u32,
    _route_cpu_count: u32,
    word_idx: usize,
) -> u32 {
    let cpu = cpu_id as usize;
    if word_idx >= NUM_U32 || cpu >= regs.core_isr.len() {
        return 0;
    }
    regs.core_isr[cpu][word_idx]
}

fn clear_core_isr_byte(
    regs: &mut EiointcRegs,
    cpu_id: u32,
    route_cpu_count: u32,
    offset: u64,
    val: u8,
) {
    let first_irq = ((offset - CORE_ISR_BASE) as usize) * 8;
    let cpu = cpu_id as usize;
    if cpu >= regs.core_isr.len() {
        return;
    }
    for bit in 0..8 {
        if val & (1 << bit) == 0 {
            continue;
        }
        let irq = first_irq + bit;
        if irq >= NUM_IRQS {
            continue;
        }
        if decoded_cpu_for_irq(regs, irq, route_cpu_count) != cpu_id {
            continue;
        }
        // Clear per-CPU core_isr only; global ISR preserves
        // source-asserted state.
        regs.core_isr[cpu][irq / 32] &= !(1u32 << (irq % 32));
    }
}

fn decoded_hwi_for_irq(regs: &EiointcRegs, irq: usize) -> u8 {
    let group = irq / 32;
    decode_one_hot(regs.ipmap.get(group).copied().unwrap_or(0), NUM_IPMAP_HWI)
        as u8
}

fn decoded_cpu_for_irq(
    regs: &EiointcRegs,
    irq: usize,
    route_cpu_count: u32,
) -> u32 {
    let raw = regs.coremap[irq];
    let core = decode_one_hot(raw & 0x0f, 4);
    let node = u32::from(raw >> 4);
    let cpu_id = node * 4 + core;
    if route_cpu_count == 0 || cpu_id >= route_cpu_count {
        0
    } else {
        cpu_id
    }
}

fn decode_one_hot(raw: u8, max: u32) -> u32 {
    if raw == 0 {
        return 0;
    }
    let bit = raw.trailing_zeros();
    if bit < max {
        bit
    } else {
        0
    }
}

fn normalize_mmio_size(size: u32) -> u32 {
    match size {
        1 | 2 | 4 | 8 => size,
        _ => 4,
    }
}

fn empty_hwi_outputs(num_cpus: u32) -> Vec<Vec<Option<InterruptSource>>> {
    let mut outputs = Vec::with_capacity(num_cpus as usize);
    for _ in 0..num_cpus {
        outputs.push(empty_hwi_output_row());
    }
    outputs
}

fn empty_hwi_output_row() -> Vec<Option<InterruptSource>> {
    let mut row = Vec::with_capacity(NUM_HWI);
    for _ in 0..NUM_HWI {
        row.push(None);
    }
    row
}
