//! Full-system execution-level SoftMMU tests.
//!
//! These tests exercise the real JIT runtime path:
//! FullSystemCpu → gen_code → translate → regalloc →
//! codegen → cpu_exec_loop_env → helper dispatch →
//! fault delivery.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use machina_accel::exec::exec_loop::{cpu_exec_loop_env, ExitReason};
use machina_accel::exec::ExecEnv;
use machina_accel::x86_64::emitter::SoftMmuConfig;
use machina_accel::X86_64CodeGen;
use machina_core::address::GPA;
use machina_core::wfi::WfiWaker;
use machina_guest_riscv::riscv::cpu::RiscvCpu;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MemoryRegion;
use machina_system::cpus::{
    fault_cause_offset, fault_pc_offset, machina_mem_read, machina_mem_write,
    tlb_offsets, tlb_ptr_offset, FullSystemCpu, SharedMip, RAM_BASE, TLB_SIZE,
};

/// Build a SoftMmuConfig for test ExecEnv.
fn test_mmu_config() -> SoftMmuConfig {
    SoftMmuConfig {
        tlb_ptr_offset: tlb_ptr_offset(),
        entry_size: tlb_offsets::ENTRY_SIZE,
        addr_read_off: tlb_offsets::ADDR_READ,
        addr_write_off: tlb_offsets::ADDR_WRITE,
        addend_off: tlb_offsets::ADDEND,
        index_mask: (TLB_SIZE - 1) as u64,
        load_helper: machina_mem_read as *const () as u64,
        store_helper: machina_mem_write as *const () as u64,
        fault_cause_offset: fault_cause_offset(),
        fault_pc_offset: fault_pc_offset(),
        dirty_offset: tlb_offsets::DIRTY,
        tb_ret_addr: 0,
    }
}

/// Create a full-system test environment.
/// Returns (ExecEnv, FullSystemCpu, AddressSpace, ram_ptr).
/// `code` is written at RAM_BASE.
fn setup_fullsys(
    ram_size: u64,
    code: &[u8],
) -> (
    ExecEnv<X86_64CodeGen>,
    FullSystemCpu,
    Box<AddressSpace>,
    *const u8,
) {
    let mut backend = X86_64CodeGen::new();
    backend.mmio = Some(test_mmu_config());
    let env = ExecEnv::new(backend);

    // Create RAM-backed address space.
    let root = MemoryRegion::container("root", u64::MAX);
    let (ram_region, ram_block) = MemoryRegion::ram("ram", ram_size);
    let mut addr_space = Box::new(AddressSpace::new(root));
    addr_space
        .root_mut()
        .add_subregion(ram_region, GPA::new(RAM_BASE));
    addr_space.update_flat_view();

    let ram_ptr = ram_block.as_ptr() as *const u8;

    // Write test code at RAM_BASE.
    unsafe {
        std::ptr::copy_nonoverlapping(
            code.as_ptr(),
            ram_block.as_ptr(),
            code.len(),
        );
    }

    let shared_mip: SharedMip = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let wfi_waker = Arc::new(WfiWaker::new());
    let stop_flag = Arc::new(AtomicBool::new(true));

    let cpu = RiscvCpu::new();
    let fscpu = unsafe {
        FullSystemCpu::new(
            cpu,
            ram_ptr,
            ram_size,
            shared_mip,
            wfi_waker,
            &*addr_space as *const AddressSpace,
            stop_flag,
        )
    };

    // Set initial PC to RAM_BASE.
    // (FullSystemCpu::new sets guest_base, as_ptr, ram_end)

    (env, fscpu, addr_space, ram_ptr)
}

// RISC-V instruction encoders for test code.

/// ADDI rd, rs1, imm12 (I-type)
fn addi(rd: u32, rs1: u32, imm: i32) -> u32 {
    let imm12 = (imm as u32) & 0xFFF;
    (imm12 << 20) | (rs1 << 15) | (0b000 << 12) | (rd << 7) | 0x13
}

/// LUI rd, imm20 (U-type)
fn lui(rd: u32, imm20: u32) -> u32 {
    (imm20 << 12) | (rd << 7) | 0x37
}

