use machina_accel::code_buffer::CodeBuffer;
use machina_accel::ir::tb::EXCP_LOONGARCH_DONE;
use machina_accel::ir::Context;
use machina_accel::translate::translate_and_execute;
use machina_accel::{HostCodeGen, X86_64CodeGen};
use machina_guest_loongarch::loongarch::cpu::{
    LoongArchCpu, GUEST_BASE_CPU_OFFSET,
};
use machina_guest_loongarch::loongarch::csr::CSR_BADV;
use machina_guest_loongarch::loongarch::ext::LoongArchCfg;
use machina_guest_loongarch::loongarch::trans::{
    LoongArchDisasContext, LoongArchTranslator,
};
use machina_guest_loongarch::translator_loop;

const OP_LD_D: u32 = 0b0010100011;
const OP_LD_B: u32 = 0b0010100000;
const OP_LD_H: u32 = 0b0010100001;
const OP_LD_W: u32 = 0b0010100010;
const OP_LD_BU: u32 = 0b0010101000;
const OP_LD_HU: u32 = 0b0010101001;
const OP_LD_WU: u32 = 0b0010101010;
const OP_ST_B: u32 = 0b0010100100;
const OP_ST_H: u32 = 0b0010100101;
const OP_ST_W: u32 = 0b0010100110;
const OP_ST_D: u32 = 0b0010100111;
const OP_LDX_B: u32 = 0b00111000000000000;
const OP_LDX_H: u32 = 0b00111000000001000;
const OP_LDX_W: u32 = 0b00111000000010000;
const OP_LDX_D: u32 = 0b00111000000011000;
const OP_STX_B: u32 = 0b00111000000100000;
const OP_STX_H: u32 = 0b00111000000101000;
const OP_STX_W: u32 = 0b00111000000110000;
const OP_STX_D: u32 = 0b00111000000111000;
const OP_LDX_BU: u32 = 0b00111000001000000;
const OP_LDX_HU: u32 = 0b00111000001001000;
const OP_LDX_WU: u32 = 0b00111000001010000;
const OP_PRELD: u32 = 0b0010101011;
const OP_LDPTR_W: u32 = 0b00100100;
const OP_STPTR_W: u32 = 0b00100101;
const OP_LDPTR_D: u32 = 0b00100110;
const OP_STPTR_D: u32 = 0b00100111;
const OP_LDGT_B: u32 = 0b00111000011110000;
const OP_LDGT_D: u32 = 0b00111000011110011;
const OP_LDLE_H: u32 = 0b00111000011110101;
const OP_STGT_D: u32 = 0b00111000011111011;
const OP_STLE_W: u32 = 0b00111000011111110;

fn r2_si12(op: u32, si12: i16, rj: u32, rd: u32) -> u32 {
    (op << 22) | ((si12 as u16 as u32 & 0x0FFF) << 10) | (rj << 5) | rd
}

fn r3(op: u32, rk: u32, rj: u32, rd: u32) -> u32 {
    (op << 15) | (rk << 10) | (rj << 5) | rd
}

fn r2_si14(op: u32, si14: i16, rj: u32, rd: u32) -> u32 {
    (op << 24) | ((si14 as u16 as u32 & 0x3FFF) << 10) | (rj << 5) | rd
}

fn run_la(cpu: &mut LoongArchCpu, insns: &[u32]) -> usize {
    let code: Vec<u8> =
        insns.iter().flat_map(|insn| insn.to_le_bytes()).collect();

    let mut backend = X86_64CodeGen::new();
    backend.set_guest_base_offset(GUEST_BASE_CPU_OFFSET);
    let mut buf = CodeBuffer::new(4096).unwrap();
    backend.emit_prologue(&mut buf);
    backend.emit_epilogue(&mut buf);

    let mut ir = Context::new();
    backend.init_context(&mut ir);

    let mut ctx =
        LoongArchDisasContext::new(0, code.as_ptr(), LoongArchCfg::default());
    ctx.base.max_insns = insns.len() as u32;
    translator_loop::<LoongArchTranslator>(&mut ctx, &mut ir);

    unsafe { translate_and_execute(&mut ir, &backend, &mut buf, cpu.env_ptr()) }
}

