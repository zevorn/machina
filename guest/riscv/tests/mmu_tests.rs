use machina_guest_riscv::riscv::csr::PrivLevel;
use machina_guest_riscv::riscv::exception::Exception;
use machina_guest_riscv::riscv::mmu::{AccessType, Mmu};

// ── Helpers ────────────────────────────────────────────────────

/// Write a 64-bit PTE into the flat memory array.
fn write_pte(mem: &mut [u8], addr: u64, pte: u64) {
    let off = addr as usize;
    mem[off..off + 8].copy_from_slice(&pte.to_le_bytes());
}

/// Build a `mem_read` closure over a byte slice.
fn mem_reader(mem: &[u8]) -> impl Fn(u64) -> u64 + '_ {
    move |addr: u64| {
        let off = addr as usize;
        u64::from_le_bytes(mem[off..off + 8].try_into().unwrap())
    }
}

// ── Sv39 PTE flag helpers ──────────────────────────────────────

const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;
const PTE_A: u64 = 1 << 6;
const PTE_D: u64 = 1 << 7;

/// Build a leaf PTE: ppn in bits [53:10], flags in [7:0].
fn leaf_pte(ppn: u64, flags: u64) -> u64 {
    (ppn << 10) | flags
}

/// Build a non-leaf (pointer) PTE.
fn ptr_pte(next_ppn: u64) -> u64 {
    (next_ppn << 10) | PTE_V
}

/// SATP value for Sv39 mode.
fn satp_sv39(asid: u16, root_ppn: u64) -> u64 {
    (8u64 << 60) | ((asid as u64) << 44) | root_ppn
}

// ── Tests ──────────────────────────────────────────────────────

#[test]
fn test_bare_mode_passthrough() {
    let mut mmu = Mmu::new();
    // satp = 0 means MODE = Bare
    mmu.set_satp(0);
    assert_eq!(mmu.get_satp(), 0);

    let dummy = |_addr: u64| -> u64 { 0 };
    let addrs: &[u64] = &[0, 0x1000, 0x8000_0000, u64::MAX];
    for &gva in addrs {
        let pa =
            mmu.translate(gva, AccessType::Read, PrivLevel::Machine, 0, &dummy);
        assert_eq!(pa, Ok(gva));
    }
}

#[test]
fn test_sv39_identity_map() {
    // Memory layout:
    //   0x1000: root page table (level-2)
    //   0x2000: level-1 page table
    //   0x3000: level-0 page table
    //   Map VA 0x0000 -> PA 0x0000 (identity, 4K page)
    let mem_size = 0x10000;
    let mut mem = vec![0u8; mem_size];

    // Root (level-2): entry 0 -> level-1 at PPN 2
    write_pte(&mut mem, 0x1000, ptr_pte(2));
    // Level-1: entry 0 -> level-0 at PPN 3
    write_pte(&mut mem, 0x2000, ptr_pte(3));
    // Level-0: entry 0 -> leaf at PPN 0 (identity)
    let flags = PTE_V | PTE_R | PTE_W | PTE_X | PTE_A | PTE_D;
    write_pte(&mut mem, 0x3000, leaf_pte(0, flags));

    let mut mmu = Mmu::new();
    // Root table at PPN 1 (physical address 0x1000)
    mmu.set_satp(satp_sv39(0, 1));

    let reader = mem_reader(&mem);

    // Translate VA 0x0100 -> PA 0x0100 (within page 0)
    let pa =
        mmu.translate(0x0100, AccessType::Read, PrivLevel::Machine, 0, &reader);
    assert_eq!(pa, Ok(0x0100));

    // Translate VA 0x0ABC -> PA 0x0ABC
    let pa = mmu.translate(
        0x0ABC,
        AccessType::Write,
        PrivLevel::Machine,
        0,
        &reader,
    );
    assert_eq!(pa, Ok(0x0ABC));
}

