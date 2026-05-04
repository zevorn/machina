use machina_accel::code_buffer::CodeBuffer;
use machina_accel::exec::{cpu_exec_loop_env, ExecEnv, ExitReason};
use machina_accel::ir::Context;
use machina_accel::translate::translate_and_execute;
use machina_accel::{GuestCpu, HostCodeGen, X86_64CodeGen};
use machina_guest_loongarch::loongarch::cpu::{
    LoongArchCpu, GUEST_BASE_CPU_OFFSET,
};
use machina_guest_loongarch::loongarch::csr::{
    CRMD_DA, CSR_BADV, CSR_CRMD, CSR_EENTRY, CSR_ESTAT, CSR_EUEN, EUEN_FPE,
};
use machina_guest_loongarch::loongarch::exception::{
    ECODE_BCE, ECODE_FPD, ECODE_FPE,
};
use machina_guest_loongarch::loongarch::ext::LoongArchCfg;
use machina_guest_loongarch::loongarch::trans::helpers;
use machina_guest_loongarch::loongarch::trans::{
    LoongArchDisasContext, LoongArchTranslator,
};
use machina_guest_loongarch::{translator_loop, DisasJumpType, TranslatorOps};

const OP_FADD_S: u32 = 0b00000001000000001;
const OP_FADD_D: u32 = 0b00000001000000010;
const OP_FSUB_S: u32 = 0b00000001000000101;
const OP_FSUB_D: u32 = 0b00000001000000110;
const OP_FMUL_S: u32 = 0b00000001000001001;
const OP_FMUL_D: u32 = 0b00000001000001010;
const OP_FDIV_S: u32 = 0b00000001000001101;
const OP_FDIV_D: u32 = 0b00000001000001110;
const OP_FMAX_S: u32 = 0b00000001000010001;
const OP_FMAX_D: u32 = 0b00000001000010010;
const OP_FMIN_S: u32 = 0b00000001000010101;
const OP_FMIN_D: u32 = 0b00000001000010110;
const OP_FMAXA_S: u32 = 0b00000001000011001;
const OP_FMAXA_D: u32 = 0b00000001000011010;
const OP_FMINA_S: u32 = 0b00000001000011101;
const OP_FMINA_D: u32 = 0b00000001000011110;
const OP_FSCALEB_S: u32 = 0b00000001000100001;
const OP_FSCALEB_D: u32 = 0b00000001000100010;
const OP_FCOPYSIGN_S: u32 = 0b00000001000100101;
const OP_FCOPYSIGN_D: u32 = 0b00000001000100110;
const OP_FLD_S: u32 = 0b0010101100;
const OP_FLD_D: u32 = 0b0010101110;
const OP_FST_S: u32 = 0b0010101101;
const OP_FST_D: u32 = 0b0010101111;
const OP_FLDX_S: u32 = 0b00111000001100000;
const OP_FLDX_D: u32 = 0b00111000001101000;
const OP_FSTX_S: u32 = 0b00111000001110000;
const OP_FSTX_D: u32 = 0b00111000001111000;
const OP_FLDGT_D: u32 = 0b00111000011101001;
const OP_FLDLE_S: u32 = 0b00111000011101010;
const OP_FSTGT_D: u32 = 0b00111000011101101;
const OP_FSTLE_S: u32 = 0b00111000011101110;
const OP_FSQRT_S: u32 = 0b0000000100010100010001;
const OP_FSQRT_D: u32 = 0b0000000100010100010010;
const OP_FRECIP_S: u32 = 0b0000000100010100010101;
const OP_FRECIP_D: u32 = 0b0000000100010100010110;
const OP_FRSQRT_S: u32 = 0b0000000100010100011001;
const OP_FRSQRT_D: u32 = 0b0000000100010100011010;
const OP_FRECIPE_S: u32 = 0b0000000100010100011101;
const OP_FRECIPE_D: u32 = 0b0000000100010100011110;
const OP_FRSQRTE_S: u32 = 0b0000000100010100100001;
const OP_FRSQRTE_D: u32 = 0b0000000100010100100010;
const OP_FLOGB_S: u32 = 0b0000000100010100001001;
const OP_FLOGB_D: u32 = 0b0000000100010100001010;
const OP_FCLASS_S: u32 = 0b0000000100010100001101;
const OP_FCLASS_D: u32 = 0b0000000100010100001110;
const OP_FMADD_S: u32 = 0b000010000001;
const OP_FMADD_D: u32 = 0b000010000010;
const OP_FMSUB_S: u32 = 0b000010000101;
const OP_FMSUB_D: u32 = 0b000010000110;
const OP_FNMADD_S: u32 = 0b000010001001;
const OP_FNMADD_D: u32 = 0b000010001010;
const OP_FNMSUB_S: u32 = 0b000010001101;
const OP_FNMSUB_D: u32 = 0b000010001110;
const OP_FCVT_S_D: u32 = 0b0000000100011001000110;
const OP_FCVT_D_S: u32 = 0b0000000100011001001001;
const OP_FTINTRM_W_S: u32 = 0b0000000100011010000001;
const OP_FTINTRM_W_D: u32 = 0b0000000100011010000010;
const OP_FTINTRM_L_S: u32 = 0b0000000100011010001001;
const OP_FTINTRM_L_D: u32 = 0b0000000100011010001010;
const OP_FTINTRP_W_S: u32 = 0b0000000100011010010001;
const OP_FTINTRP_W_D: u32 = 0b0000000100011010010010;
const OP_FTINTRP_L_S: u32 = 0b0000000100011010011001;
const OP_FTINTRP_L_D: u32 = 0b0000000100011010011010;
const OP_FTINTRZ_W_S: u32 = 0b0000000100011010100001;
const OP_FTINTRZ_W_D: u32 = 0b0000000100011010100010;
const OP_FTINTRZ_L_S: u32 = 0b0000000100011010101001;
const OP_FTINTRZ_L_D: u32 = 0b0000000100011010101010;
const OP_FTINTRNE_W_S: u32 = 0b0000000100011010110001;
const OP_FTINTRNE_W_D: u32 = 0b0000000100011010110010;
const OP_FTINTRNE_L_S: u32 = 0b0000000100011010111001;
const OP_FTINTRNE_L_D: u32 = 0b0000000100011010111010;
const OP_FTINT_W_S: u32 = 0b0000000100011011000001;
const OP_FTINT_W_D: u32 = 0b0000000100011011000010;
const OP_FTINT_L_S: u32 = 0b0000000100011011001001;
const OP_FTINT_L_D: u32 = 0b0000000100011011001010;
const OP_FFINT_S_W: u32 = 0b0000000100011101000100;
const OP_FFINT_D_W: u32 = 0b0000000100011101001000;
const OP_FFINT_S_L: u32 = 0b0000000100011101000110;
const OP_FFINT_D_L: u32 = 0b0000000100011101001010;
const OP_FRINT_S: u32 = 0b0000000100011110010001;
const OP_FRINT_D: u32 = 0b0000000100011110010010;
const OP_FCMP_S: u32 = 0b000011000001;
const OP_FCMP_D: u32 = 0b000011000010;
const OP_FSEL: u32 = 0b000011010000;
const OP_FBRANCH: u32 = 0b010010;
const OP_FMOV_D: u32 = 0b0000000100010100100110;
const OP_FABS_S: u32 = 0b0000000100010100000001;
const OP_FNEG_S: u32 = 0b0000000100010100000101;
const OP_FNEG_D: u32 = 0b0000000100010100000110;
const OP_MOVGR2FR_W: u32 = 0b0000000100010100101001;
const OP_MOVGR2FR_D: u32 = 0b0000000100010100101010;
const OP_MOVGR2FRH_W: u32 = 0b0000000100010100101011;
const OP_MOVFR2GR_S: u32 = 0b0000000100010100101101;
const OP_MOVFR2GR_D: u32 = 0b0000000100010100101110;
const OP_MOVFRH2GR_S: u32 = 0b0000000100010100101111;
const OP_MOVGR2FCSR: u32 = 0b0000000100010100110000;
const OP_MOVFCSR2GR: u32 = 0b0000000100010100110010;
const OP_MOVFR2CF: u32 = 0b0000000100010100110100;
const OP_MOVCF2FR: u32 = 0b0000000100010100110101;
const OP_MOVGR2CF: u32 = 0b0000000100010100110110;
const OP_MOVCF2GR: u32 = 0b0000000100010100110111;
const OP_ADDI_D: u32 = 0b0000001011;
const OP_B: u32 = 0b010100;
const OP_IDLE: u32 = 0b00000110010010001;

const FCSR_ENABLE_SHIFT: u32 = 0;
const FCSR_RM_SHIFT: u32 = 8;
const FCSR_FLAG_SHIFT: u32 = 16;
const FCSR_CAUSE_SHIFT: u32 = 24;
const FP_I: u32 = 1;
const FP_U: u32 = 2;
const FP_O: u32 = 4;
const FP_Z: u32 = 8;
const FP_V: u32 = 16;
const FPE_DISABLED_VECTOR: u64 = 0x9000_0000;
const FPR_SENTINEL: u64 = 0xfeed_face_dead_beef;
const GPR_SENTINEL: u64 = 0x1234_5678_9abc_def0;
const FCSR_SENTINEL: u32 = 0x1f1f_031f;

const FCMP_CAF: u32 = 0b00000;
const FCMP_CUN: u32 = 0b01000;
const FCMP_CEQ: u32 = 0b00100;
const FCMP_CUEQ: u32 = 0b01100;
const FCMP_CLT: u32 = 0b00010;
const FCMP_CLE: u32 = 0b00110;
const FCMP_CNE: u32 = 0b10000;
const FCMP_COR: u32 = 0b10100;
const FCMP_SAF: u32 = 0b00001;
const FCMP_SUN: u32 = 0b01001;
const FCMP_SEQ: u32 = 0b00101;

const QNAN_S: u32 = 0x7fc0_0001;
const SNAN_S: u32 = 0x7f80_0001;
const QNAN_D: u64 = 0x7ff8_0000_0000_0001;
const SNAN_D: u64 = 0x7ff0_0000_0000_0001;

fn fr3(op: u32, fd: u32, fj: u32, fk: u32) -> u32 {
    (op << 15) | (fk << 10) | (fj << 5) | fd
}

fn fr4(op: u32, fd: u32, fj: u32, fk: u32, fa: u32) -> u32 {
    (op << 20) | (fa << 15) | (fk << 10) | (fj << 5) | fd
}

fn frr(op: u32, fd: u32, rj: u32, rk: u32) -> u32 {
    (op << 15) | (rk << 10) | (rj << 5) | fd
}

fn fr2(op: u32, fd: u32, fj: u32) -> u32 {
    (op << 10) | (fj << 5) | fd
}

fn fcmp(op: u32, cond: u32, cd: u32, fj: u32, fk: u32) -> u32 {
    (op << 20) | (cond << 15) | (fk << 10) | (fj << 5) | cd
}

fn fsel(fd: u32, fj: u32, fk: u32, ca: u32) -> u32 {
    (OP_FSEL << 20) | (ca << 15) | (fk << 10) | (fj << 5) | fd
}

fn fbranch(kind: u32, offs21: i32, cj: u32) -> u32 {
    let imm = offs21 as u32 & 0x001F_FFFF;
    (OP_FBRANCH << 26)
        | ((imm & 0xFFFF) << 10)
        | ((cj & 0x7) << 5)
        | ((kind & 0x3) << 8)
        | ((imm >> 16) & 0x1F)
}

fn r2_si12(op: u32, si12: i16, rj: u32, rd: u32) -> u32 {
    (op << 22) | ((si12 as u16 as u32 & 0x0FFF) << 10) | (rj << 5) | rd
}

fn offs26(op: u32, offs26: i32) -> u32 {
    let imm = offs26 as u32 & 0x03FF_FFFF;
    (op << 26) | (((imm >> 16) & 0x3FF) << 0) | ((imm & 0xFFFF) << 10)
}

fn code15(op: u32, code: u32) -> u32 {
    (op << 15) | (code & 0x7FFF)
}