#[test]
fn loongarch_load_uses_loongarch_guest_base_offset() {
    const LEGACY_RISCV_GUEST_BASE_OFFSET: usize = 520;
    const REAL_VALUE: u64 = 0x1122_3344_5566_7788;
    const DECOY_VALUE: u64 = 0xDEAD_BEEF_CAFE_BABE;

    let mut real_mem = [0u8; 32];
    let mut decoy_mem = [0u8; 32];
    real_mem[8..16].copy_from_slice(&REAL_VALUE.to_le_bytes());
    decoy_mem[8..16].copy_from_slice(&DECOY_VALUE.to_le_bytes());

    let mut cpu = LoongArchCpu::new();
    cpu.set_guest_base(real_mem.as_mut_ptr() as u64);
    cpu.write_gpr(2, 8);

    unsafe {
        cpu.env_ptr()
            .add(LEGACY_RISCV_GUEST_BASE_OFFSET)
            .cast::<u64>()
            .write(decoy_mem.as_mut_ptr() as u64);
    }

    assert_eq!(run_la(&mut cpu, &[r2_si12(OP_LD_D, 0, 2, 1)]), 0);
    assert_eq!(cpu.read_gpr(1), REAL_VALUE);
    assert_eq!(cpu.read_gpr(0), 0);
}

#[test]
fn loongarch_aligned_loads_extend_by_width() {
    let mut mem = [0u8; 64];
    mem[0] = 0x80;
    mem[2..4].copy_from_slice(&0x8001u16.to_le_bytes());
    mem[4..8].copy_from_slice(&0x8000_0002u32.to_le_bytes());
    mem[8..16].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());
    mem[16] = 0x80;
    mem[18..20].copy_from_slice(&0x8001u16.to_le_bytes());
    mem[20..24].copy_from_slice(&0x8000_0002u32.to_le_bytes());

    let mut cpu = LoongArchCpu::new();
    cpu.set_guest_base(mem.as_mut_ptr() as u64);
    cpu.write_gpr(2, 0);

    let insns = [
        r2_si12(OP_LD_B, 0, 2, 1),
        r2_si12(OP_LD_H, 2, 2, 3),
        r2_si12(OP_LD_W, 4, 2, 4),
        r2_si12(OP_LD_D, 8, 2, 5),
        r2_si12(OP_LD_BU, 16, 2, 6),
        r2_si12(OP_LD_HU, 18, 2, 7),
        r2_si12(OP_LD_WU, 20, 2, 8),
        r2_si12(OP_LD_D, 8, 2, 0),
    ];

    assert_eq!(run_la(&mut cpu, &insns), 0);
    assert_eq!(cpu.read_gpr(1), (-128i64) as u64);
    assert_eq!(cpu.read_gpr(3), (-32767i64) as u64);
    assert_eq!(cpu.read_gpr(4), i64::from(i32::MIN + 2) as u64);
    assert_eq!(cpu.read_gpr(5), 0x1122_3344_5566_7788);
    assert_eq!(cpu.read_gpr(6), 0x80);
    assert_eq!(cpu.read_gpr(7), 0x8001);
    assert_eq!(cpu.read_gpr(8), 0x8000_0002);
    assert_eq!(cpu.read_gpr(0), 0);
}

#[test]
fn loongarch_aligned_stores_write_expected_widths() {
    let mut mem = [0xAAu8; 32];
    let mut cpu = LoongArchCpu::new();
    cpu.set_guest_base(mem.as_mut_ptr() as u64);
    cpu.write_gpr(10, 0);
    cpu.write_gpr(1, 0x11);
    cpu.write_gpr(2, 0x2233);
    cpu.write_gpr(3, 0x4455_6677);
    cpu.write_gpr(4, 0x8899_AABB_CCDD_EEFF);

    let insns = [
        r2_si12(OP_ST_B, 0, 10, 1),
        r2_si12(OP_ST_H, 2, 10, 2),
        r2_si12(OP_ST_W, 4, 10, 3),
        r2_si12(OP_ST_D, 8, 10, 4),
    ];

    assert_eq!(run_la(&mut cpu, &insns), 0);
    assert_eq!(mem[0], 0x11);
    assert_eq!(&mem[1..2], &[0xAA]);
    assert_eq!(&mem[2..4], &0x2233u16.to_le_bytes());
    assert_eq!(&mem[4..8], &0x4455_6677u32.to_le_bytes());
    assert_eq!(&mem[8..16], &0x8899_AABB_CCDD_EEFFu64.to_le_bytes());
    assert_eq!(&mem[16..], &[0xAA; 16]);
}

