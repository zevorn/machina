use machina_accel::code_buffer::CodeBuffer;
use machina_accel::ir::Context;
use machina_accel::translate::translate_and_execute;
use machina_accel::{HostCodeGen, X86_64CodeGen};
use machina_guest_loongarch::loongarch::cpu::{
    LoongArchCpu, GUEST_BASE_CPU_OFFSET,
};
use machina_guest_loongarch::loongarch::ext::LoongArchCfg;
use machina_guest_loongarch::loongarch::trans::{
    LoongArchDisasContext, LoongArchTranslator,
};
use machina_guest_loongarch::translator_loop;

const OP_ADD_D: u32 = 0b00000000000100001;
const OP_ADDI_D: u32 = 0b0000001011;
const OP_LD_D: u32 = 0b0010100011;
const OP_LD_WU: u32 = 0b0010101010;
const OP_ST_W: u32 = 0b0010100110;
const OP_LL_W: u32 = 0b00100000;
const OP_SC_W: u32 = 0b00100001;
const OP_LL_D: u32 = 0b00100010;
const OP_SC_D: u32 = 0b00100011;
const OP_AMSWAP_D: u32 = 0b00111000011000001;
const OP_AMADD_W: u32 = 0b00111000011000010;
const OP_AMMAX_WU: u32 = 0b00111000011001110;
const OP_AMMIN_DU: u32 = 0b00111000011010001;
const OP_DBAR: u32 = 0b00111000011100100;
const OP_IBAR: u32 = 0b00111000011100101;

fn r3(op: u32, rk: u32, rj: u32, rd: u32) -> u32 {
    (op << 15) | (rk << 10) | (rj << 5) | rd
}

fn r2_si12(op: u32, si12: i16, rj: u32, rd: u32) -> u32 {
    (op << 22) | ((si12 as u16 as u32 & 0x0FFF) << 10) | (rj << 5) | rd
}

fn r2_si14(op: u32, si14: i16, rj: u32, rd: u32) -> u32 {
    (op << 24) | ((si14 as u16 as u32 & 0x3FFF) << 10) | (rj << 5) | rd
}

fn code15(op: u32, code: u32) -> u32 {
    (op << 15) | (code & 0x7FFF)
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

fn read_u32(mem: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(mem[off..off + 4].try_into().unwrap())
}

fn read_u64(mem: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(mem[off..off + 8].try_into().unwrap())
}

#[test]
fn loongarch_ll_sc_pairs_update_memory_and_status() {
    let mut mem = [0u8; 64];
    mem[8..12].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
    mem[16..24].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());

    let mut cpu = LoongArchCpu::new();
    cpu.set_guest_base(mem.as_mut_ptr() as u64);
    cpu.set_ram_base(0);
    cpu.set_ram_end(mem.len() as u64);
    cpu.write_gpr(2, 0);

    let insns = [
        r2_si14(OP_LL_W, 2, 2, 5),
        r3(OP_ADD_D, 0, 5, 6),
        r2_si12(OP_ADDI_D, 0x123, 0, 5),
        r2_si14(OP_SC_W, 2, 2, 5),
        r2_si14(OP_LL_D, 4, 2, 7),
        r3(OP_ADD_D, 0, 7, 8),
        r2_si12(OP_ADDI_D, 0x456, 0, 7),
        r2_si14(OP_SC_D, 4, 2, 7),
    ];

    assert_eq!(run_la(&mut cpu, &insns), 0);
    assert_eq!(cpu.read_gpr(6), (-2i64) as u64);
    assert_eq!(cpu.read_gpr(5), 1);
    assert_eq!(read_u32(&mem, 8), 0x123);
    assert_eq!(cpu.read_gpr(8), 0x1122_3344_5566_7788);
    assert_eq!(cpu.read_gpr(7), 1);
    assert_eq!(read_u64(&mem, 16), 0x456);
}