fn run_la(cpu: &mut LoongArchCpu, insns: &[u32]) -> usize {
    let code: Vec<u8> =
        insns.iter().flat_map(|insn| insn.to_le_bytes()).collect();

    let mut codebuf = CodeBuffer::new(16 * 1024).unwrap();
    let mut backend = X86_64CodeGen::new();
    backend.set_guest_base_offset(GUEST_BASE_CPU_OFFSET);
    backend.emit_prologue(&mut codebuf);
    backend.emit_epilogue(&mut codebuf);

    let mut ir = Context::new();
    backend.init_context(&mut ir);

    let mut ctx =
        LoongArchDisasContext::new(0, code.as_ptr(), LoongArchCfg::default());
    ctx.base.max_insns = insns.len() as u32;
    translator_loop::<LoongArchTranslator>(&mut ctx, &mut ir);

    unsafe {
        translate_and_execute(&mut ir, &backend, &mut codebuf, cpu.env_ptr())
    }
}

struct FpuExecLoopCpu {
    cpu: LoongArchCpu,
    code: Vec<u8>,
}

impl FpuExecLoopCpu {
    fn new(insns: &[u32]) -> Self {
        let code: Vec<u8> =
            insns.iter().flat_map(|insn| insn.to_le_bytes()).collect();
        let mut cpu = LoongArchCpu::new();
        cpu.set_guest_base(code.as_ptr() as u64);
        cpu.csr_write(CSR_EUEN, EUEN_FPE);
        Self { cpu, code }
    }

    fn reset_for_fbranch(&mut self, cj: usize, fcc: u8) {
        self.cpu.set_pc(0);
        self.cpu.reset_exit_request();
        self.cpu.write_gpr(5, 0);
        for idx in 0..8 {
            self.cpu.write_fcc(idx, 0);
        }
        self.cpu.write_fcc(cj, fcc);
    }
}

impl GuestCpu for FpuExecLoopCpu {
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

fn run_fpu_exec_loop(
    env: &mut ExecEnv<X86_64CodeGen>,
    cpu: &mut FpuExecLoopCpu,
) {
    let r = unsafe { cpu_exec_loop_env(env, cpu) };
    assert_eq!(r, ExitReason::Halted);
}

const fn nanbox_s(bits: u32) -> u64 {
    0xffff_ffff_0000_0000 | bits as u64
}

const fn i32_result(value: i32) -> u64 {
    value as u32 as u64
}

const fn i64_result(value: i64) -> u64 {
    value as u64
}

const fn fcsr_rm(rm: u32) -> u32 {
    rm << FCSR_RM_SHIFT
}

const fn fcsr_enable(bits: u32) -> u32 {
    bits << FCSR_ENABLE_SHIFT
}

const fn fcsr_flags(bits: u32) -> u32 {
    bits << FCSR_FLAG_SHIFT
}

const fn fcsr_cause(bits: u32) -> u32 {
    bits << FCSR_CAUSE_SHIFT
}

fn fcsr_bits(cpu: &LoongArchCpu) -> u32 {
    cpu.read_fcsr()
}

fn assert_fcsr_cause_flags(cpu: &LoongArchCpu, cause: u32, flags: u32) {
    assert_eq!(
        fcsr_bits(cpu) & (fcsr_cause(cause) | fcsr_flags(flags)),
        fcsr_cause(cause) | fcsr_flags(flags)
    );
}

fn assert_enabled_fpe_trap<F>(
    insn: u32,
    enable: u32,
    expected_cause: u32,
    setup: F,
) where
    F: FnOnce(&mut LoongArchCpu),
{
    let mut cpu = LoongArchCpu::new();
    let sentinel = 0xfeed_face_dead_beefu64;

    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.csr_write(CSR_CRMD, CRMD_DA);
    cpu.csr_write(CSR_EENTRY, 0x9000_0000);
    cpu.write_fcsr(fcsr_enable(enable));
    cpu.write_fpr(3, sentinel);
    setup(&mut cpu);

    assert_ne!(run_la(&mut cpu, &[insn]), 0);
    assert_eq!(cpu.read_fpr(3), sentinel);
    assert_eq!(cpu.pc(), 0x9000_0000);
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 16 & 0x3f, u64::from(ECODE_FPE));
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 22 & 0x1ff, 0);
    assert_eq!(cpu.era(), 0);
    assert_eq!(
        fcsr_bits(&cpu) & fcsr_cause(expected_cause),
        fcsr_cause(expected_cause)
    );
    assert_eq!(fcsr_bits(&cpu) & fcsr_flags(0x1f), 0);
}

fn assert_enabled_fcmp_invalid_trap(
    insn: u32,
    setup: impl FnOnce(&mut LoongArchCpu),
) {
    let mut cpu = LoongArchCpu::new();
    let cd = (insn & 0x7) as usize;

    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.csr_write(CSR_CRMD, CRMD_DA);
    cpu.csr_write(CSR_EENTRY, 0x9000_0000);
    cpu.write_fcsr(fcsr_enable(FP_V));
    cpu.write_fcc(cd, 1);
    setup(&mut cpu);

    assert_ne!(run_la(&mut cpu, &[insn]), 0);
    assert_eq!(cpu.read_fcc(cd), 1);
    assert_eq!(cpu.pc(), 0x9000_0000);
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 16 & 0x3f, u64::from(ECODE_FPE));
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 22 & 0x1ff, 0);
    assert_eq!(cpu.era(), 0);
    assert_eq!(fcsr_bits(&cpu) & fcsr_cause(FP_V), fcsr_cause(FP_V));
    assert_eq!(fcsr_bits(&cpu) & fcsr_flags(FP_V), 0);
}

fn assert_predicate_fp_bce_trap(
    insn: u32,
    rj_val: u64,
    rk_val: u64,
    verify_no_side_effects: impl FnOnce(&LoongArchCpu, &[u8]),
) {
    let mut data = [0u8; 64];
    data[12..20].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());
    data[24..32].copy_from_slice(&0xaabb_ccdd_eeff_0011u64.to_le_bytes());

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.csr_write(CSR_CRMD, CRMD_DA);
    cpu.csr_write(CSR_EENTRY, 0x9000_0000);
    cpu.set_guest_base(data.as_mut_ptr() as u64);
    cpu.write_gpr(1, rj_val);
    cpu.write_gpr(2, rk_val);
    cpu.write_fpr(3, FPR_SENTINEL);
    cpu.write_fpr(4, 0x0102_0304_0506_0708);
    cpu.write_fpr(5, nanbox_s((-2.5f32).to_bits()));

    assert_ne!(run_la(&mut cpu, &[insn]), 0);
    assert_eq!(cpu.pc(), 0x9000_0000);
    assert_eq!(cpu.era(), 0);
    assert_eq!(cpu.csr_read(CSR_BADV), rj_val);
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 16 & 0x3f, u64::from(ECODE_BCE));
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 22 & 0x1ff, 0);
    verify_no_side_effects(&cpu, &data);
}

#[track_caller]
fn assert_zero_vector_predicate_fp_bce_trap(
    insn: u32,
    rj_val: u64,
    rk_val: u64,
    verify_no_side_effects: impl FnOnce(&LoongArchCpu, &[u8]),
) {
    let mut data = [0u8; 64];
    data[12..20].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.csr_write(CSR_CRMD, CRMD_DA);
    cpu.set_guest_base(data.as_mut_ptr() as u64);
    cpu.write_gpr(1, rj_val);
    cpu.write_gpr(2, rk_val);
    cpu.write_fpr(3, FPR_SENTINEL);
    cpu.write_fpr(4, 0x0102_0304_0506_0708);

    assert_ne!(run_la(&mut cpu, &[insn]), 0);
    assert_eq!(cpu.pc(), 0);
    assert_eq!(cpu.era(), 0);
    assert_eq!(cpu.csr_read(CSR_BADV), rj_val);
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 16 & 0x3f, u64::from(ECODE_BCE));
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 22 & 0x1ff, 0);
    verify_no_side_effects(&cpu, &data);
}

fn assert_disabled_fpe_trap_no_side_effects(
    insn: u32,
    setup: impl FnOnce(&mut LoongArchCpu),
    verify_no_side_effects: impl FnOnce(&LoongArchCpu),
) {
    let mut cpu = LoongArchCpu::new();

    cpu.csr_write(CSR_EUEN, 0);
    cpu.csr_write(CSR_CRMD, CRMD_DA);
    cpu.csr_write(CSR_EENTRY, FPE_DISABLED_VECTOR);
    cpu.write_fcsr(FCSR_SENTINEL);
    setup(&mut cpu);

    assert_ne!(run_la(&mut cpu, &[insn]), 0);

    assert_eq!(cpu.pc(), FPE_DISABLED_VECTOR);
    assert_eq!(cpu.csr_read(CSR_EUEN) & EUEN_FPE, 0);
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 16 & 0x3f, u64::from(ECODE_FPD));
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 22 & 0x1ff, 0);
    assert_eq!(cpu.era(), 0);
    assert_eq!(fcsr_bits(&cpu), FCSR_SENTINEL);
    verify_no_side_effects(&cpu);
}

fn assert_disabled_fpe_trap_fpr_dest(
    insn: u32,
    setup: impl FnOnce(&mut LoongArchCpu),
) {
    assert_disabled_fpe_trap_no_side_effects(
        insn,
        |cpu| {
            cpu.write_fpr(3, FPR_SENTINEL);
            setup(cpu);
        },
        |cpu| assert_eq!(cpu.read_fpr(3), FPR_SENTINEL),
    );
}

fn assert_disabled_fpe_trap_gpr_dest(
    insn: u32,
    rd: usize,
    setup: impl FnOnce(&mut LoongArchCpu),
) {
    assert_disabled_fpe_trap_no_side_effects(
        insn,
        |cpu| {
            cpu.write_gpr(rd, GPR_SENTINEL);
            setup(cpu);
        },
        |cpu| assert_eq!(cpu.read_gpr(rd), GPR_SENTINEL),
    );
}

fn assert_disabled_fpe_trap_fcc_dest(
    insn: u32,
    cd: usize,
    sentinel: u8,
    setup: impl FnOnce(&mut LoongArchCpu),
) {
    assert_disabled_fpe_trap_no_side_effects(
        insn,
        |cpu| {
            cpu.write_fcc(cd, sentinel);
            setup(cpu);
        },
        |cpu| assert_eq!(cpu.read_fcc(cd), sentinel),
    );
}

#[test]
fn task31_translated_single_fused_ops_nanbox_results() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, nanbox_s(2.0f32.to_bits()));
    cpu.write_fpr(2, nanbox_s(3.0f32.to_bits()));
    cpu.write_fpr(3, nanbox_s(1.0f32.to_bits()));
    cpu.write_fpr(4, nanbox_s(5.0f32.to_bits()));
    cpu.write_fpr(5, nanbox_s(4.0f32.to_bits()));
    cpu.write_fpr(6, nanbox_s(3.0f32.to_bits()));
    cpu.write_fpr(7, nanbox_s(2.0f32.to_bits()));
    cpu.write_fpr(8, nanbox_s(3.0f32.to_bits()));
    cpu.write_fpr(9, nanbox_s(1.0f32.to_bits()));
    cpu.write_fpr(10, nanbox_s(5.0f32.to_bits()));
    cpu.write_fpr(11, nanbox_s(4.0f32.to_bits()));
    cpu.write_fpr(12, nanbox_s(3.0f32.to_bits()));

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr4(OP_FMADD_S, 16, 1, 2, 3),
                fr4(OP_FMSUB_S, 17, 4, 5, 6),
                fr4(OP_FNMADD_S, 18, 7, 8, 9),
                fr4(OP_FNMSUB_S, 19, 10, 11, 12),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(16), nanbox_s(7.0f32.to_bits()));
    assert_eq!(cpu.read_fpr(17), nanbox_s(17.0f32.to_bits()));
    assert_eq!(cpu.read_fpr(18), nanbox_s((-7.0f32).to_bits()));
    assert_eq!(cpu.read_fpr(19), nanbox_s((-17.0f32).to_bits()));
}

