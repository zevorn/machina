use std::sync::Arc;

use machina_core::address::GPA;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MmioOps;

pub const PL080_MMIO_SIZE: u64 = 0x1000;

const PL080_CHANNELS: usize = 8;
const CHANNEL_BASE: u64 = 0x100;
const CHANNEL_END: u64 = 0x200;
const CHANNEL_STRIDE_SHIFT: u64 = 5;
const ID_BASE: u64 = 0xfe0;

const CONF_ENABLE: u32 = 0x1;
const CCONF_HALT: u32 = 0x40000;
const CCONF_ITC: u32 = 0x08000;
const CCONF_IE: u32 = 0x04000;
const CCONF_ENABLE: u32 = 0x00001;

const CCTRL_TC_IRQ: u32 = 0x8000_0000;
const CCTRL_DST_INC: u32 = 0x0800_0000;
const CCTRL_SRC_INC: u32 = 0x0400_0000;

const PL080_ID: [u8; 8] = [0x80, 0x10, 0x04, 0x0a, 0x0d, 0xf0, 0x05, 0xb1];

#[derive(Clone, Copy, Default)]
struct Pl080Channel {
    src: u32,
    dest: u32,
    lli: u32,
    ctrl: u32,
    conf: u32,
}

struct Pl080Regs {
    tc_int: u8,
    tc_mask: u8,
    err_int: u8,
    err_mask: u8,
    conf: u32,
    sync: u32,
    channels: [Pl080Channel; PL080_CHANNELS],
    running: i32,
}

impl Default for Pl080Regs {
    fn default() -> Self {
        Self {
            tc_int: 0,
            tc_mask: 0,
            err_int: 0,
            err_mask: 0,
            conf: 0,
            sync: 0,
            channels: [Pl080Channel::default(); PL080_CHANNELS],
            running: 0,
        }
    }
}

pub struct Pl080 {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: parking_lot::Mutex<Pl080Regs>,
    dma_address_space: parking_lot::Mutex<Option<Arc<AddressSpace>>>,
    irqs: parking_lot::Mutex<Vec<Option<InterruptSource>>>,
}

impl Pl080 {
    pub fn new() -> Arc<Self> {
        Self::new_named("pl080")
    }

    pub fn new_named(local_id: &str) -> Arc<Self> {
        Arc::new(Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: parking_lot::Mutex::new(Pl080Regs::default()),
            dma_address_space: parking_lot::Mutex::new(None),
            irqs: parking_lot::Mutex::new(
                std::iter::repeat_with(|| None).take(3).collect(),
            ),
        })
    }

    machina_hw_core::machina_parking_lot_sysbus_accessors!(
        state,
        irq = manual,
        before_unrealize = lower_outputs
    );

    pub fn reset_runtime(&self) {
        *self.regs.lock() = Pl080Regs::default();
        self.lower_outputs();
    }

    pub fn set_dma_address_space(&self, address_space: Arc<AddressSpace>) {
        *self.dma_address_space.lock() = Some(address_space);
    }

    pub fn connect_irq(&self, index: usize, irq: InterruptSource) {
        let mut irqs = self.irqs.lock();
        if index < irqs.len() {
            irqs[index] = Some(irq);
        }
    }

    pub fn do_read(&self, offset: u64, size: u32) -> u64 {
        if let Some(value) = read_unaligned(self, offset, size) {
            return value;
        }

        if size == 8 {
            let lo = self.do_read(offset, 4);
            let hi = self.do_read(offset.wrapping_add(4), 4);
            return lo | (hi << 32);
        }

        if (ID_BASE..PL080_MMIO_SIZE).contains(&offset) {
            let index = ((offset - ID_BASE) >> 2) as usize;
            return access_sized(
                PL080_ID.get(index).copied().map_or(0, u64::from),
                size,
            );
        }

        let regs = self.regs.lock();
        let value = if let Some((channel, reg)) = decode_channel(offset) {
            read_channel(&regs.channels[channel], reg)
        } else {
            match offset >> 2 {
                0 => u64::from(
                    (regs.tc_int & regs.tc_mask)
                        | (regs.err_int & regs.err_mask),
                ),
                1 => u64::from(regs.tc_int & regs.tc_mask),
                3 => u64::from(regs.err_int & regs.err_mask),
                5 => u64::from(regs.tc_int),
                6 => u64::from(regs.err_int),
                7 => u64::from(enabled_channel_mask(&regs)),
                8..=11 => 0,
                12 => u64::from(regs.conf),
                13 => u64::from(regs.sync),
                _ => 0,
            }
        };
        access_sized(value, size)
    }

    pub fn do_write(&self, offset: u64, size: u32, val: u64) {
        if write_unaligned(self, offset, size, val) {
            return;
        }

        if size == 8 {
            self.do_write(offset, 4, val);
            self.do_write(offset.wrapping_add(4), 4, val >> 32);
            return;
        }

        let value = access_sized(val, size) as u32;
        let dma_address_space = self.dma_address_space.lock().clone();
        let mut update_irqs = false;
        {
            let mut regs = self.regs.lock();
            if let Some((channel, reg)) = decode_channel(offset) {
                write_channel(&mut regs.channels[channel], reg, value);
                if reg == 4 {
                    run_register_side_effects(
                        &mut regs,
                        dma_address_space.as_deref(),
                    );
                    update_irqs = true;
                }
            } else {
                match offset >> 2 {
                    2 => {
                        regs.tc_int &= !(value as u8);
                        update_irqs = true;
                    }
                    4 => {
                        regs.err_int &= !(value as u8);
                        update_irqs = true;
                    }
                    8..=11 => {}
                    12 => {
                        regs.conf = value;
                        run_register_side_effects(
                            &mut regs,
                            dma_address_space.as_deref(),
                        );
                        update_irqs = true;
                    }
                    13 => regs.sync = value,
                    _ => {}
                }
            }
        }
        if update_irqs {
            self.update_irqs();
        }
    }

    fn update_irqs(&self) {
        let regs = self.regs.lock();
        let tc_level = regs.tc_int & regs.tc_mask != 0;
        let err_level = regs.err_int & regs.err_mask != 0;
        drop(regs);

        let irqs = self.irqs.lock();
        if let Some(ref irq) = irqs[0] {
            irq.set(tc_level || err_level);
        }
        if let Some(ref irq) = irqs[1] {
            irq.set(err_level);
        }
        if let Some(ref irq) = irqs[2] {
            irq.set(tc_level);
        }
    }

    fn lower_outputs(&self) {
        let irqs = self.irqs.lock();
        for irq in irqs.iter().flatten() {
            irq.lower();
        }
    }
}

