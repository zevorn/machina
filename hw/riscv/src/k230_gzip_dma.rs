//! Minimal K230 GSDMA and gzip decompressor model.
//!
//! The Kendryte SDK U-Boot `unzip.c` path programs two SDMA LLT chains and
//! then starts the gzip block. This model implements that narrow contract.

use std::collections::HashSet;
use std::io::{Error, ErrorKind, Read};
use std::sync::Arc;

use flate2::read::GzDecoder;
use machina_core::address::GPA;
use machina_core::device_cell::DeviceRegs;
use machina_hw_core::bus::SysBusDeviceState;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MmioOps;

pub const K230_GZIP_DMA_MMIO_SIZE: u64 = 0x0000_c000;

const GSDMA_DMA_CH_EN: u64 = 0x00;
const GSDMA_DMA_INT_MASK: u64 = 0x04;
const GSDMA_DMA_INT_STAT: u64 = 0x08;
const GSDMA_DMA_CFG: u64 = 0x0c;
const GSDMA_DMA_WEIGHT: u64 = 0x3c;

const SDMA_CH_CFG_BASE: u64 = 0x50;
const SDMA_CH_LENGTH: u64 = 0x30;
const SDMA_CHANNELS: usize = 8;
const SDMA_CH_CTL: u64 = 0x00;
const SDMA_CH_STATUS: u64 = 0x04;
const SDMA_CH_CFG: u64 = 0x08;
const SDMA_CH_USR_DATA: u64 = 0x0c;
const SDMA_CH_LLT_SADDR: u64 = 0x10;
const SDMA_CH_CURRENT_LLT: u64 = 0x14;

const UGZIP_BASE: u64 = 0x8000;
const UGZIP_DECOMP_START: u64 = UGZIP_BASE;
const UGZIP_GZIP_SRC_SIZE: u64 = UGZIP_BASE + 0x04;
const UGZIP_DMA_OUT_SIZE: u64 = UGZIP_BASE + 0x08;
const UGZIP_DECOMP_INTSTAT: u64 = UGZIP_BASE + 0x0c;

const ZIP_READ_CHANNEL: usize = 0;
const ZIP_WRITE_CHANNEL: usize = 1;
const GZIP_SIZE_VALID: u32 = 1 << 31;
const GZIP_DONE_INT: u32 = 1 << 10;
const GSDMA_WRITE_DONE_INT: u32 = 0x2;
const LLT_WORDS: usize = 6;
const LLT_SIZE: u32 = (LLT_WORDS * 4) as u32;
const MAX_GZIP_INPUT_LEN: usize = 128 * 1024 * 1024;
const MAX_GZIP_OUTPUT_LEN: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct SdmaChannel {
    ctl: u32,
    status: u32,
    cfg: u32,
    usr_data: u32,
    llt_saddr: u32,
    current_llt: u32,
}

#[derive(Debug, PartialEq, Eq)]
struct K230GzipDmaRegs {
    dma_ch_en: u32,
    dma_int_mask: u32,
    dma_int_stat: u32,
    dma_cfg: u32,
    dma_weight: u32,
    channels: [SdmaChannel; SDMA_CHANNELS],
    decomp_start: u32,
    gzip_src_size: u32,
    dma_out_size: u32,
    decomp_intstat: u32,
}

impl Default for K230GzipDmaRegs {
    fn default() -> Self {
        Self {
            dma_ch_en: 0,
            dma_int_mask: 0,
            dma_int_stat: 0,
            dma_cfg: 0,
            dma_weight: 0,
            channels: [SdmaChannel::default(); SDMA_CHANNELS],
            decomp_start: 0,
            gzip_src_size: 0,
            dma_out_size: 0,
            decomp_intstat: 0,
        }
    }
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "std")]
pub struct K230GzipDma {
    state: std::sync::Mutex<SysBusDeviceState>,
    regs: DeviceRegs<K230GzipDmaRegs>,
    dma_address_space: std::sync::Mutex<Option<Arc<AddressSpace>>>,
}

impl K230GzipDma {
    #[must_use]
    pub fn new_named(local_id: &str) -> Arc<Self> {
        Arc::new(Self {
            state: std::sync::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRegs::new(K230GzipDmaRegs::default()),
            dma_address_space: std::sync::Mutex::new(None),
        })
    }

    pub fn set_dma_address_space(&self, address_space: Arc<AddressSpace>) {
        *self.dma_address_space.lock().unwrap() = Some(address_space);
    }

    pub fn reset_runtime(&self) {
        *self.regs.lock() = K230GzipDmaRegs::default();
    }