#[test]
fn task31_translated_double_fused_ops_results() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, 2.0f64.to_bits());
    cpu.write_fpr(2, 3.0f64.to_bits());
    cpu.write_fpr(3, 1.0f64.to_bits());
    cpu.write_fpr(4, 5.0f64.to_bits());
    cpu.write_fpr(5, 4.0f64.to_bits());
    cpu.write_fpr(6, 3.0f64.to_bits());
    cpu.write_fpr(7, 2.0f64.to_bits());
    cpu.write_fpr(8, 3.0f64.to_bits());
    cpu.write_fpr(9, 1.0f64.to_bits());
    cpu.write_fpr(10, 5.0f64.to_bits());
    cpu.write_fpr(11, 4.0f64.to_bits());
    cpu.write_fpr(12, 3.0f64.to_bits());

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr4(OP_FMADD_D, 16, 1, 2, 3),
                fr4(OP_FMSUB_D, 17, 4, 5, 6),
                fr4(OP_FNMADD_D, 18, 7, 8, 9),
                fr4(OP_FNMSUB_D, 19, 10, 11, 12),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(16), 7.0f64.to_bits());
    assert_eq!(cpu.read_fpr(17), 17.0f64.to_bits());
    assert_eq!(cpu.read_fpr(18), (-7.0f64).to_bits());
    assert_eq!(cpu.read_fpr(19), (-17.0f64).to_bits());
}

#[test]
fn task31_translated_negative_fused_ops_preserve_signed_zero() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, nanbox_s(1.0f32.to_bits()));
    cpu.write_fpr(2, nanbox_s(1.0f32.to_bits()));
    cpu.write_fpr(3, nanbox_s((-1.0f32).to_bits()));
    cpu.write_fpr(4, nanbox_s(1.0f32.to_bits()));
    cpu.write_fpr(5, nanbox_s(1.0f32.to_bits()));
    cpu.write_fpr(6, nanbox_s(1.0f32.to_bits()));
    cpu.write_fpr(7, 1.0f64.to_bits());
    cpu.write_fpr(8, 1.0f64.to_bits());
    cpu.write_fpr(9, (-1.0f64).to_bits());
    cpu.write_fpr(10, 1.0f64.to_bits());
    cpu.write_fpr(11, 1.0f64.to_bits());
    cpu.write_fpr(12, 1.0f64.to_bits());

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr4(OP_FNMADD_S, 16, 1, 2, 3),
                fr4(OP_FNMSUB_S, 17, 4, 5, 6),
                fr4(OP_FNMADD_D, 18, 7, 8, 9),
                fr4(OP_FNMSUB_D, 19, 10, 11, 12),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(16), nanbox_s((-0.0f32).to_bits()));
    assert_eq!(cpu.read_fpr(17), nanbox_s((-0.0f32).to_bits()));
    assert_eq!(cpu.read_fpr(18), (-0.0f64).to_bits());
    assert_eq!(cpu.read_fpr(19), (-0.0f64).to_bits());
}

#[test]
fn task31_translated_negative_fused_ops_preserve_nan_signs() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    let qnan_s = 0x7fc0_0001u32;
    let qnan_d = 0x7ff8_0000_0000_0001u64;

    cpu.write_fpr(1, nanbox_s(qnan_s));
    cpu.write_fpr(2, nanbox_s(2.0f32.to_bits()));
    cpu.write_fpr(3, nanbox_s(3.0f32.to_bits()));
    cpu.write_fpr(4, nanbox_s(qnan_s));
    cpu.write_fpr(5, nanbox_s(2.0f32.to_bits()));
    cpu.write_fpr(6, nanbox_s(3.0f32.to_bits()));
    cpu.write_fpr(7, qnan_d);
    cpu.write_fpr(8, 2.0f64.to_bits());
    cpu.write_fpr(9, 3.0f64.to_bits());
    cpu.write_fpr(10, qnan_d);
    cpu.write_fpr(11, 2.0f64.to_bits());
    cpu.write_fpr(12, 3.0f64.to_bits());

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr4(OP_FNMADD_S, 16, 1, 2, 3),
                fr4(OP_FNMSUB_S, 17, 4, 5, 6),
                fr4(OP_FNMADD_D, 18, 7, 8, 9),
                fr4(OP_FNMSUB_D, 19, 10, 11, 12),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(16), nanbox_s(qnan_s));
    assert_eq!(cpu.read_fpr(17), nanbox_s(qnan_s));
    assert_eq!(cpu.read_fpr(18), qnan_d);
    assert_eq!(cpu.read_fpr(19), qnan_d);
}

#[test]
fn task31_translated_fused_ops_preserve_addend_nan_signs() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    let qnan_s = 0x7fc0_0001u32;
    let qnan_d = 0x7ff8_0000_0000_0001u64;

    cpu.write_fpr(1, nanbox_s(2.0f32.to_bits()));
    cpu.write_fpr(2, nanbox_s(3.0f32.to_bits()));
    cpu.write_fpr(3, nanbox_s(qnan_s));
    cpu.write_fpr(4, nanbox_s(2.0f32.to_bits()));
    cpu.write_fpr(5, nanbox_s(3.0f32.to_bits()));
    cpu.write_fpr(6, nanbox_s(qnan_s));
    cpu.write_fpr(7, 2.0f64.to_bits());
    cpu.write_fpr(8, 3.0f64.to_bits());
    cpu.write_fpr(9, qnan_d);
    cpu.write_fpr(10, 2.0f64.to_bits());
    cpu.write_fpr(11, 3.0f64.to_bits());
    cpu.write_fpr(12, qnan_d);

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr4(OP_FMSUB_S, 16, 1, 2, 3),
                fr4(OP_FNMSUB_S, 17, 4, 5, 6),
                fr4(OP_FMSUB_D, 18, 7, 8, 9),
                fr4(OP_FNMSUB_D, 19, 10, 11, 12),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(16), nanbox_s(qnan_s));
    assert_eq!(cpu.read_fpr(17), nanbox_s(qnan_s));
    assert_eq!(cpu.read_fpr(18), qnan_d);
    assert_eq!(cpu.read_fpr(19), qnan_d);
}

#[test]
fn task32_translated_ftint_obeys_fcsr_rounding_mode() {
    let cases = [
        (0, i32_result(2)), // RNE
        (1, i32_result(1)), // RTZ
        (2, i32_result(2)), // RUP
        (3, i32_result(1)), // RDN
    ];

    for (rm, expected) in cases {
        let mut cpu = LoongArchCpu::new();
        cpu.csr_write(CSR_EUEN, EUEN_FPE);
        cpu.write_fcsr(fcsr_rm(rm));
        cpu.write_fpr(1, 1.5f64.to_bits());

        assert_eq!(run_la(&mut cpu, &[fr2(OP_FTINT_W_D, 2, 1)]), 0);

        assert_eq!(cpu.read_fpr(2), expected, "rm={rm}");
        assert_eq!(
            fcsr_bits(&cpu) & (fcsr_cause(FP_I) | fcsr_flags(FP_I)),
            fcsr_cause(FP_I) | fcsr_flags(FP_I),
            "rm={rm}"
        );
    }
}

#[test]
fn task32_translated_fcsr_cause_flags_accumulate_and_clear() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, 1.0f64.to_bits());
    cpu.write_fpr(2, 0.0f64.to_bits());
    assert_eq!(run_la(&mut cpu, &[fr3(OP_FDIV_D, 3, 1, 2)]), 0);
    assert_eq!(cpu.read_fpr(3), f64::INFINITY.to_bits());
    assert_eq!(
        fcsr_bits(&cpu) & (fcsr_cause(FP_Z) | fcsr_flags(FP_Z)),
        fcsr_cause(FP_Z) | fcsr_flags(FP_Z)
    );

    cpu.write_fpr(4, (-1.0f64).to_bits());
    assert_eq!(run_la(&mut cpu, &[fr2(OP_FSQRT_D, 5, 4)]), 0);
    assert!(f64::from_bits(cpu.read_fpr(5)).is_nan());
    assert_eq!(
        fcsr_bits(&cpu) & (fcsr_cause(FP_V) | fcsr_flags(FP_Z | FP_V)),
        fcsr_cause(FP_V) | fcsr_flags(FP_Z | FP_V)
    );

    cpu.write_fcsr(0);
    assert_eq!(fcsr_bits(&cpu) & (fcsr_cause(0x1f) | fcsr_flags(0x1f)), 0);

    cpu.write_fpr(6, 2.5f64.to_bits());
    assert_eq!(run_la(&mut cpu, &[fr2(OP_FTINT_W_D, 7, 6)]), 0);
    assert_eq!(cpu.read_fpr(7), i32_result(2));
    assert_eq!(
        fcsr_bits(&cpu) & (fcsr_cause(FP_I) | fcsr_flags(FP_I)),
        fcsr_cause(FP_I) | fcsr_flags(FP_I)
    );
}

#[test]
fn task32_translated_fcsr_overflow_and_underflow_flags() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, f64::MAX.to_bits());
    cpu.write_fpr(2, 2.0f64.to_bits());
    assert_eq!(run_la(&mut cpu, &[fr3(OP_FMUL_D, 3, 1, 2)]), 0);
    assert_eq!(cpu.read_fpr(3), f64::INFINITY.to_bits());
    assert_eq!(
        fcsr_bits(&cpu) & (fcsr_cause(FP_O | FP_I) | fcsr_flags(FP_O | FP_I)),
        fcsr_cause(FP_O | FP_I) | fcsr_flags(FP_O | FP_I)
    );

    cpu.write_fcsr(0);
    cpu.write_fpr(4, f64::MIN_POSITIVE.to_bits());
    cpu.write_fpr(5, f64::MIN_POSITIVE.to_bits());
    assert_eq!(run_la(&mut cpu, &[fr3(OP_FMUL_D, 6, 4, 5)]), 0);
    assert_eq!(cpu.read_fpr(6), 0.0f64.to_bits());
    assert_eq!(
        fcsr_bits(&cpu) & (fcsr_cause(FP_U | FP_I) | fcsr_flags(FP_U | FP_I)),
        fcsr_cause(FP_U | FP_I) | fcsr_flags(FP_U | FP_I)
    );
}

#[test]
fn task32_translated_arithmetic_helpers_report_broad_exception_flags() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, f64::INFINITY.to_bits());
    cpu.write_fpr(2, f64::NEG_INFINITY.to_bits());
    assert_eq!(run_la(&mut cpu, &[fr3(OP_FADD_D, 3, 1, 2)]), 0);
    assert!(f64::from_bits(cpu.read_fpr(3)).is_nan());
    assert_fcsr_cause_flags(&cpu, FP_V, FP_V);

    cpu.write_fcsr(0);
    cpu.write_fpr(4, f64::INFINITY.to_bits());
    cpu.write_fpr(5, f64::INFINITY.to_bits());
    assert_eq!(run_la(&mut cpu, &[fr3(OP_FSUB_D, 6, 4, 5)]), 0);
    assert!(f64::from_bits(cpu.read_fpr(6)).is_nan());
    assert_fcsr_cause_flags(&cpu, FP_V, FP_V);

    cpu.write_fcsr(0);
    cpu.write_fpr(7, 1.0f64.to_bits());
    cpu.write_fpr(8, 3.0f64.to_bits());
    assert_eq!(run_la(&mut cpu, &[fr3(OP_FDIV_D, 9, 7, 8)]), 0);
    assert_fcsr_cause_flags(&cpu, FP_I, FP_I);

    cpu.write_fcsr(0);
    cpu.write_fpr(10, 2.0f64.to_bits());
    assert_eq!(run_la(&mut cpu, &[fr2(OP_FSQRT_D, 11, 10)]), 0);
    assert_fcsr_cause_flags(&cpu, FP_I, FP_I);
}

