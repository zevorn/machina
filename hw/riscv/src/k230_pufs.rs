//! Minimal K230 PUFS hash model.
//!
//! The Kendryte SDK U-Boot image verification path uses PUFS DMA plus the
//! HMAC/hash block to calculate SHA-256 digests. This model implements that
//! narrow contract and keeps the rest of the security engine inert.

use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRegs;
use machina_hw_core::bus::SysBusDeviceState;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MmioOps;
use sha2::{Digest, Sha256};

pub const K230_PUFS_MMIO_SIZE: u64 = 0x0000_8000;

const DMA_VERSION: u64 = 0x00;
const DMA_FEATURE: u64 = 0x08;
const DMA_STATUS_0: u64 = 0x10;
const DMA_START: u64 = 0x20;
const DMA_CFG_0: u64 = 0x24;
const DMA_DSC_CFG_0: u64 = 0x34;
const DMA_DSC_CFG_1: u64 = 0x38;
const DMA_DSC_CFG_2: u64 = 0x3c;
const DMA_DSC_CFG_3: u64 = 0x40;
const DMA_DSC_CFG_4: u64 = 0x44;
const DMA_KEY_CFG_0: u64 = 0x6c;
const DMA_CL_CFG_0: u64 = 0x70;

const CRYPTO_BASE: u64 = 0x100;
const CRYPTO_VERSION: u64 = CRYPTO_BASE;
const CRYPTO_FEATURE: u64 = CRYPTO_BASE + 0x08;
const CRYPTO_DGST_IN: u64 = CRYPTO_BASE + 0x80;
const CRYPTO_DGST_OUT: u64 = CRYPTO_BASE + 0xc0;
const CRYPTO_DGST_LEN: usize = 64;

const HMAC_BASE: u64 = 0x800;
const HMAC_VERSION: u64 = HMAC_BASE;
const HMAC_FEATURE: u64 = HMAC_BASE + 0x08;
const HMAC_STATUS: u64 = HMAC_BASE + 0x10;
const HMAC_CFG: u64 = HMAC_BASE + 0x18;
const HMAC_PLEN: u64 = HMAC_BASE + 0x20;
const HMAC_ALEN: u64 = HMAC_BASE + 0x30;

const RT_BASE: u64 = 0x3000;
const RT_VERSION: u64 = RT_BASE + 0x2c0;
const RT_STATUS: u64 = RT_BASE + 0x2c4;
const RT_OTP: u64 = RT_BASE + 0x400;
const RT_OTP_LEN: usize = 1024;

const DMA_DSC_CFG_4_TAIL: u32 = 1 << 30;
const DMA_DSC_CFG_4_HEAD: u32 = 1 << 31;
pub const K230_PUFS_MAX_HASH_LEN: usize = 128 * 1024 * 1024;

const DMA_VERSION_VALUE: u32 = 0x5044_30d6;
const CRYPTO_VERSION_VALUE: u32 = 0x5043_30d6;
const HMAC_VERSION_VALUE: u32 = 0x5048_30d6;
const RT_VERSION_VALUE: u32 = 0x504d_30d6;
const DMA_FEATURE_SGDMA: u32 = 0x1;
const CRYPTO_FEATURE_HASH: u32 = 0x80;
const CRYPTO_FEATURE_HMAC: u32 = 0x100;
const HMAC_FEATURE_HMAC: u32 = 0x1;
const HMAC_FEATURE_SHA2: u32 = 0x2;
const HMAC_FEATURE_SM3: u32 = 0x8;

#[derive(Debug, PartialEq, Eq)]
struct K230PufsRegs {
    dma_start: u32,
    dma_cfg_0: u32,
    dsc_cfg_0: u32,
    dsc_cfg_1: u32,
    dsc_cfg_2: u32,
    dsc_cfg_3: u32,
    dsc_cfg_4: u32,
    key_cfg_0: u32,
    cl_cfg_0: u32,
    hmac_cfg: u32,
    hmac_plen: u32,
    hmac_alen: u32,
    digest_in: [u8; CRYPTO_DGST_LEN],
    digest_out: [u8; CRYPTO_DGST_LEN],
}

