use std::io::Write;
use std::sync::Arc;

use flate2::write::GzEncoder;
use flate2::Compression;
use machina_core::address::GPA;
use machina_hw_riscv::k230_gzip_dma::{K230GzipDma, K230GzipDmaMmio};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

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

fn read_bytes(aspace: &AddressSpace, addr: u64, len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| aspace.read(GPA(addr + index as u64), 1) as u8)
        .collect()
}

fn write_u32(aspace: &AddressSpace, addr: u64, value: u32) {
    aspace.write(GPA(addr), 4, u64::from(value));
}

fn write_llt(
    aspace: &AddressSpace,
    addr: u64,
    src_addr: u32,
    line_size: u32,
    dst_addr: u32,
    next_addr: u32,
) {
    write_u32(aspace, addr, 1 << 28);
    write_u32(aspace, addr + 0x04, src_addr);
    write_u32(aspace, addr + 0x08, line_size);
    write_u32(aspace, addr + 0x0c, 0);
    write_u32(aspace, addr + 0x10, dst_addr);
    write_u32(aspace, addr + 0x14, next_addr);
}

fn gzip_method_9(payload: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(payload).unwrap();
    let mut gzip = encoder.finish().unwrap();
    gzip[2] = 0x09;
    gzip
}

#[test]
fn k230_gzip_dma_decompresses_sdk_llt_chain() {
    let payload = b"k230 hardware gzip output";
    let compressed = gzip_method_9(payload);
    let aspace = make_ram_aspace(0x1_0000);
    let compressed_addr = 0x1000;
    let output_addr = 0x8000;
    let read_llt = 0x4000;
    let write_llt_addr = 0x4100;

    write_bytes(&aspace, compressed_addr, &compressed);
    write_llt(
        &aspace,
        read_llt,
        compressed_addr as u32,
        compressed.len() as u32,
        0x8028_0000,
        0,
    );
    write_llt(
        &aspace,
        write_llt_addr,
        0x8020_0000,
        payload.len() as u32,
        output_addr as u32,
        0,
    );

    let dev = K230GzipDma::new_named("k230-gzip-dma");
    dev.set_dma_address_space(aspace.clone());
    let mmio = K230GzipDmaMmio(dev);

    mmio.write(0x60, 4, read_llt);
    mmio.write(0x90, 4, write_llt_addr);
    mmio.write(0x8004, 4, 0x8000_0000 | compressed.len() as u64);
    mmio.write(0x8008, 4, payload.len() as u64);
    mmio.write(0x8000, 4, 0x3);

    assert_ne!(mmio.read(0x08, 4) & 0x2, 0);
    assert_ne!(mmio.read(0x800c, 4) & (1 << 10), 0);
    assert_eq!(read_bytes(&aspace, output_addr, payload.len()), payload);
}
