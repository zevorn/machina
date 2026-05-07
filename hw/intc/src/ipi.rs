use std::sync::Arc;

use machina_core::device_cell::DeviceRefCell;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_memory::region::MmioOps;

const CORE_STATUS_OFF: u64 = 0x000;
const CORE_EN_OFF: u64 = 0x004;
const CORE_SET_OFF: u64 = 0x008;
const CORE_CLEAR_OFF: u64 = 0x00c;
const CORE_BUF_BASE: u64 = 0x020;
const IOCSR_IPI_SEND: u64 = 0x040;
const IOCSR_MAIL_SEND: u64 = 0x048;
const IOCSR_ANY_SEND: u64 = 0x158;
const NUM_MAILBOX: usize = 4;

#[derive(Clone, Copy)]
struct IpiCore {
    status: u32,
    enable: u32,
    mailbox: [u64; NUM_MAILBOX],
}

impl IpiCore {
    fn new() -> Self {
        Self {
            status: 0,
            enable: 0,
            mailbox: [0; NUM_MAILBOX],
        }
    }
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot", before_unrealize = lower_outputs)]
pub struct LoongArchIpi {
    state: parking_lot::Mutex<SysBusDeviceState>,
    cores: DeviceRefCell<Vec<IpiCore>>,
    outputs: parking_lot::Mutex<Vec<Option<InterruptSource>>>,
}

impl LoongArchIpi {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("loongarch_ipi", 1)
    }

