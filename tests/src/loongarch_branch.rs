use machina_accel::code_buffer::CodeBuffer;
use machina_accel::exec::{cpu_exec_loop_env, ExecEnv, ExitReason};
use machina_accel::ir::tb::{
    EXCP_LOONGARCH_DONE, EXCP_LOONGARCH_WFI, TB_EXIT_NOCHAIN,
};
use machina_accel::ir::Context;
use machina_accel::translate::translate_and_execute;
use machina_accel::{ArchExitAction, GuestCpu, HostCodeGen, X86_64CodeGen};
use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
use machina_guest_loongarch::loongarch::ext::LoongArchCfg;
use machina_guest_loongarch::loongarch::trans::{
    LoongArchDisasContext, LoongArchTranslator,
};
use machina_guest_loongarch::{translator_loop, DisasJumpType, TranslatorOps};

const OP_BEQ: u32 = 0b010110;
const OP_BNE: u32 = 0b010111;
const OP_BLT: u32 = 0b011000;
const OP_BGE: u32 = 0b011001;
const OP_BLTU: u32 = 0b011010;
const OP_BGEU: u32 = 0b011011;
const OP_JIRL: u32 = 0b010011;
const OP_BEQZ: u32 = 0b010000;
const OP_BNEZ: u32 = 0b010001;
const OP_B: u32 = 0b010100;
const OP_BL: u32 = 0b010101;
const OP_ADDI_D: u32 = 0b0000001011;
const OP_IDLE: u32 = 0b00000110010010001;

fn r2_si16(op: u32, si16: i16, rj: u32, rd: u32) -> u32 {
    (op << 26) | ((si16 as u16 as u32) << 10) | (rj << 5) | rd
}

