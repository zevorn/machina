use std::sync::Arc;

use machina_core::address::GPA;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MmioOps;

pub const SIFIVE_PDMA_REG_SIZE: u64 = 0x10_0000;

const CHANNELS: usize = 4;
const IRQ_COUNT: usize = CHANNELS * 2;

const DMA_CONTROL: u64 = 0x000;
const DMA_NEXT_CONFIG: u64 = 0x004;
const DMA_NEXT_BYTES: u64 = 0x008;
const DMA_NEXT_BYTES_HI: u64 = DMA_NEXT_BYTES + 4;
const DMA_NEXT_DST: u64 = 0x010;
const DMA_NEXT_DST_HI: u64 = DMA_NEXT_DST + 4;
const DMA_NEXT_SRC: u64 = 0x018;
const DMA_NEXT_SRC_HI: u64 = DMA_NEXT_SRC + 4;
const DMA_EXEC_CONFIG: u64 = 0x104;
const DMA_EXEC_BYTES: u64 = 0x108;
const DMA_EXEC_BYTES_HI: u64 = DMA_EXEC_BYTES + 4;
const DMA_EXEC_DST: u64 = 0x110;
const DMA_EXEC_DST_HI: u64 = DMA_EXEC_DST + 4;
const DMA_EXEC_SRC: u64 = 0x118;
const DMA_EXEC_SRC_HI: u64 = DMA_EXEC_SRC + 4;

const CONTROL_CLAIM: u32 = 1 << 0;
const CONTROL_RUN: u32 = 1 << 1;
const CONTROL_DONE_IE: u32 = 1 << 14;
const CONTROL_ERR_IE: u32 = 1 << 15;
const CONTROL_DONE: u32 = 1 << 30;
const CONTROL_ERR: u32 = 1 << 31;

const CONFIG_REPEAT: u32 = 1 << 2;
const CONFIG_WRSZ_SHIFT: u32 = 24;
const CONFIG_RDSZ_SHIFT: u32 = 28;
const CONFIG_SZ_MASK: u32 = 0xf;
const CONFIG_WRSZ_DEFAULT: u32 = 6;
const CONFIG_RDSZ_DEFAULT: u32 = 6;
const CONFIG_DEFAULT: u32 = (CONFIG_RDSZ_DEFAULT << CONFIG_RDSZ_SHIFT)
    | (CONFIG_WRSZ_DEFAULT << CONFIG_WRSZ_SHIFT);

#[derive(Clone, Copy, Default)]
struct SifivePdmaChannel {
    control: u32,
    next_config: u32,
    next_bytes: u64,
    next_dst: u64,
    next_src: u64,
    exec_config: u32,
    exec_bytes: u64,
    exec_dst: u64,
    exec_src: u64,
}

struct SifivePdmaRegs {
    channels: [SifivePdmaChannel; CHANNELS],
}

impl Default for SifivePdmaRegs {
    fn default() -> Self {
        Self {
            channels: [SifivePdmaChannel::default(); CHANNELS],
        }
    }
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot", irq = "manual", before_unrealize = lower_outputs)]
pub struct SifivePdma {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: parking_lot::Mutex<SifivePdmaRegs>,
    irqs: parking_lot::Mutex<Vec<Option<InterruptSource>>>,
    dma_address_space: parking_lot::Mutex<Option<Arc<AddressSpace>>>,
}

impl SifivePdma {
    pub fn new() -> Arc<Self> {
        Self::new_named("sifive-pdma")
    }

    pub fn new_named(local_id: &str) -> Arc<Self> {
        Arc::new(Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: parking_lot::Mutex::new(SifivePdmaRegs::default()),
            irqs: parking_lot::Mutex::new(
                std::iter::repeat_with(|| None).take(IRQ_COUNT).collect(),
            ),
            dma_address_space: parking_lot::Mutex::new(None),
        })
    }

    pub fn reset_runtime(&self) {
        *self.regs.lock() = SifivePdmaRegs::default();
        self.lower_outputs();
    }

    pub fn connect_irq(&self, index: usize, irq: InterruptSource) {
        let mut irqs = self.irqs.lock();
        if index < irqs.len() {
            irqs[index] = Some(irq);
        }
    }

    pub fn set_dma_address_space(&self, address_space: Arc<AddressSpace>) {
        *self.dma_address_space.lock() = Some(address_space);
    }

    pub fn do_read(&self, offset: u64, size: u32) -> u64 {
        if let Some(value) = self.read_unaligned(offset, size) {
            return value;
        }

        let Some(channel) = decode_channel(offset) else {
            return 0;
        };
        let regs = self.regs.lock();
        let channel = &regs.channels[channel];
        match size {
            8 => readq(channel, offset),
            4 => u64::from(readl(channel, offset)),
            _ => 0,
        }
    }