    fn read_reg(&self, offset: u64, size: u32) -> u64 {
        if !valid_mmio_access(offset, size) {
            return 0;
        }

        let regs = self.regs.lock();
        let value = match offset {
            GSDMA_DMA_CH_EN => regs.dma_ch_en,
            GSDMA_DMA_INT_MASK => regs.dma_int_mask,
            GSDMA_DMA_INT_STAT => regs.dma_int_stat,
            GSDMA_DMA_CFG => regs.dma_cfg,
            GSDMA_DMA_WEIGHT => regs.dma_weight,
            UGZIP_DECOMP_START => regs.decomp_start,
            UGZIP_GZIP_SRC_SIZE => regs.gzip_src_size,
            UGZIP_DMA_OUT_SIZE => regs.dma_out_size,
            UGZIP_DECOMP_INTSTAT => regs.decomp_intstat,
            _ => decode_channel(offset).map_or(0, |(channel, reg)| {
                read_channel(&regs.channels[channel], reg)
            }),
        };
        value_for_size(u64::from(value), size)
    }

    fn write_reg(&self, offset: u64, size: u32, value: u64) {
        if !valid_mmio_access(offset, size) {
            return;
        }

        let value = value_for_size(value, size) as u32;
        let mut start = false;
        {
            let mut regs = self.regs.lock();
            match offset {
                GSDMA_DMA_CH_EN => regs.dma_ch_en = value,
                GSDMA_DMA_INT_MASK => regs.dma_int_mask = value,
                GSDMA_DMA_INT_STAT => regs.dma_int_stat &= !value,
                GSDMA_DMA_CFG => regs.dma_cfg = value,
                GSDMA_DMA_WEIGHT => regs.dma_weight = value,
                UGZIP_DECOMP_START => {
                    regs.decomp_start = value;
                    start = value & 0x1 != 0;
                }
                UGZIP_GZIP_SRC_SIZE => regs.gzip_src_size = value,
                UGZIP_DMA_OUT_SIZE => regs.dma_out_size = value,
                UGZIP_DECOMP_INTSTAT => regs.decomp_intstat &= !value,
                _ => {
                    if let Some((channel, reg)) = decode_channel(offset) {
                        write_channel(&mut regs.channels[channel], reg, value);
                    }
                }
            }
        }

        if start {
            self.run_decompress();
        }
    }

    fn run_decompress(&self) {
        let Some(address_space) =
            self.dma_address_space.lock().unwrap().clone()
        else {
            return;
        };
        let (src_llt, dst_llt, src_size, out_size) = {
            let regs = self.regs.lock();
            (
                regs.channels[ZIP_READ_CHANNEL].llt_saddr,
                regs.channels[ZIP_WRITE_CHANNEL].llt_saddr,
                regs.gzip_src_size & !GZIP_SIZE_VALID,
                regs.dma_out_size,
            )
        };

        let result = (|| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
            if src_size as usize > MAX_GZIP_INPUT_LEN
                || out_size as usize > MAX_GZIP_OUTPUT_LEN
            {
                return Err(invalid_llt("gzip transfer size too large"));
            }
            let mut compressed =
                read_llt_data(&address_space, src_llt, src_size as usize)?;
            if compressed.get(2).copied() == Some(0x09) {
                compressed[2] = 0x08;
            }
            let output_len = out_size as usize;
            let decoder = GzDecoder::new(compressed.as_slice());
            let mut bounded_decoder = decoder.take(out_size.into());
            let mut output = Vec::new();
            bounded_decoder.read_to_end(&mut output)?;
            output.truncate(output_len);
            write_llt_data(&address_space, dst_llt, &output)?;
            Ok(output)
        })();

        let mut regs = self.regs.lock();
        regs.dma_int_stat |= GSDMA_WRITE_DONE_INT;
        if result.is_ok() {
            regs.decomp_intstat |= GZIP_DONE_INT;
        }
    }
}

pub struct K230GzipDmaMmio(pub Arc<K230GzipDma>);

impl MmioOps for K230GzipDmaMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.read_reg(offset, size)
    }

    fn write(&self, offset: u64, size: u32, value: u64) {
        self.0.write_reg(offset, size, value);
    }
}

fn decode_channel(offset: u64) -> Option<(usize, u64)> {
    if offset < SDMA_CH_CFG_BASE {
        return None;
    }
    let local = offset - SDMA_CH_CFG_BASE;
    let channel = (local / SDMA_CH_LENGTH) as usize;
    if channel >= SDMA_CHANNELS {
        return None;
    }
    Some((channel, local % SDMA_CH_LENGTH))
}