fn r2_si12(op: u32, si12: i16, rj: u32, rd: u32) -> u32 {
    (op << 22) | ((si12 as u16 as u32 & 0x0FFF) << 10) | (rj << 5) | rd
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

fn code15(op: u32, code: u32) -> u32 {
    (op << 15) | (code & 0x7FFF)
}

fn run_one_tb(cpu: &mut LoongArchCpu, start_pc: u64, insn: u32) -> usize {
    let mut code = vec![0u8; start_pc as usize + 4];
    code[start_pc as usize..start_pc as usize + 4]
        .copy_from_slice(&insn.to_le_bytes());

    let mut backend = X86_64CodeGen::new();
    let mut buf = CodeBuffer::new(4096).unwrap();
    backend.emit_prologue(&mut buf);
    backend.emit_epilogue(&mut buf);

    let mut ir = Context::new();
    backend.init_context(&mut ir);

    let mut ctx = LoongArchDisasContext::new(
        start_pc,
        code.as_ptr(),
        LoongArchCfg::default(),
    );
    ctx.base.max_insns = 1;
    translator_loop::<LoongArchTranslator>(&mut ctx, &mut ir);

    unsafe { translate_and_execute(&mut ir, &backend, &mut buf, cpu.env_ptr()) }
}

struct ExecLoopLoongArchCpu {
    cpu: LoongArchCpu,
    code: Vec<u8>,
}

impl ExecLoopLoongArchCpu {
    fn new(insns: &[u32]) -> Self {
        let code: Vec<u8> =
            insns.iter().flat_map(|insn| insn.to_le_bytes()).collect();
        let mut cpu = LoongArchCpu::new();
        cpu.set_guest_base(code.as_ptr() as u64);
        Self { cpu, code }
    }

    fn reset_for_branch(&mut self, rj: u64, rd: u64) {
        self.cpu.set_pc(0);
        self.cpu.reset_exit_request();
        self.cpu.write_gpr(2, rj);
        self.cpu.write_gpr(3, rd);
        self.cpu.write_gpr(5, 0);
    }

    fn branch_result(&self) -> u64 {
        self.cpu.read_gpr(5)
    }
}

impl GuestCpu for ExecLoopLoongArchCpu {
    type IrContext = Context;

    fn get_pc(&self) -> u64 {
        self.cpu.pc()
    }

    fn get_flags(&self) -> u32 {
        0
    }

    fn gen_code(&mut self, ir: &mut Context, pc: u64, max_insns: u32) -> u32 {
        if pc >= self.code.len() as u64 {
            return 0;
        }
        self.cpu.set_last_phys_pc(pc);
        let avail = (self.code.len() as u64 - pc) / 4;
        let limit = max_insns.min(avail as u32);
        let mut ctx = LoongArchDisasContext::new(
            pc,
            self.code.as_ptr(),
            LoongArchCfg::default(),
        );
        ctx.base.max_insns = limit;

        if ir.nb_globals() == 0 {
            LoongArchTranslator::init_disas_context(&mut ctx, ir);
        } else {
            ctx.bind_existing_globals(ir);
        }
        LoongArchTranslator::tb_start(&mut ctx, ir);

        loop {
            LoongArchTranslator::insn_start(&mut ctx, ir);
            LoongArchTranslator::translate_insn(&mut ctx, ir);
            if ctx.base.is_jmp != DisasJumpType::Next {
                break;
            }
            if ctx.base.num_insns >= ctx.base.max_insns {
                ctx.base.is_jmp = DisasJumpType::TooMany;
                break;
            }
        }

        LoongArchTranslator::tb_stop(&mut ctx, ir);
        ctx.base.num_insns * 4
    }

    fn env_ptr(&mut self) -> *mut u8 {
        self.cpu.env_ptr()
    }

    fn handle_arch_exit(&mut self, code: u64) -> ArchExitAction {
        match code {
            EXCP_LOONGARCH_DONE => ArchExitAction::Continue,
            EXCP_LOONGARCH_WFI => ArchExitAction::Halted,
            _ => ArchExitAction::Exit(code as usize),
        }
    }

    fn set_exit_request(&mut self) {
        self.cpu.set_exit_request();
    }

    fn reset_exit_request(&mut self) {
        self.cpu.reset_exit_request();
    }

    fn should_exit(&self) -> bool {
        self.cpu.pc() >= self.code.len() as u64
    }

    fn last_phys_pc(&self) -> u64 {
        self.cpu.last_phys_pc_val()
    }
}

fn run_exec_loop(
    env: &mut ExecEnv<X86_64CodeGen>,
    cpu: &mut ExecLoopLoongArchCpu,
) {
    let r = unsafe { cpu_exec_loop_env(env, cpu) };
    assert_eq!(r, ExitReason::Halted);
}

#[test]
fn loongarch_conditional_branches_use_qemu_slots_and_targets() {
    struct Case {
        name: &'static str,
        op: u32,
        rj: u64,
        rd: u64,
        taken: bool,
    }

    let cases = [
        Case {
            name: "beq taken",
            op: OP_BEQ,
            rj: 7,
            rd: 7,
            taken: true,
        },
        Case {
            name: "beq not taken",
            op: OP_BEQ,
            rj: 7,
            rd: 8,
            taken: false,
        },
        Case {
            name: "bne taken",
            op: OP_BNE,
            rj: 7,
            rd: 8,
            taken: true,
        },
        Case {
            name: "bne not taken",
            op: OP_BNE,
            rj: 7,
            rd: 7,
            taken: false,
        },
        Case {
            name: "blt signed taken",
            op: OP_BLT,
            rj: (-2i64) as u64,
            rd: 1,
            taken: true,
        },
        Case {
            name: "blt signed not taken",
            op: OP_BLT,
            rj: 2,
            rd: (-1i64) as u64,
            taken: false,
        },
        Case {
            name: "bge signed taken",
            op: OP_BGE,
            rj: (-1i64) as u64,
            rd: (-2i64) as u64,
            taken: true,
        },
        Case {
            name: "bge signed not taken",
            op: OP_BGE,
            rj: (-3i64) as u64,
            rd: (-2i64) as u64,
            taken: false,
        },
        Case {
            name: "bltu taken",
            op: OP_BLTU,
            rj: 1,
            rd: u64::MAX,
            taken: true,
        },
        Case {
            name: "bltu not taken",
            op: OP_BLTU,
            rj: u64::MAX,
            rd: 1,
            taken: false,
        },
        Case {
            name: "bgeu taken",
            op: OP_BGEU,
            rj: u64::MAX,
            rd: 1,
            taken: true,
        },
        Case {
            name: "bgeu not taken",
            op: OP_BGEU,
            rj: 1,
            rd: u64::MAX,
            taken: false,
        },
    ];

    for case in cases {
        let mut cpu = LoongArchCpu::new();
        cpu.write_gpr(2, case.rj);
        cpu.write_gpr(3, case.rd);

        let exit = run_one_tb(&mut cpu, 0, r2_si16(case.op, 2, 2, 3));
        assert_eq!(
            exit,
            usize::from(!case.taken),
            "wrong exit slot for {}",
            case.name
        );
        assert_eq!(
            cpu.pc(),
            if case.taken { 8 } else { 4 },
            "wrong PC for {}",
            case.name
        );
    }
}

#[test]
fn loongarch_conditional_branch_chaining_uses_explicit_goto_tb_slots() {
    let program = [
        r2_si16(OP_BEQ, 4, 2, 3),     // pc 0: taken -> pc 16
        r2_si12(OP_ADDI_D, 11, 0, 5), // pc 4: fall-through result
        offs26(OP_B, 4),              // pc 8: skip taken path to idle
        r2_si12(OP_ADDI_D, 99, 0, 5), // pc 12: unreachable
        r2_si12(OP_ADDI_D, 22, 0, 5), // pc 16: taken result
        offs26(OP_B, 1),              // pc 20: taken path to idle
        code15(OP_IDLE, 0),           // pc 24: halt through exec loop
    ];

    let mut cpu = ExecLoopLoongArchCpu::new(&program);
    let mut env = ExecEnv::new(X86_64CodeGen::new());

    cpu.reset_for_branch(1, 2);
    run_exec_loop(&mut env, &mut cpu);
    assert_eq!(cpu.branch_result(), 11);

    cpu.reset_for_branch(3, 3);
    run_exec_loop(&mut env, &mut cpu);
    assert_eq!(cpu.branch_result(), 22);

    let mut cpu = ExecLoopLoongArchCpu::new(&program);
    let mut env = ExecEnv::new(X86_64CodeGen::new());

    cpu.reset_for_branch(3, 3);
    run_exec_loop(&mut env, &mut cpu);
    assert_eq!(cpu.branch_result(), 22);

    cpu.reset_for_branch(1, 2);
    run_exec_loop(&mut env, &mut cpu);
    assert_eq!(cpu.branch_result(), 11);
}

#[test]
fn loongarch_zero_branches_and_negative_offsets_update_pc() {
    let mut cpu = LoongArchCpu::new();
    cpu.write_gpr(2, 0);
    let exit = run_one_tb(&mut cpu, 8, r1_offs21(OP_BEQZ, -1, 2));
    assert_eq!(exit, 0);
    assert_eq!(cpu.pc(), 4);

    let mut cpu = LoongArchCpu::new();
    cpu.write_gpr(2, 5);
    let exit = run_one_tb(&mut cpu, 0, r1_offs21(OP_BEQZ, 2, 2));
    assert_eq!(exit, 1);
    assert_eq!(cpu.pc(), 4);

    let mut cpu = LoongArchCpu::new();
    cpu.write_gpr(2, 5);
    let exit = run_one_tb(&mut cpu, 0, r1_offs21(OP_BNEZ, 2, 2));
    assert_eq!(exit, 0);
    assert_eq!(cpu.pc(), 8);

    let mut cpu = LoongArchCpu::new();
    cpu.write_gpr(2, 0);
    let exit = run_one_tb(&mut cpu, 0, r1_offs21(OP_BNEZ, 2, 2));
    assert_eq!(exit, 1);
    assert_eq!(cpu.pc(), 4);
}

#[test]
fn loongarch_unconditional_branch_and_link_shift_offsets() {
    let mut cpu = LoongArchCpu::new();
    let exit = run_one_tb(&mut cpu, 0, offs26(OP_B, 2));
    assert_eq!(exit, 0);
    assert_eq!(cpu.pc(), 8);

    let mut cpu = LoongArchCpu::new();
    let exit = run_one_tb(&mut cpu, 8, offs26(OP_B, -1));
    assert_eq!(exit, 0);
    assert_eq!(cpu.pc(), 4);

    let mut cpu = LoongArchCpu::new();
    let exit = run_one_tb(&mut cpu, 0, offs26(OP_BL, 2));
    assert_eq!(exit, 0);
    assert_eq!(cpu.pc(), 8);
    assert_eq!(cpu.read_gpr(1), 4);
}

#[test]
fn loongarch_jirl_updates_pc_and_link_register() {
    let mut cpu = LoongArchCpu::new();
    cpu.write_gpr(2, 100);
    let exit = run_one_tb(&mut cpu, 0, r2_si16(OP_JIRL, 2, 2, 5));
    assert_eq!(exit, TB_EXIT_NOCHAIN as usize);
    assert_eq!(cpu.pc(), 108);
    assert_eq!(cpu.read_gpr(5), 4);

    let mut cpu = LoongArchCpu::new();
    cpu.write_gpr(2, 100);
    let exit = run_one_tb(&mut cpu, 0, r2_si16(OP_JIRL, -1, 2, 0));
    assert_eq!(exit, TB_EXIT_NOCHAIN as usize);
    assert_eq!(cpu.pc(), 96);
    assert_eq!(cpu.read_gpr(0), 0);
}
