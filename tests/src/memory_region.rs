use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_memory::*;

// -- helpers --

struct MockUart {
    regs: Mutex<[u8; 256]>,
}

impl MockUart {
    fn new() -> Self {
        Self {
            regs: Mutex::new([0u8; 256]),
        }
    }
}

impl MmioOps for MockUart {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        let regs = self.regs.lock().unwrap();
        regs[offset as usize] as u64
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        let mut regs = self.regs.lock().unwrap();
        regs[offset as usize] = val as u8;
    }
}

// -- test cases --

#[test]
fn test_ram_block_alloc() {
    let block = RamBlock::new(4096);
    assert_eq!(block.size(), 4096);

    // Write pattern then read it back.
    let ptr = block.as_ptr();
    unsafe {
        for i in 0..4096u64 {
            *ptr.add(i as usize) = (i & 0xff) as u8;
        }
        for i in 0..4096u64 {
            assert_eq!(*ptr.add(i as usize), (i & 0xff) as u8,);
        }
    }
}

#[test]
fn test_flat_view_single_ram() {
    let (ram, _block) = MemoryRegion::ram("dram", 0x1_0000);

    let mut root = MemoryRegion::container("system", 0x10_0000);
    root.add_subregion(ram, GPA::new(0x8_0000));

    let fv = FlatView::from_region(&root);
    assert_eq!(fv.ranges.len(), 1);
    assert_eq!(fv.ranges[0].addr, GPA::new(0x8_0000));
    assert_eq!(fv.ranges[0].size, 0x1_0000);
    assert!(!fv.ranges[0].is_io());

    // Lookup inside the range.
    assert!(fv.lookup(GPA::new(0x8_0000)).is_some());
    assert!(fv.lookup(GPA::new(0x8_FFFF)).is_some());

    // Lookup outside.
    assert!(fv.lookup(GPA::new(0x7_FFFF)).is_none());
    assert!(fv.lookup(GPA::new(0x9_0000)).is_none());
}

#[test]
fn test_flat_view_overlap_priority() {
    // Low-priority RAM spanning 0..0x10000.
    let (ram_lo, _blk_lo) = MemoryRegion::ram("lo-ram", 0x1_0000);

    // High-priority IO covering 0x1000..0x2000 (4 KiB)
    // overlapping the RAM.
    let uart = MockUart::new();
    let io_hi = MemoryRegion::io("uart", 0x1000, Arc::new(uart));

    let mut root = MemoryRegion::container("root", 0x10_0000);
    root.add_subregion(ram_lo, GPA::new(0));
    root.add_subregion_with_priority(io_hi, GPA::new(0x1000), 10);

    let fv = FlatView::from_region(&root);

    // There should be 3 ranges:
    //   [0x0000, 0x1000) RAM
    //   [0x1000, 0x2000) IO  (higher priority)
    //   [0x2000, 0x10000) RAM
    assert_eq!(fv.ranges.len(), 3);

    let r0 = &fv.ranges[0];
    assert_eq!(r0.addr, GPA::new(0x0000));
    assert_eq!(r0.size, 0x1000);
    assert!(!r0.is_io());

    let r1 = &fv.ranges[1];
    assert_eq!(r1.addr, GPA::new(0x1000));
    assert_eq!(r1.size, 0x1000);
    assert!(r1.is_io());

    let r2 = &fv.ranges[2];
    assert_eq!(r2.addr, GPA::new(0x2000));
    assert_eq!(r2.size, 0xE000);
    assert!(!r2.is_io());
}