/// ECALL
fn ecall() -> u32 {
    0x00000073
}

/// SD rs2, offset(rs1)
fn sd(rs2: u32, rs1: u32, offset: i32) -> u32 {
    let imm = (offset as u32) & 0xFFF;
    let imm_hi = (imm >> 5) & 0x7F;
    let imm_lo = imm & 0x1F;
    (imm_hi << 25)
        | (rs2 << 20)
        | (rs1 << 15)
        | (0b011 << 12)
        | (imm_lo << 7)
        | 0x23
}

/// LD rd, offset(rs1)
fn ld(rd: u32, rs1: u32, offset: i32) -> u32 {
    let imm12 = (offset as u32) & 0xFFF;
    (imm12 << 20) | (rs1 << 15) | (0b011 << 12) | (rd << 7) | 0x03
}

/// Encode instructions to bytes (little-endian).
fn encode(insns: &[u32]) -> Vec<u8> {
    insns.iter().flat_map(|i| i.to_le_bytes()).collect()
}

// ═══════════════════════════════════════════════════════
// Full-system execution tests
// ═══════════════════════════════════════════════════════

/// Test: basic RISC-V code execution through the full
/// JIT pipeline. Loads a constant into x1 and ecalls.
#[test]
fn test_fullsys_basic_exec() {
    let code = encode(&[
        addi(1, 0, 42), // x1 = 42
        addi(2, 0, 99), // x2 = 99
        ecall(),        // exit
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys(1024 * 1024, &code);
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 },);
    assert_eq!(cpu.cpu.gpr[1], 42);
    assert_eq!(cpu.cpu.gpr[2], 99);
}

/// Test: RAM load/store through TLB in M-mode BARE.
/// Stores a value then loads it back.
#[test]
fn test_fullsys_ram_load_store() {
    // Use AUIPC to get a PC-relative address in RAM.
    // AUIPC rd, imm20: rd = PC + (imm20 << 12)
    // At PC=0x80000000, auipc x3, 0 → x3 = 0x80000000
    // Then addi x3, x3, 0x100 → x3 = 0x80000100
    fn auipc(rd: u32, imm20: u32) -> u32 {
        (imm20 << 12) | (rd << 7) | 0x17
    }
    let code = encode(&[
        auipc(3, 0),       // x3 = PC = 0x80000000
        addi(3, 3, 0x100), // x3 += 0x100
        addi(1, 0, 0x55),  // x1 = 0x55
        sd(1, 3, 0),       // *(x3) = x1
        ld(2, 3, 0),       // x2 = *(x3)
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys(1024 * 1024, &code);
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 },);
    // x2 should have the stored value.
    assert_eq!(cpu.cpu.gpr[2], 0x55);
}