#[test]
fn task32_translated_fused_helpers_report_exception_flags() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, f64::INFINITY.to_bits());
    cpu.write_fpr(2, 0.0f64.to_bits());
    cpu.write_fpr(3, 1.0f64.to_bits());
    assert_eq!(run_la(&mut cpu, &[fr4(OP_FMADD_D, 4, 1, 2, 3)]), 0);
    assert!(f64::from_bits(cpu.read_fpr(4)).is_nan());
    assert_fcsr_cause_flags(&cpu, FP_V, FP_V);

    cpu.write_fcsr(0);
    cpu.write_fpr(5, f64::MAX.to_bits());
    cpu.write_fpr(6, 2.0f64.to_bits());
    cpu.write_fpr(7, (-f64::MAX).to_bits());
    assert_eq!(run_la(&mut cpu, &[fr4(OP_FMSUB_D, 8, 5, 6, 7)]), 0);
    assert_eq!(cpu.read_fpr(8), f64::INFINITY.to_bits());
    assert_fcsr_cause_flags(&cpu, FP_O | FP_I, FP_O | FP_I);

    cpu.write_fcsr(0);
    cpu.write_fpr(9, 1.0f64.to_bits());
    cpu.write_fpr(10, 1.0f64.to_bits());
    cpu.write_fpr(11, f64::from_bits(0x3ca0_0000_0000_0000).to_bits());
    assert_eq!(run_la(&mut cpu, &[fr4(OP_FNMADD_D, 12, 9, 10, 11)]), 0);
    assert_fcsr_cause_flags(&cpu, FP_I, FP_I);
}

#[test]
fn task32_translated_fcvt_and_ffint_obey_fcsr_rounding_mode() {
    let cases = [
        (0, 0x4b80_0000u32), // RNE
        (1, 0x4b80_0000u32), // RTZ
        (2, 0x4b80_0001u32), // RUP
        (3, 0x4b80_0000u32), // RDN
    ];

    for (rm, expected) in cases {
        let mut cpu = LoongArchCpu::new();
        cpu.csr_write(CSR_EUEN, EUEN_FPE);
        cpu.write_fcsr(fcsr_rm(rm));
        cpu.write_fpr(1, 16_777_217.0f64.to_bits());
        cpu.write_fpr(2, i64_result(16_777_217));

        assert_eq!(
            run_la(
                &mut cpu,
                &[fr2(OP_FCVT_S_D, 3, 1), fr2(OP_FFINT_S_L, 4, 2)],
            ),
            0,
            "rm={rm}"
        );

        assert_eq!(cpu.read_fpr(3), nanbox_s(expected), "fcvt rm={rm}");
        assert_eq!(cpu.read_fpr(4), nanbox_s(expected), "ffint rm={rm}");
        assert_fcsr_cause_flags(&cpu, FP_I, FP_I);
    }
}

#[test]
fn task32_translated_enabled_fpe_traps_each_exception_bit_without_side_effects()
{
    assert_enabled_fpe_trap(fr3(OP_FADD_D, 3, 1, 2), FP_V, FP_V, |cpu| {
        cpu.write_fpr(1, f64::INFINITY.to_bits());
        cpu.write_fpr(2, f64::NEG_INFINITY.to_bits());
    });

    assert_enabled_fpe_trap(fr3(OP_FDIV_D, 3, 1, 2), FP_Z, FP_Z, |cpu| {
        cpu.write_fpr(1, 1.0f64.to_bits());
        cpu.write_fpr(2, 0.0f64.to_bits());
    });

    assert_enabled_fpe_trap(
        fr3(OP_FADD_D, 3, 1, 2),
        FP_O,
        FP_O | FP_I,
        |cpu| {
            cpu.write_fpr(1, f64::MAX.to_bits());
            cpu.write_fpr(2, f64::MAX.to_bits());
        },
    );

    assert_enabled_fpe_trap(
        fr3(OP_FMUL_D, 3, 1, 2),
        FP_U,
        FP_U | FP_I,
        |cpu| {
            cpu.write_fpr(1, f64::MIN_POSITIVE.to_bits());
            cpu.write_fpr(2, f64::MIN_POSITIVE.to_bits());
        },
    );

    assert_enabled_fpe_trap(fr3(OP_FDIV_D, 3, 1, 2), FP_I, FP_I, |cpu| {
        cpu.write_fpr(1, 1.0f64.to_bits());
        cpu.write_fpr(2, 3.0f64.to_bits());
    });
}

#[test]
fn task32_translated_fcmp_quiet_qnan_writes_fcc_without_invalid() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.write_fcsr(fcsr_enable(FP_V));
    cpu.write_fcc(0, 0);
    cpu.write_fcc(1, 1);
    cpu.write_fpr(1, nanbox_s(QNAN_S));
    cpu.write_fpr(2, nanbox_s(1.0f32.to_bits()));
    cpu.write_fpr(3, QNAN_D);
    cpu.write_fpr(4, 1.0f64.to_bits());

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fcmp(OP_FCMP_S, FCMP_CUN, 0, 1, 2),
                fcmp(OP_FCMP_D, FCMP_CEQ, 1, 3, 4),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fcc(0), 1);
    assert_eq!(cpu.read_fcc(1), 0);
    assert_eq!(fcsr_bits(&cpu) & (fcsr_cause(FP_V) | fcsr_flags(FP_V)), 0);
}

#[test]
fn task32_translated_fcmp_quiet_snan_sets_invalid_flags() {
    let cases = [
        (
            OP_FCMP_S,
            FCMP_CUN,
            nanbox_s(SNAN_S),
            nanbox_s(1.0f32.to_bits()),
            1,
        ),
        (OP_FCMP_D, FCMP_CEQ, SNAN_D, 1.0f64.to_bits(), 0),
        (
            OP_FCMP_S,
            FCMP_CAF,
            nanbox_s(SNAN_S),
            nanbox_s(1.0f32.to_bits()),
            0,
        ),
    ];

    for (op, cond, fj, fk, expected_fcc) in cases {
        let mut cpu = LoongArchCpu::new();
        cpu.csr_write(CSR_EUEN, EUEN_FPE);
        cpu.write_fpr(1, fj);
        cpu.write_fpr(2, fk);

        assert_eq!(run_la(&mut cpu, &[fcmp(op, cond, 3, 1, 2)]), 0);

        assert_eq!(cpu.read_fcc(3), expected_fcc);
        assert_fcsr_cause_flags(&cpu, FP_V, FP_V);
    }
}

#[test]
fn task32_translated_fcmp_signaling_nan_sets_invalid_flags() {
    let cases = [
        (OP_FCMP_D, FCMP_SEQ, QNAN_D, 1.0f64.to_bits(), 0),
        (
            OP_FCMP_S,
            FCMP_SUN,
            nanbox_s(QNAN_S),
            nanbox_s(1.0f32.to_bits()),
            1,
        ),
        (OP_FCMP_D, FCMP_SAF, QNAN_D, 1.0f64.to_bits(), 0),
        (
            OP_FCMP_S,
            FCMP_SAF,
            nanbox_s(SNAN_S),
            nanbox_s(1.0f32.to_bits()),
            0,
        ),
    ];

    for (op, cond, fj, fk, expected_fcc) in cases {
        let mut cpu = LoongArchCpu::new();
        cpu.csr_write(CSR_EUEN, EUEN_FPE);
        cpu.write_fpr(1, fj);
        cpu.write_fpr(2, fk);

        assert_eq!(run_la(&mut cpu, &[fcmp(op, cond, 4, 1, 2)]), 0);

        assert_eq!(cpu.read_fcc(4), expected_fcc);
        assert_fcsr_cause_flags(&cpu, FP_V, FP_V);
    }
}

#[test]
fn task32_translated_enabled_fcmp_invalid_traps_without_fcc_write() {
    assert_enabled_fcmp_invalid_trap(
        fcmp(OP_FCMP_D, FCMP_SAF, 5, 1, 2),
        |cpu| {
            cpu.write_fpr(1, QNAN_D);
            cpu.write_fpr(2, 1.0f64.to_bits());
        },
    );

    assert_enabled_fcmp_invalid_trap(
        fcmp(OP_FCMP_S, FCMP_CAF, 6, 1, 2),
        |cpu| {
            cpu.write_fpr(1, nanbox_s(SNAN_S));
            cpu.write_fpr(2, nanbox_s(1.0f32.to_bits()));
        },
    );
}

#[test]
fn task32_translated_movgr2fcsr_and_movfcsr2gr_use_subregister_masks() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_gpr(1, 0xffff_ffff);
    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr2(OP_MOVGR2FCSR, 1, 1),
                fr2(OP_MOVFCSR2GR, 10, 1),
                fr2(OP_MOVGR2FCSR, 2, 1),
                fr2(OP_MOVFCSR2GR, 11, 2),
                fr2(OP_MOVGR2FCSR, 3, 1),
                fr2(OP_MOVFCSR2GR, 12, 3),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_gpr(10), 0x1f);
    assert_eq!(cpu.read_gpr(11), 0x1f1f_0000);
    assert_eq!(cpu.read_gpr(12), 0x300);
    assert_eq!(fcsr_bits(&cpu), 0x1f1f_031f);
}

#[test]
fn task32_translated_enabled_fpu_exception_enters_fpe_without_dest_write() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.csr_write(CSR_CRMD, CRMD_DA);
    cpu.csr_write(CSR_EENTRY, 0x9000_0000);
    cpu.write_fcsr(fcsr_enable(FP_Z));
    cpu.write_fpr(1, 1.0f64.to_bits());
    cpu.write_fpr(2, 0.0f64.to_bits());
    cpu.write_fpr(3, 0xfeed_face_dead_beefu64);

    assert_ne!(run_la(&mut cpu, &[fr3(OP_FDIV_D, 3, 1, 2)]), 0);

    assert_eq!(cpu.read_fpr(3), 0xfeed_face_dead_beefu64);
    assert_eq!(cpu.pc(), 0x9000_0000);
    assert_eq!(cpu.csr_read(CSR_EENTRY), 0x9000_0000);
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 16 & 0x3f, u64::from(ECODE_FPE));
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 22 & 0x1ff, 0);
    assert_eq!(cpu.era(), 0);
    assert_eq!(fcsr_bits(&cpu) & fcsr_cause(FP_Z), fcsr_cause(FP_Z));
    assert_eq!(fcsr_bits(&cpu) & fcsr_flags(FP_Z), 0);
}

#[test]
fn task33_disabled_fpe_traps_fpr_destination_families_without_side_effects() {
    assert_disabled_fpe_trap_fpr_dest(fr3(OP_FADD_D, 3, 1, 2), |cpu| {
        cpu.write_fpr(1, 1.0f64.to_bits());
        cpu.write_fpr(2, 2.0f64.to_bits());
    });
    assert_disabled_fpe_trap_fpr_dest(fr3(OP_FSUB_D, 3, 1, 2), |cpu| {
        cpu.write_fpr(1, 3.0f64.to_bits());
        cpu.write_fpr(2, 1.0f64.to_bits());
    });
    assert_disabled_fpe_trap_fpr_dest(fr3(OP_FMUL_D, 3, 1, 2), |cpu| {
        cpu.write_fpr(1, 3.0f64.to_bits());
        cpu.write_fpr(2, 4.0f64.to_bits());
    });
    assert_disabled_fpe_trap_fpr_dest(fr3(OP_FDIV_D, 3, 1, 2), |cpu| {
        cpu.write_fpr(1, 8.0f64.to_bits());
        cpu.write_fpr(2, 2.0f64.to_bits());
    });
    assert_disabled_fpe_trap_fpr_dest(fr2(OP_FSQRT_D, 3, 1), |cpu| {
        cpu.write_fpr(1, 4.0f64.to_bits());
    });

    assert_disabled_fpe_trap_fpr_dest(fr2(OP_FCVT_S_D, 3, 1), |cpu| {
        cpu.write_fpr(1, 1.5f64.to_bits());
    });
    assert_disabled_fpe_trap_fpr_dest(fr2(OP_FCVT_D_S, 3, 1), |cpu| {
        cpu.write_fpr(1, nanbox_s(1.5f32.to_bits()));
    });
    assert_disabled_fpe_trap_fpr_dest(fr2(OP_FFINT_D_L, 3, 1), |cpu| {
        cpu.write_fpr(1, i64_result(42));
    });
    assert_disabled_fpe_trap_fpr_dest(fr2(OP_FTINT_W_D, 3, 1), |cpu| {
        cpu.write_fpr(1, 3.5f64.to_bits());
    });
    assert_disabled_fpe_trap_fpr_dest(fr2(OP_FTINTRZ_L_D, 3, 1), |cpu| {
        cpu.write_fpr(1, 3.5f64.to_bits());
    });

    for op in [OP_FMADD_D, OP_FMSUB_D, OP_FNMADD_D, OP_FNMSUB_D] {
        assert_disabled_fpe_trap_fpr_dest(fr4(op, 3, 1, 2, 4), |cpu| {
            cpu.write_fpr(1, 2.0f64.to_bits());
            cpu.write_fpr(2, 3.0f64.to_bits());
            cpu.write_fpr(4, 1.0f64.to_bits());
        });
    }

    assert_disabled_fpe_trap_fpr_dest(fr2(OP_FMOV_D, 3, 1), |cpu| {
        cpu.write_fpr(1, 0x1111_2222_3333_4444);
    });
    assert_disabled_fpe_trap_fpr_dest(fr2(OP_FABS_S, 3, 1), |cpu| {
        cpu.write_fpr(1, nanbox_s((-1.0f32).to_bits()));
    });
    assert_disabled_fpe_trap_fpr_dest(fr2(OP_FNEG_D, 3, 1), |cpu| {
        cpu.write_fpr(1, 1.0f64.to_bits());
    });
    assert_disabled_fpe_trap_fpr_dest(fr2(OP_MOVGR2FR_D, 3, 1), |cpu| {
        cpu.write_gpr(1, 0x0102_0304_0506_0708);
    });
    assert_disabled_fpe_trap_fpr_dest(fsel(3, 1, 2, 7), |cpu| {
        cpu.write_fpr(1, 0x1111_1111_1111_1111);
        cpu.write_fpr(2, 0x2222_2222_2222_2222);
        cpu.write_fcc(7, 1);
    });
}