#[test]
fn task47_indexed_integer_memory_paths_work_for_kernel_boot() {
    let mut mem = [0xAAu8; 64];
    mem[12] = 0x80;
    mem[14..16].copy_from_slice(&0x8001u16.to_le_bytes());
    mem[20..24].copy_from_slice(&0x8000_0002u32.to_le_bytes());
    mem[28..36].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());
    mem[40] = 0x80;
    mem[42..44].copy_from_slice(&0x8001u16.to_le_bytes());
    mem[48..52].copy_from_slice(&0x8000_0002u32.to_le_bytes());

    let mut cpu = LoongArchCpu::new();
    cpu.set_guest_base(mem.as_mut_ptr() as u64);
    cpu.write_gpr(10, 8);
    cpu.write_gpr(11, 4);
    cpu.write_gpr(12, 6);
    cpu.write_gpr(13, 12);
    cpu.write_gpr(14, 20);
    cpu.write_gpr(20, 0x11);
    cpu.write_gpr(21, 0x2233);
    cpu.write_gpr(22, 0x4455_6677);
    cpu.write_gpr(23, 0x8899_AABB_CCDD_EEFF);

    let insns = [
        r3(OP_LDX_B, 11, 10, 1),
        r3(OP_LDX_H, 12, 10, 2),
        r3(OP_LDX_W, 13, 10, 3),
        r3(OP_LDX_D, 14, 10, 4),
        r3(OP_LDX_BU, 11, 10, 5),
        r3(OP_LDX_HU, 12, 10, 6),
        r3(OP_LDX_WU, 13, 10, 7),
        r3(OP_STX_B, 11, 10, 20),
        r3(OP_STX_H, 12, 10, 21),
        r3(OP_STX_W, 13, 10, 22),
        r3(OP_STX_D, 14, 10, 23),
    ];

    assert_eq!(run_la(&mut cpu, &insns), 0);
    assert_eq!(cpu.read_gpr(1), (-128i64) as u64);
    assert_eq!(cpu.read_gpr(2), (-32767i64) as u64);
    assert_eq!(cpu.read_gpr(3), i64::from(i32::MIN + 2) as u64);
    assert_eq!(cpu.read_gpr(4), 0x1122_3344_5566_7788);
    assert_eq!(cpu.read_gpr(5), 0x80);
    assert_eq!(cpu.read_gpr(6), 0x8001);
    assert_eq!(cpu.read_gpr(7), 0x8000_0002);
    assert_eq!(mem[12], 0x11);
    assert_eq!(&mem[14..16], &0x2233u16.to_le_bytes());
    assert_eq!(&mem[20..24], &0x4455_6677u32.to_le_bytes());
    assert_eq!(&mem[28..36], &0x8899_AABB_CCDD_EEFFu64.to_le_bytes());
}

#[test]
fn task47_pointer_memory_paths_work_for_kernel_boot() {
    let mut mem = [0xAAu8; 128];
    mem[20..24].copy_from_slice(&0x8000_0002u32.to_le_bytes());
    mem[32..40].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());

    let mut cpu = LoongArchCpu::new();
    cpu.set_guest_base(mem.as_mut_ptr() as u64);
    cpu.write_gpr(10, 16);
    cpu.write_gpr(20, 0x4455_6677);
    cpu.write_gpr(21, 0x8899_AABB_CCDD_EEFF);

    let insns = [
        r2_si14(OP_LDPTR_W, 1, 10, 1),
        r2_si14(OP_LDPTR_D, 4, 10, 2),
        r2_si14(OP_STPTR_W, 2, 10, 20),
        r2_si14(OP_STPTR_D, 8, 10, 21),
    ];

    assert_eq!(run_la(&mut cpu, &insns), 0);
    assert_eq!(cpu.read_gpr(1), i64::from(i32::MIN + 2) as u64);
    assert_eq!(cpu.read_gpr(2), 0x1122_3344_5566_7788);
    assert_eq!(&mem[24..28], &0x4455_6677u32.to_le_bytes());
    assert_eq!(&mem[48..56], &0x8899_AABB_CCDD_EEFFu64.to_le_bytes());
}