    pub fn do_write(&self, offset: u64, size: u32, val: u64) {
        if self.write_unaligned(offset, size, val) {
            return;
        }

        let Some(channel_index) = decode_channel(offset) else {
            return;
        };

        let dma_address_space = self.dma_address_space.lock().clone();
        let mut should_update_irq = false;
        {
            let mut regs = self.regs.lock();
            let channel = &mut regs.channels[channel_index];
            match size {
                8 => writeq(channel, offset, val),
                4 => {
                    should_update_irq = writel(
                        channel,
                        offset,
                        val as u32,
                        dma_address_space.as_deref(),
                    );
                }
                _ => {}
            }
        }

        if should_update_irq {
            self.update_irq(channel_index);
        }
    }

    fn read_unaligned(&self, offset: u64, size: u32) -> Option<u64> {
        if !needs_unaligned_split(offset, size) {
            return None;
        }

        let mut value = 0u64;
        let mut done = 0u32;
        while done < size {
            let cur = offset + u64::from(done);
            let chunk = aligned_chunk_size(cur, size - done);
            value |=
                (self.do_read(cur, chunk) & access_mask(chunk)) << (done * 8);
            done += chunk;
        }
        Some(value)
    }

    fn write_unaligned(&self, offset: u64, size: u32, val: u64) -> bool {
        if !needs_unaligned_split(offset, size) {
            return false;
        }

        let mut done = 0u32;
        while done < size {
            let cur = offset + u64::from(done);
            let chunk = aligned_chunk_size(cur, size - done);
            let chunk_value = (val >> (done * 8)) & access_mask(chunk);
            self.do_write(cur, chunk, chunk_value);
            done += chunk;
        }
        true
    }

    fn update_irq(&self, channel: usize) {
        let regs = self.regs.lock();
        let control = regs.channels[channel].control;
        let done =
            control & CONTROL_DONE_IE != 0 && control & CONTROL_DONE != 0;
        let err = control & CONTROL_ERR_IE != 0 && control & CONTROL_ERR != 0;
        drop(regs);

        let irqs = self.irqs.lock();
        if let Some(ref irq) = irqs[channel * 2] {
            irq.set(done);
        }
        if let Some(ref irq) = irqs[channel * 2 + 1] {
            irq.set(err);
        }
    }

    fn lower_outputs(&self) {
        let irqs = self.irqs.lock();
        for irq in irqs.iter().flatten() {
            irq.lower();
        }
    }
}

fn decode_channel(offset: u64) -> Option<usize> {
    if offset >= SIFIVE_PDMA_REG_SIZE {
        return None;
    }
    let channel = ((offset & (SIFIVE_PDMA_REG_SIZE - 1)) >> 12) as usize;
    (channel < CHANNELS).then_some(channel)
}

fn register_offset(offset: u64) -> u64 {
    offset & 0xfff
}