#[test]
fn loongarch_sc_fails_without_or_after_lost_reservation() {
    let mut mem = [0u8; 64];
    mem[8..12].copy_from_slice(&0xAAAA_AAAAu32.to_le_bytes());

    let mut cpu = LoongArchCpu::new();
    cpu.set_guest_base(mem.as_mut_ptr() as u64);
    cpu.set_ram_base(0);
    cpu.set_ram_end(mem.len() as u64);
    cpu.write_gpr(2, 0);
    cpu.write_gpr(5, 0x1111_1111);
    cpu.write_gpr(9, 0xBBBB_BBBB);

    let insns = [
        r2_si14(OP_SC_W, 2, 2, 5),
        r2_si14(OP_LL_W, 2, 2, 6),
        r2_si12(OP_ST_W, 8, 2, 9),
        r2_si12(OP_ADDI_D, 0x333, 0, 6),
        r2_si14(OP_SC_W, 2, 2, 6),
    ];

    assert_eq!(run_la(&mut cpu, &insns), 0);
    assert_eq!(cpu.read_gpr(5), 0);
    assert_eq!(cpu.read_gpr(6), 0);
    assert_eq!(read_u32(&mem, 8), 0xBBBB_BBBB);
}

#[test]
fn loongarch_atomic_rmw_variants_return_old_and_update_memory() {
    let mut mem = [0u8; 64];
    mem[0..4].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
    mem[8..16].copy_from_slice(&10u64.to_le_bytes());
    mem[16..20].copy_from_slice(&0x8000_0001u32.to_le_bytes());
    mem[24..32].copy_from_slice(&u64::MAX.wrapping_sub(5).to_le_bytes());

    let mut cpu = LoongArchCpu::new();
    cpu.set_guest_base(mem.as_mut_ptr() as u64);
    cpu.write_gpr(2, 0);
    cpu.write_gpr(3, 8);
    cpu.write_gpr(4, 16);
    cpu.write_gpr(5, 24);
    cpu.write_gpr(11, 5);
    cpu.write_gpr(13, 0x1122_3344_5566_7788);
    cpu.write_gpr(15, 2);
    cpu.write_gpr(17, 7);

    let insns = [
        r3(OP_AMADD_W, 11, 2, 10),
        r2_si12(OP_LD_WU, 0, 2, 18),
        r3(OP_AMSWAP_D, 13, 3, 12),
        r2_si12(OP_LD_D, 0, 3, 19),
        r3(OP_AMMAX_WU, 15, 4, 14),
        r2_si12(OP_LD_WU, 0, 4, 20),
        r3(OP_AMMIN_DU, 17, 5, 16),
        r2_si12(OP_LD_D, 0, 5, 21),
    ];

    assert_eq!(run_la(&mut cpu, &insns), 0);
    assert_eq!(cpu.read_gpr(10), (-2i64) as u64);
    assert_eq!(cpu.read_gpr(18), 3);
    assert_eq!(cpu.read_gpr(12), 10);
    assert_eq!(cpu.read_gpr(19), 0x1122_3344_5566_7788);
    assert_eq!(cpu.read_gpr(14), 0xFFFF_FFFF_8000_0001);
    assert_eq!(cpu.read_gpr(20), 0x8000_0001);
    assert_eq!(cpu.read_gpr(16), u64::MAX.wrapping_sub(5));
    assert_eq!(cpu.read_gpr(21), 7);
}

#[test]
fn loongarch_barriers_preserve_registers_and_ibar_stops_tb() {
    let mut cpu = LoongArchCpu::new();
    let dbar_program = [
        r2_si12(OP_ADDI_D, 1, 0, 1),
        code15(OP_DBAR, 0),
        r2_si12(OP_ADDI_D, 2, 0, 1),
    ];

    assert_eq!(run_la(&mut cpu, &dbar_program), 0);
    assert_eq!(cpu.read_gpr(1), 2);

    let mut cpu = LoongArchCpu::new();
    let ibar_program = [
        r2_si12(OP_ADDI_D, 1, 0, 1),
        code15(OP_IBAR, 0),
        r2_si12(OP_ADDI_D, 2, 0, 1),
    ];

    assert_eq!(run_la(&mut cpu, &ibar_program), 0);
    assert_eq!(cpu.read_gpr(1), 1);
    assert_eq!(cpu.pc(), 8);
}