#[test]
fn test_sv39_page_fault() {
    // Only map VA page 0; accessing page 1 should fault.
    let mem_size = 0x10000;
    let mut mem = vec![0u8; mem_size];

    write_pte(&mut mem, 0x1000, ptr_pte(2));
    write_pte(&mut mem, 0x2000, ptr_pte(3));
    let flags = PTE_V | PTE_R | PTE_W | PTE_X | PTE_A | PTE_D;
    write_pte(&mut mem, 0x3000, leaf_pte(0, flags));
    // Level-0 entry 1 is zero (unmapped)

    let mut mmu = Mmu::new();
    mmu.set_satp(satp_sv39(0, 1));

    let reader = mem_reader(&mem);

    // Page 0 works
    assert!(mmu
        .translate(0x0000, AccessType::Read, PrivLevel::Machine, 0, &reader)
        .is_ok());

    // Flush TLB so we force a walk for the next address
    mmu.flush();

    // Page 1 (VA 0x1000) is unmapped -> LoadPageFault
    let err =
        mmu.translate(0x1000, AccessType::Read, PrivLevel::Machine, 0, &reader);
    assert_eq!(err, Err(Exception::LoadPageFault));

    // Execute fault
    mmu.flush();
    let err = mmu.translate(
        0x1000,
        AccessType::Execute,
        PrivLevel::Machine,
        0,
        &reader,
    );
    assert_eq!(err, Err(Exception::InstructionPageFault));

    // Store fault
    mmu.flush();
    let err = mmu.translate(
        0x1000,
        AccessType::Write,
        PrivLevel::Machine,
        0,
        &reader,
    );
    assert_eq!(err, Err(Exception::StorePageFault));
}

#[test]
fn test_tlb_hit() {
    let mem_size = 0x10000;
    let mut mem = vec![0u8; mem_size];

    write_pte(&mut mem, 0x1000, ptr_pte(2));
    write_pte(&mut mem, 0x2000, ptr_pte(3));
    let flags = PTE_V | PTE_R | PTE_W | PTE_X | PTE_A | PTE_D;
    write_pte(&mut mem, 0x3000, leaf_pte(0, flags));

    let mut mmu = Mmu::new();
    mmu.set_satp(satp_sv39(0, 1));

    let reader = mem_reader(&mem);

    // First access: TLB miss -> page walk
    let pa1 =
        mmu.translate(0x0042, AccessType::Read, PrivLevel::Machine, 0, &reader);
    assert_eq!(pa1, Ok(0x0042));
    let walks_after_first = mmu.stats().page_walks;
    assert_eq!(walks_after_first, 1);
    assert_eq!(mmu.stats().tlb_misses, 1);

    // Second access to same page: TLB hit, no new walk
    let pa2 =
        mmu.translate(0x0084, AccessType::Read, PrivLevel::Machine, 0, &reader);
    assert_eq!(pa2, Ok(0x0084));
    assert_eq!(mmu.stats().page_walks, walks_after_first);
    assert_eq!(mmu.stats().tlb_hits, 1);
}

#[test]
fn test_superpage() {
    // 2MiB megapage mapping:
    //   Root (level-2) at 0x1000: entry 0 -> level-1 at PPN 2
    //   Level-1 at 0x2000: entry 0 -> leaf megapage at PPN 0x200
    //     (PPN 0x200 = physical base 0x200 * 4096 = 0x200000,
    //      PPN[0] = 0 so superpage alignment is satisfied)
    let mem_size = 0x10000;
    let mut mem = vec![0u8; mem_size];

    write_pte(&mut mem, 0x1000, ptr_pte(2));
    // Level-1, entry 0: leaf megapage at PPN 0x200 (aligned)
    // PPN 0x200 = 0b10_0000_0000, PPN[0] (low 9 bits) = 0
    let mega_ppn: u64 = 0x200; // 0x200 << 12 = 0x20_0000
    let flags = PTE_V | PTE_R | PTE_W | PTE_X | PTE_U | PTE_A | PTE_D;
    write_pte(&mut mem, 0x2000, leaf_pte(mega_ppn, flags));

    let mut mmu = Mmu::new();
    mmu.set_satp(satp_sv39(0, 1));

    let reader = mem_reader(&mem);

    // VA 0x0000_0000 -> PA 0x0020_0000 (megapage base)
    let pa = mmu.translate(
        0x0000_0000,
        AccessType::Read,
        PrivLevel::User,
        0,
        &reader,
    );
    assert_eq!(pa, Ok(0x0020_0000));

    // VA 0x0010_0000 (offset 1MiB into megapage)
    // -> PA 0x0020_0000 + 0x0010_0000 = 0x0030_0000
    mmu.flush();
    let pa = mmu.translate(
        0x0010_0000,
        AccessType::Write,
        PrivLevel::User,
        0,
        &reader,
    );
    assert_eq!(pa, Ok(0x0030_0000));

    // VA 0x001F_FFFF (last byte of the megapage)
    // -> PA 0x0020_0000 + 0x001F_FFFF = 0x003F_FFFF
    mmu.flush();
    let pa = mmu.translate(
        0x001F_FFFF,
        AccessType::Execute,
        PrivLevel::User,
        0,
        &reader,
    );
    assert_eq!(pa, Ok(0x003F_FFFF));
}
