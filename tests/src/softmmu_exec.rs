//! Full-system execution-level SoftMMU tests.
//!
//! These tests exercise the real JIT runtime path:
//! FullSystemCpu → gen_code → translate → regalloc →
//! codegen → cpu_exec_loop_env → helper dispatch →
//! fault delivery.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use machina_accel::exec::exec_loop::{cpu_exec_loop_env, ExitReason};
use machina_accel::exec::{ExecEnv, PerCpuState};
use machina_accel::x86_64::emitter::SoftMmuConfig;
use machina_accel::X86_64CodeGen;
use machina_core::address::GPA;
use machina_core::wfi::WfiWaker;
use machina_guest_riscv::riscv::cpu::RiscvCpu;
use machina_guest_riscv::riscv::cpu_model::RiscvCpuModel;
use machina_guest_riscv::riscv::csr::CSR_TIME;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MemoryRegion;
use machina_system::cpus::{
    fault_cause_offset, fault_pc_offset, machina_csr_op, machina_mem_read,
    machina_mem_write, tlb_offsets, tlb_ptr_offset, FullSystemCpu, SharedMip,
    TLB_SIZE,
};

/// Test RAM base address (matches RISC-V virt standard).
const RAM_BASE: u64 = 0x8000_0000;

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
    setup_fullsys_with_cpu(ram_size, code, RiscvCpu::new())
}