impl Default for K230PufsRegs {
    fn default() -> Self {
        Self {
            dma_start: 0,
            dma_cfg_0: 0,
            dsc_cfg_0: 0,
            dsc_cfg_1: 0,
            dsc_cfg_2: 0,
            dsc_cfg_3: 0,
            dsc_cfg_4: 0,
            key_cfg_0: 0,
            cl_cfg_0: 0,
            hmac_cfg: 0,
            hmac_plen: 0,
            hmac_alen: 0,
            digest_in: [0; CRYPTO_DGST_LEN],
            digest_out: [0; CRYPTO_DGST_LEN],
        }
    }
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "std")]
pub struct K230Pufs {
    state: std::sync::Mutex<SysBusDeviceState>,
    regs: DeviceRegs<K230PufsRegs>,
    hash_input: std::sync::Mutex<Vec<u8>>,
    dma_address_space: std::sync::Mutex<Option<Arc<AddressSpace>>>,
}

impl K230Pufs {
    #[must_use]
    pub fn new_named(local_id: &str) -> Arc<Self> {
        Arc::new(Self {
            state: std::sync::Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRegs::new(K230PufsRegs::default()),
            hash_input: std::sync::Mutex::new(Vec::new()),
            dma_address_space: std::sync::Mutex::new(None),
        })
    }

    pub fn set_dma_address_space(&self, address_space: Arc<AddressSpace>) {
        *self.dma_address_space.lock().unwrap() = Some(address_space);
    }

    pub fn reset_runtime(&self) {
        self.hash_input.lock().unwrap().clear();
        *self.regs.lock() = K230PufsRegs::default();
    }

    fn read_reg(&self, offset: u64, size: u32) -> u64 {
        if !valid_mmio_access(offset, size) {
            return 0;
        }

        let regs = self.regs.lock();
        match offset {
            DMA_VERSION => value_for_size(u64::from(DMA_VERSION_VALUE), size),
            DMA_FEATURE => value_for_size(u64::from(DMA_FEATURE_SGDMA), size),
            DMA_STATUS_0 => 0,
            DMA_START => value_for_size(u64::from(regs.dma_start), size),
            DMA_CFG_0 => value_for_size(u64::from(regs.dma_cfg_0), size),
            DMA_DSC_CFG_0 => value_for_size(u64::from(regs.dsc_cfg_0), size),
            DMA_DSC_CFG_1 => value_for_size(u64::from(regs.dsc_cfg_1), size),
            DMA_DSC_CFG_2 => value_for_size(u64::from(regs.dsc_cfg_2), size),
            DMA_DSC_CFG_3 => value_for_size(u64::from(regs.dsc_cfg_3), size),
            DMA_DSC_CFG_4 => value_for_size(u64::from(regs.dsc_cfg_4), size),
            DMA_KEY_CFG_0 => value_for_size(u64::from(regs.key_cfg_0), size),
            DMA_CL_CFG_0 => value_for_size(u64::from(regs.cl_cfg_0), size),
            CRYPTO_VERSION => {
                value_for_size(u64::from(CRYPTO_VERSION_VALUE), size)
            }
            CRYPTO_FEATURE => value_for_size(
                u64::from(CRYPTO_FEATURE_HASH | CRYPTO_FEATURE_HMAC),
                size,
            ),
            HMAC_VERSION => value_for_size(u64::from(HMAC_VERSION_VALUE), size),
            HMAC_FEATURE => value_for_size(
                u64::from(
                    HMAC_FEATURE_HMAC | HMAC_FEATURE_SHA2 | HMAC_FEATURE_SM3,
                ),
                size,
            ),
            HMAC_STATUS => 0,
            HMAC_CFG => value_for_size(u64::from(regs.hmac_cfg), size),
            HMAC_PLEN => value_for_size(u64::from(regs.hmac_plen), size),
            HMAC_ALEN => value_for_size(u64::from(regs.hmac_alen), size),
            RT_VERSION => value_for_size(u64::from(RT_VERSION_VALUE), size),
            RT_STATUS => 0,
            _ if in_range(offset, CRYPTO_DGST_IN, CRYPTO_DGST_LEN) => {
                read_bytes_le(&regs.digest_in, offset - CRYPTO_DGST_IN, size)
            }
            _ if in_range(offset, CRYPTO_DGST_OUT, CRYPTO_DGST_LEN) => {
                read_digest_out(
                    &regs.digest_out,
                    offset - CRYPTO_DGST_OUT,
                    size,
                )
            }
            _ if in_range(offset, RT_OTP, RT_OTP_LEN) => 0,
            _ => 0,
        }
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
                DMA_START => {
                    regs.dma_start = value;
                    start = value & 0x1 != 0;
                }
                DMA_CFG_0 => regs.dma_cfg_0 = value,
                DMA_DSC_CFG_0 => regs.dsc_cfg_0 = value,
                DMA_DSC_CFG_1 => regs.dsc_cfg_1 = value,
                DMA_DSC_CFG_2 => regs.dsc_cfg_2 = value,
                DMA_DSC_CFG_3 => regs.dsc_cfg_3 = value,
                DMA_DSC_CFG_4 => regs.dsc_cfg_4 = value,
                DMA_KEY_CFG_0 => regs.key_cfg_0 = value,
                DMA_CL_CFG_0 => regs.cl_cfg_0 = value,
                HMAC_CFG => regs.hmac_cfg = value,
                HMAC_PLEN => regs.hmac_plen = value,
                HMAC_ALEN => regs.hmac_alen = value,
                _ if in_range(offset, CRYPTO_DGST_IN, CRYPTO_DGST_LEN) => {
                    write_bytes_le(
                        &mut regs.digest_in,
                        offset - CRYPTO_DGST_IN,
                        size,
                        value,
                    );
                }
                _ => {}
            }
        }

        if start {
            self.run_hash();
        }
    }

    fn run_hash(&self) {
        let Some(address_space) =
            self.dma_address_space.lock().unwrap().clone()
        else {
            return;
        };
        let (src, len, block_cfg) = {
            let regs = self.regs.lock();
            (regs.dsc_cfg_0, regs.dsc_cfg_2, regs.dsc_cfg_4)
        };
        let len = len as usize;
        let (alen, digest) = {
            let mut input = self.hash_input.lock().unwrap();
            if block_cfg & DMA_DSC_CFG_4_HEAD != 0 {
                input.clear();
            }
            let Some(total_len) = input.len().checked_add(len) else {
                return;
            };
            if total_len > K230_PUFS_MAX_HASH_LEN {
                return;
            }
            let chunk = read_dma_bytes(&address_space, src, len);
            input.extend_from_slice(&chunk);

            let alen = input.len() as u32;
            let digest = if block_cfg & DMA_DSC_CFG_4_TAIL != 0 {
                let hash = Sha256::digest(input.as_slice());
                let mut digest = [0; 32];
                digest.copy_from_slice(&hash);
                input.clear();
                Some(digest)
            } else {
                None
            };
            (alen, digest)
        };

        let mut regs = self.regs.lock();
        regs.hmac_alen = alen;
        if let Some(digest) = digest {
            regs.digest_out.fill(0);
            regs.digest_out[..32].copy_from_slice(&digest);
        }
    }
}