fn read_channel(channel: &SdmaChannel, reg: u64) -> u32 {
    match reg {
        SDMA_CH_CTL => channel.ctl,
        SDMA_CH_STATUS => channel.status,
        SDMA_CH_CFG => channel.cfg,
        SDMA_CH_USR_DATA => channel.usr_data,
        SDMA_CH_LLT_SADDR => channel.llt_saddr,
        SDMA_CH_CURRENT_LLT => channel.current_llt,
        _ => 0,
    }
}

fn write_channel(channel: &mut SdmaChannel, reg: u64, value: u32) {
    match reg {
        SDMA_CH_CTL => channel.ctl = value,
        SDMA_CH_STATUS => channel.status = value,
        SDMA_CH_CFG => channel.cfg = value,
        SDMA_CH_USR_DATA => channel.usr_data = value,
        SDMA_CH_LLT_SADDR => {
            channel.llt_saddr = value;
            channel.current_llt = value;
        }
        SDMA_CH_CURRENT_LLT => channel.current_llt = value,
        _ => {}
    }
}

fn read_llt_data(
    address_space: &AddressSpace,
    mut llt_addr: u32,
    len: usize,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut data = Vec::with_capacity(len);
    let mut seen = HashSet::new();
    while llt_addr != 0 && data.len() < len {
        validate_llt_progress(&mut seen, llt_addr)?;
        let desc = read_llt(address_space, llt_addr)?;
        let remaining = len - data.len();
        let chunk_len = remaining.min(desc.line_size as usize);
        if chunk_len == 0 {
            return Err(invalid_llt("zero-length LLT descriptor"));
        }
        data.extend(read_guest_bytes(address_space, desc.src_addr, chunk_len));
        llt_addr = desc.next_llt_addr;
    }
    Ok(data)
}

fn write_llt_data(
    address_space: &AddressSpace,
    mut llt_addr: u32,
    data: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut offset = 0;
    let mut seen = HashSet::new();
    while llt_addr != 0 && offset < data.len() {
        validate_llt_progress(&mut seen, llt_addr)?;
        let desc = read_llt(address_space, llt_addr)?;
        let remaining = data.len() - offset;
        let chunk_len = remaining.min(desc.line_size as usize);
        if chunk_len == 0 {
            return Err(invalid_llt("zero-length LLT descriptor"));
        }
        write_guest_bytes(
            address_space,
            desc.dst_addr,
            &data[offset..offset + chunk_len],
        );
        offset += chunk_len;
        llt_addr = desc.next_llt_addr;
    }
    if offset < data.len() {
        return Err(invalid_llt("truncated LLT descriptor chain"));
    }
    Ok(())
}

fn validate_llt_progress(
    seen: &mut HashSet<u32>,
    llt_addr: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    if !seen.insert(llt_addr) {
        return Err(invalid_llt("cyclic LLT descriptor chain"));
    }
    Ok(())
}

fn invalid_llt(message: &'static str) -> Box<dyn std::error::Error> {
    Box::new(Error::new(ErrorKind::InvalidData, message))
}

#[derive(Clone, Copy)]
struct LltDescriptor {
    src_addr: u32,
    line_size: u32,
    dst_addr: u32,
    next_llt_addr: u32,
}

fn read_llt(
    address_space: &AddressSpace,
    address: u32,
) -> Result<LltDescriptor, Box<dyn std::error::Error>> {
    let mut words = [0u32; LLT_WORDS];
    for (index, word) in words.iter_mut().enumerate() {
        *word = address_space
            .read(GPA(u64::from(address) + index as u64 * 4), 4)
            as u32;
    }
    let _ = LLT_SIZE;
    Ok(LltDescriptor {
        src_addr: words[1],
        line_size: words[2],
        dst_addr: words[4],
        next_llt_addr: words[5],
    })
}

fn read_guest_bytes(
    address_space: &AddressSpace,
    address: u32,
    len: usize,
) -> Vec<u8> {
    (0..len)
        .map(|index| {
            address_space.read(GPA(u64::from(address) + index as u64), 1) as u8
        })
        .collect()
}

fn write_guest_bytes(address_space: &AddressSpace, address: u32, data: &[u8]) {
    for (index, byte) in data.iter().copied().enumerate() {
        address_space.write(
            GPA(u64::from(address) + index as u64),
            1,
            u64::from(byte),
        );
    }
}

fn value_for_size(value: u64, size: u32) -> u64 {
    match size {
        1 => value & 0xff,
        2 => value & 0xffff,
        4 => value & 0xffff_ffff,
        _ => 0,
    }
}

fn valid_mmio_access(offset: u64, size: u32) -> bool {
    matches!(size, 1 | 2 | 4) && offset.is_multiple_of(u64::from(size))
}