#[test]
fn task81_preld_is_a_noop_hint() {
    let mut mem = [0xAAu8; 32];
    let mut cpu = LoongArchCpu::new();
    cpu.set_guest_base(mem.as_mut_ptr() as u64);
    cpu.write_gpr(2, 0x10);
    cpu.write_gpr(3, 0x1122_3344);

    assert_eq!(run_la(&mut cpu, &[r2_si12(OP_PRELD, -4, 2, 3)]), 0);
    assert_eq!(cpu.read_gpr(2), 0x10);
    assert_eq!(cpu.read_gpr(3), 0x1122_3344);
    assert_eq!(mem, [0xAAu8; 32]);
}

#[test]
fn task81_integer_predicate_load_store_success_paths() {
    let mut mem = [0xAAu8; 64];
    mem[16] = 0x80;
    mem[20..22].copy_from_slice(&0x8001u16.to_le_bytes());
    mem[32..40].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());

    let mut cpu = LoongArchCpu::new();
    cpu.set_guest_base(mem.as_mut_ptr() as u64);
    cpu.write_gpr(10, 16);
    cpu.write_gpr(11, 8);
    cpu.write_gpr(12, 20);
    cpu.write_gpr(13, 24);
    cpu.write_gpr(14, 32);
    cpu.write_gpr(15, 40);
    cpu.write_gpr(21, 0x4455_6677);
    cpu.write_gpr(22, 0x8899_AABB_CCDD_EEFF);

    let insns = [
        r3(OP_LDGT_B, 11, 10, 1),
        r3(OP_LDLE_H, 13, 12, 2),
        r3(OP_STLE_W, 15, 14, 21),
        r3(OP_STGT_D, 11, 10, 22),
    ];

    assert_eq!(run_la(&mut cpu, &insns), 0);
    assert_eq!(cpu.read_gpr(1), (-128i64) as u64);
    assert_eq!(cpu.read_gpr(2), (-32767i64) as u64);
    assert_eq!(&mem[32..36], &0x4455_6677u32.to_le_bytes());
    assert_eq!(&mem[16..24], &0x8899_AABB_CCDD_EEFFu64.to_le_bytes());
}

#[test]
fn task81_integer_predicate_load_failure_traps_before_memory_access() {
    let mut mem = [0xAAu8; 64];
    mem[8..16].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());

    let mut cpu = LoongArchCpu::new();
    cpu.set_guest_base(mem.as_mut_ptr() as u64);
    cpu.write_gpr(10, 8);
    cpu.write_gpr(11, 16);

    assert_eq!(
        run_la(&mut cpu, &[r3(OP_LDGT_D, 11, 10, 1)]),
        EXCP_LOONGARCH_DONE as usize
    );
    assert_eq!(cpu.read_gpr(1), 0);
    assert_eq!(cpu.csr_read(CSR_BADV), 8);
}

#[test]
fn task81_integer_predicate_store_failure_does_not_write_memory() {
    let mut mem = [0xAAu8; 64];
    let mut cpu = LoongArchCpu::new();
    cpu.set_guest_base(mem.as_mut_ptr() as u64);
    cpu.write_gpr(10, 32);
    cpu.write_gpr(11, 16);
    cpu.write_gpr(20, 0x1122_3344_5566_7788);

    assert_eq!(
        run_la(&mut cpu, &[r3(OP_STLE_W, 11, 10, 20)]),
        EXCP_LOONGARCH_DONE as usize
    );
    assert_eq!(mem, [0xAAu8; 64]);
    assert_eq!(cpu.csr_read(CSR_BADV), 32);
}