/// Test: MMIO write goes through AddressSpace (not
/// fast-path RAM). Write to unmapped MMIO address
/// produces a fault.
#[test]
fn test_fullsys_mmio_write_no_crash() {
    // Write to address 0x1000_0000 (UART range).
    // No UART device mapped → AddressSpace silently
    // drops the write (unmapped write returns).
    let code = encode(&[
        lui(3, 0x10000),  // x3 = 0x10000000
        addi(1, 0, 0x41), // x1 = 'A'
        sd(1, 3, 0),      // *(0x10000000) = 'A'
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys(1024 * 1024, &code);
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    // Should reach ecall without crash.
    assert_eq!(r, ExitReason::Ecall { priv_level: 3 },);
}

/// FENCE.I instruction encoding.
fn fence_i() -> u32 {
    0x0000_100F
}

/// CSRRW rd, csr, rs1
fn csrrw(rd: u32, csr: u16, rs1: u32) -> u32 {
    ((csr as u32) << 20) | (rs1 << 15) | (0b001 << 12) | (rd << 7) | 0x73
}

/// CSRRS rd, csr, rs1 (read-set; rs1=0 = read-only)
fn csrrs(rd: u32, csr: u16, rs1: u32) -> u32 {
    ((csr as u32) << 20) | (rs1 << 15) | (0b010 << 12) | (rd << 7) | 0x73
}

/// JAL rd, offset (J-type, offset in bytes)
fn jal(rd: u32, offset: i32) -> u32 {
    let imm = offset as u32;
    let b20 = (imm >> 20) & 1;
    let b10_1 = (imm >> 1) & 0x3FF;
    let b11 = (imm >> 11) & 1;
    let b19_12 = (imm >> 12) & 0xFF;
    (b20 << 31)
        | (b10_1 << 21)
        | (b11 << 20)
        | (b19_12 << 12)
        | (rd << 7)
        | 0x6F
}

// ═══════════════════════════════════════════════════════
// AC-6: fence.i self-modifying code retranslation
// ═══════════════════════════════════════════════════════

/// Test: fence.i causes retranslation of modified code.
///
/// Phase 1: Execute code at offset 0x100 that sets
///          x1 = 42, then ecall.
/// Phase 2: Overwrite the code at 0x100 with x1 = 99,
///          execute fence.i, jump to 0x100, ecall.
///          Should get x1 = 99 (retranslated).
#[test]
fn test_fullsys_fence_i_retranslation() {
    let ram_size: u64 = 1024 * 1024;

    // Phase 1 code at offset 0: jump to 0x100 (skip).
    // Phase 1 code at offset 0x100: addi x1,0,42; ecall
    // Phase 2 entry at offset 0x200: overwrite 0x100
    // with "addi x1,0,99", then fence.i, jump to 0x100.
    let mut code = vec![0u8; 0x400];

    // Offset 0x000: Phase 1 entry — jump to 0x100.
    let phase1_entry = encode(&[jal(0, 0x100)]);
    code[0..4].copy_from_slice(&phase1_entry);

    // Offset 0x100: addi x1, x0, 42; ecall
    let target_code_v1 = encode(&[addi(1, 0, 42), ecall()]);
    code[0x100..0x108].copy_from_slice(&target_code_v1);

    let (mut env, mut cpu, _as, ram) = setup_fullsys(ram_size, &code);
    cpu.cpu.pc = RAM_BASE;

    // Phase 1: execute, expect x1 = 42.
    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };
    assert_eq!(r, ExitReason::Ecall { priv_level: 3 },);
    assert_eq!(cpu.cpu.gpr[1], 42, "phase 1: x1 should be 42",);

    // Phase 2: overwrite code at 0x100 with x1=99.
    let target_code_v2 = encode(&[addi(1, 0, 99), ecall()]);
    unsafe {
        std::ptr::copy_nonoverlapping(
            target_code_v2.as_ptr(),
            (ram as *mut u8).add(0x100),
            target_code_v2.len(),
        );
    }

    // Write phase 2 entry at 0x200: fence.i + jal 0x100
    // Jump offset from 0x200 to 0x100 = -0x100
    let phase2_code = encode(&[
        fence_i(),
        jal(0, -0x104), // from 0x204 to 0x100
    ]);
    unsafe {
        std::ptr::copy_nonoverlapping(
            phase2_code.as_ptr(),
            (ram as *mut u8).add(0x200),
            phase2_code.len(),
        );
    }

    // Execute phase 2 from offset 0x200.
    cpu.cpu.pc = RAM_BASE + 0x200;
    let r2 = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };
    assert_eq!(r2, ExitReason::Ecall { priv_level: 3 },);
    assert_eq!(
        cpu.cpu.gpr[1], 99,
        "phase 2: x1 should be 99 after fence.i \
         retranslation",
    );
}

// ═══════════════════════════════════════════════════════
// AC-4: Precise fault mepc/mtval verification
// ═══════════════════════════════════════════════════════