    #[must_use]
    pub fn new_named(local_id: &str, num_cpus: u32) -> Self {
        let count = num_cpus.max(1) as usize;
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            cores: DeviceRefCell::new(vec![IpiCore::new(); count]),
            outputs: parking_lot::Mutex::new(empty_outputs(count)),
        }
    }

    pub fn connect_output(&self, cpu_id: u32, irq: InterruptSource) {
        let cpu_id = cpu_id as usize;
        let mut outputs = self.outputs.lock();
        while outputs.len() <= cpu_id {
            outputs.push(None);
        }
        outputs[cpu_id] = Some(irq);
        drop(outputs);
        self.update_outputs();
    }

    #[must_use]
    pub fn mmio_read(&self, cpu_id: u32, offset: u64) -> u64 {
        self.mmio_read_sized(cpu_id, offset, 4)
    }

    pub fn mmio_write(&self, cpu_id: u32, offset: u64, val: u64) {
        self.mmio_write_sized(cpu_id, offset, 4, val);
    }

    #[must_use]
    pub fn mmio_read_sized(&self, cpu_id: u32, offset: u64, size: u32) -> u64 {
        if !valid_iocsr_size(size) {
            return 0;
        }
        let cores = self.cores.borrow();
        let Some(core) = cores.get(cpu_id as usize) else {
            return 0;
        };
        let offset = normalize_iocsr_offset(offset);
        let mut val = 0u64;
        for byte in 0..size {
            val |= u64::from(read_core_byte(core, offset + u64::from(byte)))
                << (byte * 8);
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
        if !valid_iocsr_size(size) {
            return;
        }
        let offset = normalize_iocsr_offset(offset);
        let mut needs_update = false;
        {
            let mut cores = self.cores.borrow();
            // IPI_SEND works with 4/8-byte access;
            // MAIL_SEND and ANY_SEND are 8-byte only.
            let send_access = ((offset == IOCSR_MAIL_SEND
                || offset == IOCSR_ANY_SEND)
                && size == 8)
                || (offset == IOCSR_IPI_SEND && (size == 4 || size == 8));
            if send_access {
                needs_update |= write_send_register(&mut cores, offset, val);
            } else if let Some(core) = cores.get_mut(cpu_id as usize) {
                for byte in 0..size {
                    needs_update |= write_core_byte(
                        core,
                        offset + u64::from(byte),
                        ((val >> (byte * 8)) & 0xff) as u8,
                    );
                }
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
        let cores = self.cores.borrow();
        let outputs = self.outputs.lock();
        for (cpu_id, line) in outputs.iter().enumerate() {
            if let Some(line) = line {
                let level = cores
                    .get(cpu_id)
                    .is_some_and(|core| core.status & core.enable != 0);
                line.set(level);
            }
        }
    }
}

impl Default for LoongArchIpi {
    fn default() -> Self {
        Self::new()
    }
}

pub struct LoongArchIpiMmio(pub Arc<LoongArchIpi>, pub u32);

impl MmioOps for LoongArchIpiMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.mmio_read_sized(self.1, offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.mmio_write_sized(self.1, offset, size, val);
    }
}

fn read_core_byte(core: &IpiCore, offset: u64) -> u8 {
    match offset {
        CORE_STATUS_OFF..=0x007 => {
            let combined =
                u64::from(core.status) | (u64::from(core.enable) << 32);
            read_u64_byte(combined, (offset - CORE_STATUS_OFF) as usize)
        }
        CORE_SET_OFF..=0x00f => 0,
        CORE_BUF_BASE..=0x03f => {
            let rel = (offset - CORE_BUF_BASE) as usize;
            read_u64_byte(core.mailbox[rel / 8], rel % 8)
        }
        _ => 0,
    }
}

fn write_core_byte(core: &mut IpiCore, offset: u64, val: u8) -> bool {
    match offset {
        CORE_STATUS_OFF..=0x003 => false,
        CORE_EN_OFF..=0x007 => {
            write_u32_byte(
                &mut core.enable,
                (offset - CORE_EN_OFF) as usize,
                val,
            );
            // Enable writes just store the value without
            // triggering immediate output recomputation.
            false
        }
        CORE_SET_OFF..=0x00b => {
            core.status |= u32::from(val) << ((offset - CORE_SET_OFF) * 8);
            true
        }
        CORE_CLEAR_OFF..=0x00f => {
            core.status &= !(u32::from(val) << ((offset - CORE_CLEAR_OFF) * 8));
            true
        }
        CORE_BUF_BASE..=0x03f => {
            let rel = (offset - CORE_BUF_BASE) as usize;
            write_u64_byte(&mut core.mailbox[rel / 8], rel % 8, val);
            false
        }
        _ => false,
    }
}

fn write_send_register(cores: &mut [IpiCore], offset: u64, val: u64) -> bool {
    match offset {
        IOCSR_IPI_SEND => write_ipi_send(cores, val),
        IOCSR_MAIL_SEND => {
            let target = target_cpu(val);
            let dest = CORE_BUF_BASE + (val & 0x1c);
            write_ipi_data(cores, target, dest, val)
        }
        IOCSR_ANY_SEND => {
            let target = target_cpu(val);
            let dest = normalize_iocsr_offset(val & 0xffff);
            write_ipi_data(cores, target, dest, val)
        }
        _ => false,
    }
}

fn write_ipi_send(cores: &mut [IpiCore], val: u64) -> bool {
    let target = target_cpu(val);
    let vector = (val & 0x1f) as u32;
    if let Some(core) = cores.get_mut(target) {
        core.status |= 1u32 << vector;
        true
    } else {
        false
    }
}

fn write_ipi_data(
    cores: &mut [IpiCore],
    target: usize,
    dest: u64,
    val: u64,
) -> bool {
    if target >= cores.len() {
        return false;
    }

    let data = (val >> 32) as u32;
    let byte_mask = ((val >> 27) & 0xf) as u32;
    let old = read_core_word(&cores[target], dest);
    let mut merged = data;
    for byte in 0..4 {
        if byte_mask & (1 << byte) != 0 {
            let mask = 0xffu32 << (byte * 8);
            merged = (merged & !mask) | (old & mask);
        }
    }
    write_core_word(cores, target, dest, merged)
}

fn read_core_word(core: &IpiCore, dest: u64) -> u32 {
    let dest = normalize_iocsr_offset(dest);
    match dest {
        CORE_STATUS_OFF => core.status,
        CORE_EN_OFF => core.enable,
        CORE_BUF_BASE..=0x03c => {
            let rel = (dest - CORE_BUF_BASE) as usize;
            let mailbox = core.mailbox[rel / 8];
            if rel % 8 >= 4 {
                (mailbox >> 32) as u32
            } else {
                mailbox as u32
            }
        }
        _ => 0,
    }
}

fn write_core_word(
    cores: &mut [IpiCore],
    target: usize,
    dest: u64,
    val: u32,
) -> bool {
    let dest = normalize_iocsr_offset(dest);
    match dest {
        CORE_STATUS_OFF => false,
        CORE_EN_OFF => {
            cores[target].enable = val;
            // Enable writes just store, without triggering recalc.
            false
        }
        CORE_SET_OFF => {
            cores[target].status |= val;
            true
        }
        CORE_CLEAR_OFF => {
            cores[target].status &= !val;
            true
        }
        CORE_BUF_BASE..=0x03c => {
            let rel = (dest - CORE_BUF_BASE) as usize;
            let mailbox = &mut cores[target].mailbox[rel / 8];
            if rel % 8 >= 4 {
                *mailbox = (*mailbox & 0xffff_ffff) | (u64::from(val) << 32);
            } else {
                *mailbox = (*mailbox & !0xffff_ffff) | u64::from(val);
            }
            false
        }
        IOCSR_IPI_SEND => write_ipi_send(cores, u64::from(val)),
        _ => false,
    }
}

fn target_cpu(val: u64) -> usize {
    ((val >> 16) & 0x3ff) as usize
}

fn normalize_iocsr_offset(offset: u64) -> u64 {
    offset & 0xfff
}

fn read_u64_byte(word: u64, byte: usize) -> u8 {
    ((word >> (byte * 8)) & 0xff) as u8
}

fn write_u64_byte(word: &mut u64, byte: usize, val: u8) {
    let shift = byte * 8;
    let mask = 0xffu64 << shift;
    *word = (*word & !mask) | (u64::from(val) << shift);
}

fn write_u32_byte(word: &mut u32, byte: usize, val: u8) {
    let shift = byte * 8;
    let mask = 0xffu32 << shift;
    *word = (*word & !mask) | (u32::from(val) << shift);
}

fn valid_iocsr_size(size: u32) -> bool {
    matches!(size, 4 | 8)
}

fn empty_outputs(num_cpus: usize) -> Vec<Option<InterruptSource>> {
    let mut outputs = Vec::with_capacity(num_cpus);
    outputs.resize_with(num_cpus, || None);
    outputs
}