pub struct K230PufsMmio(pub Arc<K230Pufs>);

impl MmioOps for K230PufsMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.read_reg(offset, size)
    }

    fn write(&self, offset: u64, size: u32, value: u64) {
        self.0.write_reg(offset, size, value);
    }
}

fn valid_mmio_access(offset: u64, size: u32) -> bool {
    matches!(size, 1 | 2 | 4) && offset.is_multiple_of(u64::from(size))
}

fn value_for_size(value: u64, size: u32) -> u64 {
    match size {
        1 => value & 0xff,
        2 => value & 0xffff,
        4 => value & 0xffff_ffff,
        _ => 0,
    }
}

fn in_range(offset: u64, base: u64, len: usize) -> bool {
    offset >= base && offset < base + len as u64
}

fn read_digest_out(
    bytes: &[u8; CRYPTO_DGST_LEN],
    offset: u64,
    size: u32,
) -> u64 {
    let index = offset as usize;
    if size == 4 && index + 4 <= bytes.len() {
        let chunk = [
            bytes[index],
            bytes[index + 1],
            bytes[index + 2],
            bytes[index + 3],
        ];
        u64::from(u32::from_be_bytes(chunk))
    } else {
        read_bytes_le(bytes, offset, size)
    }
}

fn read_bytes_le(bytes: &[u8; CRYPTO_DGST_LEN], offset: u64, size: u32) -> u64 {
    let index = offset as usize;
    let len = size as usize;
    if index + len > bytes.len() {
        return 0;
    }
    let mut value = 0;
    for (shift, byte) in bytes[index..index + len].iter().copied().enumerate() {
        value |= u64::from(byte) << (shift * 8);
    }
    value
}

fn write_bytes_le(
    bytes: &mut [u8; CRYPTO_DGST_LEN],
    offset: u64,
    size: u32,
    value: u32,
) {
    let index = offset as usize;
    let len = size as usize;
    if index + len > bytes.len() {
        return;
    }
    let value = value.to_le_bytes();
    bytes[index..index + len].copy_from_slice(&value[..len]);
}

fn read_dma_bytes(
    address_space: &AddressSpace,
    addr: u32,
    len: usize,
) -> Vec<u8> {
    (0..len)
        .map(|index| {
            address_space.read(GPA(u64::from(addr) + index as u64), 1) as u8
        })
        .collect()
}