/// Test: PMP-denied load sets correct mepc and mtval.
///
/// Setup:
/// - PMP: deny region 0x8008_0000..0x8008_1000
/// - mtvec = RAM_BASE + 0x200 (trap handler: ecall)
/// - Code at RAM_BASE: load from denied address, then
///   addi x2,0,1 (should NOT execute after fault).
///
/// After fault: mepc should point to the faulting load,
/// and x2 should still be 0 (no post-fault side effect).
#[test]
fn test_fullsys_precise_fault_mepc() {
    use machina_guest_riscv::riscv::csr::{CSR_PMPADDR0, CSR_PMPCFG0};

    let ram_size: u64 = 1024 * 1024;
    let mut code = vec![0u8; 0x400];

    // Code at 0x000:
    //   auipc x3, 0x80  → x3 = PC + 0x80000
    //                      = 0x80000000 + 0x80000
    //                      = 0x80080000 (denied region)
    //   ld x4, 0(x3)    → faulting load (PMP deny)
    //   addi x2, x0, 1  → should NOT execute
    //   ecall
    fn auipc(rd: u32, imm20: u32) -> u32 {
        (imm20 << 12) | (rd << 7) | 0x17
    }
    let main_code = encode(&[
        auipc(3, 0x80), // x3 = 0x80080000
        ld(4, 3, 0),    // faulting load
        addi(2, 0, 1),  // should not execute
        ecall(),
    ]);
    code[0..main_code.len()].copy_from_slice(&main_code);

    // Trap handler at 0x200: save mepc/mtval, ecall.
    // CSR 0x341 = mepc, 0x343 = mtval
    let trap = encode(&[
        csrrs(5, 0x341, 0), // x5 = mepc
        csrrs(6, 0x343, 0), // x6 = mtval
        ecall(),
    ]);
    code[0x200..0x200 + trap.len()].copy_from_slice(&trap);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys(ram_size, &code);
    cpu.cpu.pc = RAM_BASE;

    // Set mtvec to trap handler.
    cpu.cpu.csr.mtvec = RAM_BASE + 0x200;

    // PMP: entry 0 = TOR up to 0x80080000 (allow RWX)
    // entry 1 = TOR up to 0x80081000 (deny all)
    // entry 2 = TOR up to MAX (allow RWX)
    cpu.cpu
        .csr
        .write(
            CSR_PMPADDR0,
            0x8008_0000 >> 2,
            machina_guest_riscv::riscv::csr::PrivLevel::Machine,
        )
        .unwrap();
    cpu.cpu
        .csr
        .write(
            CSR_PMPADDR0 + 1,
            0x8008_1000 >> 2,
            machina_guest_riscv::riscv::csr::PrivLevel::Machine,
        )
        .unwrap();
    // Two additional entries for full coverage.
    cpu.cpu
        .csr
        .write(
            CSR_PMPADDR0 + 2,
            0x3FFF_FFFF_FFFF,
            machina_guest_riscv::riscv::csr::PrivLevel::Machine,
        )
        .unwrap();
    // pmpcfg0: entry0=TOR|RWX(0x0F),
    // entry1=TOR|Lock(0x88, deny M-mode),
    // entry2=TOR|RWX(0x0F)
    cpu.cpu
        .csr
        .write(
            CSR_PMPCFG0,
            0x0F_88_0F,
            machina_guest_riscv::riscv::csr::PrivLevel::Machine,
        )
        .unwrap();
    cpu.cpu
        .pmp
        .sync_from_csr(&cpu.cpu.csr.pmpcfg, &cpu.cpu.csr.pmpaddr);

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    // Should reach the trap handler's ecall.
    assert_eq!(r, ExitReason::Ecall { priv_level: 3 },);

    // x5 = mepc saved by trap handler.
    // The trap handler read mepc via csrrs after the
    // load fault was delivered.
    let mepc = cpu.cpu.gpr[5];
    // mepc should be within the first TB's code range.
    // mepc should point to the faulting LD instruction
    // (RAM_BASE + 4, after the auipc).
    assert_eq!(
        mepc,
        RAM_BASE + 4,
        "mepc should point to faulting LD, got {:#x}",
        mepc,
    );

    // x6 = mtval saved by trap handler.
    let mtval = cpu.cpu.gpr[6];
    assert_eq!(
        mtval, 0x8008_0000,
        "mtval should be faulting address, got {:#x}",
        mtval,
    );

    // x2 should be 0 (no post-fault side effect).
    assert_eq!(
        cpu.cpu.gpr[2], 0,
        "x2 should be 0: instruction after fault \
         should not execute",
    );
}