#[test]
fn test_address_space_read_write() {
    let (ram, _block) = MemoryRegion::ram("dram", 0x1_0000);
    let mut root = MemoryRegion::container("root", 0x10_0000);
    root.add_subregion(ram, GPA::new(0));

    let a = AddressSpace::new(root);

    // Write a u32 then read it back.
    a.write_u32(GPA::new(0x100), 0xDEAD_BEEF);
    assert_eq!(a.read_u32(GPA::new(0x100)), 0xDEAD_BEEF);

    // Write individual bytes and read back as u32.
    let msg = b"abcd";
    for (i, &b) in msg.iter().enumerate() {
        a.write(GPA::new(0x200 + i as u64), 1, b as u64);
    }
    let val = a.read(GPA::new(0x200), 4) as u32;
    assert_eq!(&val.to_le_bytes(), msg);
}

// ===== FlatView alias / disabled-region regression tests (#69) =====
//
// These tests assert FlatView::ranges {addr, size, kind,
// offset_in_region} for the four cases called out in the issue
// checklist: RAM alias offset forwarding, container alias clipping,
// disabled subregion suppression, and an alias overlapped by a
// higher-priority MMIO mapping.

fn ram_block_of(range: &FlatRange) -> Arc<RamBlock> {
    match &range.kind {
        FlatRangeKind::Ram { block } => Arc::clone(block),
        _ => panic!("expected Ram range, got non-RAM kind"),
    }
}

#[test]
fn test_flat_view_ram_alias_forwards_offset_into_block() {
    // 1 MiB RAM, aliased through a 0x2000-byte window starting at
    // byte 0x4000 of the underlying block.
    let (ram, ram_block) = MemoryRegion::ram("dram", 0x10_0000);
    let alias = MemoryRegion::alias("alias", ram, 0x4000, 0x2000);

    let mut root = MemoryRegion::container("root", u64::MAX);
    root.add_subregion(alias, GPA::new(0x10_0000));

    let fv = FlatView::from_region(&root);
    assert_eq!(fv.ranges.len(), 1);
    let r = &fv.ranges[0];
    assert_eq!(r.addr, GPA::new(0x10_0000));
    assert_eq!(r.size, 0x2000);
    assert!(!r.is_io());
    assert_eq!(
        r.offset_in_region, 0x4000,
        "alias offset must surface as offset_in_region for the leaf",
    );
    assert!(
        Arc::ptr_eq(&ram_block, &ram_block_of(r)),
        "alias must point at the original RAM block, not a copy",
    );
}

#[test]
fn test_flat_view_container_alias_clips_subregions() {
    // Container with two RAM children:
    //   sub_a: [0x0000..0x1000)
    //   sub_b: [0x4000..0x5000)
    // Alias window covers container bytes [0x800..0x4800), i.e. it
    // spans the tail of sub_a (0x800..0x1000), the gap (0x1000..0x4000),
    // and the head of sub_b (0x4000..0x4800). Only the parts that hit
    // a live subregion should appear in the FlatView.
    let (sub_a, _ba) = MemoryRegion::ram("sub_a", 0x1000);
    let (sub_b, _bb) = MemoryRegion::ram("sub_b", 0x1000);
    let mut container = MemoryRegion::container("c", 0x10_000);
    container.add_subregion(sub_a, GPA::new(0x0000));
    container.add_subregion(sub_b, GPA::new(0x4000));

    let alias = MemoryRegion::alias("alias", container, 0x800, 0x4000);
    let mut root = MemoryRegion::container("root", u64::MAX);
    root.add_subregion(alias, GPA::new(0x20_0000));

    let fv = FlatView::from_region(&root);
    assert_eq!(
        fv.ranges.len(),
        2,
        "alias clipping should produce exactly two clipped fragments",
    );

    // First fragment: [base..base+0x800), offset_in_region=0x800 (tail
    // of sub_a starts at byte 0x800 of sub_a).
    let r0 = &fv.ranges[0];
    assert_eq!(r0.addr, GPA::new(0x20_0000));
    assert_eq!(r0.size, 0x800);
    assert_eq!(r0.offset_in_region, 0x800);

    // Second fragment: head of sub_b, sized 0x800, mapped to
    // [base + (sub_b.start - alias_off) .. base + 0x4000) =
    // [base + 0x3800 .. base + 0x4000).
    let r1 = &fv.ranges[1];
    assert_eq!(r1.addr, GPA::new(0x20_0000 + 0x3800));
    assert_eq!(r1.size, 0x800);
    assert_eq!(
        r1.offset_in_region, 0,
        "head of sub_b is read from offset 0 of that subregion",
    );
}