fn access_mask(size: u32) -> u64 {
    match size {
        1 => 0xff,
        2 => 0xffff,
        4 => 0xffff_ffff,
        _ => u64::MAX,
    }
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

fn readq(channel: &SifivePdmaChannel, offset: u64) -> u64 {
    match register_offset(offset) {
        DMA_NEXT_BYTES => channel.next_bytes,
        DMA_NEXT_DST => channel.next_dst,
        DMA_NEXT_SRC => channel.next_src,
        DMA_EXEC_BYTES => channel.exec_bytes,
        DMA_EXEC_DST => channel.exec_dst,
        DMA_EXEC_SRC => channel.exec_src,
        _ => 0,
    }
}

fn readl(channel: &SifivePdmaChannel, offset: u64) -> u32 {
    match register_offset(offset) {
        DMA_CONTROL => channel.control,
        DMA_NEXT_CONFIG => channel.next_config,
        DMA_NEXT_BYTES => low32(channel.next_bytes),
        DMA_NEXT_BYTES_HI => high32(channel.next_bytes),
        DMA_NEXT_DST => low32(channel.next_dst),
        DMA_NEXT_DST_HI => high32(channel.next_dst),
        DMA_NEXT_SRC => low32(channel.next_src),
        DMA_NEXT_SRC_HI => high32(channel.next_src),
        DMA_EXEC_CONFIG => channel.exec_config,
        DMA_EXEC_BYTES => low32(channel.exec_bytes),
        DMA_EXEC_BYTES_HI => high32(channel.exec_bytes),
        DMA_EXEC_DST => low32(channel.exec_dst),
        DMA_EXEC_DST_HI => high32(channel.exec_dst),
        DMA_EXEC_SRC => low32(channel.exec_src),
        DMA_EXEC_SRC_HI => high32(channel.exec_src),
        _ => 0,
    }
}

fn writeq(channel: &mut SifivePdmaChannel, offset: u64, value: u64) {
    match register_offset(offset) {
        DMA_NEXT_BYTES => channel.next_bytes = value,
        DMA_NEXT_DST => channel.next_dst = value,
        DMA_NEXT_SRC => channel.next_src = value,
        DMA_EXEC_BYTES | DMA_EXEC_DST | DMA_EXEC_SRC => {}
        _ => {}
    }
}

fn writel(
    channel: &mut SifivePdmaChannel,
    offset: u64,
    value: u32,
    dma_address_space: Option<&AddressSpace>,
) -> bool {
    match register_offset(offset) {
        DMA_CONTROL => write_control(channel, value, dma_address_space),
        DMA_NEXT_CONFIG => {
            channel.next_config = value;
            false
        }
        DMA_NEXT_BYTES => {
            set_low32(&mut channel.next_bytes, value);
            false
        }
        DMA_NEXT_BYTES_HI => {
            set_high32(&mut channel.next_bytes, value);
            false
        }
        DMA_NEXT_DST => {
            set_low32(&mut channel.next_dst, value);
            false
        }
        DMA_NEXT_DST_HI => {
            set_high32(&mut channel.next_dst, value);
            false
        }
        DMA_NEXT_SRC => {
            set_low32(&mut channel.next_src, value);
            false
        }
        DMA_NEXT_SRC_HI => {
            set_high32(&mut channel.next_src, value);
            false
        }
        DMA_EXEC_CONFIG | DMA_EXEC_BYTES | DMA_EXEC_BYTES_HI | DMA_EXEC_DST
        | DMA_EXEC_DST_HI | DMA_EXEC_SRC | DMA_EXEC_SRC_HI => false,
        _ => false,
    }
}

fn write_control(
    channel: &mut SifivePdmaChannel,
    mut value: u32,
    dma_address_space: Option<&AddressSpace>,
) -> bool {
    let claimed = channel.control & CONTROL_CLAIM != 0;
    let run = channel.control & CONTROL_RUN != 0;

    if !claimed && value & CONTROL_CLAIM != 0 {
        channel.next_config = CONFIG_DEFAULT;
        channel.next_bytes = 0;
        channel.next_dst = 0;
        channel.next_src = 0;
    }

    if run && value & CONTROL_CLAIM == 0 {
        value |= CONTROL_CLAIM;
    }

    channel.control = value;

    if !claimed || (!run && value & CONTROL_CLAIM == 0) {
        channel.control &= !CONTROL_RUN;
        return false;
    }

    if value & CONTROL_RUN != 0 {
        run_channel(channel, dma_address_space);
    }

    true
}

fn run_channel(
    channel: &mut SifivePdmaChannel,
    dma_address_space: Option<&AddressSpace>,
) {
    let bytes = channel.next_bytes;
    let dst = channel.next_dst;
    let src = channel.next_src;
    let config = channel.next_config;

    if bytes == 0 {
        mark_done(channel);
        return;
    }

    let wsize = (config >> CONFIG_WRSZ_SHIFT) & CONFIG_SZ_MASK;
    let rsize = (config >> CONFIG_RDSZ_SHIFT) & CONFIG_SZ_MASK;
    if wsize != rsize {
        channel.control |= CONTROL_ERR | CONTROL_DONE;
        return;
    }

    channel.exec_config = config;
    channel.exec_bytes = bytes;
    channel.exec_dst = dst;
    channel.exec_src = src;
    channel.control &= !(CONTROL_DONE | CONTROL_ERR);

    if let Some(address_space) = dma_address_space {
        for _ in 0..bytes {
            let byte = address_space.read(GPA(channel.exec_src), 1);
            address_space.write(GPA(channel.exec_dst), 1, byte);
            channel.exec_src = channel.exec_src.saturating_add(1);
            channel.exec_dst = channel.exec_dst.saturating_add(1);
            channel.exec_bytes = channel.exec_bytes.saturating_sub(1);
        }
    } else {
        channel.exec_src = src.saturating_add(bytes);
        channel.exec_dst = dst.saturating_add(bytes);
        channel.exec_bytes = 0;
    }

    if config & CONFIG_REPEAT != 0 {
        channel.exec_bytes = bytes;
        channel.exec_dst = dst;
        channel.exec_src = src;
    }

    mark_done(channel);
}

fn mark_done(channel: &mut SifivePdmaChannel) {
    channel.control &= !CONTROL_RUN;
    channel.control |= CONTROL_DONE;
}

fn low32(value: u64) -> u32 {
    value as u32
}

fn high32(value: u64) -> u32 {
    (value >> 32) as u32
}

fn set_low32(target: &mut u64, value: u32) {
    *target = (*target & 0xffff_ffff_0000_0000) | u64::from(value);
}

fn set_high32(target: &mut u64, value: u32) {
    *target = (*target & 0x0000_0000_ffff_ffff) | (u64::from(value) << 32);
}

pub struct SifivePdmaMmio(pub Arc<SifivePdma>);

impl MmioOps for SifivePdmaMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.do_read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.do_write(offset, size, val);
    }
}
