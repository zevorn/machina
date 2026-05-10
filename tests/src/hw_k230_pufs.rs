use std::sync::Arc;

use machina_core::address::GPA;
use machina_hw_riscv::k230_pufs::{K230Pufs, K230PufsMmio};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};
use sha2::{Digest, Sha256};

const DMA_START: u64 = 0x20;
const DMA_DSC_CFG_0: u64 = 0x34;
const DMA_DSC_CFG_2: u64 = 0x3c;
const DMA_DSC_CFG_4: u64 = 0x44;
const CRYPTO_DGST_OUT: u64 = 0x1c0;
const HMAC_CFG: u64 = 0x818;
const HMAC_PLEN: u64 = 0x820;
const HMAC_ALEN: u64 = 0x830;

const DMA_DSC_CFG_4_TAIL: u64 = 1 << 30;
const DMA_DSC_CFG_4_HEAD: u64 = 1 << 31;

fn make_ram_aspace(size: u64) -> Arc<AddressSpace> {
    let mut root = MemoryRegion::container("root", size);
    let (ram, _block) = MemoryRegion::ram("ram", size);
    root.add_subregion(ram, GPA(0));
    Arc::new(AddressSpace::new(root))
}

fn write_bytes(aspace: &AddressSpace, addr: u64, data: &[u8]) {
    for (index, byte) in data.iter().copied().enumerate() {
        aspace.write(GPA(addr + index as u64), 1, u64::from(byte));
    }
}

fn uboot_digest(mmio: &K230PufsMmio) -> [u8; 32] {
    let mut out = [0u8; 32];
    for index in 0..8 {
        let word = mmio.read(CRYPTO_DGST_OUT + index * 4, 4) as u32;
        let swapped = word.swap_bytes();
        out[index as usize * 4..index as usize * 4 + 4]
            .copy_from_slice(&swapped.to_le_bytes());
    }
    out
}

fn run_hash_chunk(
    mmio: &K230PufsMmio,
    addr: u64,
    len: usize,
    plen: u64,
    block_cfg: u64,
) {
    mmio.write(DMA_DSC_CFG_0, 4, addr);
    mmio.write(DMA_DSC_CFG_2, 4, len as u64);
    mmio.write(DMA_DSC_CFG_4, 4, block_cfg);
    mmio.write(HMAC_CFG, 4, 0x03);
    mmio.write(HMAC_PLEN, 4, plen);
    mmio.write(DMA_START, 4, 1);
}

#[test]
fn k230_pufs_hashes_single_sha256_dma_segment() {
    let payload = b"abc";
    let aspace = make_ram_aspace(0x1000);
    write_bytes(&aspace, 0x100, payload);

    let dev = K230Pufs::new_named("k230-pufs");
    dev.set_dma_address_space(aspace);
    let mmio = K230PufsMmio(dev);

    run_hash_chunk(
        &mmio,
        0x100,
        payload.len(),
        0,
        DMA_DSC_CFG_4_HEAD | DMA_DSC_CFG_4_TAIL,
    );

    let expected = Sha256::digest(payload);
    assert_eq!(uboot_digest(&mmio).as_slice(), expected.as_slice());
}

#[test]
fn k230_pufs_hashes_chunked_sha256_dma_segments() {
    let first = b"k230 ";
    let second = b"pufs hash";
    let aspace = make_ram_aspace(0x1000);
    write_bytes(&aspace, 0x100, first);
    write_bytes(&aspace, 0x200, second);

    let dev = K230Pufs::new_named("k230-pufs");
    dev.set_dma_address_space(aspace);
    let mmio = K230PufsMmio(dev);

    run_hash_chunk(&mmio, 0x100, first.len(), 0, DMA_DSC_CFG_4_HEAD);
    assert_eq!(mmio.read(HMAC_ALEN, 4), first.len() as u64);
    run_hash_chunk(
        &mmio,
        0x200,
        second.len(),
        first.len() as u64,
        DMA_DSC_CFG_4_TAIL,
    );

    let mut expected_input = Vec::new();
    expected_input.extend_from_slice(first);
    expected_input.extend_from_slice(second);
    let expected = Sha256::digest(&expected_input);
    assert_eq!(uboot_digest(&mmio).as_slice(), expected.as_slice());
}