#[test]
fn task33_disabled_fpe_traps_fcc_fcsr_gpr_and_branch_families_without_side_effects(
) {
    assert_disabled_fpe_trap_fcc_dest(
        fcmp(OP_FCMP_D, FCMP_CEQ, 5, 1, 2),
        5,
        0,
        |cpu| {
            cpu.write_fpr(1, 1.0f64.to_bits());
            cpu.write_fpr(2, 1.0f64.to_bits());
        },
    );
    assert_disabled_fpe_trap_fcc_dest(
        fcmp(OP_FCMP_S, FCMP_SAF, 6, 1, 2),
        6,
        1,
        |cpu| {
            cpu.write_fpr(1, nanbox_s(QNAN_S));
            cpu.write_fpr(2, nanbox_s(1.0f32.to_bits()));
        },
    );

    assert_disabled_fpe_trap_no_side_effects(
        fr2(OP_MOVGR2FCSR, 1, 2),
        |cpu| cpu.write_gpr(2, 0),
        |_| {},
    );
    assert_disabled_fpe_trap_gpr_dest(fr2(OP_MOVFCSR2GR, 10, 1), 10, |_| {});
    assert_disabled_fpe_trap_gpr_dest(fr2(OP_MOVFR2GR_D, 10, 1), 10, |cpu| {
        cpu.write_fpr(1, 0x1111_2222_3333_4444)
    });

    assert_disabled_fpe_trap_no_side_effects(
        fbranch(0, 2, 1),
        |cpu| cpu.write_fcc(1, 0),
        |cpu| assert_eq!(cpu.read_fcc(1), 0),
    );
    assert_disabled_fpe_trap_no_side_effects(
        fbranch(1, 2, 2),
        |cpu| cpu.write_fcc(2, 1),
        |cpu| assert_eq!(cpu.read_fcc(2), 1),
    );
}

#[test]
fn task33_disabled_fpe_traps_fp_load_store_before_memory_side_effects() {
    let load_data = [0x1111_2222_3333_4444u64];
    assert_disabled_fpe_trap_no_side_effects(
        r2_si12(OP_FLD_D, 0, 1, 3),
        |cpu| {
            cpu.set_guest_base(load_data.as_ptr() as u64);
            cpu.write_gpr(1, 0);
            cpu.write_fpr(3, FPR_SENTINEL);
        },
        |cpu| assert_eq!(cpu.read_fpr(3), FPR_SENTINEL),
    );
    assert_eq!(load_data[0], 0x1111_2222_3333_4444);

    let mut store_data = [0x5555_6666_7777_8888u64];
    assert_disabled_fpe_trap_no_side_effects(
        r2_si12(OP_FST_D, 0, 1, 3),
        |cpu| {
            cpu.set_guest_base(store_data.as_mut_ptr() as u64);
            cpu.write_gpr(1, 0);
            cpu.write_fpr(3, 0x9999_aaaa_bbbb_cccc);
        },
        |cpu| assert_eq!(cpu.read_fpr(3), 0x9999_aaaa_bbbb_cccc),
    );
    assert_eq!(store_data[0], 0x5555_6666_7777_8888);
}

#[test]
fn task34_single_moves_preserve_nanboxing_and_high_halves() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, nanbox_s((-1.25f32).to_bits()));
    cpu.write_fpr(16, nanbox_s(1.25f32.to_bits()));
    cpu.write_fpr(4, 0x1111_2222_3333_4444);
    cpu.write_gpr(5, 0xdead_beef_89ab_cdef);
    cpu.write_gpr(6, 0x0123_4567_f654_3210);

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr2(OP_FABS_S, 2, 1),
                fr2(OP_FNEG_S, 3, 16),
                fr2(OP_MOVFR2GR_S, 8, 3),
                fr2(OP_MOVGR2FR_W, 4, 5),
                fr2(OP_MOVGR2FRH_W, 4, 6),
                fr2(OP_MOVFRH2GR_S, 7, 4),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(2), nanbox_s(1.25f32.to_bits()));
    assert_eq!(cpu.read_fpr(3), nanbox_s((-1.25f32).to_bits()));
    assert_eq!(cpu.read_gpr(8), 0xffff_ffff_bfa0_0000);
    assert_eq!(cpu.read_fpr(4), 0xf654_3210_89ab_cdef);
    assert_eq!(cpu.read_gpr(7), 0xffff_ffff_f654_3210);
}

#[test]
fn task34_condition_code_moves_transfer_low_bits_only() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.write_gpr(1, 0xaaaa_aaaa_aaaa_aaa2);
    cpu.write_gpr(2, 0xbbbb_bbbb_bbbb_bbb3);
    cpu.write_fpr(3, 0x1111_2222_3333_4445);
    cpu.write_fpr(4, 0x5555_6666_7777_8888);

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr2(OP_MOVGR2CF, 5, 1),
                fr2(OP_MOVGR2CF, 6, 2),
                fr2(OP_MOVCF2GR, 10, 6),
                fr2(OP_MOVFR2CF, 7, 3),
                fr2(OP_MOVCF2FR, 8, 7),
                fr2(OP_MOVFR2CF, 1, 4),
                fr2(OP_MOVCF2GR, 11, 1),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fcc(5), 0);
    assert_eq!(cpu.read_fcc(6), 1);
    assert_eq!(cpu.read_gpr(10), 1);
    assert_eq!(cpu.read_fcc(7), 1);
    assert_eq!(cpu.read_fpr(8), 1);
    assert_eq!(cpu.read_fcc(1), 0);
    assert_eq!(cpu.read_gpr(11), 0);
}

#[test]
fn task34_indexed_and_predicate_fp_memory_paths_work_and_nanbox_singles() {
    let mut data = [0u8; 64];
    data[12..16].copy_from_slice(&1.5f32.to_bits().to_le_bytes());
    data[16..24].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());
    data[32..40].copy_from_slice(&0x8877_6655_4433_2211u64.to_le_bytes());
    data[44..48].copy_from_slice(&7.5f32.to_bits().to_le_bytes());

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.set_guest_base(data.as_mut_ptr() as u64);
    cpu.write_gpr(1, 8);
    cpu.write_gpr(2, 4);
    cpu.write_gpr(3, 16);
    cpu.write_gpr(4, 0);
    cpu.write_gpr(5, 20);
    cpu.write_gpr(6, 4);
    cpu.write_gpr(7, 24);
    cpu.write_gpr(8, 8);
    cpu.write_gpr(9, 4);
    cpu.write_gpr(10, 24);
    cpu.write_gpr(11, 8);
    cpu.write_gpr(17, 44);
    cpu.write_gpr(19, 4);
    cpu.write_gpr(20, 8);
    cpu.write_fpr(12, nanbox_s((-2.5f32).to_bits()));
    cpu.write_fpr(13, 0xaabb_ccdd_eeff_0011);
    cpu.write_fpr(14, 0x0102_0304_0506_0708);
    cpu.write_fpr(15, nanbox_s(3.25f32.to_bits()));
    cpu.write_fpr(18, nanbox_s((-7.5f32).to_bits()));

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                frr(OP_FLDX_S, 20, 1, 2),
                frr(OP_FLDX_D, 21, 3, 4),
                frr(OP_FLDGT_D, 22, 10, 11),
                frr(OP_FLDLE_S, 23, 19, 20),
                frr(OP_FSTX_S, 12, 5, 6),
                frr(OP_FSTX_D, 13, 7, 8),
                frr(OP_FSTGT_D, 14, 10, 11),
                frr(OP_FSTLE_S, 15, 19, 20),
                r2_si12(OP_FLD_S, 0, 17, 24),
                r2_si12(OP_FST_S, 4, 17, 18),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(20), nanbox_s(1.5f32.to_bits()));
    assert_eq!(cpu.read_fpr(21), 0x1122_3344_5566_7788);
    assert_eq!(cpu.read_fpr(22), 0x8877_6655_4433_2211);
    assert_eq!(cpu.read_fpr(23), nanbox_s(1.5f32.to_bits()));
    assert_eq!(cpu.read_fpr(24), nanbox_s(7.5f32.to_bits()));
    assert_eq!(
        u32::from_le_bytes(data[24..28].try_into().unwrap()),
        (-2.5f32).to_bits()
    );
    assert_eq!(
        u64::from_le_bytes(data[32..40].try_into().unwrap()),
        0x0102_0304_0506_0708
    );
    assert_eq!(
        u32::from_le_bytes(data[12..16].try_into().unwrap()),
        3.25f32.to_bits()
    );
    assert_eq!(
        u32::from_le_bytes(data[48..52].try_into().unwrap()),
        (-7.5f32).to_bits()
    );
}

