pub mod gen_common;

use machina_accel::ir::tb::{EXCP_UNDEF, TB_EXIT_IDX0};
use machina_accel::ir::{Context, TempIdx, Type};

use super::cpu::{gpr_offset, NUM_GPRS, PC_OFFSET};
use super::ext::LoongArchCfg;
use super::insn_decode;
use crate::{DisasContextBase, DisasJumpType, TranslatorOps};

pub struct LoongArchDisasContext {
    pub base: DisasContextBase,
    pub cfg: LoongArchCfg,
    pub env: TempIdx,
    pub gpr: [TempIdx; NUM_GPRS],
    pub pc: TempIdx,
    pub opcode: u32,
    pub guest_base: *const u8,
}

impl LoongArchDisasContext {
    #[must_use]
    pub fn new(pc: u64, guest_base: *const u8, cfg: LoongArchCfg) -> Self {
        Self {
            base: DisasContextBase {
                pc_first: pc,
                pc_next: pc,
                is_jmp: DisasJumpType::Next,
                num_insns: 0,
                max_insns: 512,
            },
            cfg,
            env: TempIdx(0),
            gpr: [TempIdx(0); NUM_GPRS],
            pc: TempIdx(0),
            opcode: 0,
            guest_base,
        }
    }
}

pub struct LoongArchTranslator;

impl TranslatorOps for LoongArchTranslator {
    type DisasContext = LoongArchDisasContext;

    fn init_disas_context(ctx: &mut Self::DisasContext, ir: &mut Context) {
        ctx.env = ir.new_fixed(Type::I64, 5, "env");
        for i in 0..NUM_GPRS {
            ctx.gpr[i] = ir.new_global(
                Type::I64,
                ctx.env,
                i64::try_from(gpr_offset(i)).unwrap(),
                "gpr",
            );
        }
        ctx.pc = ir.new_global(
            Type::I64,
            ctx.env,
            i64::try_from(PC_OFFSET).unwrap(),
            "pc",
        );
    }

    fn tb_start(_ctx: &mut Self::DisasContext, _ir: &mut Context) {}

    fn insn_start(ctx: &mut Self::DisasContext, ir: &mut Context) {
        ir.gen_insn_start(ctx.base.pc_next);
        ctx.base.num_insns += 1;
    }

    fn translate_insn(ctx: &mut Self::DisasContext, ir: &mut Context) {
        let pc = ctx.base.pc_next;
        // SAFETY: guest_base + pc must be a valid, readable 4-byte
        // host address. The caller (system crate or test harness)
        // ensures this by mapping guest memory before translation.
        let insn = unsafe {
            let ptr = ctx.guest_base.add(usize::try_from(pc).unwrap());
            ptr.cast::<u32>().read_unaligned()
        };
        ctx.opcode = insn;
        ctx.base.pc_next = pc + 4;

        if !decode_insn(ctx, ir, insn) {
            ctx.base.pc_next = pc;
            let c = ir.new_const(Type::I64, pc);
            ir.gen_mov(Type::I64, ctx.pc, c);
            ir.gen_exit_tb(EXCP_UNDEF);
            ctx.base.is_jmp = DisasJumpType::NoReturn;
        }
    }

    fn tb_stop(ctx: &mut Self::DisasContext, ir: &mut Context) {
        match ctx.base.is_jmp {
            DisasJumpType::Next | DisasJumpType::TooMany => {
                let c = ir.new_const(Type::I64, ctx.base.pc_next);
                ir.gen_mov(Type::I64, ctx.pc, c);
                ir.gen_goto_tb(0);
                ir.gen_exit_tb(TB_EXIT_IDX0);
            }
            DisasJumpType::NoReturn => {}
        }
    }

    fn base(ctx: &Self::DisasContext) -> &DisasContextBase {
        &ctx.base
    }

    fn base_mut(ctx: &mut Self::DisasContext) -> &mut DisasContextBase {
        &mut ctx.base
    }
}

impl insn_decode::Decode<Context> for LoongArchDisasContext {
    fn trans_addi_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let src = gpr_get(&self.gpr, ir, a.rj as u8);
        let imm = ir.new_const(Type::I64, a.si12 as u64);
        let dst = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, dst, src, imm);
        gpr_set(&self.gpr, ir, a.rd as u8, dst);
        true
    }
}

fn decode_insn(
    ctx: &mut LoongArchDisasContext,
    ir: &mut Context,
    insn: u32,
) -> bool {
    insn_decode::decode(ctx, ir, insn)
}
