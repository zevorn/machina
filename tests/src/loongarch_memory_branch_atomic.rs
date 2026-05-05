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

const OP_ADDI_D: u32 = 0b0000001011;
const OP_LD_WU: u32 = 0b0010101010;
const OP_ST_W: u32 = 0b0010100110;
const OP_LL_W: u32 = 0b00100000;
const OP_SC_W: u32 = 0b00100001;
const OP_AMADD_W: u32 = 0b00111000011000010;
const OP_BEQ: u32 = 0b010110;
const OP_BEQZ: u32 = 0b010000;
const OP_B: u32 = 0b010100;

fn r3(op: u32, rk: u32, rj: u32, rd: u32) -> u32 {
    (op << 15) | (rk << 10) | (rj << 5) | rd
}

fn r2_si12(op: u32, si12: i16, rj: u32, rd: u32) -> u32 {
    (op << 22) | ((si12 as u16 as u32 & 0x0FFF) << 10) | (rj << 5) | rd
}

fn r2_si14(op: u32, si14: i16, rj: u32, rd: u32) -> u32 {
    (op << 24) | ((si14 as u16 as u32 & 0x3FFF) << 10) | (rj << 5) | rd
}

fn r2_si16(op: u32, si16: i16, rj: u32, rd: u32) -> u32 {
    (op << 26) | ((si16 as u16 as u32) << 10) | (rj << 5) | rd
}

fn r1_offs21(op: u32, offs21: i32, rj: u32) -> u32 {
    let imm = offs21 as u32 & 0x001F_FFFF;
    (op << 26)
        | (((imm >> 16) & 0x1F) << 0)
        | ((imm & 0xFFFF) << 10)
        | (rj << 5)
}

fn offs26(op: u32, offs26: i32) -> u32 {
    let imm = offs26 as u32 & 0x03FF_FFFF;
    (op << 26) | (((imm >> 16) & 0x3FF) << 0) | ((imm & 0xFFFF) << 10)
}

fn run_until_end(cpu: &mut LoongArchCpu, insns: &[u32]) {
    let code: Vec<u8> =
        insns.iter().flat_map(|insn| insn.to_le_bytes()).collect();
    let code_len = code.len() as u64;

    for _ in 0..(insns.len() + 8) {
        let pc = cpu.pc();
        if pc >= code_len {
            return;
        }
        assert_eq!(pc & 3, 0, "LoongArch PC must remain aligned");
        assert!(
            pc + 4 <= code_len,
            "LoongArch PC outside test program: pc={pc:#x}, len={code_len:#x}"
        );

        let mut backend = X86_64CodeGen::new();
        backend.set_guest_base_offset(GUEST_BASE_CPU_OFFSET);
        let mut buf = CodeBuffer::new(4096).unwrap();
        backend.emit_prologue(&mut buf);
        backend.emit_epilogue(&mut buf);

        let mut ir = Context::new();
        backend.init_context(&mut ir);

        let mut ctx = LoongArchDisasContext::new(
            pc,
            code.as_ptr(),
            LoongArchCfg::default(),
        );
        ctx.base.max_insns = ((code_len - pc) / 4) as u32;
        translator_loop::<LoongArchTranslator>(&mut ctx, &mut ir);

        let exit = unsafe {
            translate_and_execute(&mut ir, &backend, &mut buf, cpu.env_ptr())
        };
        assert!(exit <= 2, "unexpected LoongArch TB exit: {exit}");
    }

    panic!(
        "LoongArch test program did not finish: pc={:#x}, len={:#x}",
        cpu.pc(),
        code_len
    );
}

fn read_u32(mem: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(mem[off..off + 4].try_into().unwrap())
}