#[test]
fn task34_extended_scalar_fpu_ops_cover_rounding_class_and_nanboxing() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.write_fcsr(fcsr_rm(0));

    cpu.write_fpr(1, 2.5f64.to_bits());
    cpu.write_fpr(2, (-3.0f64).to_bits());
    cpu.write_fpr(3, nanbox_s(1.25f32.to_bits()));
    cpu.write_fpr(4, nanbox_s(2.5f32.to_bits()));
    cpu.write_fpr(5, nanbox_s((-5.0f32).to_bits()));
    cpu.write_fpr(6, nanbox_s(4.0f32.to_bits()));
    cpu.write_fpr(7, 3.5f64.to_bits());
    cpu.write_fpr(8, (-0.0f64).to_bits());
    cpu.write_fpr(9, nanbox_s(3.5f32.to_bits()));
    cpu.write_fpr(10, nanbox_s((-0.0f32).to_bits()));
    cpu.write_fpr(11, 1.5f64.to_bits());
    cpu.write_fpr(12, 3);
    cpu.write_fpr(13, nanbox_s(1.5f32.to_bits()));
    cpu.write_fpr(14, 2);
    cpu.write_fpr(15, 4.0f64.to_bits());
    cpu.write_fpr(16, nanbox_s(4.0f32.to_bits()));
    cpu.write_fpr(17, 2.5f64.to_bits());
    cpu.write_fpr(18, nanbox_s(2.5f32.to_bits()));
    cpu.write_fpr(19, 8.0f64.to_bits());
    cpu.write_fpr(20, nanbox_s(0.125f32.to_bits()));
    cpu.write_fpr(21, f64::INFINITY.to_bits());
    cpu.write_fpr(22, nanbox_s(SNAN_S));

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr3(OP_FMAX_D, 24, 1, 2),
                fr3(OP_FMIN_D, 25, 1, 2),
                fr3(OP_FMAXA_D, 26, 1, 2),
                fr3(OP_FMINA_D, 27, 1, 2),
                fr3(OP_FMAX_S, 28, 3, 4),
                fr3(OP_FMINA_S, 29, 5, 6),
                fr3(OP_FMIN_S, 23, 3, 4),
                fr3(OP_FMAXA_S, 0, 5, 6),
                fr3(OP_FCOPYSIGN_D, 30, 7, 8),
                fr3(OP_FCOPYSIGN_S, 31, 9, 10),
                fr3(OP_FSCALEB_D, 1, 11, 12),
                fr3(OP_FSCALEB_S, 2, 13, 14),
                fr2(OP_FRECIP_D, 3, 15),
                fr2(OP_FRSQRT_D, 4, 15),
                fr2(OP_FRECIPE_S, 5, 16),
                fr2(OP_FRSQRTE_S, 6, 16),
                fr2(OP_FRECIP_S, 13, 16),
                fr2(OP_FRSQRT_S, 14, 16),
                fr2(OP_FRINT_D, 7, 17),
                fr2(OP_FRINT_S, 8, 18),
                fr2(OP_FLOGB_D, 9, 19),
                fr2(OP_FLOGB_S, 10, 20),
                fr2(OP_FCLASS_D, 11, 21),
                fr2(OP_FCLASS_S, 12, 22),
                fr2(OP_FRECIPE_D, 18, 15),
                fr2(OP_FRSQRTE_D, 19, 15),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(24), 2.5f64.to_bits());
    assert_eq!(cpu.read_fpr(25), (-3.0f64).to_bits());
    assert_eq!(cpu.read_fpr(26), (-3.0f64).to_bits());
    assert_eq!(cpu.read_fpr(27), 2.5f64.to_bits());
    assert_eq!(cpu.read_fpr(28), nanbox_s(2.5f32.to_bits()));
    assert_eq!(cpu.read_fpr(29), nanbox_s(4.0f32.to_bits()));
    assert_eq!(cpu.read_fpr(23), nanbox_s(1.25f32.to_bits()));
    assert_eq!(cpu.read_fpr(0), nanbox_s((-5.0f32).to_bits()));
    assert_eq!(cpu.read_fpr(30), (-3.5f64).to_bits());
    assert_eq!(cpu.read_fpr(31), nanbox_s((-3.5f32).to_bits()));
    assert_eq!(cpu.read_fpr(1), 12.0f64.to_bits());
    assert_eq!(cpu.read_fpr(2), nanbox_s(6.0f32.to_bits()));
    assert_eq!(cpu.read_fpr(3), 0.25f64.to_bits());
    assert_eq!(cpu.read_fpr(4), 0.5f64.to_bits());
    assert_eq!(cpu.read_fpr(5), nanbox_s(0.25f32.to_bits()));
    assert_eq!(cpu.read_fpr(6), nanbox_s(0.5f32.to_bits()));
    assert_eq!(cpu.read_fpr(13), nanbox_s(0.25f32.to_bits()));
    assert_eq!(cpu.read_fpr(14), nanbox_s(0.5f32.to_bits()));
    assert_eq!(cpu.read_fpr(18), 0.25f64.to_bits());
    assert_eq!(cpu.read_fpr(19), 0.5f64.to_bits());
    assert_eq!(cpu.read_fpr(7), 2.0f64.to_bits());
    assert_eq!(cpu.read_fpr(8), nanbox_s(2.0f32.to_bits()));
    assert_eq!(cpu.read_fpr(9), 3.0f64.to_bits());
    assert_eq!(cpu.read_fpr(10), nanbox_s((-3.0f32).to_bits()));
    assert_eq!(cpu.read_fpr(11), 1 << 6);
    assert_eq!(cpu.read_fpr(12), 1 << 0);
}

#[test]
fn task34_predicate_fp_memory_failures_raise_bce_without_side_effects() {
    assert_predicate_fp_bce_trap(frr(OP_FLDGT_D, 3, 1, 2), 4, 8, |cpu, _| {
        assert_eq!(cpu.read_fpr(3), FPR_SENTINEL);
    });

    assert_predicate_fp_bce_trap(frr(OP_FSTGT_D, 4, 1, 2), 4, 8, |_, data| {
        assert_eq!(
            u64::from_le_bytes(data[12..20].try_into().unwrap()),
            0x1122_3344_5566_7788
        );
    });

    assert_predicate_fp_bce_trap(frr(OP_FLDLE_S, 3, 1, 2), 16, 4, |cpu, _| {
        assert_eq!(cpu.read_fpr(3), FPR_SENTINEL);
    });

    assert_predicate_fp_bce_trap(frr(OP_FSTLE_S, 5, 1, 2), 16, 4, |_, data| {
        assert_eq!(u32::from_le_bytes(data[20..24].try_into().unwrap()), 0);
    });
}

#[test]
fn task34_zero_vector_predicate_fp_traps_stop_before_memory_side_effects() {
    assert_zero_vector_predicate_fp_bce_trap(
        frr(OP_FLDGT_D, 3, 1, 2),
        4,
        8,
        |cpu, _| {
            assert_eq!(cpu.read_fpr(3), FPR_SENTINEL);
        },
    );

    assert_zero_vector_predicate_fp_bce_trap(
        frr(OP_FSTGT_D, 4, 1, 2),
        4,
        8,
        |_, data| {
            assert_eq!(
                u64::from_le_bytes(data[12..20].try_into().unwrap()),
                0x1122_3344_5566_7788
            );
        },
    );
}

#[test]
fn task34_flogb_reports_edge_fcsr_state_and_enabled_traps() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, 0.0f64.to_bits());
    assert_eq!(run_la(&mut cpu, &[fr2(OP_FLOGB_D, 3, 1)]), 0);
    assert_eq!(cpu.read_fpr(3), f64::NEG_INFINITY.to_bits());
    assert_fcsr_cause_flags(&cpu, FP_Z, FP_Z);

    cpu.write_fcsr(0);
    cpu.write_fpr(1, nanbox_s((-0.0f32).to_bits()));
    assert_eq!(run_la(&mut cpu, &[fr2(OP_FLOGB_S, 3, 1)]), 0);
    assert_eq!(cpu.read_fpr(3), nanbox_s(f32::NEG_INFINITY.to_bits()));
    assert_fcsr_cause_flags(&cpu, FP_Z, FP_Z);

    cpu.write_fcsr(0);
    cpu.write_fpr(1, (-2.0f64).to_bits());
    assert_eq!(run_la(&mut cpu, &[fr2(OP_FLOGB_D, 3, 1)]), 0);
    assert!(f64::from_bits(cpu.read_fpr(3)).is_nan());
    assert_fcsr_cause_flags(&cpu, FP_V, FP_V);

    cpu.write_fcsr(0);
    cpu.write_fpr(1, nanbox_s(f32::NEG_INFINITY.to_bits()));
    assert_eq!(run_la(&mut cpu, &[fr2(OP_FLOGB_S, 3, 1)]), 0);
    assert!(f32::from_bits(cpu.read_fpr(3) as u32).is_nan());
    assert_eq!(cpu.read_fpr(3) >> 32, 0xffff_ffff);
    assert_fcsr_cause_flags(&cpu, FP_V, FP_V);

    cpu.write_fcsr(0);
    cpu.write_fpr(1, QNAN_D);
    assert_eq!(run_la(&mut cpu, &[fr2(OP_FLOGB_D, 3, 1)]), 0);
    assert!(f64::from_bits(cpu.read_fpr(3)).is_nan());
    assert_eq!(fcsr_bits(&cpu) & (fcsr_cause(0x1f) | fcsr_flags(0x1f)), 0);

    cpu.write_fcsr(0);
    cpu.write_fpr(1, nanbox_s(SNAN_S));
    assert_eq!(run_la(&mut cpu, &[fr2(OP_FLOGB_S, 3, 1)]), 0);
    assert!(f32::from_bits(cpu.read_fpr(3) as u32).is_nan());
    assert_eq!(cpu.read_fpr(3) >> 32, 0xffff_ffff);
    assert_fcsr_cause_flags(&cpu, FP_V, FP_V);

    assert_enabled_fpe_trap(fr2(OP_FLOGB_D, 3, 1), FP_Z, FP_Z, |cpu| {
        cpu.write_fpr(1, 0.0f64.to_bits());
    });
    assert_enabled_fpe_trap(fr2(OP_FLOGB_S, 3, 1), FP_V, FP_V, |cpu| {
        cpu.write_fpr(1, nanbox_s((-2.0f32).to_bits()));
    });
}

#[test]
fn task34_zero_vector_enabled_flogb_trap_stops_same_tb_execution() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.csr_write(CSR_CRMD, CRMD_DA);
    cpu.write_fcsr(fcsr_enable(FP_Z));
    cpu.write_fpr(1, 0.0f64.to_bits());
    cpu.write_fpr(3, FPR_SENTINEL);
    cpu.write_gpr(4, GPR_SENTINEL);

    assert_ne!(
        run_la(
            &mut cpu,
            &[fr2(OP_FLOGB_D, 3, 1), r2_si12(OP_ADDI_D, 7, 0, 4)]
        ),
        0
    );
    assert_eq!(cpu.pc(), 0);
    assert_eq!(cpu.era(), 0);
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 16 & 0x3f, u64::from(ECODE_FPE));
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 22 & 0x1ff, 0);
    assert_eq!(cpu.read_fpr(3), FPR_SENTINEL);
    assert_eq!(cpu.read_gpr(4), GPR_SENTINEL);
    assert_eq!(fcsr_bits(&cpu) & fcsr_cause(FP_Z), fcsr_cause(FP_Z));
    assert_eq!(fcsr_bits(&cpu) & fcsr_flags(FP_Z), 0);
}

#[test]
fn task34_zero_vector_disabled_fpe_trap_stops_same_tb_execution() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, 0);
    cpu.csr_write(CSR_CRMD, CRMD_DA);
    cpu.write_fcsr(FCSR_SENTINEL);
    cpu.write_fpr(1, 1.0f64.to_bits());
    cpu.write_fpr(2, 2.0f64.to_bits());
    cpu.write_fpr(3, FPR_SENTINEL);
    cpu.write_gpr(4, GPR_SENTINEL);

    assert_ne!(
        run_la(
            &mut cpu,
            &[fr3(OP_FADD_D, 3, 1, 2), r2_si12(OP_ADDI_D, 7, 0, 4)]
        ),
        0
    );
    assert_eq!(cpu.pc(), 0);
    assert_eq!(cpu.era(), 0);
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 16 & 0x3f, u64::from(ECODE_FPD));
    assert_eq!(cpu.csr_read(CSR_ESTAT) >> 22 & 0x1ff, 0);
    assert_eq!(cpu.read_fpr(3), FPR_SENTINEL);
    assert_eq!(cpu.read_gpr(4), GPR_SENTINEL);
    assert_eq!(fcsr_bits(&cpu), FCSR_SENTINEL);
}

#[test]
fn task34_fneg_s_is_pure_sign_op_and_preserves_fcsr_cause() {
    let mut cpu = LoongArchCpu::new();
    let fcsr = fcsr_rm(2) | fcsr_flags(FP_I | FP_U) | fcsr_cause(FP_Z | FP_V);

    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.write_fcsr(fcsr);
    cpu.write_fpr(1, nanbox_s(1.25f32.to_bits()));

    assert_eq!(run_la(&mut cpu, &[fr2(OP_FNEG_S, 3, 1)]), 0);

    assert_eq!(cpu.read_fpr(3), nanbox_s((-1.25f32).to_bits()));
    assert_eq!(fcsr_bits(&cpu), fcsr);
}

#[test]
fn task31_translated_fmadd_d_uses_fused_precision() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    let fj = f64::from_bits(0x3ff0_0000_0000_0001);
    let fk = f64::from_bits(0x3fef_ffff_ffff_fffe);
    let fa = -1.0f64;
    let fused = fj.mul_add(fk, fa);
    let separate = fj * fk + fa;
    assert_ne!(fused.to_bits(), separate.to_bits());

    cpu.write_fpr(1, fj.to_bits());
    cpu.write_fpr(2, fk.to_bits());
    cpu.write_fpr(3, fa.to_bits());

    assert_eq!(run_la(&mut cpu, &[fr4(OP_FMADD_D, 16, 1, 2, 3)]), 0);
    assert_eq!(cpu.read_fpr(16), fused.to_bits());
}

