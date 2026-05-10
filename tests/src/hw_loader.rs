use machina_core::address::GPA;
use machina_memory::region::MemoryRegion;
use machina_memory::AddressSpace;

use machina_hw_core::loader::{elf_phys_entry, load_binary, load_elf};

/// Create a minimal AddressSpace with `size` bytes of RAM
/// starting at guest physical address 0.
fn make_ram_as(size: u64) -> AddressSpace {
    let (ram, _block) = MemoryRegion::ram("ram", size);
    let mut root = MemoryRegion::container("root", size);
    root.add_subregion(ram, GPA::new(0));
    let mut as_ = AddressSpace::new(root);
    as_.update_flat_view();
    as_
}

fn minimal_exec_elf(
    entry: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_memsz: u64,
) -> Vec<u8> {
    let mut elf = vec![0u8; 64 + 56];
    elf[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
    elf[4] = 2;
    elf[5] = 1;
    elf[6] = 1;
    elf[16..18].copy_from_slice(&2u16.to_le_bytes());
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf[24..32].copy_from_slice(&entry.to_le_bytes());
    elf[32..40].copy_from_slice(&64u64.to_le_bytes());
    elf[52..54].copy_from_slice(&64u16.to_le_bytes());
    elf[54..56].copy_from_slice(&56u16.to_le_bytes());
    elf[56..58].copy_from_slice(&1u16.to_le_bytes());

    let ph = 64usize;
    elf[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
    let p_offset: u64 = 120;
    elf[ph + 8..ph + 16].copy_from_slice(&p_offset.to_le_bytes());
    elf[ph + 16..ph + 24].copy_from_slice(&p_vaddr.to_le_bytes());
    elf[ph + 24..ph + 32].copy_from_slice(&p_paddr.to_le_bytes());
    elf[ph + 32..ph + 40].copy_from_slice(&1u64.to_le_bytes());
    elf[ph + 40..ph + 48].copy_from_slice(&p_memsz.to_le_bytes());
    elf.push(0);
    elf
}

#[test]
fn test_load_binary() {
    let as_ = make_ram_as(0x1000);
    let data: Vec<u8> = (0u8..16).collect();
    let info =
        load_binary(&data, GPA::new(0x100), &as_).expect("load_binary failed");
    assert_eq!(info.entry, GPA::new(0x100));
    assert_eq!(info.size, 16);
    assert_eq!(info.low_addr, 0x100);

    // Read back and verify.
    let v0 = as_.read_u32(GPA::new(0x100));
    assert_eq!(v0, u32::from_le_bytes([0, 1, 2, 3]));
    let v1 = as_.read_u32(GPA::new(0x104));
    assert_eq!(v1, u32::from_le_bytes([4, 5, 6, 7]));
}

#[test]
fn test_load_binary_alignment() {
    let as_ = make_ram_as(0x1000);
    // 5 bytes -- not a multiple of 4.
    let data = vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE];
    let info =
        load_binary(&data, GPA::new(0x200), &as_).expect("load_binary failed");
    assert_eq!(info.size, 5);

    // First 4 bytes form one u32 write.
    let v0 = as_.read_u32(GPA::new(0x200));
    assert_eq!(v0, u32::from_le_bytes([0xAA, 0xBB, 0xCC, 0xDD]),);
    // Remaining 1 byte is written as a partial u32.
    let v1 = as_.read_u32(GPA::new(0x204));
    assert_eq!(v1, 0xEE);
}

#[test]
fn test_load_binary_odd_size() {
    // Load 5 bytes; verify byte 5 is correct and byte 6
    // stays at its initial value.
    let as_ = make_ram_as(0x1000);

    // Pre-fill sentinel at offset 0x105.
    as_.write(GPA::new(0x105), 1, 0xFF);

    let data: [u8; 5] = [0x10, 0x20, 0x30, 0x40, 0x50];
    let info =
        load_binary(&data, GPA::new(0x100), &as_).expect("load_binary failed");
    assert_eq!(info.size, 5);

    // Byte 4 (index 4) must be 0x50.
    let b4 = as_.read(GPA::new(0x104), 1) as u8;
    assert_eq!(b4, 0x50, "byte 5 must be correct");

    // Byte 5 (index 5) must still be the sentinel 0xFF.
    let b5 = as_.read(GPA::new(0x105), 1) as u8;
    assert_eq!(b5, 0xFF, "byte 6 must be untouched");
}

#[test]
fn test_load_elf_simple() {
    // Build a minimal ELF-64 LE executable with one
    // PT_LOAD segment.
    let as_ = make_ram_as(0x10000);

    let entry: u64 = 0x1000;
    let payload: [u8; 7] = [0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0x42];
    let p_paddr: u64 = 0x2000;

    // -- ELF header (64 bytes) --
    let mut elf = vec![0u8; 64 + 56];

    // e_ident
    elf[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
    elf[4] = 2; // ELFCLASS64
    elf[5] = 1; // ELFDATA2LSB
    elf[6] = 1; // EV_CURRENT

    // e_type = ET_EXEC (2)
    elf[16..18].copy_from_slice(&2u16.to_le_bytes());
    // e_version
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    // e_entry
    elf[24..32].copy_from_slice(&entry.to_le_bytes());
    // e_phoff = 64 (immediately after ehdr)
    elf[32..40].copy_from_slice(&64u64.to_le_bytes());
    // e_ehsize (52..54) = 64
    elf[52..54].copy_from_slice(&64u16.to_le_bytes());
    // e_phentsize (54..56) = 56
    elf[54..56].copy_from_slice(&56u16.to_le_bytes());
    // e_phnum (56..58) = 1
    elf[56..58].copy_from_slice(&1u16.to_le_bytes());

    // -- Program header (56 bytes at offset 64) --
    let ph = 64usize;
    // p_type = PT_LOAD (1)
    elf[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
    // p_offset = end of headers (64 + 56 = 120)
    let p_offset: u64 = 120;
    elf[ph + 8..ph + 16].copy_from_slice(&p_offset.to_le_bytes());
    // p_vaddr
    elf[ph + 16..ph + 24].copy_from_slice(&p_paddr.to_le_bytes());
    // p_paddr
    elf[ph + 24..ph + 32].copy_from_slice(&p_paddr.to_le_bytes());
    // p_filesz
    let filesz = payload.len() as u64;
    elf[ph + 32..ph + 40].copy_from_slice(&filesz.to_le_bytes());
    // p_memsz (same as filesz, no BSS)
    elf[ph + 40..ph + 48].copy_from_slice(&filesz.to_le_bytes());

    // Append payload
    elf.extend_from_slice(&payload);

    let info = load_elf(&elf, 0, &as_).expect("load_elf failed");
    assert_eq!(info.entry, GPA::new(entry));
    assert_eq!(info.size, payload.len() as u64);
    assert_eq!(info.low_addr, p_paddr);

    // Verify loaded bytes.
    for (i, &expected) in payload.iter().enumerate() {
        let actual = as_.read(GPA::new(p_paddr + i as u64), 1) as u8;
        assert_eq!(actual, expected, "mismatch at offset {i}");
    }
}

#[test]
fn test_elf_phys_entry_handles_paddr_above_vaddr() {
    let elf = minimal_exec_elf(0x1010, 0x1000, 0x8000, 0x100);

    assert_eq!(elf_phys_entry(&elf, 0x1010), Some(0x8010));
}

#[test]
fn test_elf_phys_entry_handles_paddr_below_vaddr() {
    let elf = minimal_exec_elf(0x8010, 0x8000, 0x1000, 0x100);

    assert_eq!(elf_phys_entry(&elf, 0x8010), Some(0x1010));
}

#[test]
fn test_load_elf_dyn_pie() {
    // Build a minimal ELF-64 LE ET_DYN (PIE) with one
    // PT_LOAD segment at vaddr 0x1000, then load it
    // at base 0x100_0000 and verify relocation bias.
    let as_ = make_ram_as(0x1000_0000);

    let entry_rel: u64 = 0x1100; // entry relative to vaddr 0
    let p_vaddr: u64 = 0x1000; // segment vaddr
    let base: u64 = 0x100_0000; // load base
    let payload: [u8; 8] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];

    // -- ELF header (64 bytes) --
    let mut elf = vec![0u8; 64 + 56];

    elf[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
    elf[4] = 2; // ELFCLASS64
    elf[5] = 1; // ELFDATA2LSB
    elf[6] = 1; // EV_CURRENT
                // e_type = ET_DYN (3)
    elf[16..18].copy_from_slice(&3u16.to_le_bytes());
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf[24..32].copy_from_slice(&entry_rel.to_le_bytes());
    elf[32..40].copy_from_slice(&64u64.to_le_bytes()); // e_phoff
    elf[52..54].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize
    elf[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
    elf[56..58].copy_from_slice(&1u16.to_le_bytes()); // e_phnum

    // -- Program header (56 bytes at offset 64) --
    let ph = 64usize;
    elf[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
    let p_offset: u64 = 120;
    elf[ph + 8..ph + 16].copy_from_slice(&p_offset.to_le_bytes());
    elf[ph + 16..ph + 24].copy_from_slice(&p_vaddr.to_le_bytes()); // p_vaddr
    elf[ph + 24..ph + 32].copy_from_slice(&p_vaddr.to_le_bytes()); // p_paddr
    let filesz = payload.len() as u64;
    elf[ph + 32..ph + 40].copy_from_slice(&filesz.to_le_bytes());
    elf[ph + 40..ph + 48].copy_from_slice(&filesz.to_le_bytes());

    elf.extend_from_slice(&payload);

    let info = load_elf(&elf, base, &as_).expect("load_elf ET_DYN failed");
    let expected = base + p_vaddr;
    assert_eq!(info.entry, GPA::new(base + entry_rel));
    assert!(info.bias.is_some());
    assert_eq!(info.bias.unwrap(), base);
    assert_eq!(info.size, filesz);
    assert_eq!(info.low_addr, expected);

    for (i, &exp) in payload.iter().enumerate() {
        let actual = as_.read(GPA::new(expected + i as u64), 1) as u8;
        assert_eq!(actual, exp, "mismatch at offset {i}");
    }
}

// ===== Malformed-ELF boundary checks (#57) =====
//
// These tests cover four classes of malformed input that the
// loader must reject without panicking and without leaving guest
// memory partially written: p_filesz > p_memsz, out-of-bounds
// program headers, out-of-bounds segment file data, and arithmetic
// overflow in the offset computation.

/// Build an ELF-64 ET_EXEC blob with one PT_LOAD segment whose
/// raw header fields can be patched per-test.
fn build_one_segment_elf(
    p_offset: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    payload: &[u8],
) -> Vec<u8> {
    let mut elf = vec![0u8; 64 + 56];
    elf[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
    elf[4] = 2; // ELFCLASS64
    elf[5] = 1; // ELFDATA2LSB
    elf[6] = 1; // EV_CURRENT
    elf[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf[24..32].copy_from_slice(&p_paddr.to_le_bytes()); // e_entry
    elf[32..40].copy_from_slice(&64u64.to_le_bytes()); // e_phoff
    elf[52..54].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize
    elf[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
    elf[56..58].copy_from_slice(&1u16.to_le_bytes()); // e_phnum

    let ph = 64usize;
    elf[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
    elf[ph + 8..ph + 16].copy_from_slice(&p_offset.to_le_bytes());
    elf[ph + 16..ph + 24].copy_from_slice(&p_paddr.to_le_bytes());
    elf[ph + 24..ph + 32].copy_from_slice(&p_paddr.to_le_bytes());
    elf[ph + 32..ph + 40].copy_from_slice(&p_filesz.to_le_bytes());
    elf[ph + 40..ph + 48].copy_from_slice(&p_memsz.to_le_bytes());
    elf.extend_from_slice(payload);
    elf
}

#[test]
fn test_load_elf_rejects_p_filesz_greater_than_p_memsz() {
    let as_ = make_ram_as(0x10000);
    // p_filesz = 8, p_memsz = 4 -> illegal per ELF spec.
    let payload: [u8; 8] = [0xAA; 8];
    let elf = build_one_segment_elf(120, 0x2000, 8, 4, &payload);

    let err = load_elf(&elf, 0, &as_).expect_err("must reject filesz > memsz");
    assert!(
        err.contains("p_filesz") && err.contains("p_memsz"),
        "error message should name p_filesz/p_memsz, got: {err}",
    );
}

#[test]
fn test_load_elf_rejects_phdr_out_of_bounds() {
    let as_ = make_ram_as(0x10000);
    // Build a normal ELF, then bump e_phoff past the end of the
    // file so the program header read would walk off the buffer.
    let payload: [u8; 4] = [0x55; 4];
    let mut elf = build_one_segment_elf(120, 0x2000, 4, 4, &payload);
    let bogus_phoff: u64 = elf.len() as u64 + 1024;
    elf[32..40].copy_from_slice(&bogus_phoff.to_le_bytes());

    let err = load_elf(&elf, 0, &as_).expect_err("must reject phdr OOB");
    assert!(
        err.contains("phdr"),
        "error message should mention phdr, got: {err}",
    );
}

#[test]
fn test_load_elf_rejects_segment_file_data_out_of_bounds() {
    let as_ = make_ram_as(0x10000);
    // Claim p_filesz=4096 but only attach 8 bytes of payload.
    let payload: [u8; 8] = [0x12; 8];
    let elf = build_one_segment_elf(120, 0x2000, 4096, 4096, &payload);

    let err = load_elf(&elf, 0, &as_).expect_err("must reject segment OOB");
    assert!(
        err.contains("file data out of bounds"),
        "error should describe segment-file-data bounds, got: {err}",
    );
}

#[test]
fn test_load_elf_rejects_p_offset_arithmetic_overflow() {
    let as_ = make_ram_as(0x10000);
    // p_offset = u64::MAX, p_filesz = 1 -> p_offset+p_filesz wraps.
    let payload: [u8; 0] = [];
    let elf = build_one_segment_elf(u64::MAX, 0x2000, 1, 1, &payload);

    let err =
        load_elf(&elf, 0, &as_).expect_err("must reject overflowing offset");
    // Either the usize conversion fails or the checked_add fails.
    assert!(
        err.contains("p_offset")
            || err.contains("file range arithmetic overflow")
            || err.contains("out of bounds"),
        "error should describe an arithmetic / bounds problem, got: {err}",
    );
}

#[test]
fn test_load_elf_rejection_does_not_partially_write_memory() {
    let as_ = make_ram_as(0x10000);
    // Construct a two-segment ELF where segment 0 is valid but
    // segment 1 has p_filesz > p_memsz. The first valid segment
    // must NOT be written before the loader rejects the input.
    let entry: u64 = 0x1000;
    let payload0: [u8; 4] = [0xAB, 0xCD, 0xEF, 0x12];
    let p_paddr0: u64 = 0x1000;
    let p_paddr1: u64 = 0x2000;

    // Layout:
    //   [0 ..  64) ehdr
    //   [64.. 120) phdr0
    //   [120..176) phdr1
    //   [176..180) payload0
    let mut elf = vec![0u8; 64 + 56 * 2];
    elf[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
    elf[4] = 2;
    elf[5] = 1;
    elf[6] = 1;
    elf[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf[24..32].copy_from_slice(&entry.to_le_bytes());
    elf[32..40].copy_from_slice(&64u64.to_le_bytes());
    elf[52..54].copy_from_slice(&64u16.to_le_bytes());
    elf[54..56].copy_from_slice(&56u16.to_le_bytes());
    elf[56..58].copy_from_slice(&2u16.to_le_bytes()); // e_phnum = 2

    // phdr0: valid, p_filesz=4, p_memsz=4.
    let ph0 = 64usize;
    let p_offset0: u64 = 64 + 56 * 2;
    elf[ph0..ph0 + 4].copy_from_slice(&1u32.to_le_bytes());
    elf[ph0 + 8..ph0 + 16].copy_from_slice(&p_offset0.to_le_bytes());
    elf[ph0 + 16..ph0 + 24].copy_from_slice(&p_paddr0.to_le_bytes());
    elf[ph0 + 24..ph0 + 32].copy_from_slice(&p_paddr0.to_le_bytes());
    elf[ph0 + 32..ph0 + 40].copy_from_slice(&4u64.to_le_bytes());
    elf[ph0 + 40..ph0 + 48].copy_from_slice(&4u64.to_le_bytes());

    // phdr1: malformed, p_filesz=8 > p_memsz=4.
    let ph1 = 64 + 56;
    elf[ph1..ph1 + 4].copy_from_slice(&1u32.to_le_bytes());
    elf[ph1 + 8..ph1 + 16].copy_from_slice(&p_offset0.to_le_bytes());
    elf[ph1 + 16..ph1 + 24].copy_from_slice(&p_paddr1.to_le_bytes());
    elf[ph1 + 24..ph1 + 32].copy_from_slice(&p_paddr1.to_le_bytes());
    elf[ph1 + 32..ph1 + 40].copy_from_slice(&8u64.to_le_bytes());
    elf[ph1 + 40..ph1 + 48].copy_from_slice(&4u64.to_le_bytes());

    elf.extend_from_slice(&payload0);

    // Pre-fill the target with a sentinel so we can detect any
    // accidental write from segment 0.
    for i in 0..4 {
        as_.write(GPA::new(p_paddr0 + i), 1, 0xA5);
    }

    let err = load_elf(&elf, 0, &as_)
        .expect_err("two-pass validation must reject this ELF");
    assert!(err.contains("p_filesz"));

    for i in 0..4u64 {
        let got = as_.read(GPA::new(p_paddr0 + i), 1) as u8;
        assert_eq!(
            got, 0xA5,
            "segment 0 byte {i} should still be the sentinel; load_elf \
             must validate every segment before writing any of them",
        );
    }
}