#[test]
fn test_flat_view_disabled_subregion_is_omitted() {
    let (ram_live, _live) = MemoryRegion::ram("live", 0x1000);
    let (mut ram_dead, _dead) = MemoryRegion::ram("dead", 0x1000);
    ram_dead.enabled = false;

    let mut root = MemoryRegion::container("root", 0x10_000);
    root.add_subregion(ram_live, GPA::new(0x0000));
    root.add_subregion(ram_dead, GPA::new(0x1000));

    let fv = FlatView::from_region(&root);
    assert_eq!(fv.ranges.len(), 1);
    assert_eq!(fv.ranges[0].addr, GPA::new(0x0000));

    // Lookup inside the disabled window must miss.
    assert!(
        fv.lookup(GPA::new(0x1500)).is_none(),
        "disabled region must not be reachable via lookup",
    );
    // And reads through AddressSpace return 0 rather than panicking.
    let aspace = AddressSpace::new(root);
    assert_eq!(aspace.read(GPA::new(0x1500), 1), 0);
}

#[test]
fn test_flat_view_alias_to_disabled_target_yields_no_range() {
    let (mut ram, _b) = MemoryRegion::ram("dram", 0x1000);
    ram.enabled = false;
    let alias = MemoryRegion::alias("alias", ram, 0, 0x1000);

    let mut root = MemoryRegion::container("root", 0x10_000);
    root.add_subregion(alias, GPA::new(0));

    let fv = FlatView::from_region(&root);
    assert!(
        fv.ranges.is_empty(),
        "alias whose target is disabled must contribute nothing",
    );
}

#[test]
fn test_flat_view_alias_overlapped_by_higher_priority_io_keeps_offsets() {
    // Alias [base..base+0x1000) into RAM at byte 0x5000, priority 0.
    // High-priority MMIO at [base+0x100..base+0x200), priority 10.
    // Expected layout (sorted by address):
    //   [0x0    .. 0x100)  RAM, offset_in_region = 0x5000
    //   [0x100  .. 0x200)  IO,  priority 10
    //   [0x200  .. 0x1000) RAM, offset_in_region = 0x5200
    let (ram, _block) = MemoryRegion::ram("dram", 0x10_0000);
    let alias = MemoryRegion::alias("alias", ram, 0x5000, 0x1000);
    let io = MemoryRegion::io("uart", 0x100, Arc::new(MockUart::new()));

    let mut root = MemoryRegion::container("root", u64::MAX);
    root.add_subregion(alias, GPA::new(0));
    root.add_subregion_with_priority(io, GPA::new(0x100), 10);

    let fv = FlatView::from_region(&root);
    assert_eq!(fv.ranges.len(), 3);

    let r0 = &fv.ranges[0];
    assert_eq!(r0.addr, GPA::new(0));
    assert_eq!(r0.size, 0x100);
    assert!(!r0.is_io());
    assert_eq!(
        r0.offset_in_region, 0x5000,
        "left fragment must keep the alias base offset",
    );

    let r1 = &fv.ranges[1];
    assert_eq!(r1.addr, GPA::new(0x100));
    assert_eq!(r1.size, 0x100);
    assert!(r1.is_io());

    let r2 = &fv.ranges[2];
    assert_eq!(r2.addr, GPA::new(0x200));
    assert_eq!(r2.size, 0x1000 - 0x200);
    assert!(!r2.is_io());
    assert_eq!(
        r2.offset_in_region,
        0x5000 + 0x200,
        "right fragment must shift offset_in_region by the gap",
    );
}