#[test]
fn task31_translated_fused_ops_propagate_nan_operands() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, nanbox_s(f32::NAN.to_bits()));
    cpu.write_fpr(2, nanbox_s(2.0f32.to_bits()));
    cpu.write_fpr(3, nanbox_s(3.0f32.to_bits()));
    cpu.write_fpr(4, f64::NAN.to_bits());
    cpu.write_fpr(5, 2.0f64.to_bits());
    cpu.write_fpr(6, 3.0f64.to_bits());

    assert_eq!(
        run_la(
            &mut cpu,
            &[fr4(OP_FMADD_S, 16, 1, 2, 3), fr4(OP_FNMSUB_D, 17, 4, 5, 6),],
        ),
        0
    );

    let single = cpu.read_fpr(16);
    assert_eq!(single >> 32, 0xffff_ffff);
    assert!(f32::from_bits(single as u32).is_nan());
    assert!(f64::from_bits(cpu.read_fpr(17)).is_nan());
}

#[test]
fn task30_translated_fcmp_writes_all_fcc_lanes_independently() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    for idx in 0..8 {
        cpu.write_fcc(idx, 1);
    }

    cpu.write_fpr(1, nanbox_s(1.0f32.to_bits()));
    cpu.write_fpr(2, nanbox_s(2.0f32.to_bits()));
    cpu.write_fpr(3, nanbox_s(f32::NAN.to_bits()));
    cpu.write_fpr(4, 1.0f64.to_bits());
    cpu.write_fpr(5, 2.0f64.to_bits());
    cpu.write_fpr(6, f64::NAN.to_bits());

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fcmp(OP_FCMP_S, FCMP_CAF, 0, 1, 2),
                fcmp(OP_FCMP_S, FCMP_CEQ, 1, 1, 1),
                fcmp(OP_FCMP_D, FCMP_CLT, 2, 4, 5),
                fcmp(OP_FCMP_D, FCMP_CLE, 3, 5, 4),
                fcmp(OP_FCMP_S, FCMP_CUN, 4, 3, 1),
                fcmp(OP_FCMP_D, FCMP_CUEQ, 5, 6, 4),
                fcmp(OP_FCMP_S, FCMP_CNE, 6, 1, 2),
                fcmp(OP_FCMP_D, FCMP_COR, 7, 6, 4),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fcc(0), 0);
    assert_eq!(cpu.read_fcc(1), 1);
    assert_eq!(cpu.read_fcc(2), 1);
    assert_eq!(cpu.read_fcc(3), 0);
    assert_eq!(cpu.read_fcc(4), 1);
    assert_eq!(cpu.read_fcc(5), 1);
    assert_eq!(cpu.read_fcc(6), 1);
    assert_eq!(cpu.read_fcc(7), 0);
}

#[test]
fn task30_translated_fsel_uses_selected_fcc_lane_only() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    for idx in 0..8 {
        cpu.write_fcc(idx, (idx % 2) as u8);
    }
    cpu.write_fcc(2, 0);
    cpu.write_fcc(5, 1);
    cpu.write_fpr(1, 0x1111_1111_1111_1111);
    cpu.write_fpr(2, 0x2222_2222_2222_2222);

    assert_eq!(run_la(&mut cpu, &[fsel(10, 1, 2, 2), fsel(11, 1, 2, 5)]), 0);

    assert_eq!(cpu.read_fpr(10), 0x1111_1111_1111_1111);
    assert_eq!(cpu.read_fpr(11), 0x2222_2222_2222_2222);
    for idx in 0..8 {
        let expected = if idx == 2 { 0 } else { (idx % 2) as u8 };
        assert_eq!(cpu.read_fcc(idx), expected);
    }
}

#[test]
fn task30_fbranches_use_qemu_taken_and_fallthrough_slots() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.write_fcc(0, 0);
    let exit = run_la(&mut cpu, &[fbranch(0, 2, 0)]);
    assert_eq!(exit, 0);
    assert_eq!(cpu.pc(), 8);

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.write_fcc(0, 1);
    let exit = run_la(&mut cpu, &[fbranch(0, 2, 0)]);
    assert_eq!(exit, 1);
    assert_eq!(cpu.pc(), 4);

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.write_fcc(1, 1);
    let exit = run_la(&mut cpu, &[fbranch(1, 2, 1)]);
    assert_eq!(exit, 0);
    assert_eq!(cpu.pc(), 8);

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.write_fcc(1, 0);
    let exit = run_la(&mut cpu, &[fbranch(1, 2, 1)]);
    assert_eq!(exit, 1);
    assert_eq!(cpu.pc(), 4);
}

#[test]
fn task30_exec_loop_fbranches_patch_taken_and_fallthrough_paths() {
    let program = [
        fbranch(0, 4, 3),             // pc 0: BCEQZ fcc3 -> pc 16
        r2_si12(OP_ADDI_D, 11, 0, 5), // pc 4: fall-through result
        offs26(OP_B, 4),              // pc 8: skip taken path to idle
        r2_si12(OP_ADDI_D, 99, 0, 5), // pc 12: unreachable
        r2_si12(OP_ADDI_D, 22, 0, 5), // pc 16: taken result
        offs26(OP_B, 1),              // pc 20: taken path to idle
        code15(OP_IDLE, 0),           // pc 24: halt through exec loop
    ];
    let mut cpu = FpuExecLoopCpu::new(&program);
    let mut env = ExecEnv::new(X86_64CodeGen::new());

    cpu.reset_for_fbranch(3, 1);
    run_fpu_exec_loop(&mut env, &mut cpu);
    assert_eq!(cpu.cpu.read_gpr(5), 11);

    cpu.reset_for_fbranch(3, 0);
    run_fpu_exec_loop(&mut env, &mut cpu);
    assert_eq!(cpu.cpu.read_gpr(5), 22);

    let program = [
        fbranch(1, 4, 4),             // pc 0: BCNEZ fcc4 -> pc 16
        r2_si12(OP_ADDI_D, 33, 0, 5), // pc 4: fall-through result
        offs26(OP_B, 4),              // pc 8: skip taken path to idle
        r2_si12(OP_ADDI_D, 99, 0, 5), // pc 12: unreachable
        r2_si12(OP_ADDI_D, 44, 0, 5), // pc 16: taken result
        offs26(OP_B, 1),              // pc 20: taken path to idle
        code15(OP_IDLE, 0),           // pc 24: halt through exec loop
    ];
    let mut cpu = FpuExecLoopCpu::new(&program);
    let mut env = ExecEnv::new(X86_64CodeGen::new());

    cpu.reset_for_fbranch(4, 1);
    run_fpu_exec_loop(&mut env, &mut cpu);
    assert_eq!(cpu.cpu.read_gpr(5), 44);

    cpu.reset_for_fbranch(4, 0);
    run_fpu_exec_loop(&mut env, &mut cpu);
    assert_eq!(cpu.cpu.read_gpr(5), 33);
}

#[test]
fn fadd_s_basic() {
    let a = f32::to_bits(1.5);
    let b = f32::to_bits(2.5);
    let r = helpers::loongarch_helper_fadd_s(u64::from(a), u64::from(b));
    assert_eq!(f32::from_bits(r as u32), 4.0);
}

#[test]
fn fadd_d_basic() {
    let a = f64::to_bits(1.5);
    let b = f64::to_bits(2.5);
    let r = helpers::loongarch_helper_fadd_d(a, b);
    assert_eq!(f64::from_bits(r), 4.0);
}

#[test]
fn fmul_s_basic() {
    let a = f32::to_bits(3.0);
    let b = f32::to_bits(4.0);
    let r = helpers::loongarch_helper_fmul_s(u64::from(a), u64::from(b));
    assert_eq!(f32::from_bits(r as u32), 12.0);
}

#[test]
fn fdiv_d_basic() {
    let a = f64::to_bits(10.0);
    let b = f64::to_bits(2.0);
    let r = helpers::loongarch_helper_fdiv_d(a, b);
    assert_eq!(f64::from_bits(r), 5.0);
}

#[test]
fn fsqrt_d_basic() {
    let a = f64::to_bits(9.0);
    let r = helpers::loongarch_helper_fsqrt_d(a);
    assert_eq!(f64::from_bits(r), 3.0);
}

#[test]
fn task28_translated_single_arithmetic_nanboxes_results() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, 0xaaaa_aaaa_0000_0000 | u64::from(1.5f32.to_bits()));
    cpu.write_fpr(2, 0xbbbb_bbbb_0000_0000 | u64::from(2.25f32.to_bits()));
    cpu.write_fpr(3, u64::from(7.0f32.to_bits()));
    cpu.write_fpr(4, u64::from(2.5f32.to_bits()));
    cpu.write_fpr(5, u64::from((-3.0f32).to_bits()));
    cpu.write_fpr(6, u64::from(2.0f32.to_bits()));
    cpu.write_fpr(7, u64::from(9.0f32.to_bits()));
    cpu.write_fpr(8, u64::from(4.0f32.to_bits()));
    cpu.write_fpr(9, u64::from(16.0f32.to_bits()));

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr3(OP_FADD_S, 10, 1, 2),
                fr3(OP_FSUB_S, 11, 3, 4),
                fr3(OP_FMUL_S, 12, 5, 6),
                fr3(OP_FDIV_S, 13, 7, 8),
                fr2(OP_FSQRT_S, 14, 9),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(10), nanbox_s(3.75f32.to_bits()));
    assert_eq!(cpu.read_fpr(11), nanbox_s(4.5f32.to_bits()));
    assert_eq!(cpu.read_fpr(12), nanbox_s((-6.0f32).to_bits()));
    assert_eq!(cpu.read_fpr(13), nanbox_s(2.25f32.to_bits()));
    assert_eq!(cpu.read_fpr(14), nanbox_s(4.0f32.to_bits()));
}

#[test]
fn task28_translated_double_arithmetic_results() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, 1.5f64.to_bits());
    cpu.write_fpr(2, 2.25f64.to_bits());
    cpu.write_fpr(3, 7.0f64.to_bits());
    cpu.write_fpr(4, 2.5f64.to_bits());
    cpu.write_fpr(5, (-3.0f64).to_bits());
    cpu.write_fpr(6, 2.0f64.to_bits());
    cpu.write_fpr(7, 9.0f64.to_bits());
    cpu.write_fpr(8, 4.0f64.to_bits());
    cpu.write_fpr(9, 16.0f64.to_bits());

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr3(OP_FADD_D, 10, 1, 2),
                fr3(OP_FSUB_D, 11, 3, 4),
                fr3(OP_FMUL_D, 12, 5, 6),
                fr3(OP_FDIV_D, 13, 7, 8),
                fr2(OP_FSQRT_D, 14, 9),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(10), 3.75f64.to_bits());
    assert_eq!(cpu.read_fpr(11), 4.5f64.to_bits());
    assert_eq!(cpu.read_fpr(12), (-6.0f64).to_bits());
    assert_eq!(cpu.read_fpr(13), 2.25f64.to_bits());
    assert_eq!(cpu.read_fpr(14), 4.0f64.to_bits());
}

#[test]
fn task29_translated_float_and_integer_to_float_conversions() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, 2.5f64.to_bits());
    cpu.write_fpr(2, nanbox_s((-1.25f32).to_bits()));
    cpu.write_fpr(3, 0xaaaa_aaaa_0000_0000 | i32_result(-42));
    cpu.write_fpr(4, 0xbbbb_bbbb_0000_0000 | i32_result(1234));
    cpu.write_fpr(5, i64_result(-1_234_567_890_123));
    cpu.write_fpr(6, i64_result(9_876_543_210));

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr2(OP_FCVT_S_D, 10, 1),
                fr2(OP_FCVT_D_S, 11, 2),
                fr2(OP_FFINT_S_W, 12, 3),
                fr2(OP_FFINT_D_W, 13, 4),
                fr2(OP_FFINT_S_L, 14, 5),
                fr2(OP_FFINT_D_L, 15, 6),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(10), nanbox_s(2.5f32.to_bits()));
    assert_eq!(cpu.read_fpr(11), (-1.25f64).to_bits());
    assert_eq!(cpu.read_fpr(12), nanbox_s((-42.0f32).to_bits()));
    assert_eq!(cpu.read_fpr(13), 1234.0f64.to_bits());
    assert_eq!(
        cpu.read_fpr(14),
        nanbox_s((-1_234_567_890_123i64 as f32).to_bits())
    );
    assert_eq!(cpu.read_fpr(15), (9_876_543_210i64 as f64).to_bits());
}