#[test]
fn loongarch_memory_loaded_value_selects_branch_store_path() {
    let program = [
        r2_si12(OP_LD_WU, 0, 20, 1),
        r2_si16(OP_BEQ, 4, 1, 0),
        r2_si12(OP_ADDI_D, 0x11, 0, 2),
        r2_si12(OP_ST_W, 8, 20, 2),
        offs26(OP_B, 3),
        r2_si12(OP_ADDI_D, 0x22, 0, 2),
        r2_si12(OP_ST_W, 8, 20, 2),
        r2_si12(OP_LD_WU, 8, 20, 3),
    ];

    for (flag, expected) in [(5u32, 0x11u64), (0, 0x22)] {
        let mut mem = [0u8; 32];
        mem[0..4].copy_from_slice(&flag.to_le_bytes());

        let mut cpu = LoongArchCpu::new();
        cpu.set_guest_base(mem.as_mut_ptr() as u64);
        cpu.write_gpr(20, 0);

        run_until_end(&mut cpu, &program);
        assert_eq!(cpu.read_gpr(3), expected);
        assert_eq!(read_u32(&mem, 8), expected as u32);
    }
}

#[test]
fn loongarch_branch_selects_atomic_or_plain_memory_path() {
    let program = [
        r2_si12(OP_LD_WU, 0, 20, 1),
        r1_offs21(OP_BEQZ, 5, 1),
        r2_si12(OP_ADDI_D, 8, 20, 21),
        r3(OP_AMADD_W, 10, 21, 5),
        r2_si12(OP_LD_WU, 0, 21, 6),
        offs26(OP_B, 3),
        r2_si12(OP_ADDI_D, 0, 0, 5),
        r2_si12(OP_LD_WU, 8, 20, 6),
        r2_si12(OP_ST_W, 16, 20, 6),
    ];

    for (flag, old, updated) in [(1u32, 10u64, 17u64), (0, 0, 10)] {
        let mut mem = [0u8; 32];
        mem[0..4].copy_from_slice(&flag.to_le_bytes());
        mem[8..12].copy_from_slice(&10u32.to_le_bytes());

        let mut cpu = LoongArchCpu::new();
        cpu.set_guest_base(mem.as_mut_ptr() as u64);
        cpu.write_gpr(10, 7);
        cpu.write_gpr(20, 0);

        run_until_end(&mut cpu, &program);
        assert_eq!(cpu.read_gpr(5), old);
        assert_eq!(cpu.read_gpr(6), updated);
        assert_eq!(read_u32(&mem, 8), updated as u32);
        assert_eq!(read_u32(&mem, 16), updated as u32);
    }
}

#[test]
fn loongarch_ll_sc_status_tracks_control_flow_and_interference() {
    let program = [
        r2_si14(OP_LL_W, 2, 20, 5),
        r2_si12(OP_LD_WU, 0, 20, 1),
        r1_offs21(OP_BEQZ, 4, 1),
        r2_si12(OP_ADDI_D, 0x123, 0, 5),
        r2_si14(OP_SC_W, 2, 20, 5),
        offs26(OP_B, 5),
        r2_si12(OP_ADDI_D, 0x777, 0, 9),
        r2_si12(OP_ST_W, 8, 20, 9),
        r2_si12(OP_ADDI_D, 0x456, 0, 5),
        r2_si14(OP_SC_W, 2, 20, 5),
        r2_si12(OP_LD_WU, 8, 20, 6),
    ];

    for (flag, status, value) in [(1u32, 1u64, 0x123u64), (0, 0, 0x777)] {
        let mut mem = [0u8; 32];
        mem[0..4].copy_from_slice(&flag.to_le_bytes());
        mem[8..12].copy_from_slice(&0x100u32.to_le_bytes());

        let mut cpu = LoongArchCpu::new();
        cpu.set_guest_base(mem.as_mut_ptr() as u64);
        cpu.set_ram_base(0);
        cpu.set_ram_end(mem.len() as u64);
        cpu.write_gpr(20, 0);

        run_until_end(&mut cpu, &program);
        assert_eq!(cpu.read_gpr(5), status);
        assert_eq!(cpu.read_gpr(6), value);
        assert_eq!(read_u32(&mem, 8), value as u32);
    }
}