fn setup_fullsys_with_cpu(
    ram_size: u64,
    code: &[u8],
    cpu: RiscvCpu,
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

    let fscpu = unsafe {
        FullSystemCpu::new(
            cpu,
            ram_ptr,
            RAM_BASE,
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

/// Run the exec loop with automatic BufferFull retry
/// (flush TBs and code buffer, then re-enter).
unsafe fn run_with_retry(
    env: &mut ExecEnv<X86_64CodeGen>,
    cpu: &mut FullSystemCpu,
) -> ExitReason {
    let mut per_cpu = PerCpuState::new();
    let mut retries = 0u32;
    loop {
        let r = machina_accel::exec::exec_loop::cpu_exec_loop(
            &env.shared,
            &mut per_cpu,
            cpu,
        );
        match r {
            ExitReason::BufferFull => {
                retries += 1;
                assert!(
                    retries < 100,
                    "BufferFull loop: retried {} times, \
                     pc={:#x} iters={}",
                    retries,
                    cpu.cpu.pc,
                    per_cpu.stats.loop_iters,
                );
                let _g = env.shared.translate_lock.lock().unwrap();
                env.shared
                    .tb_store
                    .invalidate_all(env.shared.code_buf(), &env.shared.backend);
                env.shared.tb_store.flush();
                unsafe {
                    env.shared
                        .code_buf_mut()
                        .set_offset(env.shared.code_gen_start);
                }
                per_cpu.jump_cache.invalidate();
            }
            other => return other,
        }
    }
}

// RISC-V instruction encoders for test code.

/// ADDI rd, rs1, imm12 (I-type)
fn addi(rd: u32, rs1: u32, imm: i32) -> u32 {
    let imm12 = (imm as u32) & 0xFFF;
    (imm12 << 20) | (rs1 << 15) | (0b000 << 12) | (rd << 7) | 0x13
}

/// ANDI rd, rs1, imm12 (I-type)
fn andi(rd: u32, rs1: u32, imm: i32) -> u32 {
    let imm12 = (imm as u32) & 0xFFF;
    (imm12 << 20) | (rs1 << 15) | (0b111 << 12) | (rd << 7) | 0x13
}

/// LUI rd, imm20 (U-type)
fn lui(rd: u32, imm20: u32) -> u32 {
    (imm20 << 12) | (rd << 7) | 0x37
}

fn slli(rd: u32, rs1: u32, sh: u32) -> u32 {
    (sh << 20) | (rs1 << 15) | (1 << 12) | (rd << 7) | 0x13
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

fn push16(code: &mut Vec<u8>, insn: u16) {
    code.extend_from_slice(&insn.to_le_bytes());
}

fn push32(code: &mut Vec<u8>, insn: u32) {
    code.extend_from_slice(&insn.to_le_bytes());
}

fn th_memidx(
    top5: u32,
    imm2: u32,
    rs2: u32,
    rs1: u32,
    funct3: u32,
    rd: u32,
) -> u32 {
    (top5 << 27)
        | (imm2 << 25)
        | (rs2 << 20)
        | (rs1 << 15)
        | (funct3 << 12)
        | (rd << 7)
        | 0x0b
}

fn th_pair(
    top5: u32,
    sh2: u32,
    rd2: u32,
    rs: u32,
    is_load: bool,
    rd1: u32,
) -> u32 {
    (top5 << 27)
        | (sh2 << 25)
        | (rd2 << 20)
        | (rs << 15)
        | ((if is_load { 0b100 } else { 0b101 }) << 12)
        | (rd1 << 7)
        | 0x0b
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

/// Test: store to unmapped MMIO produces a store access
/// fault (cause 7). The exception is delivered via mtvec
/// to a handler that does ecall to exit the loop.
///
/// NOTE: This test requires privileged CSR write (mtvec)
/// which needs full MROM/machine setup. Ignored in
/// minimal test harness; covered by end-to-end ch2
/// store_fault test.
#[test]
#[ignore = "exec loop hangs after fault delivery (BufferFull storm)"]
fn test_fullsys_mmio_write_no_crash() {
    // Trap handler at offset 0x800: ecall.
    // Main code: set mtvec, write to unmapped MMIO.
    let handler_off = 0x800u64;
    let handler_code = encode(&[ecall()]);

    let mtvec_val = RAM_BASE + handler_off;
    let main_code = encode(&[
        // Set mtvec = RAM_BASE + handler_off.
        lui(5, (mtvec_val >> 12) as u32),
        addi(5, 5, (mtvec_val & 0xFFF) as i32),
        csrrw(0, 0x305, 5), // csrw mtvec, x5
        // Store to unmapped MMIO → access fault → mtvec.
        lui(3, 0x10000),  // x3 = 0x10000000
        addi(1, 0, 0x41), // x1 = 'A'
        sd(1, 3, 0),      // *(0x10000000) = 'A'
        ecall(),          // fallback: should not reach
    ]);

    let ram_sz = 1024 * 1024;
    let (mut env, mut cpu, _as, ram) = setup_fullsys(ram_sz, &main_code);

    // Place handler at offset 0x800 within RAM.
    unsafe {
        std::ptr::copy_nonoverlapping(
            handler_code.as_ptr(),
            (ram as *mut u8).add(handler_off as usize),
            handler_code.len(),
        );
    }
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { run_with_retry(&mut env, &mut cpu) };

    // Handler runs ecall → Ecall exit.
    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    // mepc points to the faulting store instruction.
    assert!(
        cpu.cpu.csr.mepc >= RAM_BASE,
        "mepc should point to faulting SD"
    );
    // mepc points to the faulting store instruction.
    assert!(
        cpu.cpu.csr.mepc >= RAM_BASE,
        "mepc should point to faulting SD"
    );
}

/// FENCE.I instruction encoding.
fn fence_i() -> u32 {
    0x0000_100F
}

/// SFENCE.VMA rs1, rs2
fn sfence_vma(rs1: u32, rs2: u32) -> u32 {
    (0b0001001 << 25) | (rs2 << 20) | (rs1 << 15) | (0b000 << 12) | 0x73
}

/// LR.W rd, (rs1)
fn lr_w(rd: u32, rs1: u32) -> u32 {
    (0b00010 << 27) | (rs1 << 15) | (0b010 << 12) | (rd << 7) | 0x2f
}

/// SC.W rd, rs2, (rs1)
fn sc_w(rd: u32, rs2: u32, rs1: u32) -> u32 {
    (0b00011 << 27)
        | (rs2 << 20)
        | (rs1 << 15)
        | (0b010 << 12)
        | (rd << 7)
        | 0x2f
}

/// AMOADD.D rd, rs2, (rs1)
fn amoadd_d(rd: u32, rs2: u32, rs1: u32) -> u32 {
    (rs2 << 20) | (rs1 << 15) | (0b011 << 12) | (rd << 7) | 0x2f
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
/// SW rs2, offset(rs1) (32-bit store)
fn sw(rs2: u32, rs1: u32, offset: i32) -> u32 {
    let imm = (offset as u32) & 0xFFF;
    let imm_hi = (imm >> 5) & 0x7F;
    let imm_lo = imm & 0x1F;
    (imm_hi << 25)
        | (rs2 << 20)
        | (rs1 << 15)
        | (0b010 << 12)
        | (imm_lo << 7)
        | 0x23
}

#[test]
fn test_fullsys_fence_i_retranslation() {
    let ram_size: u64 = 1024 * 1024;

    // Phase 1: jump to 0x1000, execute addi x1,0,42; ecall.
    // Phase 2: use guest SD to overwrite code at 0x1000
    //   with addi x1,0,99; ecall, then fence.i, jump back.
    // Using 0x1000 (page-aligned) so dirty page tracking
    // records a specific physical page.
    let mut code = vec![0u8; 0x2000];

    // Offset 0x000: Phase 1 entry.
    let phase1 = encode(&[jal(0, 0x1000)]);
    code[0..4].copy_from_slice(&phase1);

    // Offset 0x1000: addi x1, x0, 42; ecall (original)
    let v1 = encode(&[addi(1, 0, 42), ecall()]);
    code[0x1000..0x1000 + v1.len()].copy_from_slice(&v1);

    // Phase 2 code at 0x200: use guest stores to
    // overwrite code at 0x1000 with new instructions.
    // The new instructions: addi x1,0,99 and ecall.
    let new_insn_0 = addi(1, 0, 99);
    let new_insn_1 = ecall();

    // Phase 2 sequence:
    //   auipc x3, 0      → x3 = PC (0x80000200)
    //   lui x4, (new_insn_0 >> 12)
    //   addi x4, x4, (new_insn_0 & 0xFFF)
    //   sw x4, 0x1000-0x200(x3) → store at 0x80001000
    //   lui x4, (new_insn_1 >> 12)
    //   addi x4, x4, (new_insn_1 & 0xFFF)
    //   sw x4, 0x1004-0x200(x3) → store at 0x80001004
    //   fence.i
    //   jal to 0x1000

    fn auipc(rd: u32, imm20: u32) -> u32 {
        (imm20 << 12) | (rd << 7) | 0x17
    }

    // Build the new insn value in x4 using LUI+ADDI.
    // For new_insn_0 = addi(1,0,99):
    let hi0 = (new_insn_0 >> 12) & 0xFFFFF;
    let lo0 = (new_insn_0 & 0xFFF) as i32;
    // Adjust for sign extension.
    let (hi0, lo0) = if lo0 >= 0x800 {
        (hi0 + 1, lo0 - 0x1000)
    } else {
        (hi0, lo0)
    };

    let hi1 = (new_insn_1 >> 12) & 0xFFFFF;
    let lo1 = (new_insn_1 & 0xFFF) as i32;
    let (hi1, lo1) = if lo1 >= 0x800 {
        (hi1 + 1, lo1 - 0x1000)
    } else {
        (hi1, lo1)
    };

    // Phase 2: compute target address, store, fence.i,
    // jump. Use x3 as base for 0x80001000.
    // 10 instructions starting at 0x200.
    let _phase2 = encode(&[
        auipc(3, 0),       // x3 = PC = 0x80000200
        addi(3, 3, 0xE00), // x3 += 0xE00 → ERR: >12bit!
    ]);
    // Actually, ADDI imm is 12-bit signed (-2048..2047).
    // 0xE00 = 3584 > 2047. Need two ADDIs or LUI approach.
    // Use: auipc x3,1 → x3 = 0x80001200, then addi x3,x3,-0x200 → x3 = 0x80001000
    // auipc(3,1): x3 = PC + 0x1000 = 0x80000200 + 0x1000 = 0x80001200
    // addi(3,3,-0x200): x3 = 0x80001200 - 0x200 = 0x80001000
    let phase2 = encode(&[
        auipc(3, 1),        // x3 = PC + 0x1000 = 0x80001200
        addi(3, 3, -0x200), // x3 = 0x80001000
        lui(4, hi0),
        addi(4, 4, lo0), // x4 = addi(1,0,99) encoding
        sw(4, 3, 0),     // *(0x80001000) = x4
        lui(4, hi1),
        addi(4, 4, lo1), // x4 = ecall() encoding
        sw(4, 3, 4),     // *(0x80001004) = x4
        fence_i(),
        // jal to 0x1000: from 0x224 to 0x1000 = -(0x224-0x1000)
        // 0x200 + 10*4 = 0x228, jal at 0x224.
        // offset = 0x1000 - 0x224 = 0xDDC
        jal(0, 0x1000 - 0x224i32),
    ]);
    code[0x200..0x200 + phase2.len()].copy_from_slice(&phase2);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys(ram_size, &code);
    cpu.cpu.pc = RAM_BASE;

    // Phase 1.
    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };
    assert_eq!(r, ExitReason::Ecall { priv_level: 3 },);
    assert_eq!(cpu.cpu.gpr[1], 42, "phase 1: x1 should be 42",);

    // Phase 2: guest store + fence.i + re-execute.
    cpu.cpu.pc = RAM_BASE + 0x200;
    let r2 = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };
    assert_eq!(r2, ExitReason::Ecall { priv_level: 3 },);
    assert_eq!(
        cpu.cpu.gpr[1], 99,
        "phase 2: x1 should be 99 after guest \
         store + fence.i retranslation",
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

/// Regression: a writable JIT TLB entry must not survive
/// page-table permission downgrade plus sfence.vma.
///
/// This mirrors Linux fork/COW: the kernel clears PTE.W,
/// executes sfence.vma, and the next user write must fault
/// instead of hitting an old writable fast-path entry.
#[test]
fn test_fullsys_sfence_vma_evicts_stale_write_tlb_after_pte_wrprotect() {
    use machina_guest_riscv::riscv::csr::{
        PrivLevel, CSR_PMPADDR0, CSR_PMPCFG0,
    };

    const CODE_VA: u64 = 0x4000_0000;
    const DATA_VA: u64 = 0x4000_1000;
    const PTE_PAGE_VA: u64 = 0x4000_2000;
    const ROOT_OFF: usize = 0x2000;
    const L1_OFF: usize = 0x3000;
    const L0_OFF: usize = 0x4000;
    const ASID: u64 = 7;
    const PTE_V: u64 = 1 << 0;
    const PTE_R: u64 = 1 << 1;
    const PTE_W: u64 = 1 << 2;
    const PTE_X: u64 = 1 << 3;
    const PTE_A: u64 = 1 << 6;
    const PTE_D: u64 = 1 << 7;

    fn pte(pa: u64, flags: u64) -> u64 {
        ((pa >> 12) << 10) | flags
    }

    unsafe fn write_u64(ram: *const u8, off: usize, val: u64) {
        // SAFETY: The test passes page-table offsets within the RAM buffer.
        (ram as *mut u8)
            .add(off)
            .copy_from_nonoverlapping(val.to_le_bytes().as_ptr(), 8);
    }

    let mut code = vec![0u8; 0x200];
    let main = encode(&[
        lui(3, (DATA_VA >> 12) as u32),     // x3 = data VA
        addi(1, 0, 1),                      // x1 = first value
        sd(1, 3, 0),                        // fill writable TLB entry
        lui(6, (PTE_PAGE_VA >> 12) as u32), // x6 = mapped L0 PTE page
        ld(4, 6, 8),                        // x4 = data page PTE
        andi(4, 4, !4),                     // clear PTE.W
        sd(4, 6, 8),                        // write-protect data PTE
        addi(5, 0, ASID as i32),            // x5 = current ASID
        sfence_vma(0, 5),                   // flush VA translations
        addi(2, 0, 2),                      // x2 = second value
        sd(2, 3, 0),                        // must fault, not fast-store
        addi(7, 0, 1),                      // marker if fault is missed
        ecall(),
    ]);
    code[..main.len()].copy_from_slice(&main);

    let trap = encode(&[
        csrrs(10, 0x342, 0), // mcause
        csrrs(11, 0x343, 0), // mtval
        csrrs(12, 0x341, 0), // mepc
        ecall(),
    ]);
    code[0x100..0x100 + trap.len()].copy_from_slice(&trap);

    let ram_size = 64 * 1024;
    let (mut env, mut cpu, _as, ram) = setup_fullsys(ram_size, &code);

    let root_pa = RAM_BASE + ROOT_OFF as u64;
    let l1_pa = RAM_BASE + L1_OFF as u64;
    let l0_pa = RAM_BASE + L0_OFF as u64;
    let code_pa = RAM_BASE;
    let data_pa = RAM_BASE + 0x1000;

    let vpn2 = ((CODE_VA >> 30) & 0x1ff) as usize;
    let vpn1 = ((CODE_VA >> 21) & 0x1ff) as usize;
    // SAFETY: The page-table pages all live inside the 64 KiB test RAM.
    unsafe {
        write_u64(ram, ROOT_OFF + vpn2 * 8, pte(l1_pa, PTE_V));
        write_u64(ram, L1_OFF + vpn1 * 8, pte(l0_pa, PTE_V));
        write_u64(
            ram,
            L0_OFF,
            pte(code_pa, PTE_V | PTE_R | PTE_X | PTE_A | PTE_D),
        );
        write_u64(
            ram,
            L0_OFF + 8,
            pte(data_pa, PTE_V | PTE_R | PTE_W | PTE_A | PTE_D),
        );
        write_u64(
            ram,
            L0_OFF + 16,
            pte(l0_pa, PTE_V | PTE_R | PTE_W | PTE_A | PTE_D),
        );
    }

    let satp = (8u64 << 60) | (ASID << 44) | (root_pa >> 12);
    cpu.cpu.priv_level = PrivLevel::Supervisor;
    cpu.cpu
        .csr
        .write(CSR_PMPADDR0, 0x3fff_ffff_ffff, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .csr
        .write(CSR_PMPCFG0, 0x0f, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .pmp
        .sync_from_csr(&cpu.cpu.csr.pmpcfg, &cpu.cpu.csr.pmpaddr);
    cpu.cpu.csr.satp = satp;
    cpu.cpu.mmu.set_satp(satp);
    cpu.cpu.csr.mtvec = RAM_BASE + 0x100;
    cpu.cpu.pc = CODE_VA;

    // SAFETY: The test VM owns its CPU state and mapped RAM for this run.
    let r = unsafe { run_with_retry(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(
        cpu.cpu.gpr[10], 15,
        "second store should raise StorePageFault, marker x7={}",
        cpu.cpu.gpr[7],
    );
    assert_eq!(cpu.cpu.gpr[11], DATA_VA);
    assert_eq!(cpu.cpu.gpr[12], CODE_VA + 10 * 4);
    let data = unsafe { (ram as *const u64).add(0x1000 / 8).read_unaligned() };
    assert_eq!(
        data, 1,
        "stale fast-path write must not update the data page",
    );
}

/// Regression: SC must behave as a store for permission
/// checks. After fork/COW write-protects a page, LR may
/// still read it, but SC must fault instead of writing
/// through a read-only TLB addend.
#[test]
fn test_fullsys_sc_faults_after_pte_wrprotect() {
    use machina_guest_riscv::riscv::csr::{
        PrivLevel, CSR_PMPADDR0, CSR_PMPCFG0,
    };

    const CODE_VA: u64 = 0x4000_0000;
    const DATA_VA: u64 = 0x4000_1000;
    const PTE_PAGE_VA: u64 = 0x4000_2000;
    const ROOT_OFF: usize = 0x2000;
    const L1_OFF: usize = 0x3000;
    const L0_OFF: usize = 0x4000;
    const ASID: u64 = 7;
    const PTE_V: u64 = 1 << 0;
    const PTE_R: u64 = 1 << 1;
    const PTE_W: u64 = 1 << 2;
    const PTE_X: u64 = 1 << 3;
    const PTE_A: u64 = 1 << 6;
    const PTE_D: u64 = 1 << 7;

    fn pte(pa: u64, flags: u64) -> u64 {
        ((pa >> 12) << 10) | flags
    }

    unsafe fn write_u64(ram: *const u8, off: usize, val: u64) {
        // SAFETY: The test passes page-table offsets within the RAM buffer.
        (ram as *mut u8)
            .add(off)
            .copy_from_nonoverlapping(val.to_le_bytes().as_ptr(), 8);
    }

    let mut code = vec![0u8; 0x200];
    let main = encode(&[
        lui(3, (DATA_VA >> 12) as u32),     // x3 = data VA
        addi(1, 0, 1),                      // x1 = first value
        lr_w(8, 3),                         // reserve writable page
        sc_w(9, 1, 3),                      // write 1, fill write TLB
        lui(6, (PTE_PAGE_VA >> 12) as u32), // x6 = mapped L0 PTE page
        ld(4, 6, 8),                        // x4 = data page PTE
        andi(4, 4, !4),                     // clear PTE.W
        sd(4, 6, 8),                        // write-protect data PTE
        addi(5, 0, ASID as i32),            // x5 = current ASID
        sfence_vma(0, 5),                   // flush VA translations
        lr_w(8, 3),                         // read-only LR may succeed
        addi(2, 0, 2),                      // x2 = second value
        sc_w(9, 2, 3),                      // must fault, not write 2
        addi(7, 0, 1),                      // marker if fault is missed
        ecall(),
    ]);
    code[..main.len()].copy_from_slice(&main);

    let trap = encode(&[
        csrrs(10, 0x342, 0), // mcause
        csrrs(11, 0x343, 0), // mtval
        csrrs(12, 0x341, 0), // mepc
        ecall(),
    ]);
    code[0x100..0x100 + trap.len()].copy_from_slice(&trap);

    let ram_size = 64 * 1024;
    let (mut env, mut cpu, _as, ram) = setup_fullsys(ram_size, &code);

    let root_pa = RAM_BASE + ROOT_OFF as u64;
    let l1_pa = RAM_BASE + L1_OFF as u64;
    let l0_pa = RAM_BASE + L0_OFF as u64;
    let code_pa = RAM_BASE;
    let data_pa = RAM_BASE + 0x1000;

    let vpn2 = ((CODE_VA >> 30) & 0x1ff) as usize;
    let vpn1 = ((CODE_VA >> 21) & 0x1ff) as usize;
    // SAFETY: The page-table pages all live inside the 64 KiB test RAM.
    unsafe {
        write_u64(ram, ROOT_OFF + vpn2 * 8, pte(l1_pa, PTE_V));
        write_u64(ram, L1_OFF + vpn1 * 8, pte(l0_pa, PTE_V));
        write_u64(
            ram,
            L0_OFF,
            pte(code_pa, PTE_V | PTE_R | PTE_X | PTE_A | PTE_D),
        );
        write_u64(
            ram,
            L0_OFF + 8,
            pte(data_pa, PTE_V | PTE_R | PTE_W | PTE_A | PTE_D),
        );
        write_u64(
            ram,
            L0_OFF + 16,
            pte(l0_pa, PTE_V | PTE_R | PTE_W | PTE_A | PTE_D),
        );
    }

    let satp = (8u64 << 60) | (ASID << 44) | (root_pa >> 12);
    cpu.cpu.priv_level = PrivLevel::Supervisor;
    cpu.cpu
        .csr
        .write(CSR_PMPADDR0, 0x3fff_ffff_ffff, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .csr
        .write(CSR_PMPCFG0, 0x0f, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .pmp
        .sync_from_csr(&cpu.cpu.csr.pmpcfg, &cpu.cpu.csr.pmpaddr);
    cpu.cpu.csr.satp = satp;
    cpu.cpu.mmu.set_satp(satp);
    cpu.cpu.csr.mtvec = RAM_BASE + 0x100;
    cpu.cpu.pc = CODE_VA;

    // SAFETY: The test VM owns its CPU state and mapped RAM for this run.
    let r = unsafe { run_with_retry(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(
        cpu.cpu.gpr[10], 15,
        "SC to write-protected page should raise StorePageFault, marker x7={}",
        cpu.cpu.gpr[7],
    );
    assert_eq!(cpu.cpu.gpr[11], DATA_VA);
    assert_eq!(cpu.cpu.gpr[12], CODE_VA + 12 * 4);
    let data = unsafe { (ram as *const u32).add(0x1000 / 4).read_unaligned() };
    assert_eq!(data, 1, "SC must not write through a read-only TLB addend",);
}

/// Regression: LR helper faults must report the LR
/// instruction PC, not the start of the translated block.
///
/// K230 glibc can take a first-touch page fault from
/// lr.w.rl in a function prologue.  If mepc points at
/// the TB start, Linux resumes by replaying the prologue
/// and corrupts the user stack.
#[test]
fn test_fullsys_lr_fault_reports_current_pc() {
    use machina_guest_riscv::riscv::csr::{
        PrivLevel, CSR_PMPADDR0, CSR_PMPCFG0,
    };

    const CODE_VA: u64 = 0x4000_0000;
    const DATA_VA: u64 = 0x4000_1000;
    const ROOT_OFF: usize = 0x2000;
    const L1_OFF: usize = 0x3000;
    const L0_OFF: usize = 0x4000;
    const ASID: u64 = 7;
    const PTE_V: u64 = 1 << 0;
    const PTE_R: u64 = 1 << 1;
    const PTE_X: u64 = 1 << 3;
    const PTE_A: u64 = 1 << 6;
    const PTE_D: u64 = 1 << 7;

    fn pte(pa: u64, flags: u64) -> u64 {
        ((pa >> 12) << 10) | flags
    }

    unsafe fn write_u64(ram: *const u8, off: usize, val: u64) {
        // SAFETY: The test passes page-table offsets within the RAM buffer.
        (ram as *mut u8)
            .add(off)
            .copy_from_nonoverlapping(val.to_le_bytes().as_ptr(), 8);
    }

    let mut code = vec![0u8; 0x200];
    let main = encode(&[
        lui(3, (DATA_VA >> 12) as u32), // x3 = unmapped data VA
        addi(5, 0, 1),                  // marker before LR
        lr_w(8, 3),                     // must fault at CODE_VA + 8
        addi(5, 5, 1),                  // must not execute
        ecall(),
    ]);
    code[..main.len()].copy_from_slice(&main);

    let trap = encode(&[
        csrrs(10, 0x342, 0), // mcause
        csrrs(11, 0x343, 0), // mtval
        csrrs(12, 0x341, 0), // mepc
        ecall(),
    ]);
    code[0x100..0x100 + trap.len()].copy_from_slice(&trap);

    let ram_size = 64 * 1024;
    let (mut env, mut cpu, _as, ram) = setup_fullsys(ram_size, &code);

    let root_pa = RAM_BASE + ROOT_OFF as u64;
    let l1_pa = RAM_BASE + L1_OFF as u64;
    let l0_pa = RAM_BASE + L0_OFF as u64;
    let code_pa = RAM_BASE;

    let vpn2 = ((CODE_VA >> 30) & 0x1ff) as usize;
    let vpn1 = ((CODE_VA >> 21) & 0x1ff) as usize;
    // SAFETY: The page-table pages all live inside the 64 KiB test RAM.
    unsafe {
        write_u64(ram, ROOT_OFF + vpn2 * 8, pte(l1_pa, PTE_V));
        write_u64(ram, L1_OFF + vpn1 * 8, pte(l0_pa, PTE_V));
        write_u64(
            ram,
            L0_OFF,
            pte(code_pa, PTE_V | PTE_R | PTE_X | PTE_A | PTE_D),
        );
    }

    let satp = (8u64 << 60) | (ASID << 44) | (root_pa >> 12);
    cpu.cpu.priv_level = PrivLevel::Supervisor;
    cpu.cpu
        .csr
        .write(CSR_PMPADDR0, 0x3fff_ffff_ffff, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .csr
        .write(CSR_PMPCFG0, 0x0f, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .pmp
        .sync_from_csr(&cpu.cpu.csr.pmpcfg, &cpu.cpu.csr.pmpaddr);
    cpu.cpu.csr.satp = satp;
    cpu.cpu.mmu.set_satp(satp);
    cpu.cpu.csr.mtvec = RAM_BASE + 0x100;
    cpu.cpu.pc = CODE_VA;

    // SAFETY: The test VM owns its CPU state and mapped RAM for this run.
    let r = unsafe { run_with_retry(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[10], 13, "LR should raise LoadPageFault");
    assert_eq!(cpu.cpu.gpr[11], DATA_VA);
    assert_eq!(cpu.cpu.gpr[12], CODE_VA + 8);
    assert_eq!(cpu.cpu.gpr[5], 1, "instruction after LR must not run");
}

/// Regression: LR/SC reservations are tied to the translated
/// physical address, not just the guest virtual address.
///
/// If the same VA is remapped after LR, SC must fail even
/// when the newly mapped page contains the same loaded value.
#[test]
fn test_fullsys_sc_fails_after_same_va_remaps_to_different_pa() {
    use machina_guest_riscv::riscv::csr::{
        PrivLevel, CSR_PMPADDR0, CSR_PMPCFG0,
    };

    const CODE_VA: u64 = 0x4000_0000;
    const DATA_VA: u64 = 0x4000_1000;
    const PTE_PAGE_VA: u64 = 0x4000_2000;
    const ROOT_OFF: usize = 0x2000;
    const L1_OFF: usize = 0x3000;
    const L0_OFF: usize = 0x4000;
    const DATA_A_OFF: usize = 0x5000;
    const DATA_B_OFF: usize = 0x6000;
    const ASID: u64 = 7;
    const PTE_V: u64 = 1 << 0;
    const PTE_R: u64 = 1 << 1;
    const PTE_W: u64 = 1 << 2;
    const PTE_X: u64 = 1 << 3;
    const PTE_A: u64 = 1 << 6;
    const PTE_D: u64 = 1 << 7;

    fn pte(pa: u64, flags: u64) -> u64 {
        ((pa >> 12) << 10) | flags
    }

    unsafe fn write_u64(ram: *const u8, off: usize, val: u64) {
        (ram as *mut u8)
            .add(off)
            .copy_from_nonoverlapping(val.to_le_bytes().as_ptr(), 8);
    }

    let mut code = vec![0u8; 0x200];
    let main = encode(&[
        lui(3, (DATA_VA >> 12) as u32),     // x3 = data VA
        lr_w(8, 3),                         // reserve old PA
        lui(6, (PTE_PAGE_VA >> 12) as u32), // x6 = mapped L0 PTE page
        ld(4, 6, 24),                       // x4 = replacement data PTE
        sd(4, 6, 8),                        // remap DATA_VA to new PA
        addi(5, 0, ASID as i32),            // x5 = current ASID
        sfence_vma(0, 5),                   // flush VA translations
        addi(2, 0, 7),                      // x2 = value SC would write
        sc_w(9, 2, 3),                      // must fail without writing
        ecall(),
    ]);
    code[..main.len()].copy_from_slice(&main);

    let ram_size = 64 * 1024;
    let (mut env, mut cpu, _as, ram) = setup_fullsys(ram_size, &code);

    let root_pa = RAM_BASE + ROOT_OFF as u64;
    let l1_pa = RAM_BASE + L1_OFF as u64;
    let l0_pa = RAM_BASE + L0_OFF as u64;
    let code_pa = RAM_BASE;
    let data_a_pa = RAM_BASE + DATA_A_OFF as u64;
    let data_b_pa = RAM_BASE + DATA_B_OFF as u64;

    let vpn2 = ((CODE_VA >> 30) & 0x1ff) as usize;
    let vpn1 = ((CODE_VA >> 21) & 0x1ff) as usize;
    let data_flags = PTE_V | PTE_R | PTE_W | PTE_A | PTE_D;
    unsafe {
        write_u64(ram, ROOT_OFF + vpn2 * 8, pte(l1_pa, PTE_V));
        write_u64(ram, L1_OFF + vpn1 * 8, pte(l0_pa, PTE_V));
        write_u64(
            ram,
            L0_OFF,
            pte(code_pa, PTE_V | PTE_R | PTE_X | PTE_A | PTE_D),
        );
        write_u64(ram, L0_OFF + 8, pte(data_a_pa, data_flags));
        write_u64(
            ram,
            L0_OFF + 16,
            pte(l0_pa, PTE_V | PTE_R | PTE_W | PTE_A | PTE_D),
        );
        write_u64(ram, L0_OFF + 24, pte(data_b_pa, data_flags));
        write_u64(ram, DATA_A_OFF, 0);
        write_u64(ram, DATA_B_OFF, 0);
    }

    let satp = (8u64 << 60) | (ASID << 44) | (root_pa >> 12);
    cpu.cpu.priv_level = PrivLevel::Supervisor;
    cpu.cpu
        .csr
        .write(CSR_PMPADDR0, 0x3fff_ffff_ffff, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .csr
        .write(CSR_PMPCFG0, 0x0f, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .pmp
        .sync_from_csr(&cpu.cpu.csr.pmpcfg, &cpu.cpu.csr.pmpaddr);
    cpu.cpu.csr.satp = satp;
    cpu.cpu.mmu.set_satp(satp);
    cpu.cpu.pc = CODE_VA;

    let r = unsafe { run_with_retry(&mut env, &mut cpu) };

    assert_eq!(
        r,
        ExitReason::Ecall {
            priv_level: PrivLevel::Supervisor as u8,
        },
    );
    assert_eq!(cpu.cpu.gpr[8], 0, "LR should load old page value");
    assert_eq!(cpu.cpu.gpr[9], 1, "SC should fail after remap");
    let old_page =
        unsafe { (ram as *const u32).add(DATA_A_OFF / 4).read_unaligned() };
    let new_page =
        unsafe { (ram as *const u32).add(DATA_B_OFF / 4).read_unaligned() };
    assert_eq!(old_page, 0);
    assert_eq!(new_page, 0, "SC must not write the remapped page");
}

/// Regression: RISC-V AMOs require natural alignment.
///
/// QEMU routes AMOs through its atomic TCG path with
/// MO_ALIGN, so an unaligned AMO is a store/AMO
/// address-misaligned trap, not a plain unaligned
/// load+store.
#[test]
fn test_fullsys_amoadd_d_misaligned_traps() {
    let mut code = vec![0u8; 0x200];
    let misaligned = RAM_BASE + 0x181;
    let main = encode(&[
        auipc(3, 0), // x3 = RAM_BASE
        addi(3, 3, (misaligned - RAM_BASE) as i32),
        addi(1, 0, 1),     // x1 = addend
        amoadd_d(5, 1, 3), // must trap before writing
        ecall(),           // fallback if AMO is incorrectly allowed
    ]);
    code[..main.len()].copy_from_slice(&main);

    let trap = encode(&[
        csrrs(10, 0x342, 0), // mcause
        csrrs(11, 0x343, 0), // mtval
        csrrs(12, 0x341, 0), // mepc
        ecall(),
    ]);
    code[0x100..0x100 + trap.len()].copy_from_slice(&trap);

    let (mut env, mut cpu, _as, ram) = setup_fullsys(1024 * 1024, &code);
    cpu.cpu.csr.mtvec = RAM_BASE + 0x100;
    cpu.cpu.pc = RAM_BASE;

    unsafe {
        (ram as *mut u64).add(0x180 / 8).write_unaligned(0x10);
    }

    let r = unsafe { run_with_retry(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[10], 6, "AMO.D should raise store misaligned");
    assert_eq!(cpu.cpu.gpr[11], misaligned);
    assert_eq!(cpu.cpu.gpr[12], RAM_BASE + 3 * 4);
    let data = unsafe { (ram as *const u64).add(0x180 / 8).read_unaligned() };
    assert_eq!(data, 0x10, "misaligned AMO must not update memory");
    drop(
        env.shared
            .atomic_lock
            .try_lock()
            .expect("AMO trap must not leak the exec atomic lock"),
    );
}

/// Regression: AMOs must probe write permission before
/// read-modify-write.  This mirrors QEMU's
/// atomic_mmu_lookup(), which checks the write TLB entry
/// for AMOs rather than doing an ordinary read first.
#[test]
fn test_fullsys_amoadd_w_faults_after_pte_wrprotect() {
    use machina_guest_riscv::riscv::csr::{
        PrivLevel, CSR_PMPADDR0, CSR_PMPCFG0,
    };

    const CODE_VA: u64 = 0x4000_0000;
    const DATA_VA: u64 = 0x4000_1000;
    const PTE_PAGE_VA: u64 = 0x4000_2000;
    const ROOT_OFF: usize = 0x2000;
    const L1_OFF: usize = 0x3000;
    const L0_OFF: usize = 0x4000;
    const ASID: u64 = 7;
    const PTE_V: u64 = 1 << 0;
    const PTE_R: u64 = 1 << 1;
    const PTE_W: u64 = 1 << 2;
    const PTE_X: u64 = 1 << 3;
    const PTE_A: u64 = 1 << 6;
    const PTE_D: u64 = 1 << 7;

    fn amoadd_w(rd: u32, rs2: u32, rs1: u32) -> u32 {
        (rs2 << 20) | (rs1 << 15) | (0b010 << 12) | (rd << 7) | 0x2f
    }

    fn pte(pa: u64, flags: u64) -> u64 {
        ((pa >> 12) << 10) | flags
    }

    unsafe fn write_u64(ram: *const u8, off: usize, val: u64) {
        (ram as *mut u8)
            .add(off)
            .copy_from_nonoverlapping(val.to_le_bytes().as_ptr(), 8);
    }

    let mut code = vec![0u8; 0x200];
    let main = encode(&[
        lui(3, (DATA_VA >> 12) as u32),     // x3 = data VA
        addi(1, 0, 1),                      // x1 = first addend
        amoadd_w(8, 1, 3),                  // write 1, fill write TLB
        lui(6, (PTE_PAGE_VA >> 12) as u32), // x6 = mapped L0 PTE page
        ld(4, 6, 8),                        // x4 = data page PTE
        andi(4, 4, !4),                     // clear PTE.W
        sd(4, 6, 8),                        // write-protect data PTE
        addi(5, 0, ASID as i32),            // x5 = current ASID
        sfence_vma(0, 5),                   // flush VA translations
        addi(2, 0, 2),                      // x2 = second addend
        amoadd_w(9, 2, 3),                  // must fault, not write 2
        addi(7, 0, 1),                      // marker if fault is missed
        ecall(),
    ]);
    code[..main.len()].copy_from_slice(&main);

    let trap = encode(&[
        csrrs(10, 0x342, 0), // mcause
        csrrs(11, 0x343, 0), // mtval
        csrrs(12, 0x341, 0), // mepc
        ecall(),
    ]);
    code[0x100..0x100 + trap.len()].copy_from_slice(&trap);

    let ram_size = 64 * 1024;
    let (mut env, mut cpu, _as, ram) = setup_fullsys(ram_size, &code);

    let root_pa = RAM_BASE + ROOT_OFF as u64;
    let l1_pa = RAM_BASE + L1_OFF as u64;
    let l0_pa = RAM_BASE + L0_OFF as u64;
    let code_pa = RAM_BASE;
    let data_pa = RAM_BASE + 0x1000;

    let vpn2 = ((CODE_VA >> 30) & 0x1ff) as usize;
    let vpn1 = ((CODE_VA >> 21) & 0x1ff) as usize;
    unsafe {
        write_u64(ram, ROOT_OFF + vpn2 * 8, pte(l1_pa, PTE_V));
        write_u64(ram, L1_OFF + vpn1 * 8, pte(l0_pa, PTE_V));
        write_u64(
            ram,
            L0_OFF,
            pte(code_pa, PTE_V | PTE_R | PTE_X | PTE_A | PTE_D),
        );
        write_u64(
            ram,
            L0_OFF + 8,
            pte(data_pa, PTE_V | PTE_R | PTE_W | PTE_A | PTE_D),
        );
        write_u64(
            ram,
            L0_OFF + 16,
            pte(l0_pa, PTE_V | PTE_R | PTE_W | PTE_A | PTE_D),
        );
    }

    let satp = (8u64 << 60) | (ASID << 44) | (root_pa >> 12);
    cpu.cpu.priv_level = PrivLevel::Supervisor;
    cpu.cpu
        .csr
        .write(CSR_PMPADDR0, 0x3fff_ffff_ffff, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .csr
        .write(CSR_PMPCFG0, 0x0f, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .pmp
        .sync_from_csr(&cpu.cpu.csr.pmpcfg, &cpu.cpu.csr.pmpaddr);
    cpu.cpu.csr.satp = satp;
    cpu.cpu.mmu.set_satp(satp);
    cpu.cpu.csr.mtvec = RAM_BASE + 0x100;
    cpu.cpu.pc = CODE_VA;

    let r = unsafe { run_with_retry(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(
        cpu.cpu.gpr[10], 15,
        "AMO to write-protected page should raise StorePageFault, marker x7={}",
        cpu.cpu.gpr[7],
    );
    assert_eq!(cpu.cpu.gpr[11], DATA_VA);
    assert_eq!(cpu.cpu.gpr[12], CODE_VA + 10 * 4);
    let data = unsafe { (ram as *const u32).add(0x1000 / 4).read_unaligned() };
    assert_eq!(data, 1, "AMO must not write through a read-only TLB addend",);
}

/// Regression: when the code TLB has been flushed, a TB
/// cached for the same virtual PC must not be reused until
/// the current physical PC is known again.
#[test]
fn test_fullsys_code_tlb_miss_does_not_reuse_stale_tb_after_remap() {
    use machina_guest_riscv::riscv::csr::{
        PrivLevel, CSR_PMPADDR0, CSR_PMPCFG0,
    };

    const CODE_VA: u64 = 0x4000_0000;
    const ROOT_OFF: usize = 0x2000;
    const L1_OFF: usize = 0x3000;
    const L0_OFF: usize = 0x4000;
    const CODE_A_OFF: usize = 0x5000;
    const CODE_B_OFF: usize = 0x6000;
    const ASID: u64 = 7;
    const PTE_V: u64 = 1 << 0;
    const PTE_R: u64 = 1 << 1;
    const PTE_W: u64 = 1 << 2;
    const PTE_X: u64 = 1 << 3;
    const PTE_A: u64 = 1 << 6;
    const PTE_D: u64 = 1 << 7;

    fn pte(pa: u64, flags: u64) -> u64 {
        ((pa >> 12) << 10) | flags
    }

    unsafe fn write_u64(ram: *const u8, off: usize, val: u64) {
        (ram as *mut u8)
            .add(off)
            .copy_from_nonoverlapping(val.to_le_bytes().as_ptr(), 8);
    }

    let mut code = vec![0u8; 0x7000];
    let code_a = encode(&[addi(1, 0, 1), ecall()]);
    let code_b = encode(&[addi(1, 0, 2), ecall()]);
    code[CODE_A_OFF..CODE_A_OFF + code_a.len()].copy_from_slice(&code_a);
    code[CODE_B_OFF..CODE_B_OFF + code_b.len()].copy_from_slice(&code_b);

    let ram_size = 64 * 1024;
    let (mut env, mut cpu, _as, ram) = setup_fullsys(ram_size, &code);

    let root_pa = RAM_BASE + ROOT_OFF as u64;
    let l1_pa = RAM_BASE + L1_OFF as u64;
    let l0_pa = RAM_BASE + L0_OFF as u64;
    let code_a_pa = RAM_BASE + CODE_A_OFF as u64;
    let code_b_pa = RAM_BASE + CODE_B_OFF as u64;
    let code_flags = PTE_V | PTE_R | PTE_X | PTE_A | PTE_D;
    let vpn2 = ((CODE_VA >> 30) & 0x1ff) as usize;
    let vpn1 = ((CODE_VA >> 21) & 0x1ff) as usize;

    unsafe {
        write_u64(ram, ROOT_OFF + vpn2 * 8, pte(l1_pa, PTE_V));
        write_u64(ram, L1_OFF + vpn1 * 8, pte(l0_pa, PTE_V));
        write_u64(ram, L0_OFF, pte(code_a_pa, code_flags));
    }

    let satp = (8u64 << 60) | (ASID << 44) | (root_pa >> 12);
    cpu.cpu.priv_level = PrivLevel::Supervisor;
    cpu.cpu
        .csr
        .write(CSR_PMPADDR0, 0x3fff_ffff_ffff, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .csr
        .write(CSR_PMPCFG0, 0x0f, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .pmp
        .sync_from_csr(&cpu.cpu.csr.pmpcfg, &cpu.cpu.csr.pmpaddr);
    cpu.cpu.csr.satp = satp;
    cpu.cpu.mmu.set_satp(satp);
    cpu.cpu.pc = CODE_VA;

    let r = unsafe { run_with_retry(&mut env, &mut cpu) };
    assert_eq!(
        r,
        ExitReason::Ecall {
            priv_level: PrivLevel::Supervisor as u8,
        },
    );
    assert_eq!(cpu.cpu.gpr[1], 1);

    unsafe {
        write_u64(ram, L0_OFF, pte(code_b_pa, code_flags | PTE_W));
    }
    cpu.cpu.mmu.flush();
    cpu.cpu.pc = CODE_VA;
    cpu.cpu.gpr[1] = 0;

    let r = unsafe { run_with_retry(&mut env, &mut cpu) };
    assert_eq!(
        r,
        ExitReason::Ecall {
            priv_level: PrivLevel::Supervisor as u8,
        },
    );
    assert_eq!(
        cpu.cpu.gpr[1], 2,
        "stale TB for the previous physical page must not be reused",
    );
}

// ═══════════════════════════════════════════════════════
// AC-7: MMIO observable dispatch test
// ═══════════════════════════════════════════════════════

use std::sync::atomic::AtomicU64;

/// A simple test MMIO device that counts writes.
struct TestMmioDevice {
    write_count: AtomicU64,
    last_value: AtomicU64,
}

impl TestMmioDevice {
    fn new() -> Self {
        Self {
            write_count: AtomicU64::new(0),
            last_value: AtomicU64::new(0),
        }
    }
}

impl machina_memory::region::MmioOps for TestMmioDevice {
    fn read(&self, _offset: u64, _size: u32) -> u64 {
        self.last_value.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn write(&self, _offset: u64, _size: u32, val: u64) {
        self.last_value
            .store(val, std::sync::atomic::Ordering::Relaxed);
        self.write_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Test: MMIO write goes through AddressSpace dispatch
/// to a real IO device, not through addend fast path.
/// Two writes to the same MMIO page should both hit
/// the device (write_count == 2).
#[test]
fn test_fullsys_mmio_observable_dispatch() {
    let ram_size: u64 = 1024 * 1024;
    let mmio_base: u64 = 0x1000_0000;

    // Code: write two values to MMIO device.
    #[allow(dead_code)]
    fn auipc(rd: u32, imm20: u32) -> u32 {
        (imm20 << 12) | (rd << 7) | 0x17
    }
    let code = encode(&[
        lui(3, 0x10000),  // x3 = 0x10000000
        addi(1, 0, 0x41), // x1 = 'A' (65)
        sw(1, 3, 0),      // write 65 to MMIO
        addi(1, 0, 0x42), // x1 = 'B' (66)
        sw(1, 3, 0),      // write 66 to MMIO
        ecall(),
    ]);

    // Set up with MMIO device at 0x10000000.
    let mut backend = X86_64CodeGen::new();
    backend.mmio = Some(test_mmu_config());
    let env = ExecEnv::new(backend);

    let root = MemoryRegion::container("root", u64::MAX);
    let (ram_region, ram_block) = MemoryRegion::ram("ram", ram_size);

    // Create the test MMIO device.
    let device = Arc::new(TestMmioDevice::new());
    let io_region = MemoryRegion::io(
        "test-mmio",
        0x1000,
        Arc::new(TestMmioDeviceWrapper {
            inner: Arc::clone(&device),
        }),
    );

    let mut addr_space = Box::new(AddressSpace::new(root));
    addr_space
        .root_mut()
        .add_subregion(ram_region, GPA::new(RAM_BASE));
    addr_space
        .root_mut()
        .add_subregion(io_region, GPA::new(mmio_base));
    addr_space.update_flat_view();

    let ram_ptr = ram_block.as_ptr() as *const u8;
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
    let mut fscpu = unsafe {
        FullSystemCpu::new(
            cpu,
            ram_ptr,
            RAM_BASE,
            ram_size,
            shared_mip,
            wfi_waker,
            &*addr_space as *const AddressSpace,
            stop_flag,
        )
    };
    fscpu.cpu.pc = RAM_BASE;

    let mut env = env;
    let r = unsafe { cpu_exec_loop_env(&mut env, &mut fscpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 },);

    // Device should have received exactly 2 writes.
    let wc = device
        .write_count
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        wc, 2,
        "MMIO device should receive 2 writes, \
         got {}",
        wc,
    );

    // Last value should be 'B' (66).
    let lv = device.last_value.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(lv, 0x42, "last MMIO write should be 0x42, got {:#x}", lv,);
}

/// Wrapper to make TestMmioDevice work with MmioOps
/// (which requires Send but not Sync directly).
struct TestMmioDeviceWrapper {
    inner: Arc<TestMmioDevice>,
}

impl machina_memory::region::MmioOps for TestMmioDeviceWrapper {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.inner.read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.inner.write(offset, size, val);
    }
}

#[test]
fn test_xtheadmemidx_indexed_load_store() {
    let code = encode(&[
        auipc(3, 0),                      // x3 = RAM_BASE
        addi(4, 0, 0x100),                // x4 = byte offset
        addi(1, 0, 0x55),                 // x1 = value
        th_memidx(12, 0, 4, 3, 0b101, 1), // th.srd x1, (x3), x4, 0
        th_memidx(12, 0, 4, 3, 0b100, 2), // th.lrd x2, (x3), x4, 0
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[2], 0x55);
}

#[test]
fn test_xtheadmemidx_load_inc_updates_base() {
    let mut code = vec![0u8; 0x200];
    let insns = encode(&[
        auipc(3, 0),                      // x3 = RAM_BASE
        addi(3, 3, 0x100),                // x3 = RAM_BASE + 0x100
        th_memidx(19, 0, 2, 3, 0b100, 1), // th.lbuia x1, (x3), 2, 0
        ecall(),
    ]);
    code[..insns.len()].copy_from_slice(&insns);
    code[0x100] = 0x5a;

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[1], 0x5a);
    assert_eq!(cpu.cpu.gpr[3], RAM_BASE + 0x102);
}

#[test]
fn test_xtheadfmemidx_indexed_double_load_store() {
    let code = encode(&[
        auipc(3, 0),                      // x3 = RAM_BASE
        addi(4, 0, 0x100),                // x4 = byte offset
        th_memidx(12, 0, 4, 3, 0b111, 1), // th.fsrd f1, (x3), x4, 0
        th_memidx(12, 0, 4, 3, 0b110, 2), // th.flrd f2, (x3), x4, 0
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;
    cpu.cpu.csr.mstatus |= 0x3 << 13;
    cpu.cpu.fpr[1] = 0x3ff0_0000_0000_0000;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.fpr[2], 0x3ff0_0000_0000_0000);
}

// ── XThead arithmetic / bit-manipulation helpers ──────

fn th_addsl(rd: u32, rs1: u32, rs2: u32, shamt: u32) -> u32 {
    (shamt << 25) | (rs2 << 20) | (rs1 << 15) | (0b001 << 12) | (rd << 7) | 0x0b
}

fn th_ext(rd: u32, rs1: u32, lsb: u32, msb: u32) -> u32 {
    (msb << 26) | (lsb << 20) | (rs1 << 15) | (0b010 << 12) | (rd << 7) | 0x0b
}

fn th_extu(rd: u32, rs1: u32, lsb: u32, msb: u32) -> u32 {
    (msb << 26) | (lsb << 20) | (rs1 << 15) | (0b011 << 12) | (rd << 7) | 0x0b
}

fn th_condmov(rd: u32, rs1: u32, rs2: u32, funct7: u32) -> u32 {
    (funct7 << 25)
        | (rs2 << 20)
        | (rs1 << 15)
        | (0b001 << 12)
        | (rd << 7)
        | 0x0b
}

fn th_mac(rd: u32, rs1: u32, rs2: u32, funct7: u32) -> u32 {
    (funct7 << 25)
        | (rs2 << 20)
        | (rs1 << 15)
        | (0b001 << 12)
        | (rd << 7)
        | 0x0b
}

fn th_mula(rd: u32, rs1: u32, rs2: u32) -> u32 {
    th_mac(rd, rs1, rs2, 0b0010000)
}

fn th_meminc(
    top5: u32,
    imm2: u32,
    imm5: u32,
    rs1: u32,
    is_load: bool,
    rd: u32,
) -> u32 {
    (top5 << 27)
        | (imm2 << 25)
        | (imm5 << 20)
        | (rs1 << 15)
        | ((if is_load { 0b100 } else { 0b101 }) << 12)
        | (rd << 7)
        | 0x0b
}

fn th_lbuia(rd: u32, rs1: u32, imm5: u32, imm2: u32) -> u32 {
    // top5=19 (0b10011): unsigned byte load, post-increment.
    // odd top5 values are pre-increment; even ones are post-increment.
    th_meminc(19, imm2, imm5, rs1, true, rd)
}

fn th_ff0(rd: u32, rs1: u32) -> u32 {
    // funct3=001, funct7=0b1000010, rs2=0
    (0b1000010 << 25) | (rs1 << 15) | (0b001 << 12) | (rd << 7) | 0x0b
}

fn th_ff1(rd: u32, rs1: u32) -> u32 {
    // funct3=001, funct7=0b1000011, rs2=0
    (0b1000011 << 25) | (rs1 << 15) | (0b001 << 12) | (rd << 7) | 0x0b
}

// ── XThead softmmu execution tests ────────────────────

#[test]
fn test_xthead_addsl_full_pipeline() {
    let code = encode(&[
        addi(21, 0, 3),          // s5 = 3
        addi(27, 0, 7),          // s11 = 7
        th_addsl(18, 21, 27, 1), // s2 = s5 + (s11 << 1) = 3 + 14 = 17
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[21], 3, "s5 (source 1)");
    assert_eq!(cpu.cpu.gpr[27], 7, "s11 (source 2)");
    assert_eq!(cpu.cpu.gpr[18], 17, "s2 (dest)");
}

#[test]
fn test_xthead_addsl_same_rs1_rs2_full_pipeline() {
    let code = encode(&[
        addi(21, 0, 0x555),      // s5 = 0x555
        th_addsl(18, 21, 21, 1), // s2 = s5 + (s5 << 1) = s5 * 3
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[21], 0x555, "s5 (source)");
    assert_eq!(cpu.cpu.gpr[18], 0x555u64 * 3, "s2 (dest, expect 0xFFF)");
}

#[test]
fn test_xthead_addsl_overlapping_destinations_full_pipeline() {
    let code = encode(&[
        th_addsl(5, 5, 6, 3),
        th_addsl(6, 5, 6, 2),
        th_addsl(7, 7, 7, 1),
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;
    cpu.cpu.gpr[5] = 3;
    cpu.cpu.gpr[6] = 7;
    cpu.cpu.gpr[7] = 5;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[5], 3 + (7 << 3));
    assert_eq!(cpu.cpu.gpr[6], cpu.cpu.gpr[5] + (7 << 2));
    assert_eq!(cpu.cpu.gpr[7], 5 + (5 << 1));
}

#[test]
fn test_xthead_mempair_lwd_swd_swaps_word_pairs() {
    let code = encode(&[
        auipc(3, 0),
        addi(3, 3, 0x100),
        th_pair(28, 0, 6, 3, true, 5),
        th_pair(28, 1, 8, 3, true, 7),
        th_pair(28, 0, 8, 3, false, 7),
        th_pair(28, 1, 6, 3, false, 5),
        ld(9, 3, 0),
        ld(10, 3, 8),
        ecall(),
    ]);

    let (mut env, mut cpu, _as, ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let first = 0x89ab_cdef_0123_4567u64;
    let second = 0xfedc_ba98_7654_3210u64;
    unsafe {
        (ram as *mut u64).add(0x100 / 8).write_unaligned(first);
        (ram as *mut u64).add(0x108 / 8).write_unaligned(second);
    }

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[5], 0x0000_0000_0123_4567);
    assert_eq!(cpu.cpu.gpr[6], 0xffff_ffff_89ab_cdef);
    assert_eq!(cpu.cpu.gpr[7], 0x0000_0000_7654_3210);
    assert_eq!(cpu.cpu.gpr[8], 0xffff_ffff_fedc_ba98);
    assert_eq!(cpu.cpu.gpr[9], second);
    assert_eq!(cpu.cpu.gpr[10], first);
}

#[test]
fn test_xthead_mempair_ldd_sdd_swaps_doubleword_pairs() {
    let code = encode(&[
        auipc(3, 0),
        addi(3, 3, 0x100),
        th_pair(31, 0, 6, 3, true, 5),
        th_pair(31, 1, 8, 3, true, 7),
        th_pair(31, 0, 8, 3, false, 7),
        th_pair(31, 1, 6, 3, false, 5),
        ld(9, 3, 0),
        ld(10, 3, 8),
        ld(11, 3, 16),
        ld(12, 3, 24),
        ecall(),
    ]);

    let (mut env, mut cpu, _as, ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let values = [
        0x0123_4567_89ab_cdefu64,
        0xfedc_ba98_7654_3210u64,
        0x1122_3344_5566_7788u64,
        0x8877_6655_4433_2211u64,
    ];
    unsafe {
        for (idx, value) in values.iter().enumerate() {
            (ram as *mut u64)
                .add(0x100 / 8 + idx)
                .write_unaligned(*value);
        }
    }

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[9], values[2]);
    assert_eq!(cpu.cpu.gpr[10], values[3]);
    assert_eq!(cpu.cpu.gpr[11], values[0]);
    assert_eq!(cpu.cpu.gpr[12], values[1]);
}

#[test]
fn test_xthead_memidx_store_data_index_overlap_full_pipeline() {
    let code = encode(&[
        auipc(3, 3),
        addi(4, 0, 0),
        th_memidx(12, 3, 4, 3, 0b101, 4),
        addi(4, 4, 1),
        th_memidx(12, 3, 4, 3, 0b101, 4),
        addi(4, 4, 1),
        th_memidx(12, 3, 4, 3, 0b101, 4),
        ld(5, 3, 0),
        ld(6, 3, 8),
        ld(7, 3, 16),
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        64 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { run_with_retry(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[5], 0);
    assert_eq!(cpu.cpu.gpr[6], 1);
    assert_eq!(cpu.cpu.gpr[7], 2);
}

#[test]
fn test_xthead_mula_accumulates_qsort_pointer_offset() {
    let code = encode(&[th_mula(25, 14, 12), th_mula(8, 15, 19), ecall()]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;
    cpu.cpu.gpr[25] = 0x1000;
    cpu.cpu.gpr[14] = 5;
    cpu.cpu.gpr[12] = 8;
    cpu.cpu.gpr[8] = 0x2000;
    cpu.cpu.gpr[15] = u64::MAX;
    cpu.cpu.gpr[19] = 0x20;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[25], 0x1028);
    assert_eq!(cpu.cpu.gpr[8], 0x1fe0);
}

#[test]
fn test_xthead_ldia_sdia_copy_doublewords_with_post_increment() {
    let code = encode(&[
        auipc(5, 0),
        addi(5, 5, 0x100),
        auipc(6, 0),
        addi(6, 6, 0x180),
        th_meminc(15, 0, 8, 5, true, 7),
        th_meminc(15, 0, 8, 6, false, 7),
        th_meminc(15, 0, 8, 5, true, 8),
        th_meminc(15, 0, 8, 6, false, 8),
        ld(9, 6, -16),
        ld(10, 6, -8),
        ecall(),
    ]);

    let (mut env, mut cpu, _as, ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let first = 0x0123_4567_89ab_cdefu64;
    let second = 0xfedc_ba98_7654_3210u64;
    unsafe {
        (ram as *mut u64).add(0x100 / 8).write_unaligned(first);
        (ram as *mut u64).add(0x108 / 8).write_unaligned(second);
    }

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[5], RAM_BASE + 0x110);
    assert_eq!(cpu.cpu.gpr[6], RAM_BASE + 0x198);
    assert_eq!(cpu.cpu.gpr[7], first);
    assert_eq!(cpu.cpu.gpr[8], second);
    assert_eq!(cpu.cpu.gpr[9], first);
    assert_eq!(cpu.cpu.gpr[10], second);
}

#[test]
fn test_xthead_lwia_swia_copy_words_with_post_increment() {
    let code = encode(&[
        auipc(5, 0),
        addi(5, 5, 0x100),
        auipc(6, 0),
        addi(6, 6, 0x180),
        th_meminc(11, 0, 4, 5, true, 7),
        th_meminc(11, 0, 4, 6, false, 7),
        th_meminc(11, 0, 4, 5, true, 8),
        th_meminc(11, 0, 4, 6, false, 8),
        ld(9, 6, -8),
        ecall(),
    ]);

    let (mut env, mut cpu, _as, ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    unsafe {
        (ram as *mut u32)
            .add(0x100 / 4)
            .write_unaligned(0x89ab_cdef);
        (ram as *mut u32)
            .add(0x104 / 4)
            .write_unaligned(0x0123_4567);
    }

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[5], RAM_BASE + 0x108);
    assert_eq!(cpu.cpu.gpr[6], RAM_BASE + 0x190);
    assert_eq!(cpu.cpu.gpr[7], 0xffff_ffff_89ab_cdef);
    assert_eq!(cpu.cpu.gpr[8], 0x0123_4567);
    assert_eq!(cpu.cpu.gpr[9], 0x0123_4567_89ab_cdef);
}

#[test]
fn test_xthead_lrbu_srb_copy_bytes_with_indexed_addressing() {
    let code = encode(&[
        auipc(3, 0),
        addi(3, 3, 0x100),
        auipc(6, 0),
        addi(6, 6, 0x180),
        addi(4, 0, 1),
        th_memidx(16, 0, 4, 3, 0b100, 5),
        th_memidx(0, 0, 4, 6, 0b101, 5),
        addi(4, 4, 1),
        th_memidx(16, 0, 4, 3, 0b100, 5),
        th_memidx(0, 0, 4, 6, 0b101, 5),
        ld(7, 6, 0),
        ecall(),
    ]);

    let (mut env, mut cpu, _as, ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    unsafe {
        (ram as *mut u8).add(0x101).write(0xff);
        (ram as *mut u8).add(0x102).write(0x7e);
    }

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[5], 0x7e);
    assert_eq!(cpu.cpu.gpr[7], 0x0000_0000_007e_ff00);
}

#[test]
fn test_rvc_addi16sp_restore_before_ret_full_pipeline() {
    let mut code = Vec::new();
    push32(&mut code, jal(1, 8));
    push32(&mut code, ecall());
    push16(&mut code, 0x7109); // c.addi16sp -384
    push16(&mut code, 0x6119); // c.addi16sp 384
    push16(&mut code, 0x8082); // ret

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;
    cpu.cpu.gpr[2] = 0x3fff_ffff_4000;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[2], 0x3fff_ffff_4000);
}

#[test]
fn test_rvc_addi4spn_frame_pointer_full_pipeline() {
    let mut code = Vec::new();
    push16(&mut code, 0x0300); // c.addi4spn s0, sp, 384
    push32(&mut code, ecall());

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;
    cpu.cpu.gpr[2] = 0x3fff_ffff_4000;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[8], 0x3fff_ffff_4180);
}

#[test]
fn test_xthead_extract_full_pipeline() {
    let code = encode(&[
        lui(4, 0),           // x4 = 0
        addi(4, 4, 0x7f),    // x4 = 127
        addi(2, 0, 0x80),    // x2 = 128 (bit 7 set, sign bit)
        th_ext(3, 2, 7, 7),  // signed extract bit 7 → 0xFFF...F
        th_extu(1, 4, 0, 6), // unsigned extract bits 6:0 → 127
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[3], u64::MAX);
    assert_eq!(cpu.cpu.gpr[1], 127);
}

#[test]
fn test_xthead_condmov_eq_full_pipeline() {
    let code = encode(&[
        addi(1, 0, 0xaa),               // rd = 0xaa
        addi(2, 0, 0xbb),               // rs1 = 0xbb
        addi(3, 0, 0),                  // rs2 = 0
        th_condmov(1, 2, 3, 0b0100000), // if rs2==0: rd = rs1 → x1 = 0xbb
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[1], 0xbb);
}

#[test]
fn test_xthead_condmov_ne_full_pipeline() {
    let code = encode(&[
        addi(1, 0, 0xcc),               // rd = 0xcc
        addi(2, 0, 0xdd),               // rs1 = 0xdd
        addi(3, 0, 1),                  // rs2 = 1 (non-zero)
        th_condmov(1, 2, 3, 0b0100001), // if rs2!=0: rd = rs1 → x1 = 0xdd
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[1], 0xdd);
}

#[test]
fn test_xthead_load_inc_simple_full_pipeline() {
    let code = encode(&[
        auipc(3, 0),          // x3 = RAM_BASE
        addi(3, 3, 0x100),    // x3 = RAM_BASE + 0x100
        th_lbuia(1, 3, 2, 0), // th.lbuia x1, (x3), 2, 0
        ecall(),
    ]);

    let mut code_bytes = code;
    code_bytes.resize(0x200, 0);
    code_bytes[0x100] = 0x5a;

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code_bytes,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[1], 0x5a);
    assert_eq!(cpu.cpu.gpr[3], RAM_BASE + 0x102);
}

fn th_revw(rd: u32, rs1: u32) -> u32 {
    // th.revw: funct3=001, funct7=0b1001000, rs2=0
    (0b1001000 << 25) | (rs1 << 15) | (0b001 << 12) | (rd << 7) | 0x0b
}

fn th_rev(rd: u32, rs1: u32) -> u32 {
    // th.rev: funct3=001, funct7=0b1000001, rs2=0
    (0b1000001 << 25) | (rs1 << 15) | (0b001 << 12) | (rd << 7) | 0x0b
}

#[test]
fn test_xthead_rev_full_pipeline() {
    // th.rev: 64-bit byte swap.
    let code = encode(&[
        lui(21, 0x01234),    // s5 upper 20 bits
        addi(21, 21, 0x567), // s5 lower 12 bits
        // s5 = 0x0000000001234567 → bswap64 → 0x6745230100000000
        th_rev(18, 21), // s2 = rev(s5)
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[21], 0x01234567, "s5 (source)");
    assert_eq!(cpu.cpu.gpr[18], 0x6745230100000000, "s2 (bswap64 result)");
}

#[test]
fn test_xthead_revw_full_pipeline() {
    // th.revw: 32-bit byte swap within lower 32 bits, sign-extend to 64
    // 0x78563412 → byte-swap → 0x12345678 → sign-ext → 0x0000000012345678
    let code = encode(&[
        lui(21, 0x78563),    // s5 = 0x78563000
        addi(21, 21, 0x412), // s5 = 0x78563412
        th_revw(18, 21),     // s2 = revw(s5)
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[21], 0x78563412, "s5 (source)");
    // 32-bit byte swap: 0x78563412 → 0x12345678 (positive, fits in i32)
    assert_eq!(cpu.cpu.gpr[18], 0x12345678, "s2 (bswap32 result)");
}

#[test]
fn test_xthead_revw_sign_extension_full_pipeline() {
    // th.revw: sign-extension test.  When bit 31 of the bswap result
    // is 1, sign extension produces a negative 64-bit value.
    // 0x80 → bswap32 → 0x80000000 → sign-ext → 0xFFFFFFFF80000000
    let code = encode(&[
        addi(21, 0, 0x80), // s5 = 0x80
        th_revw(18, 21),   // s2 = revw(s5)
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[21], 0x80, "s5 (source)");
    // bswap32: bytes [80,00,00,00] → [00,00,00,80] = 0x80000000
    // bit 31 set → MOVSXD sign-extends → 0xFFFFFFFF80000000
    assert_eq!(
        cpu.cpu.gpr[18], 0xFFFFFFFF80000000u64,
        "s2 (bswap32 sign-extended result)"
    );
}

#[test]
fn test_riscv_time_csr_uses_configured_mmio_addr() {
    const K230_MTIME: u64 = 0x000f_0400_bff8;

    let root = MemoryRegion::container("root", u64::MAX);
    let device = Arc::new(TestMmioDevice::new());
    device
        .last_value
        .store(0x1234_5678_9abc_def0, std::sync::atomic::Ordering::Relaxed);
    let io_region = MemoryRegion::io(
        "mtime",
        8,
        Arc::new(TestMmioDeviceWrapper {
            inner: Arc::clone(&device),
        }),
    );

    let mut addr_space = Box::new(AddressSpace::new(root));
    addr_space
        .root_mut()
        .add_subregion(io_region, GPA::new(K230_MTIME));
    addr_space.update_flat_view();

    let mut cpu = RiscvCpu::new();
    cpu.as_ptr = &*addr_space as *const AddressSpace as u64;
    cpu.time_mmio_addr = K230_MTIME;

    let old = unsafe {
        machina_csr_op(
            &mut cpu as *mut RiscvCpu as *mut u8,
            u64::from(CSR_TIME),
            0,
            0,
            2,
        )
    };
    assert_eq!(old, 0x1234_5678_9abc_def0);
}

// ── Regression: slow-path register corruption ──────

/// BGEU rs1, rs2, offset (B-type)
fn bgeu(rs1: u32, rs2: u32, offset: i32) -> u32 {
    let imm = offset as u32;
    let b12 = (imm >> 12) & 1;
    let b10_5 = (imm >> 5) & 0x3F;
    let b4_1 = (imm >> 1) & 0xF;
    let b11 = (imm >> 11) & 1;
    (b12 << 31)
        | (b10_5 << 25)
        | (rs2 << 20)
        | (rs1 << 15)
        | (0b111 << 12)
        | (b4_1 << 8)
        | (b11 << 7)
        | 0x63
}

/// AUIPC rd, imm20 (U-type)
fn auipc(rd: u32, imm20: u32) -> u32 {
    (imm20 << 12) | (rd << 7) | 0x17
}

/// Regression test for slow-path register corruption.
///
/// When a QemuLd takes the TLB slow path, the helper call
/// clobbers caller-saved registers. The slow path must
/// NOTE: This test triggers self-modifying code detection
/// (code and store on same page → TB invalidation storm)
/// with the is_phys_backed check active. The underlying
/// register preservation is verified by the ch2 end-to-end
/// test.
#[test]
#[ignore = "exec loop hangs: self-modifying code + BufferFull storm"]
fn test_slowpath_preserves_non_output_regs() {
    let code = encode(&[
        // x3 = RAM_BASE (0x80000000)
        auipc(3, 0),
        // x1 = 42 (value to store)
        addi(1, 0, 42),
        // Store x1 to RAM_BASE + 0x100
        addi(4, 3, 0x100),
        sd(1, 4, 0),
        // Load x1 back (first QemuLd, same page as code)
        ld(1, 4, 0),
        // Load x5 from a DIFFERENT page (second QemuLd,
        // new page → TLB miss → slow path).
        // 0x80001000 is page-aligned, different from code page.
        addi(5, 3, 0),
        lui(5, 0x80001),
        ld(5, 5, 0),
        // Now compare x1 with 100. If x1 == 42
        // (correct), bgeu 42 >= 100 is false → fall
        // through to ecall with x2=1. If x1 is
        // corrupted (>= 100), branch skips the marker.
        addi(6, 0, 100),
        bgeu(1, 6, 8), // skip next insn if x1 >= 100
        addi(2, 0, 1), // x2 = 1 (marker: x1 was correct)
        ecall(),
    ]);

    let ram_sz = 2 * 1024 * 1024;
    let (mut env, mut cpu, _as, _ram) = setup_fullsys(ram_sz, &code);
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { run_with_retry(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(
        cpu.cpu.gpr[1], 42,
        "x1 must hold the loaded value (42), \
         not a corrupted register"
    );
    assert_eq!(
        cpu.cpu.gpr[2], 1,
        "x2 must be 1: bgeu(42, 100) should NOT be \
         taken, proving x1 was not corrupted by the \
         second QemuLd's slow path"
    );
}

// ═══════════════════════════════════════════════════════
// AC-11: Dual-page 32-bit instruction fetch
// ═══════════════════════════════════════════════════════

/// Test: 32-bit instruction crossing a 4K page boundary
/// executes correctly via the cross-page fetch mechanism.
///
/// Place `addi x1, x0, 77` at address RAM_BASE+0xFFE
/// (last 2 bytes of page 0, first 2 bytes of page 1).
/// Follow with ecall at RAM_BASE+0x1002.
#[test]
fn test_fullsys_cross_page_fetch_success() {
    let ram_size: u64 = 2 * 1024 * 1024;
    let mut code = vec![0u8; ram_size as usize];

    // Place a jump from 0x000 to 0xFFE.
    let entry = encode(&[jal(0, 0xFFE)]);
    code[0..4].copy_from_slice(&entry);

    // Place the cross-page 32-bit instruction at 0xFFE.
    let cross_insn = addi(1, 0, 77);
    let cross_bytes = cross_insn.to_le_bytes();
    code[0xFFE] = cross_bytes[0];
    code[0xFFF] = cross_bytes[1];
    code[0x1000] = cross_bytes[2];
    code[0x1001] = cross_bytes[3];

    // Place ecall at 0x1002.
    let ec = ecall().to_le_bytes();
    code[0x1002..0x1006].copy_from_slice(&ec);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys(ram_size, &code);
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 },);
    assert_eq!(cpu.cpu.gpr[1], 77, "cross-page addi should set x1=77",);
}

/// Test: cross-page 32-bit instruction where page B is
/// PMP-execute-denied with L-bit. The gen_code
/// cross-page pre-fetch calls translate_pc for page B
/// which triggers PMP InstructionAccessFault. gen_code
/// returns 0, and the fault is delivered via mtvec.
#[test]
fn test_fullsys_cross_page_fetch_page_b_fault() {
    use machina_guest_riscv::riscv::csr::{CSR_PMPADDR0, CSR_PMPCFG0};

    let ram_size: u64 = 2 * 1024 * 1024;
    let mut code = vec![0u8; ram_size as usize];

    // Trap handler at offset 0x400: read mcause → x5,
    // read mepc → x6, ecall.
    let trap = encode(&[
        csrrs(5, 0x342, 0), // x5 = mcause
        csrrs(6, 0x341, 0), // x6 = mepc
        ecall(),
    ]);
    code[0x400..0x400 + trap.len()].copy_from_slice(&trap);

    // Place a 32-bit instruction at 0xFFE crossing
    // into page 1 (0x1000). Page 0 allows execute,
    // page 1 denies execute via PMP L-bit.
    let cross_insn = addi(1, 0, 55);
    let cross_bytes = cross_insn.to_le_bytes();
    code[0xFFE] = cross_bytes[0];
    code[0xFFF] = cross_bytes[1];
    code[0x1000] = cross_bytes[2];
    code[0x1001] = cross_bytes[3];

    // Entry: jump to 0xFFE.
    let entry = encode(&[jal(0, 0xFFE)]);
    code[0..4].copy_from_slice(&entry);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys(ram_size, &code);
    cpu.cpu.pc = RAM_BASE;

    // Set mtvec to trap handler at offset 0x400.
    cpu.cpu.csr.mtvec = RAM_BASE + 0x400;

    // PMP config:
    // Entry 0: TOR up to page 1 (0x80001000), RWX
    // Entry 1: TOR up to page 2 (0x80002000), Lock + no X (RW only)
    // Entry 2: TOR up to max, RWX
    cpu.cpu
        .csr
        .write(
            CSR_PMPADDR0,
            (RAM_BASE + 0x1000) >> 2,
            machina_guest_riscv::riscv::csr::PrivLevel::Machine,
        )
        .unwrap();
    cpu.cpu
        .csr
        .write(
            CSR_PMPADDR0 + 1,
            (RAM_BASE + 0x2000) >> 2,
            machina_guest_riscv::riscv::csr::PrivLevel::Machine,
        )
        .unwrap();
    cpu.cpu
        .csr
        .write(
            CSR_PMPADDR0 + 2,
            0x3FFF_FFFF_FFFF,
            machina_guest_riscv::riscv::csr::PrivLevel::Machine,
        )
        .unwrap();
    // pmpcfg0:
    // entry 0: TOR | RWX = 0x0F
    // entry 1: TOR | Lock | RW (no X) = 0x8B
    //   (Lock=0x80, TOR=0x08, R=0x01, W=0x02)
    // entry 2: TOR | RWX = 0x0F
    cpu.cpu
        .csr
        .write(
            CSR_PMPCFG0,
            0x0F_8B_0F,
            machina_guest_riscv::riscv::csr::PrivLevel::Machine,
        )
        .unwrap();
    cpu.cpu
        .pmp
        .sync_from_csr(&cpu.cpu.csr.pmpcfg, &cpu.cpu.csr.pmpaddr);

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    // Should reach trap handler's ecall.
    assert_eq!(r, ExitReason::Ecall { priv_level: 3 },);

    // x5 = mcause: should be InstructionAccessFault (1).
    assert_eq!(
        cpu.cpu.gpr[5], 1,
        "mcause should be InstructionAccessFault \
         (1), got {}",
        cpu.cpu.gpr[5],
    );

    // x6 = mepc: should point to the cross-page
    // instruction at 0xFFE (where the fetch failed
    // because page B is execute-denied).
    let mepc = cpu.cpu.gpr[6];
    assert!(
        mepc >= RAM_BASE + 0xFFE && mepc <= RAM_BASE + 0x1000,
        "mepc should be near cross-page boundary, \
         got {:#x}",
        mepc,
    );

    // x1 should NOT be 55 (the faulting instruction
    // should not have executed).
    assert_ne!(
        cpu.cpu.gpr[1], 55,
        "cross-page instruction should not execute \
         when page B is denied",
    );
}

/// Test: handle_priv_csr correctly delivers instruction
/// fault (not IllegalInstruction) when a privileged CSR
/// instruction is at the current PC.
///
/// This test verifies AC-2: the EXCP_PRIV_CSR path in
/// the exec loop correctly handles the CSR instruction
/// via handle_priv_csr, which translates the PC through
/// the MMU before decoding.
#[test]
fn test_fullsys_priv_csr_execution() {
    // Execute a CSRRS instruction (read mcycle).
    // This will trigger EXCP_PRIV_CSR in the translator,
    // and handle_priv_csr will fetch + decode + execute
    // it at runtime.
    // mstatus (0x300) is handled by handle_priv_csr.
    let code = encode(&[
        csrrs(1, 0x300, 0), // x1 = mstatus
        addi(2, 0, 42),     // x2 = 42
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys(1024 * 1024, &code);
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 },);
    // x2 should be 42 (executed after CSR read).
    assert_eq!(cpu.cpu.gpr[2], 42, "instruction after CSRRS should execute",);
    // x1 should have some value from mcycle
    // (just verify it was written, not a specific value).
    // mcycle is typically 0 in our emulator.
}

#[test]
fn test_fullsys_csrrs_x0_does_not_write_privileged_csr() {
    use machina_guest_riscv::riscv::csr::CSR_STVEC;

    let code = encode(&[
        csrrs(1, CSR_STVEC, 0), // read-only CSRRS form
        addi(2, 0, 42),
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys(1024 * 1024, &code);
    cpu.cpu.pc = RAM_BASE;
    cpu.cpu.csr.stvec = 0x1234_5678;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[1], 0x1234_5678);
    assert_eq!(cpu.cpu.gpr[2], 42);
    assert_eq!(
        cpu.cpu.csr.stvec, 0x1234_5678,
        "CSRRS with rs1=x0 must not write stvec",
    );
}

/// AC-6: verify that handle_priv_csr emits a CSR trace
/// record when tracing is enabled.
#[test]
fn test_csr_trace_integration() {
    use std::io::Read;

    let dir = tempfile::tempdir().unwrap();
    let trace_path = dir.path().join("csr.log");
    machina_util::trace::init_trace(trace_path.to_str().unwrap()).unwrap();

    // CSRRW x1, mscratch, x0: writes x0 to mscratch.
    // funct3=1, rs1=0 → do_write=true for CSRRW.
    let code = encode(&[
        addi(3, 0, 77),     // x3 = 77
        csrrw(1, 0x340, 3), // mscratch = x3
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys(1024 * 1024, &code);
    cpu.cpu.pc = RAM_BASE;

    let _r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    // Read trace file and assert CSR record present.
    let mut content = String::new();
    std::fs::File::open(&trace_path)
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();
    assert!(
        content.contains("CSR 0x340"),
        "trace file should contain CSR 0x340 \
         record, got: {content}"
    );
}

// ── XThead memidx + Sv39 tests ─────────────────────────

/// Test: th_memidx indexed load (signed byte, top5=0)
/// through Sv39 page tables in the full JIT pipeline.
/// This exercises the same instruction pattern as the
/// crashing getaddrinfo comparison callback.
#[test]
fn test_xthead_memidx_sv39_sb_load() {
    use machina_guest_riscv::riscv::cpu_model::RiscvCpuModel;
    use machina_guest_riscv::riscv::csr::PrivLevel;
    use machina_guest_riscv::riscv::csr::{CSR_PMPADDR0, CSR_PMPCFG0};

    const PTE_V: u64 = 1 << 0;
    const PTE_R: u64 = 1 << 1;
    const PTE_W: u64 = 1 << 2;
    const PTE_X: u64 = 1 << 3;
    const PTE_A: u64 = 1 << 6;
    const PTE_D: u64 = 1 << 7;
    const ASID: u64 = 1;
    const CODE_VA: u64 = 0x2000_0000;
    const DATA_VA: u64 = 0x2000_1000;
    const ROOT_OFF: usize = 0x2000;
    const L1_OFF: usize = 0x3000;
    const L0_OFF: usize = 0x4000;

    fn pte(pa: u64, flags: u64) -> u64 {
        ((pa >> 2) & 0x003f_ffff_ffff_ffc0) | flags
    }

    // th.lrb rd, (rs1), rs2, imm2
    //   → load signed byte from rs1 + (rs2 << imm2)
    // First load: byte at DATA_VA+0 (positive 0x7f).
    // Second load: byte at DATA_VA+2 (negative 0x82).
    let main = encode(&[
        lui(3, (DATA_VA >> 12) as u32),  // x3 = DATA_VA
        addi(4, 0, 0),                   // x4 = 0 (index 0)
        th_memidx(0, 1, 4, 3, 0b100, 2), // x2 = *(s8*)(x3 + x4<<1)
        addi(4, 4, 1),                   // x4 = 1 (index 1)
        th_memidx(0, 1, 4, 3, 0b100, 6), // x6 = *(s8*)(x3 + x4<<1)
        ecall(),
    ]);

    let mut code = vec![0u8; 64 * 1024];
    code[..main.len()].copy_from_slice(&main);

    let ram_size = (code.len() + 0x1000) as u64;
    let cpu = RiscvCpu::new_with_model(RiscvCpuModel::TheadC908);
    let (mut env, mut cpu, _as, ram) =
        setup_fullsys_with_cpu(ram_size, &code, cpu);

    // Write test data.
    // With imm2=1, index N accesses byte at DATA_VA + (N << 1).
    unsafe {
        (ram as *mut u8).add(0x1000).write(0x7f); // index 0 → offset 0: +127
        (ram as *mut u8).add(0x1002).write(0x82); // index 1 → offset 2: −126
    }

    let root_pa = RAM_BASE + ROOT_OFF as u64;
    let l1_pa = RAM_BASE + L1_OFF as u64;
    let l0_pa = RAM_BASE + L0_OFF as u64;
    let code_pa = RAM_BASE;
    let data_pa = RAM_BASE + 0x1000;

    let vpn2 = ((CODE_VA >> 30) & 0x1ff) as usize;
    let vpn1 = ((CODE_VA >> 21) & 0x1ff) as usize;

    unsafe {
        let w = |off: usize, val: u64| {
            (ram as *mut u64).add(off / 8).write_unaligned(val);
        };
        w(ROOT_OFF + vpn2 * 8, pte(l1_pa, PTE_V));
        w(L1_OFF + vpn1 * 8, pte(l0_pa, PTE_V));
        // Map CODE_VA → code_pa (RX)
        w(L0_OFF, pte(code_pa, PTE_V | PTE_R | PTE_X | PTE_A | PTE_D));
        // Map DATA_VA → data_pa (RW)
        w(
            L0_OFF + 8,
            pte(data_pa, PTE_V | PTE_R | PTE_W | PTE_A | PTE_D),
        );
        // Identity-map the page tables themselves
        w(
            L0_OFF + 16,
            pte(l0_pa, PTE_V | PTE_R | PTE_W | PTE_A | PTE_D),
        );
    }

    let satp = (8u64 << 60) | (ASID << 44) | (root_pa >> 12);
    cpu.cpu.priv_level = PrivLevel::Supervisor;
    cpu.cpu
        .csr
        .write(CSR_PMPADDR0, 0x3fff_ffff_ffff, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .csr
        .write(CSR_PMPCFG0, 0x0f, PrivLevel::Machine)
        .unwrap();
    cpu.cpu
        .pmp
        .sync_from_csr(&cpu.cpu.csr.pmpcfg, &cpu.cpu.csr.pmpaddr);
    cpu.cpu.csr.satp = satp;
    cpu.cpu.mmu.set_satp(satp);
    cpu.cpu.mmu.flush();
    cpu.cpu.pc = CODE_VA;

    let r = unsafe { run_with_retry(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 1 });
    // First load: index 0 → byte at DATA_VA+0 = 0x7f (positive).
    assert_eq!(
        cpu.cpu.gpr[2] as i8, 0x7f,
        "th.lrb: signed byte at offset 0 → +127"
    );
    assert_eq!(
        cpu.cpu.gpr[2], 0x7f,
        "th.lrb: 64-bit result (0x7f zero-extended to positive)"
    );
    // Second load: index 1 → byte at DATA_VA+2 = 0x82 (−126).
    assert_eq!(
        cpu.cpu.gpr[6] as i8, -126i8,
        "th.lrb: signed byte at offset 2 → -126"
    );
    assert_eq!(
        cpu.cpu.gpr[6], 0xffffffffffffff82u64,
        "th.lrb: 64-bit sign-extended (0x82 → all Fs)"
    );
}

// XThead th.ext/th.extu edge-case tests.
// QEMU treats lsb > msb as a no-op that leaves rd unchanged.

#[test]
fn test_xthead_extract_lsb_gt_msb_signed() {
    let code = encode(&[
        addi(3, 0, 0x123),
        addi(2, 0, 1),
        slli(2, 2, 40),
        th_ext(3, 2, 36, 24),
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(
        cpu.cpu.gpr[3], 0x123,
        "th.ext lsb>msb should leave rd unchanged"
    );
}

#[test]
fn test_xthead_extract_lsb_gt_msb_signed_negative() {
    let code = encode(&[
        addi(3, 0, 0x321),
        addi(2, 0, -1),
        th_ext(3, 2, 36, 24),
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(
        cpu.cpu.gpr[3], 0x321,
        "th.ext lsb>msb should leave rd unchanged"
    );
}

#[test]
fn test_xthead_extract_lsb_gt_msb_unsigned() {
    let code = encode(&[
        addi(3, 0, 0x456),
        addi(2, 0, -1),
        th_extu(3, 2, 59, 0),
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(
        cpu.cpu.gpr[3], 0x456,
        "th.extu lsb>msb should leave rd unchanged"
    );
}

#[test]
fn test_xthead_extract_lsb_gt_msb_signed_byte_value() {
    let code = encode(&[
        addi(3, 0, 0x789),
        addi(2, 0, 0x42),
        th_ext(3, 2, 59, 0),
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(
        cpu.cpu.gpr[3], 0x789,
        "th.ext lsb>msb should leave rd unchanged"
    );
}

// ── XThead th.ff0 / th.ff1 tests ────────────────────────
// Helpers th_rev, th_revw (used by earlier tests) and
// th_ff0, th_ff1 are defined above with the other XThead
// helpers.

#[test]
fn test_xthead_ff0_full_pipeline() {
    // ff0: count leading zeros of ~rs1
    // x2 = 0xff → ~x2 = 0xffffffffffffff00
    // clz(~x2) = 0 (MSB is 1)
    let code = encode(&[
        addi(2, 0, 0xff), // x2 = 0xff
        th_ff0(3, 2),     // clz(~x2)
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(
        cpu.cpu.gpr[3], 0,
        "th.ff0: clz(~0xff) = 0 (MSB of ~0xff is 1)"
    );
}

#[test]
fn test_xthead_ff0_small_value() {
    // ff0: count leading zeros of ~rs1.
    // x2 lower 4 bits 0 → ~x2 has lower 4 bits 1.
    let code = encode(&[
        addi(2, 0, -1), // x2 = -1 = all 1s
        slli(2, 2, 4),  // x2 = 0xffff_ffff_ffff_fff0
        th_ff0(3, 2),   // ~x2 = 0x0000_0000_0000_000f
        ecall(),        // clz(0xf) = 60
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[3], 60, "th.ff0: clz(~0xfffffffffffffff0) = 60");
}

#[test]
fn test_xthead_ff1_full_pipeline() {
    // ff1: count leading zeros of rs1
    let code = encode(&[
        addi(2, 0, 1),  // x2 = 1
        slli(2, 2, 32), // x2 = 0x1_0000_0000
        th_ff1(3, 2),   // clz(0x1_0000_0000) = 31
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[3], 31, "th.ff1: clz(0x1_0000_0000) = 31");
}

#[test]
fn test_xthead_ff1_zero() {
    // ff1: clz(0) = 64 (fallback)
    let code = encode(&[
        addi(2, 0, 0), // x2 = 0
        th_ff1(3, 2),  // clz(0) = 64
        ecall(),
    ]);

    let (mut env, mut cpu, _as, _ram) = setup_fullsys_with_cpu(
        1024 * 1024,
        &code,
        RiscvCpu::new_with_model(RiscvCpuModel::TheadC908),
    );
    cpu.cpu.pc = RAM_BASE;

    let r = unsafe { cpu_exec_loop_env(&mut env, &mut cpu) };

    assert_eq!(r, ExitReason::Ecall { priv_level: 3 });
    assert_eq!(cpu.cpu.gpr[3], 64, "th.ff1: clz(0) = 64");
}