#[test]
fn task29_translated_fixed_round_float_to_integer_conversions() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, nanbox_s((-1.25f32).to_bits()));
    cpu.write_fpr(2, (-1.25f64).to_bits());
    cpu.write_fpr(3, nanbox_s((-9.75f32).to_bits()));
    cpu.write_fpr(4, (-9.75f64).to_bits());
    cpu.write_fpr(5, nanbox_s(1.25f32.to_bits()));
    cpu.write_fpr(6, 1.25f64.to_bits());
    cpu.write_fpr(7, nanbox_s(9.25f32.to_bits()));
    cpu.write_fpr(8, 9.25f64.to_bits());
    cpu.write_fpr(9, nanbox_s((-1.75f32).to_bits()));
    cpu.write_fpr(10, (-1.75f64).to_bits());
    cpu.write_fpr(11, nanbox_s((-9.75f32).to_bits()));
    cpu.write_fpr(12, 9.75f64.to_bits());
    cpu.write_fpr(13, nanbox_s(3.5f32.to_bits()));
    cpu.write_fpr(14, 2.5f64.to_bits());
    cpu.write_fpr(15, nanbox_s((-2.5f32).to_bits()));
    cpu.write_fpr(16, (-3.5f64).to_bits());

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr2(OP_FTINTRNE_L_D, 16, 16),
                fr2(OP_FTINTRM_W_S, 17, 1),
                fr2(OP_FTINTRM_W_D, 18, 2),
                fr2(OP_FTINTRM_L_S, 19, 3),
                fr2(OP_FTINTRM_L_D, 20, 4),
                fr2(OP_FTINTRP_W_S, 21, 5),
                fr2(OP_FTINTRP_W_D, 22, 6),
                fr2(OP_FTINTRP_L_S, 23, 7),
                fr2(OP_FTINTRP_L_D, 24, 8),
                fr2(OP_FTINTRZ_W_S, 25, 9),
                fr2(OP_FTINTRZ_W_D, 26, 10),
                fr2(OP_FTINTRZ_L_S, 27, 11),
                fr2(OP_FTINTRZ_L_D, 28, 12),
                fr2(OP_FTINTRNE_W_S, 29, 13),
                fr2(OP_FTINTRNE_W_D, 30, 14),
                fr2(OP_FTINTRNE_L_S, 31, 15),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(16), i64_result(-4));
    assert_eq!(cpu.read_fpr(17), i32_result(-2));
    assert_eq!(cpu.read_fpr(18), i32_result(-2));
    assert_eq!(cpu.read_fpr(19), i64_result(-10));
    assert_eq!(cpu.read_fpr(20), i64_result(-10));
    assert_eq!(cpu.read_fpr(21), i32_result(2));
    assert_eq!(cpu.read_fpr(22), i32_result(2));
    assert_eq!(cpu.read_fpr(23), i64_result(10));
    assert_eq!(cpu.read_fpr(24), i64_result(10));
    assert_eq!(cpu.read_fpr(25), i32_result(-1));
    assert_eq!(cpu.read_fpr(26), i32_result(-1));
    assert_eq!(cpu.read_fpr(27), i64_result(-9));
    assert_eq!(cpu.read_fpr(28), i64_result(9));
    assert_eq!(cpu.read_fpr(29), i32_result(4));
    assert_eq!(cpu.read_fpr(30), i32_result(2));
    assert_eq!(cpu.read_fpr(31), i64_result(-2));
}

#[test]
fn task29_translated_ftint_uses_default_nearest_even_rounding() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, nanbox_s(2.5f32.to_bits()));
    cpu.write_fpr(2, 3.5f64.to_bits());
    cpu.write_fpr(3, nanbox_s((-2.5f32).to_bits()));
    cpu.write_fpr(4, (-3.5f64).to_bits());

    assert_eq!(
        run_la(
            &mut cpu,
            &[
                fr2(OP_FTINT_W_S, 10, 1),
                fr2(OP_FTINT_W_D, 11, 2),
                fr2(OP_FTINT_L_S, 12, 3),
                fr2(OP_FTINT_L_D, 13, 4),
            ],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(10), i32_result(2));
    assert_eq!(cpu.read_fpr(11), i32_result(4));
    assert_eq!(cpu.read_fpr(12), i64_result(-2));
    assert_eq!(cpu.read_fpr(13), i64_result(-4));
}

#[test]
fn task29_translated_float_to_integer_nan_returns_zero() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    cpu.write_fpr(1, nanbox_s(f32::NAN.to_bits()));
    cpu.write_fpr(2, f64::NAN.to_bits());

    assert_eq!(
        run_la(
            &mut cpu,
            &[fr2(OP_FTINTRP_W_S, 10, 1), fr2(OP_FTINTRNE_L_D, 11, 2),],
        ),
        0
    );

    assert_eq!(cpu.read_fpr(10), 0);
    assert_eq!(cpu.read_fpr(11), 0);
}

#[test]
fn fmadd_s_fused() {
    let a = f32::to_bits(2.0);
    let b = f32::to_bits(3.0);
    let c = f32::to_bits(1.0);
    let r = helpers::loongarch_helper_fmadd_s(
        u64::from(a),
        u64::from(b),
        u64::from(c),
    );
    assert_eq!(f32::from_bits(r as u32), 7.0); // 2*3+1
}

#[test]
fn fmsub_d_fused() {
    let a = f64::to_bits(5.0);
    let b = f64::to_bits(4.0);
    let c = f64::to_bits(3.0);
    let r = helpers::loongarch_helper_fmsub_d(a, b, c);
    assert_eq!(f64::from_bits(r), 17.0); // 5*4-3
}

#[test]
fn fcmp_ceq_s_equal() {
    let a = f32::to_bits(1.0);
    let r = helpers::loongarch_helper_fcmp_ceq_s(u64::from(a), u64::from(a));
    assert_eq!(r, 1);
}

#[test]
fn fcmp_clt_d_less() {
    let a = f64::to_bits(1.0);
    let b = f64::to_bits(2.0);
    assert_eq!(helpers::loongarch_helper_fcmp_clt_d(a, b), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_clt_d(b, a), 0);
}

#[test]
fn fcmp_cun_s_nan() {
    let nan = f32::to_bits(f32::NAN);
    let one = f32::to_bits(1.0);
    assert_eq!(
        helpers::loongarch_helper_fcmp_cun_s(u64::from(nan), u64::from(one)),
        1,
    );
    assert_eq!(
        helpers::loongarch_helper_fcmp_cun_s(u64::from(one), u64::from(one)),
        0,
    );
}

#[test]
fn ffint_d_w_positive() {
    let r = helpers::loongarch_helper_ffint_d_w(42);
    assert_eq!(f64::from_bits(r), 42.0);
}

#[test]
fn ftintrz_w_s_truncates() {
    let a = f32::to_bits(3.7);
    let r = helpers::loongarch_helper_ftintrz_w_s(u64::from(a));
    assert_eq!(r as i32, 3);
}

#[test]
fn fcvt_s_d_converts() {
    let a = f64::to_bits(2.5);
    let r = helpers::loongarch_helper_fcvt_s_d(a);
    assert_eq!(f32::from_bits(r as u32), 2.5);
}

#[test]
fn check_fpe_disabled_raises_fpd() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
    use machina_guest_loongarch::loongarch::csr::*;

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, 0); // FPE disabled
    cpu.set_pc(0x1000);
    cpu.csr_write(CSR_EENTRY, 0x9000_0000);

    let status = unsafe { helpers::loongarch_helper_check_fpe(cpu.env_ptr()) };

    assert_eq!(status, 1);
    assert_eq!(cpu.pc(), 0x9000_0000);
    assert_eq!((cpu.csr_read(CSR_ESTAT) >> 16) & 0x3F, 0x0F);
}

#[test]
fn check_fpe_enabled_returns_zero() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
    use machina_guest_loongarch::loongarch::csr::*;

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.set_pc(0x2000);

    let vec = unsafe { helpers::loongarch_helper_check_fpe(cpu.env_ptr()) };
    assert_eq!(vec, 0);
}

#[test]
fn fcmp_cueq_true_for_nan() {
    let nan = u64::from(f32::NAN.to_bits());
    let one = u64::from(1.0f32.to_bits());
    assert_eq!(helpers::loongarch_helper_fcmp_cueq_s(nan, one), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cueq_s(one, one), 1);
    assert_eq!(
        helpers::loongarch_helper_fcmp_cueq_s(one, u64::from(2.0f32.to_bits())),
        0
    );
}

#[test]
fn fcmp_cult_true_for_nan() {
    let nan = u64::from(f32::NAN.to_bits());
    let one = u64::from(1.0f32.to_bits());
    let two = u64::from(2.0f32.to_bits());
    assert_eq!(helpers::loongarch_helper_fcmp_cult_s(nan, one), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cult_s(one, two), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cult_s(two, one), 0);
}

#[test]
fn fcmp_cne_ordered_not_equal() {
    let one = u64::from(1.0f32.to_bits());
    let two = u64::from(2.0f32.to_bits());
    let nan = u64::from(f32::NAN.to_bits());
    assert_eq!(helpers::loongarch_helper_fcmp_cne_s(one, two), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cne_s(one, one), 0);
    assert_eq!(helpers::loongarch_helper_fcmp_cne_s(nan, one), 0);
}

#[test]
fn fcmp_cor_ordered() {
    let one = u64::from(1.0f32.to_bits());
    let nan = u64::from(f32::NAN.to_bits());
    assert_eq!(helpers::loongarch_helper_fcmp_cor_s(one, one), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cor_s(nan, one), 0);
}

#[test]
fn fcmp_cune_unordered_or_ne() {
    let one = u64::from(1.0f32.to_bits());
    let two = u64::from(2.0f32.to_bits());
    let nan = u64::from(f32::NAN.to_bits());
    assert_eq!(helpers::loongarch_helper_fcmp_cune_s(nan, one), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cune_s(one, two), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cune_s(one, one), 0);
}

#[test]
fn fcsr_read_write() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;

    let mut cpu = LoongArchCpu::new();
    assert_eq!(cpu.read_fcsr(), 0);
    cpu.write_fcsr(0x0300); // RM = RTZ (bits[9:8]=3)
    assert_eq!(cpu.read_fcsr() & 0x300, 0x300);
    cpu.write_fcsr(0xFFFF_FFFF);
    assert_eq!(cpu.read_fcsr(), 0x1F1F_031F);
}

#[test]
fn fcsr_helper_roundtrip() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
    use machina_guest_loongarch::loongarch::csr::*;

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    unsafe {
        helpers::loongarch_helper_movgr2fcsr(cpu.env_ptr(), 0x0200);
    }
    let val = unsafe { helpers::loongarch_helper_movfcsr2gr(cpu.env_ptr()) };
    assert_eq!(val, 0x0200);
}

#[test]
fn cpu_pending_interrupt_ie_disabled() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
    use machina_guest_loongarch::loongarch::csr::*;

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, 0); // IE=0
    cpu.csr_write(CSR_ECFG, 0x1FFF);
    cpu.set_estat_hw(1 << 11); // timer pending
    assert!(!cpu.pending_interrupt());
}

#[test]
fn cpu_pending_interrupt_ie_enabled_masked() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
    use machina_guest_loongarch::loongarch::csr::*;

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, CRMD_IE | CRMD_DA);
    cpu.csr_write(CSR_ECFG, 0); // all masked
    cpu.set_estat_hw(1 << 11);
    assert!(!cpu.pending_interrupt());
}

#[test]
fn cpu_pending_interrupt_fires() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
    use machina_guest_loongarch::loongarch::csr::*;

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, CRMD_IE | CRMD_DA);
    cpu.csr_write(CSR_ECFG, 1 << 11); // timer enabled
    cpu.set_estat_hw(1 << 11); // timer pending
    assert!(cpu.pending_interrupt());
}