fn decode_channel(offset: u64) -> Option<(usize, u64)> {
    if !(CHANNEL_BASE..CHANNEL_END).contains(&offset) {
        return None;
    }
    let channel = ((offset & 0xe0) >> CHANNEL_STRIDE_SHIFT) as usize;
    if channel >= PL080_CHANNELS {
        return None;
    }
    Some((channel, (offset >> 2) & 7))
}

fn read_unaligned(dev: &Pl080, offset: u64, size: u32) -> Option<u64> {
    if !needs_unaligned_split(offset, size) {
        return None;
    }

    let mut value = 0u64;
    let mut done = 0u32;
    while done < size {
        let cur = offset + u64::from(done);
        let chunk = aligned_chunk_size(cur, size - done);
        let chunk_value = dev.do_read(cur, chunk);
        value |= access_sized(chunk_value, chunk) << (done * 8);
        done += chunk;
    }
    Some(value)
}

fn write_unaligned(dev: &Pl080, offset: u64, size: u32, val: u64) -> bool {
    if !needs_unaligned_split(offset, size) {
        return false;
    }

    let mut done = 0u32;
    while done < size {
        let cur = offset + u64::from(done);
        let chunk = aligned_chunk_size(cur, size - done);
        let chunk_value = access_sized(val >> (done * 8), chunk);
        dev.do_write(cur, chunk, chunk_value);
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

fn access_sized(value: u64, size: u32) -> u64 {
    match size {
        1 => value & 0xff,
        2 => value & 0xffff,
        4 => value & 0xffff_ffff,
        _ => value,
    }
}

fn read_channel(channel: &Pl080Channel, reg: u64) -> u64 {
    match reg {
        0 => u64::from(channel.src),
        1 => u64::from(channel.dest),
        2 => u64::from(channel.lli),
        3 => u64::from(channel.ctrl),
        4 => u64::from(channel.conf),
        _ => 0,
    }
}

fn write_channel(channel: &mut Pl080Channel, reg: u64, value: u32) {
    match reg {
        0 => channel.src = value,
        1 => channel.dest = value,
        2 => channel.lli = value,
        3 => channel.ctrl = value,
        4 => channel.conf = value,
        _ => {}
    }
}

fn enabled_channel_mask(regs: &Pl080Regs) -> u32 {
    let mut mask = 0;
    for (index, channel) in regs.channels.iter().enumerate() {
        if channel.conf & CONF_ENABLE != 0 {
            mask |= 1 << index;
        }
    }
    mask
}

fn run_register_side_effects(
    regs: &mut Pl080Regs,
    dma_address_space: Option<&AddressSpace>,
) {
    regs.tc_mask = 0;
    for (index, channel) in regs.channels.iter().enumerate() {
        let bit = 1 << index;
        if channel.conf & CCONF_ITC != 0 {
            regs.tc_mask |= bit;
        }
        if channel.conf & CCONF_IE != 0 {
            regs.err_mask |= bit;
        }
    }
    if regs.conf & CONF_ENABLE == 0 || regs.running != 0 {
        return;
    }
    regs.running = 1;
    for index in 0..PL080_CHANNELS {
        run_channel(regs, index, dma_address_space);
    }
    regs.running = 0;
}

fn run_channel(
    regs: &mut Pl080Regs,
    index: usize,
    dma_address_space: Option<&AddressSpace>,
) {
    loop {
        let mut set_terminal_count = false;
        let mut follow_linked_list = false;

        {
            let channel = &mut regs.channels[index];
            if channel.conf & (CCONF_HALT | CCONF_ENABLE) != CCONF_ENABLE {
                return;
            }

            let flow = (channel.conf >> 11) & 7;
            if flow != 0 {
                return;
            }

            let swidth = 1u32 << ((channel.ctrl >> 18) & 7);
            let dwidth = 1u32 << ((channel.ctrl >> 21) & 7);
            if swidth > 4 || dwidth > 4 {
                return;
            }

            let mut size = channel.ctrl & 0x0fff;
            if size == 0 || !(size * swidth).is_multiple_of(dwidth) {
                return;
            }

            let xsize = swidth.max(dwidth);
            while size != 0 {
                let mut buffer = [0u8; 4];
                let mut offset = 0;
                while offset < xsize {
                    read_dma_bytes(
                        dma_address_space,
                        channel.src,
                        swidth,
                        &mut buffer[offset as usize..],
                    );
                    if channel.ctrl & CCTRL_SRC_INC != 0 {
                        channel.src = channel.src.wrapping_add(swidth);
                    }
                    offset += swidth;
                }

                offset = 0;
                while offset < xsize {
                    write_dma_bytes(
                        dma_address_space,
                        channel.dest,
                        dwidth,
                        &buffer[offset as usize..],
                    );
                    if channel.ctrl & CCTRL_DST_INC != 0 {
                        channel.dest = channel.dest.wrapping_add(dwidth);
                    }
                    offset += dwidth;
                }

                size -= xsize / swidth;
                channel.ctrl = (channel.ctrl & 0xffff_f000) | size;
            }

            let next_lli = channel.lli & !3;
            if next_lli != 0 {
                load_linked_list_item(dma_address_space, channel, next_lli);
                follow_linked_list = true;
            } else {
                channel.conf &= !CCONF_ENABLE;
            }

            if channel.ctrl & CCTRL_TC_IRQ != 0 {
                set_terminal_count = true;
            }
        }

        if set_terminal_count {
            regs.tc_int |= 1 << index;
        }

        if !follow_linked_list {
            return;
        }
    }
}

fn load_linked_list_item(
    dma_address_space: Option<&AddressSpace>,
    channel: &mut Pl080Channel,
    address: u32,
) {
    channel.src = read_dma_u32(dma_address_space, address);
    channel.dest = read_dma_u32(dma_address_space, address.wrapping_add(4));
    channel.lli = read_dma_u32(dma_address_space, address.wrapping_add(8));
    channel.ctrl = read_dma_u32(dma_address_space, address.wrapping_add(12));
}

fn read_dma_u32(dma_address_space: Option<&AddressSpace>, address: u32) -> u32 {
    dma_address_space.map_or(0, |address_space| {
        address_space.read(GPA(u64::from(address)), 4) as u32
    })
}

fn read_dma_bytes(
    dma_address_space: Option<&AddressSpace>,
    address: u32,
    width: u32,
    output: &mut [u8],
) {
    let Some(address_space) = dma_address_space else {
        return;
    };
    let value = address_space.read(GPA(u64::from(address)), width);
    output[..width as usize]
        .copy_from_slice(&value.to_le_bytes()[..width as usize]);
}

fn write_dma_bytes(
    dma_address_space: Option<&AddressSpace>,
    address: u32,
    width: u32,
    input: &[u8],
) {
    let Some(address_space) = dma_address_space else {
        return;
    };
    let mut bytes = [0u8; 8];
    bytes[..width as usize].copy_from_slice(&input[..width as usize]);
    address_space.write(
        GPA(u64::from(address)),
        width,
        u64::from_le_bytes(bytes),
    );
}

pub struct Pl080Mmio(pub Arc<Pl080>);

impl MmioOps for Pl080Mmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.do_read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.do_write(offset, size, val);
    }
}
