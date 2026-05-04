pub mod gen_common;
pub mod gen_float;
pub mod helpers;

use machina_accel::ir::tb::{EXCP_ARCH_DONE, EXCP_UNDEF, TB_EXIT_IDX0};
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
    pub llbctl: TempIdx,
    pub ll_res_addr: TempIdx,
    pub ll_res_val: TempIdx,
    pub opcode: u32,
    pub guest_base: *const u8,
}

impl LoongArchDisasContext {
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
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
            llbctl: TempIdx(0),
            ll_res_addr: TempIdx(0),
            ll_res_val: TempIdx(0),
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
        ctx.llbctl = ir.new_global(
            Type::I64,
            ctx.env,
            i64::try_from(super::cpu::LLBCTL_OFFSET).unwrap(),
            "llbctl",
        );
        ctx.ll_res_addr = ir.new_global(
            Type::I64,
            ctx.env,
            i64::try_from(super::cpu::LL_RES_ADDR_OFFSET).unwrap(),
            "ll_res_addr",
        );
        ctx.ll_res_val = ir.new_global(
            Type::I64,
            ctx.env,
            i64::try_from(super::cpu::LL_RES_VAL_OFFSET).unwrap(),
            "ll_res_val",
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

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
impl insn_decode::Decode<Context> for LoongArchDisasContext {
    fn trans_add_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, d, s1, s2);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_sub_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_sub(Type::I64, d, s1, s2);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_and(&mut self, ir: &mut Context, a: &insn_decode::ArgsR3) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, d, s1, s2);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_or(&mut self, ir: &mut Context, a: &insn_decode::ArgsR3) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_or(Type::I64, d, s1, s2);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_xor(&mut self, ir: &mut Context, a: &insn_decode::ArgsR3) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_xor(Type::I64, d, s1, s2);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_nor(&mut self, ir: &mut Context, a: &insn_decode::ArgsR3) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let t = ir.new_temp(Type::I64);
        ir.gen_or(Type::I64, t, s1, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_not(Type::I64, d, t);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_slt(&mut self, ir: &mut Context, a: &insn_decode::ArgsR3) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::Cond;
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_setcond(Type::I64, d, s1, s2, Cond::Lt);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_sltu(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::Cond;
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_setcond(Type::I64, d, s1, s2, Cond::Ltu);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_sll_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, d, s1, s2);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_srl_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_shr(Type::I64, d, s1, s2);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_sra_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_sar(Type::I64, d, s1, s2);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_addi_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let src = gpr_get(&self.gpr, ir, a.rj as u8);
        let imm = ir.new_const(Type::I64, a.si12 as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, d, src, imm);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_andi(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Ui12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let src = gpr_get(&self.gpr, ir, a.rj as u8);
        let imm = ir.new_const(Type::I64, a.ui12 as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, d, src, imm);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_ori(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Ui12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let src = gpr_get(&self.gpr, ir, a.rj as u8);
        let imm = ir.new_const(Type::I64, a.ui12 as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_or(Type::I64, d, src, imm);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_xori(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Ui12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let src = gpr_get(&self.gpr, ir, a.rj as u8);
        let imm = ir.new_const(Type::I64, a.ui12 as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_xor(Type::I64, d, src, imm);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_lu12i_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR1Si20,
    ) -> bool {
        use gen_common::gpr_set;
        let val = (a.si20 << 12) as u64;
        let d = ir.new_const(Type::I64, val);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_lu32i_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR1Si20,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let src = gpr_get(&self.gpr, ir, a.rd as u8);
        let mask = ir.new_const(Type::I64, 0xFFFF_FFFF);
        let low = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, low, src, mask);
        let hi = ir.new_const(Type::I64, (a.si20 << 32) as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_or(Type::I64, d, low, hi);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_pcaddu12i(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR1Si20,
    ) -> bool {
        use gen_common::gpr_set;
        let pc_val = self.base.pc_next - 4;
        let result = pc_val.wrapping_add((a.si20 << 12) as u64);
        let d = ir.new_const(Type::I64, result);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_mul_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_mul(Type::I64, d, s1, s2);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_div_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_div_d as *const () as u64,
            &[s1, s2],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_div_du(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_div_du as *const () as u64,
            &[s1, s2],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_mod_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_mod_d as *const () as u64,
            &[s1, s2],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_mod_du(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_mod_du as *const () as u64,
            &[s1, s2],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_div_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_div_w as *const () as u64,
            &[s1, s2],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_div_wu(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_div_wu as *const () as u64,
            &[s1, s2],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_mod_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_mod_w as *const () as u64,
            &[s1, s2],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_mod_wu(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_mod_wu as *const () as u64,
            &[s1, s2],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_add_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let t = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, t, s1, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(d, t);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_sub_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let t = ir.new_temp(Type::I64);
        ir.gen_sub(Type::I64, t, s1, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(d, t);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_addi_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let src = gpr_get(&self.gpr, ir, a.rj as u8);
        let imm = ir.new_const(Type::I64, a.si12 as u64);
        let t = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, t, src, imm);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(d, t);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_slti(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::Cond;
        let src = gpr_get(&self.gpr, ir, a.rj as u8);
        let imm = ir.new_const(Type::I64, a.si12 as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_setcond(Type::I64, d, src, imm, Cond::Lt);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_sltui(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::Cond;
        let src = gpr_get(&self.gpr, ir, a.rj as u8);
        let imm = ir.new_const(Type::I64, a.si12 as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_setcond(Type::I64, d, src, imm, Cond::Ltu);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_orn(&mut self, ir: &mut Context, a: &insn_decode::ArgsR3) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let not_s2 = ir.new_temp(Type::I64);
        ir.gen_not(Type::I64, not_s2, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_or(Type::I64, d, s1, not_s2);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_andn(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let not_s2 = ir.new_temp(Type::I64);
        ir.gen_not(Type::I64, not_s2, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, d, s1, not_s2);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_lu52i_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let src = gpr_get(&self.gpr, ir, a.rj as u8);
        let mask = ir.new_const(Type::I64, 0x000F_FFFF_FFFF_FFFF);
        let low = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, low, src, mask);
        let hi = ir.new_const(Type::I64, (a.si12 << 52) as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_or(Type::I64, d, low, hi);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_pcaddi(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR1Si20,
    ) -> bool {
        use gen_common::gpr_set;
        let pc_val = self.base.pc_next - 4;
        let result = pc_val.wrapping_add((a.si20 << 2) as u64);
        let d = ir.new_const(Type::I64, result);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_ext_w_b(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let src = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(d, src);
        let mask_byte = ir.new_const(Type::I64, 0xFF);
        let byte_val = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, byte_val, src, mask_byte);
        let sext = ir.new_temp(Type::I64);
        let shift = ir.new_const(Type::I64, 56);
        ir.gen_shl(Type::I64, sext, byte_val, shift);
        let result = ir.new_temp(Type::I64);
        ir.gen_sar(Type::I64, result, sext, shift);
        gpr_set(&self.gpr, ir, a.rd as u8, result);
        true
    }

    fn trans_ext_w_h(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let src = gpr_get(&self.gpr, ir, a.rj as u8);
        let mask_hw = ir.new_const(Type::I64, 0xFFFF);
        let hw_val = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, hw_val, src, mask_hw);
        let shift = ir.new_const(Type::I64, 48);
        let sext = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, sext, hw_val, shift);
        let result = ir.new_temp(Type::I64);
        ir.gen_sar(Type::I64, result, sext, shift);
        gpr_set(&self.gpr, ir, a.rd as u8, result);
        true
    }

    fn trans_mul_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let t = ir.new_temp(Type::I64);
        ir.gen_mul(Type::I64, t, s1, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(d, t);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_sll_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let t = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I32, t, s1, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(d, t);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_srl_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let t = ir.new_temp(Type::I64);
        ir.gen_shr(Type::I32, t, s1, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(d, t);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_sra_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let t = ir.new_temp(Type::I64);
        ir.gen_sar(Type::I32, t, s1, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(d, t);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_rotr_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let t = ir.new_temp(Type::I64);
        ir.gen_rotr(Type::I32, t, s1, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(d, t);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_rotr_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_rotr(Type::I64, d, s1, s2);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_bstrpick_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Msbd,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let msbd = a.msbd as u64;
        let lsbd = a.lsbd as u64;
        if msbd < lsbd {
            return false;
        }
        let src = gpr_get(&self.gpr, ir, a.rj as u8);
        let shift = ir.new_const(Type::I64, lsbd);
        let shifted = ir.new_temp(Type::I64);
        ir.gen_shr(Type::I64, shifted, src, shift);
        let width = msbd - lsbd + 1;
        let mask_val = if width >= 64 {
            u64::MAX
        } else {
            (1u64 << width) - 1
        };
        let mask = ir.new_const(Type::I64, mask_val);
        let d = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, d, shifted, mask);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_bstrins_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Msbd,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let msbd = a.msbd as u64;
        let lsbd = a.lsbd as u64;
        if msbd < lsbd {
            return false;
        }
        let width = msbd - lsbd + 1;
        let src = gpr_get(&self.gpr, ir, a.rj as u8);
        let dst_old = gpr_get(&self.gpr, ir, a.rd as u8);
        let field_mask = if width >= 64 {
            u64::MAX
        } else {
            (1u64 << width) - 1
        };
        let src_mask = ir.new_const(Type::I64, field_mask);
        let src_masked = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, src_masked, src, src_mask);
        let shift = ir.new_const(Type::I64, lsbd);
        let src_shifted = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, src_shifted, src_masked, shift);
        let clear_mask = ir.new_const(Type::I64, !(field_mask << lsbd));
        let dst_cleared = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, dst_cleared, dst_old, clear_mask);
        let d = ir.new_temp(Type::I64);
        ir.gen_or(Type::I64, d, dst_cleared, src_shifted);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    // --- Shift/rotate by immediate ---

    fn trans_slli_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Ui5,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let sh = ir.new_const(Type::I64, a.ui5 as u64);
        let t = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I32, t, s, sh);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(d, t);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_slli_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Ui6,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let sh = ir.new_const(Type::I64, a.ui6 as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, d, s, sh);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_srli_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Ui5,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let sh = ir.new_const(Type::I64, a.ui5 as u64);
        let t = ir.new_temp(Type::I64);
        ir.gen_shr(Type::I32, t, s, sh);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(d, t);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_srli_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Ui6,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let sh = ir.new_const(Type::I64, a.ui6 as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_shr(Type::I64, d, s, sh);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_srai_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Ui5,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let sh = ir.new_const(Type::I64, a.ui5 as u64);
        let t = ir.new_temp(Type::I64);
        ir.gen_sar(Type::I32, t, s, sh);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(d, t);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_srai_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Ui6,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let sh = ir.new_const(Type::I64, a.ui6 as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_sar(Type::I64, d, s, sh);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_rotri_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Ui5,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let sh = ir.new_const(Type::I64, a.ui5 as u64);
        let t = ir.new_temp(Type::I64);
        ir.gen_rotr(Type::I32, t, s, sh);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(d, t);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_rotri_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Ui6,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let sh = ir.new_const(Type::I64, a.ui6 as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_rotr(Type::I64, d, s, sh);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    // --- Count leading/trailing zeros/ones ---

    fn trans_clz_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        let zv = ir.new_const(Type::I64, 32);
        ir.gen_clz(Type::I32, d, s, zv);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_clz_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        let zv = ir.new_const(Type::I64, 64);
        ir.gen_clz(Type::I64, d, s, zv);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_ctz_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        let zv = ir.new_const(Type::I64, 32);
        ir.gen_ctz(Type::I32, d, s, zv);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_ctz_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        let zv = ir.new_const(Type::I64, 64);
        ir.gen_ctz(Type::I64, d, s, zv);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_clo_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let inv = ir.new_temp(Type::I64);
        ir.gen_not(Type::I64, inv, s);
        let d = ir.new_temp(Type::I64);
        let zv = ir.new_const(Type::I64, 32);
        ir.gen_clz(Type::I32, d, inv, zv);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_clo_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let inv = ir.new_temp(Type::I64);
        ir.gen_not(Type::I64, inv, s);
        let d = ir.new_temp(Type::I64);
        let zv = ir.new_const(Type::I64, 64);
        ir.gen_clz(Type::I64, d, inv, zv);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_cto_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let inv = ir.new_temp(Type::I64);
        ir.gen_not(Type::I64, inv, s);
        let d = ir.new_temp(Type::I64);
        let zv = ir.new_const(Type::I64, 32);
        ir.gen_ctz(Type::I32, d, inv, zv);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_cto_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let inv = ir.new_temp(Type::I64);
        ir.gen_not(Type::I64, inv, s);
        let d = ir.new_temp(Type::I64);
        let zv = ir.new_const(Type::I64, 64);
        ir.gen_ctz(Type::I64, d, inv, zv);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    // --- Byte reversal (via bswap) ---

    fn trans_revb_2h(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_revb_2h as *const () as u64,
            &[s],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_revb_4h(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_revb_4h as *const () as u64,
            &[s],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_revb_2w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_revb_2w as *const () as u64,
            &[s],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_revb_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_bswap64(Type::I64, d, s, 0);
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_bitrev_4b(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_bitrev_4b as *const () as u64,
            &[s],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_bitrev_8b(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_bitrev_8b as *const () as u64,
            &[s],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_bitrev_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_bitrev_w as *const () as u64,
            &[s],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_bitrev_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_bitrev_d as *const () as u64,
            &[s],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    // --- MULH (high multiply via helpers) ---

    fn trans_mulh_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_mulh_d as *const () as u64,
            &[s1, s2],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_mulh_du(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_mulh_du as *const () as u64,
            &[s1, s2],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_mulh_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_mulh_w as *const () as u64,
            &[s1, s2],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_mulh_wu(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_mulh_wu as *const () as u64,
            &[s1, s2],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    // --- Load/Store (Phase 2, task11) ---

    fn trans_ld_b(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let d = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, d, addr, u32::from(MemOp::sb().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_ld_h(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let d = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, d, addr, u32::from(MemOp::sw().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_ld_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let d = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, d, addr, u32::from(MemOp::sl().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_ld_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let d = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, d, addr, u32::from(MemOp::uq().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_ld_bu(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let d = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, d, addr, u32::from(MemOp::ub().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_ld_hu(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let d = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, d, addr, u32::from(MemOp::uw().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_ld_wu(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let d = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, d, addr, u32::from(MemOp::ul().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_st_b(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::gpr_get;
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let val = gpr_get(&self.gpr, ir, a.rd as u8);
        ir.gen_qemu_st(Type::I64, val, addr, u32::from(MemOp::ub().bits()));
        true
    }

    fn trans_st_h(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::gpr_get;
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let val = gpr_get(&self.gpr, ir, a.rd as u8);
        ir.gen_qemu_st(Type::I64, val, addr, u32::from(MemOp::uw().bits()));
        true
    }

    fn trans_st_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::gpr_get;
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let val = gpr_get(&self.gpr, ir, a.rd as u8);
        ir.gen_qemu_st(Type::I64, val, addr, u32::from(MemOp::ul().bits()));
        true
    }

    fn trans_st_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        use gen_common::gpr_get;
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let val = gpr_get(&self.gpr, ir, a.rd as u8);
        ir.gen_qemu_st(Type::I64, val, addr, u32::from(MemOp::uq().bits()));
        true
    }

    // --- Branch/Jump (Phase 2, task12) ---

    fn trans_beq(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si16,
    ) -> bool {
        use gen_common::gpr_get;
        use machina_accel::ir::tb::TB_EXIT_IDX1;
        use machina_accel::ir::Cond;
        let pc = self.base.pc_next - 4;
        let target = pc.wrapping_add((a.si16 << 2) as u64);
        let next_pc = self.base.pc_next;
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rd as u8);
        let label_taken = ir.new_label();
        ir.gen_brcond(Type::I64, s1, s2, Cond::Eq, label_taken);
        let c_next = ir.new_const(Type::I64, next_pc);
        ir.gen_mov(Type::I64, self.pc, c_next);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        ir.gen_set_label(label_taken);
        let c_target = ir.new_const(Type::I64, target);
        ir.gen_mov(Type::I64, self.pc, c_target);
        ir.gen_goto_tb(1);
        ir.gen_exit_tb(TB_EXIT_IDX1);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_bne(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si16,
    ) -> bool {
        use gen_common::gpr_get;
        use machina_accel::ir::tb::TB_EXIT_IDX1;
        use machina_accel::ir::Cond;
        let pc = self.base.pc_next - 4;
        let target = pc.wrapping_add((a.si16 << 2) as u64);
        let next_pc = self.base.pc_next;
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rd as u8);
        let label_taken = ir.new_label();
        ir.gen_brcond(Type::I64, s1, s2, Cond::Ne, label_taken);
        let c_next = ir.new_const(Type::I64, next_pc);
        ir.gen_mov(Type::I64, self.pc, c_next);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        ir.gen_set_label(label_taken);
        let c_target = ir.new_const(Type::I64, target);
        ir.gen_mov(Type::I64, self.pc, c_target);
        ir.gen_goto_tb(1);
        ir.gen_exit_tb(TB_EXIT_IDX1);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_blt(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si16,
    ) -> bool {
        use gen_common::gpr_get;
        use machina_accel::ir::tb::TB_EXIT_IDX1;
        use machina_accel::ir::Cond;
        let pc = self.base.pc_next - 4;
        let target = pc.wrapping_add((a.si16 << 2) as u64);
        let next_pc = self.base.pc_next;
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rd as u8);
        let label_taken = ir.new_label();
        ir.gen_brcond(Type::I64, s1, s2, Cond::Lt, label_taken);
        let c_next = ir.new_const(Type::I64, next_pc);
        ir.gen_mov(Type::I64, self.pc, c_next);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        ir.gen_set_label(label_taken);
        let c_target = ir.new_const(Type::I64, target);
        ir.gen_mov(Type::I64, self.pc, c_target);
        ir.gen_goto_tb(1);
        ir.gen_exit_tb(TB_EXIT_IDX1);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_bge(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si16,
    ) -> bool {
        use gen_common::gpr_get;
        use machina_accel::ir::tb::TB_EXIT_IDX1;
        use machina_accel::ir::Cond;
        let pc = self.base.pc_next - 4;
        let target = pc.wrapping_add((a.si16 << 2) as u64);
        let next_pc = self.base.pc_next;
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rd as u8);
        let label_taken = ir.new_label();
        ir.gen_brcond(Type::I64, s1, s2, Cond::Ge, label_taken);
        let c_next = ir.new_const(Type::I64, next_pc);
        ir.gen_mov(Type::I64, self.pc, c_next);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        ir.gen_set_label(label_taken);
        let c_target = ir.new_const(Type::I64, target);
        ir.gen_mov(Type::I64, self.pc, c_target);
        ir.gen_goto_tb(1);
        ir.gen_exit_tb(TB_EXIT_IDX1);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_bltu(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si16,
    ) -> bool {
        use gen_common::gpr_get;
        use machina_accel::ir::tb::TB_EXIT_IDX1;
        use machina_accel::ir::Cond;
        let pc = self.base.pc_next - 4;
        let target = pc.wrapping_add((a.si16 << 2) as u64);
        let next_pc = self.base.pc_next;
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rd as u8);
        let label_taken = ir.new_label();
        ir.gen_brcond(Type::I64, s1, s2, Cond::Ltu, label_taken);
        let c_next = ir.new_const(Type::I64, next_pc);
        ir.gen_mov(Type::I64, self.pc, c_next);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        ir.gen_set_label(label_taken);
        let c_target = ir.new_const(Type::I64, target);
        ir.gen_mov(Type::I64, self.pc, c_target);
        ir.gen_goto_tb(1);
        ir.gen_exit_tb(TB_EXIT_IDX1);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_bgeu(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si16,
    ) -> bool {
        use gen_common::gpr_get;
        use machina_accel::ir::tb::TB_EXIT_IDX1;
        use machina_accel::ir::Cond;
        let pc = self.base.pc_next - 4;
        let target = pc.wrapping_add((a.si16 << 2) as u64);
        let next_pc = self.base.pc_next;
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let s2 = gpr_get(&self.gpr, ir, a.rd as u8);
        let label_taken = ir.new_label();
        ir.gen_brcond(Type::I64, s1, s2, Cond::Geu, label_taken);
        let c_next = ir.new_const(Type::I64, next_pc);
        ir.gen_mov(Type::I64, self.pc, c_next);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        ir.gen_set_label(label_taken);
        let c_target = ir.new_const(Type::I64, target);
        ir.gen_mov(Type::I64, self.pc, c_target);
        ir.gen_goto_tb(1);
        ir.gen_exit_tb(TB_EXIT_IDX1);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_beqz(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR1Offs21,
    ) -> bool {
        use gen_common::gpr_get;
        use machina_accel::ir::tb::TB_EXIT_IDX1;
        use machina_accel::ir::Cond;
        let pc = self.base.pc_next - 4;
        let target = pc.wrapping_add((a.offs21 << 2) as u64);
        let next_pc = self.base.pc_next;
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let zero = ir.new_const(Type::I64, 0);
        let label_taken = ir.new_label();
        ir.gen_brcond(Type::I64, s1, zero, Cond::Eq, label_taken);
        let c_next = ir.new_const(Type::I64, next_pc);
        ir.gen_mov(Type::I64, self.pc, c_next);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        ir.gen_set_label(label_taken);
        let c_target = ir.new_const(Type::I64, target);
        ir.gen_mov(Type::I64, self.pc, c_target);
        ir.gen_goto_tb(1);
        ir.gen_exit_tb(TB_EXIT_IDX1);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_bnez(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR1Offs21,
    ) -> bool {
        use gen_common::gpr_get;
        use machina_accel::ir::tb::TB_EXIT_IDX1;
        use machina_accel::ir::Cond;
        let pc = self.base.pc_next - 4;
        let target = pc.wrapping_add((a.offs21 << 2) as u64);
        let next_pc = self.base.pc_next;
        let s1 = gpr_get(&self.gpr, ir, a.rj as u8);
        let zero = ir.new_const(Type::I64, 0);
        let label_taken = ir.new_label();
        ir.gen_brcond(Type::I64, s1, zero, Cond::Ne, label_taken);
        let c_next = ir.new_const(Type::I64, next_pc);
        ir.gen_mov(Type::I64, self.pc, c_next);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        ir.gen_set_label(label_taken);
        let c_target = ir.new_const(Type::I64, target);
        ir.gen_mov(Type::I64, self.pc, c_target);
        ir.gen_goto_tb(1);
        ir.gen_exit_tb(TB_EXIT_IDX1);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_b(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsOffs26,
    ) -> bool {
        let pc = self.base.pc_next - 4;
        let target = pc.wrapping_add((a.offs26 << 2) as u64);
        let c = ir.new_const(Type::I64, target);
        ir.gen_mov(Type::I64, self.pc, c);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_bl(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsOffs26,
    ) -> bool {
        use gen_common::gpr_set;
        let pc = self.base.pc_next - 4;
        let next_pc = self.base.pc_next;
        let target = pc.wrapping_add((a.offs26 << 2) as u64);
        let ret = ir.new_const(Type::I64, next_pc);
        gpr_set(&self.gpr, ir, 1, ret);
        let c = ir.new_const(Type::I64, target);
        ir.gen_mov(Type::I64, self.pc, c);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_jirl(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si16,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::tb::TB_EXIT_NOCHAIN;
        let next_pc = self.base.pc_next;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, (a.si16 << 2) as u64);
        let target = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, target, base, off);
        let ret = ir.new_const(Type::I64, next_pc);
        gpr_set(&self.gpr, ir, a.rd as u8, ret);
        ir.gen_mov(Type::I64, self.pc, target);
        ir.gen_exit_tb(TB_EXIT_NOCHAIN);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_dbar(
        &mut self,
        ir: &mut Context,
        _a: &insn_decode::ArgsCode,
    ) -> bool {
        ir.gen_mb(0);
        true
    }

    fn trans_ibar(
        &mut self,
        ir: &mut Context,
        _a: &insn_decode::ArgsCode,
    ) -> bool {
        ir.gen_mb(0);
        true
    }

    fn trans_ll_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si14,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, (a.si14 << 2) as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let d = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, d, addr, u32::from(MemOp::sl().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        let one = ir.new_const(Type::I64, 1);
        ir.gen_mov(Type::I64, self.llbctl, one);
        ir.gen_mov(Type::I64, self.ll_res_addr, addr);
        ir.gen_mov(Type::I64, self.ll_res_val, d);
        true
    }

    fn trans_ll_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si14,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, (a.si14 << 2) as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let d = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, d, addr, u32::from(MemOp::uq().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        let one = ir.new_const(Type::I64, 1);
        ir.gen_mov(Type::I64, self.llbctl, one);
        ir.gen_mov(Type::I64, self.ll_res_addr, addr);
        ir.gen_mov(Type::I64, self.ll_res_val, d);
        true
    }

    fn trans_sc_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si14,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, (a.si14 << 2) as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let val = gpr_get(&self.gpr, ir, a.rd as u8);
        let env_tmp = self.env;
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_sc_w as *const () as u64,
            &[env_tmp, addr, val],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_sc_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si14,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, (a.si14 << 2) as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let val = gpr_get(&self.gpr, ir, a.rd as u8);
        let env_tmp = self.env;
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_sc_d as *const () as u64,
            &[env_tmp, addr, val],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_amadd_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::uq().bits()));
        let new = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, new, old, src);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::uq().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_amadd_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::sl().bits()));
        let new = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, new, old, src);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::ul().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_amswap_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::uq().bits()));
        ir.gen_qemu_st(Type::I64, src, addr, u32::from(MemOp::uq().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_amswap_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::sl().bits()));
        ir.gen_qemu_st(Type::I64, src, addr, u32::from(MemOp::ul().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_amand_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::uq().bits()));
        let new = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, new, old, src);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::uq().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_amand_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::sl().bits()));
        let new = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, new, old, src);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::ul().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_amor_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::uq().bits()));
        let new = ir.new_temp(Type::I64);
        ir.gen_or(Type::I64, new, old, src);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::uq().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_amor_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::sl().bits()));
        let new = ir.new_temp(Type::I64);
        ir.gen_or(Type::I64, new, old, src);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::ul().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_amxor_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::uq().bits()));
        let new = ir.new_temp(Type::I64);
        ir.gen_xor(Type::I64, new, old, src);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::uq().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_amxor_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::MemOp;
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::sl().bits()));
        let new = ir.new_temp(Type::I64);
        ir.gen_xor(Type::I64, new, old, src);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::ul().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_ammax_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::{Cond, MemOp};
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::uq().bits()));
        let new = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, new, old, src, old, src, Cond::Ge);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::uq().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_ammin_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::{Cond, MemOp};
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::uq().bits()));
        let new = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, new, old, src, old, src, Cond::Le);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::uq().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_ammax_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::{Cond, MemOp};
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::sl().bits()));
        let src_ext = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(src_ext, src);
        let new = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, new, old, src_ext, old, src_ext, Cond::Ge);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::ul().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_ammin_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::{Cond, MemOp};
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::sl().bits()));
        let src_ext = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(src_ext, src);
        let new = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, new, old, src_ext, old, src_ext, Cond::Le);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::ul().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_ammax_wu(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::{Cond, MemOp};
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::ul().bits()));
        let src_trunc = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(src_trunc, src);
        let src_u = ir.new_temp(Type::I64);
        let mask32 = ir.new_const(Type::I64, 0xFFFF_FFFF);
        ir.gen_and(Type::I64, src_u, src, mask32);
        let new = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, new, old, src_u, old, src_u, Cond::Geu);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::ul().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_ammin_wu(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::{Cond, MemOp};
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::ul().bits()));
        let src_u = ir.new_temp(Type::I64);
        let mask32 = ir.new_const(Type::I64, 0xFFFF_FFFF);
        ir.gen_and(Type::I64, src_u, src, mask32);
        let new = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, new, old, src_u, old, src_u, Cond::Leu);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::ul().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_ammax_du(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::{Cond, MemOp};
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::uq().bits()));
        let new = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, new, old, src, old, src, Cond::Geu);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::uq().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_ammin_du(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        ir.contains_atomic = true;
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::{Cond, MemOp};
        let addr = gpr_get(&self.gpr, ir, a.rj as u8);
        let src = gpr_get(&self.gpr, ir, a.rk as u8);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, u32::from(MemOp::uq().bits()));
        let new = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, new, old, src, old, src, Cond::Leu);
        ir.gen_qemu_st(Type::I64, new, addr, u32::from(MemOp::uq().bits()));
        gpr_set(&self.gpr, ir, a.rd as u8, old);
        true
    }

    fn trans_csrrd(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsCsr,
    ) -> bool {
        use gen_common::gpr_set;
        use machina_accel::ir::Cond;
        let env_tmp = self.env;
        let pc_val = ir.new_const(Type::I64, self.base.pc_next - 4);
        ir.gen_mov(Type::I64, self.pc, pc_val);
        let chk = ir.new_temp(Type::I64);
        ir.gen_call(
            chk,
            helpers::loongarch_helper_check_plv as *const () as u64,
            &[env_tmp],
        );
        let zero = ir.new_const(Type::I64, 0);
        let label_ok = ir.new_label();
        ir.gen_brcond(Type::I64, chk, zero, Cond::Eq, label_ok);
        ir.gen_mov(Type::I64, self.pc, chk);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        ir.gen_set_label(label_ok);
        let csr_num = ir.new_const(Type::I64, a.csr_num as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_csrrd as *const () as u64,
            &[env_tmp, csr_num],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_csrwr(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsCsr,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::Cond;
        let env_tmp = self.env;
        let pc_val = ir.new_const(Type::I64, self.base.pc_next - 4);
        ir.gen_mov(Type::I64, self.pc, pc_val);
        let chk = ir.new_temp(Type::I64);
        ir.gen_call(
            chk,
            helpers::loongarch_helper_check_plv as *const () as u64,
            &[env_tmp],
        );
        let zero = ir.new_const(Type::I64, 0);
        let label_ok = ir.new_label();
        ir.gen_brcond(Type::I64, chk, zero, Cond::Eq, label_ok);
        ir.gen_mov(Type::I64, self.pc, chk);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        ir.gen_set_label(label_ok);
        let csr_num = ir.new_const(Type::I64, a.csr_num as u64);
        let val = gpr_get(&self.gpr, ir, a.rd as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_csrwr as *const () as u64,
            &[env_tmp, csr_num, val],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        let pc_next = ir.new_const(Type::I64, self.base.pc_next);
        ir.gen_mov(Type::I64, self.pc, pc_next);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_csrxchg(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsCsr,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        use machina_accel::ir::Cond;
        if a.rj == 0 || a.rj == 1 {
            return false;
        }
        let env_tmp = self.env;
        let pc_val = ir.new_const(Type::I64, self.base.pc_next - 4);
        ir.gen_mov(Type::I64, self.pc, pc_val);
        let chk = ir.new_temp(Type::I64);
        ir.gen_call(
            chk,
            helpers::loongarch_helper_check_plv as *const () as u64,
            &[env_tmp],
        );
        let zero = ir.new_const(Type::I64, 0);
        let label_ok = ir.new_label();
        ir.gen_brcond(Type::I64, chk, zero, Cond::Eq, label_ok);
        ir.gen_mov(Type::I64, self.pc, chk);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        ir.gen_set_label(label_ok);
        let csr_num = ir.new_const(Type::I64, a.csr_num as u64);
        let val = gpr_get(&self.gpr, ir, a.rd as u8);
        let mask = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_csrxchg as *const () as u64,
            &[env_tmp, csr_num, val, mask],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        let pc_next = ir.new_const(Type::I64, self.base.pc_next);
        ir.gen_mov(Type::I64, self.pc, pc_next);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_cpucfg(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        use gen_common::{gpr_get, gpr_set};
        let env_tmp = self.env;
        let idx = gpr_get(&self.gpr, ir, a.rj as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_cpucfg as *const () as u64,
            &[env_tmp, idx],
        );
        gpr_set(&self.gpr, ir, a.rd as u8, d);
        true
    }

    fn trans_syscall(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsCode,
    ) -> bool {
        let env_tmp = self.env;
        let pc = ir.new_const(Type::I64, self.base.pc_next - 4);
        ir.gen_mov(Type::I64, self.pc, pc);
        let ecode = ir.new_const(Type::I64, 0x0B);
        let code = ir.new_const(Type::I64, a.code15 as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_raise_exception as *const () as u64,
            &[env_tmp, ecode, code],
        );
        ir.gen_mov(Type::I64, self.pc, d);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_break_(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsCode,
    ) -> bool {
        let env_tmp = self.env;
        let pc = ir.new_const(Type::I64, self.base.pc_next - 4);
        ir.gen_mov(Type::I64, self.pc, pc);
        let ecode = ir.new_const(Type::I64, 0x0C);
        let code = ir.new_const(Type::I64, a.code15 as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_raise_exception as *const () as u64,
            &[env_tmp, ecode, code],
        );
        ir.gen_mov(Type::I64, self.pc, d);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_ertn(
        &mut self,
        ir: &mut Context,
        _a: &insn_decode::ArgsEmpty,
    ) -> bool {
        use machina_accel::ir::Cond;
        let env_tmp = self.env;
        let pc_val = ir.new_const(Type::I64, self.base.pc_next - 4);
        ir.gen_mov(Type::I64, self.pc, pc_val);
        let chk = ir.new_temp(Type::I64);
        ir.gen_call(
            chk,
            helpers::loongarch_helper_check_plv as *const () as u64,
            &[env_tmp],
        );
        let zero = ir.new_const(Type::I64, 0);
        let label_ok = ir.new_label();
        ir.gen_brcond(Type::I64, chk, zero, Cond::Eq, label_ok);
        ir.gen_mov(Type::I64, self.pc, chk);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        ir.gen_set_label(label_ok);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_ertn as *const () as u64,
            &[env_tmp],
        );
        ir.gen_mov(Type::I64, self.pc, d);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_idle(
        &mut self,
        ir: &mut Context,
        _a: &insn_decode::ArgsCode,
    ) -> bool {
        use machina_accel::ir::Cond;
        let env_tmp = self.env;
        let pc_val = ir.new_const(Type::I64, self.base.pc_next - 4);
        ir.gen_mov(Type::I64, self.pc, pc_val);
        let chk = ir.new_temp(Type::I64);
        ir.gen_call(
            chk,
            helpers::loongarch_helper_check_plv as *const () as u64,
            &[env_tmp],
        );
        let zero = ir.new_const(Type::I64, 0);
        let label_ok = ir.new_label();
        ir.gen_brcond(Type::I64, chk, zero, Cond::Eq, label_ok);
        ir.gen_mov(Type::I64, self.pc, chk);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        ir.gen_set_label(label_ok);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_idle as *const () as u64,
            &[env_tmp],
        );
        let pc_next = ir.new_const(Type::I64, self.base.pc_next);
        ir.gen_mov(Type::I64, self.pc, pc_next);
        ir.gen_exit_tb(machina_accel::ir::tb::EXCP_WFI);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_tlbsrch(
        &mut self,
        ir: &mut Context,
        _a: &insn_decode::ArgsEmpty,
    ) -> bool {
        use machina_accel::ir::Cond;
        let env_tmp = self.env;
        let pc_val = ir.new_const(Type::I64, self.base.pc_next - 4);
        ir.gen_mov(Type::I64, self.pc, pc_val);
        let chk = ir.new_temp(Type::I64);
        ir.gen_call(
            chk,
            helpers::loongarch_helper_check_plv as *const () as u64,
            &[env_tmp],
        );
        let zero = ir.new_const(Type::I64, 0);
        let label_ok = ir.new_label();
        ir.gen_brcond(Type::I64, chk, zero, Cond::Eq, label_ok);
        ir.gen_mov(Type::I64, self.pc, chk);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        ir.gen_set_label(label_ok);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_tlbsrch as *const () as u64,
            &[env_tmp],
        );
        true
    }

    fn trans_tlbrd(
        &mut self,
        ir: &mut Context,
        _a: &insn_decode::ArgsEmpty,
    ) -> bool {
        use machina_accel::ir::Cond;
        let env_tmp = self.env;
        let pc_val = ir.new_const(Type::I64, self.base.pc_next - 4);
        ir.gen_mov(Type::I64, self.pc, pc_val);
        let chk = ir.new_temp(Type::I64);
        ir.gen_call(
            chk,
            helpers::loongarch_helper_check_plv as *const () as u64,
            &[env_tmp],
        );
        let zero = ir.new_const(Type::I64, 0);
        let label_ok = ir.new_label();
        ir.gen_brcond(Type::I64, chk, zero, Cond::Eq, label_ok);
        ir.gen_mov(Type::I64, self.pc, chk);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        ir.gen_set_label(label_ok);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_tlbrd as *const () as u64,
            &[env_tmp],
        );
        true
    }

    fn trans_tlbwr(
        &mut self,
        ir: &mut Context,
        _a: &insn_decode::ArgsEmpty,
    ) -> bool {
        use machina_accel::ir::Cond;
        let env_tmp = self.env;
        let pc_val = ir.new_const(Type::I64, self.base.pc_next - 4);
        ir.gen_mov(Type::I64, self.pc, pc_val);
        let chk = ir.new_temp(Type::I64);
        ir.gen_call(
            chk,
            helpers::loongarch_helper_check_plv as *const () as u64,
            &[env_tmp],
        );
        let zero = ir.new_const(Type::I64, 0);
        let label_ok = ir.new_label();
        ir.gen_brcond(Type::I64, chk, zero, Cond::Eq, label_ok);
        ir.gen_mov(Type::I64, self.pc, chk);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        ir.gen_set_label(label_ok);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_tlbwr as *const () as u64,
            &[env_tmp],
        );
        true
    }

    fn trans_tlbfill(
        &mut self,
        ir: &mut Context,
        _a: &insn_decode::ArgsEmpty,
    ) -> bool {
        use machina_accel::ir::Cond;
        let env_tmp = self.env;
        let pc_val = ir.new_const(Type::I64, self.base.pc_next - 4);
        ir.gen_mov(Type::I64, self.pc, pc_val);
        let chk = ir.new_temp(Type::I64);
        ir.gen_call(
            chk,
            helpers::loongarch_helper_check_plv as *const () as u64,
            &[env_tmp],
        );
        let zero = ir.new_const(Type::I64, 0);
        let label_ok = ir.new_label();
        ir.gen_brcond(Type::I64, chk, zero, Cond::Eq, label_ok);
        ir.gen_mov(Type::I64, self.pc, chk);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        ir.gen_set_label(label_ok);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_tlbfill as *const () as u64,
            &[env_tmp],
        );
        true
    }

    fn trans_invtlb(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR3,
    ) -> bool {
        use gen_common::gpr_get;
        use machina_accel::ir::Cond;
        let env_tmp = self.env;
        let pc_val = ir.new_const(Type::I64, self.base.pc_next - 4);
        ir.gen_mov(Type::I64, self.pc, pc_val);
        let chk = ir.new_temp(Type::I64);
        ir.gen_call(
            chk,
            helpers::loongarch_helper_check_plv as *const () as u64,
            &[env_tmp],
        );
        let zero = ir.new_const(Type::I64, 0);
        let label_ok = ir.new_label();
        ir.gen_brcond(Type::I64, chk, zero, Cond::Eq, label_ok);
        ir.gen_mov(Type::I64, self.pc, chk);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        ir.gen_set_label(label_ok);
        let op = ir.new_const(Type::I64, a.rd as u64);
        let asid_val = gpr_get(&self.gpr, ir, a.rj as u8);
        let va = gpr_get(&self.gpr, ir, a.rk as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_invtlb as *const () as u64,
            &[env_tmp, op, asid_val, va],
        );
        let zero2 = ir.new_const(Type::I64, 0);
        let label_done = ir.new_label();
        ir.gen_brcond(Type::I64, d, zero2, Cond::Eq, label_done);
        ir.gen_mov(Type::I64, self.pc, d);
        ir.gen_exit_tb(EXCP_ARCH_DONE);
        ir.gen_set_label(label_done);
        true
    }

    fn trans_iocsrrd_b(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        gen_iocsr_rd(self, ir, a, 1)
    }

    fn trans_iocsrrd_h(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        gen_iocsr_rd(self, ir, a, 2)
    }

    fn trans_iocsrrd_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        gen_iocsr_rd(self, ir, a, 4)
    }

    fn trans_iocsrrd_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        gen_iocsr_rd(self, ir, a, 8)
    }

    fn trans_iocsrwr_b(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        gen_iocsr_wr(self, ir, a, 1)
    }

    fn trans_iocsrwr_h(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        gen_iocsr_wr(self, ir, a, 2)
    }

    fn trans_iocsrwr_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        gen_iocsr_wr(self, ir, a, 4)
    }

    fn trans_iocsrwr_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2,
    ) -> bool {
        gen_iocsr_wr(self, ir, a, 8)
    }

    // FP load/store
    fn trans_fld_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        use gen_common::gpr_get;
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let d = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, d, addr, u32::from(MemOp::ul().bits()));
        gen_float::fpr_set(self, ir, a.rd as u8, d);
        true
    }

    fn trans_fld_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        use gen_common::gpr_get;
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let d = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, d, addr, u32::from(MemOp::uq().bits()));
        gen_float::fpr_set(self, ir, a.rd as u8, d);
        true
    }

    fn trans_fst_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        use gen_common::gpr_get;
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let val = gen_float::fpr_get(self, ir, a.rd as u8);
        ir.gen_qemu_st(Type::I64, val, addr, u32::from(MemOp::ul().bits()));
        true
    }

    fn trans_fst_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsR2Si12,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        use gen_common::gpr_get;
        use machina_accel::ir::MemOp;
        let base = gpr_get(&self.gpr, ir, a.rj as u8);
        let off = ir.new_const(Type::I64, a.si12 as u64);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, off);
        let val = gen_float::fpr_get(self, ir, a.rd as u8);
        ir.gen_qemu_st(Type::I64, val, addr, u32::from(MemOp::uq().bits()));
        true
    }

    // FP arithmetic
    fn trans_fadd_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr3,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_arith_s(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fadd_s,
        );
        true
    }

    fn trans_fadd_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr3,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_arith_s(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fadd_d,
        );
        true
    }

    fn trans_fsub_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr3,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_arith_s(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fsub_s,
        );
        true
    }

    fn trans_fsub_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr3,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_arith_s(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fsub_d,
        );
        true
    }

    fn trans_fmul_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr3,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_arith_s(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fmul_s,
        );
        true
    }

    fn trans_fmul_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr3,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_arith_s(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fmul_d,
        );
        true
    }

    fn trans_fdiv_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr3,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_arith_s(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fdiv_s,
        );
        true
    }

    fn trans_fdiv_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr3,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_arith_s(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fdiv_d,
        );
        true
    }

    fn trans_fsqrt_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_unary(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            helpers::loongarch_helper_fsqrt_s,
        );
        true
    }

    fn trans_fsqrt_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_unary(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            helpers::loongarch_helper_fsqrt_d,
        );
        true
    }

    // FP conversion
    fn trans_fcvt_s_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_unary(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            helpers::loongarch_helper_fcvt_s_d,
        );
        true
    }

    fn trans_fcvt_d_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_unary(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            helpers::loongarch_helper_fcvt_d_s,
        );
        true
    }

    fn trans_ffint_s_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_unary(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            helpers::loongarch_helper_ffint_s_w,
        );
        true
    }

    fn trans_ffint_d_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_unary(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            helpers::loongarch_helper_ffint_d_w,
        );
        true
    }

    fn trans_ffint_s_l(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_unary(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            helpers::loongarch_helper_ffint_s_l,
        );
        true
    }

    fn trans_ffint_d_l(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_unary(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            helpers::loongarch_helper_ffint_d_l,
        );
        true
    }

    fn trans_ftintrz_w_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_unary(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            helpers::loongarch_helper_ftintrz_w_s,
        );
        true
    }

    fn trans_ftintrz_w_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_unary(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            helpers::loongarch_helper_ftintrz_w_d,
        );
        true
    }

    fn trans_ftintrz_l_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_unary(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            helpers::loongarch_helper_ftintrz_l_s,
        );
        true
    }

    fn trans_ftintrz_l_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_unary(
            self,
            ir,
            a.fd as u8,
            a.fj as u8,
            helpers::loongarch_helper_ftintrz_l_d,
        );
        true
    }

    // FP compare (subset: CEQ, CLT, CLE, CUN)
    fn trans_fcmp_ceq_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_ceq_s,
        );
        true
    }

    fn trans_fcmp_ceq_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_ceq_d,
        );
        true
    }

    fn trans_fcmp_clt_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_clt_s,
        );
        true
    }

    fn trans_fcmp_clt_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_clt_d,
        );
        true
    }

    fn trans_fcmp_cle_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cle_s,
        );
        true
    }

    fn trans_fcmp_cle_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cle_d,
        );
        true
    }

    fn trans_fcmp_cun_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cun_s,
        );
        true
    }

    fn trans_fcmp_cun_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cun_d,
        );
        true
    }

    // FP move
    fn trans_fmov_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let v = gen_float::fpr_get(self, ir, a.fj as u8);
        gen_float::fpr_set(self, ir, a.fd as u8, v);
        true
    }

    fn trans_fmov_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let v = gen_float::fpr_get(self, ir, a.fj as u8);
        gen_float::fpr_set(self, ir, a.fd as u8, v);
        true
    }

    fn trans_movgr2fr_w(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        use gen_common::gpr_get;
        let v = gpr_get(&self.gpr, ir, a.fj as u8);
        let trunc = ir.new_temp(Type::I64);
        let mask = ir.new_const(Type::I64, 0xFFFF_FFFF);
        ir.gen_and(Type::I64, trunc, v, mask);
        gen_float::fpr_set(self, ir, a.fd as u8, trunc);
        true
    }

    fn trans_movgr2fr_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        use gen_common::gpr_get;
        let v = gpr_get(&self.gpr, ir, a.fj as u8);
        gen_float::fpr_set(self, ir, a.fd as u8, v);
        true
    }

    fn trans_movfr2gr_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        use gen_common::gpr_set;
        let v = gen_float::fpr_get(self, ir, a.fj as u8);
        let ext = ir.new_temp(Type::I64);
        ir.gen_ext_i32_i64(ext, v);
        gpr_set(&self.gpr, ir, a.fd as u8, ext);
        true
    }

    fn trans_movfr2gr_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        use gen_common::gpr_set;
        let v = gen_float::fpr_get(self, ir, a.fj as u8);
        gpr_set(&self.gpr, ir, a.fd as u8, v);
        true
    }

    fn trans_movgr2fcsr(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        use gen_common::gpr_get;
        let env_tmp = self.env;
        let val = gpr_get(&self.gpr, ir, a.fj as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_movgr2fcsr as *const () as u64,
            &[env_tmp, val],
        );
        true
    }

    fn trans_movfcsr2gr(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        use gen_common::gpr_set;
        let env_tmp = self.env;
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_movfcsr2gr as *const () as u64,
            &[env_tmp],
        );
        gpr_set(&self.gpr, ir, a.fd as u8, d);
        true
    }

    fn trans_fabs_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let v = gen_float::fpr_get(self, ir, a.fj as u8);
        let mask = ir.new_const(Type::I64, 0x7FFF_FFFF);
        let d = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, d, v, mask);
        gen_float::fpr_set(self, ir, a.fd as u8, d);
        true
    }

    fn trans_fabs_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let v = gen_float::fpr_get(self, ir, a.fj as u8);
        let mask = ir.new_const(Type::I64, 0x7FFF_FFFF_FFFF_FFFF);
        let d = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, d, v, mask);
        gen_float::fpr_set(self, ir, a.fd as u8, d);
        true
    }

    fn trans_fneg_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let v = gen_float::fpr_get(self, ir, a.fj as u8);
        let bit = ir.new_const(Type::I64, 0x8000_0000);
        let d = ir.new_temp(Type::I64);
        ir.gen_xor(Type::I64, d, v, bit);
        gen_float::fpr_set(self, ir, a.fd as u8, d);
        true
    }

    fn trans_fneg_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr2,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let v = gen_float::fpr_get(self, ir, a.fj as u8);
        let bit = ir.new_const(Type::I64, 0x8000_0000_0000_0000);
        let d = ir.new_temp(Type::I64);
        ir.gen_xor(Type::I64, d, v, bit);
        gen_float::fpr_set(self, ir, a.fd as u8, d);
        true
    }

    fn trans_fmadd_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr4,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let fj = gen_float::fpr_get(self, ir, a.fj as u8);
        let fk = gen_float::fpr_get(self, ir, a.fk as u8);
        let fa = gen_float::fpr_get(self, ir, a.fa as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_fmadd_s as *const () as u64,
            &[fj, fk, fa],
        );
        gen_float::fpr_set(self, ir, a.fd as u8, d);
        true
    }

    fn trans_fmadd_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr4,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let fj = gen_float::fpr_get(self, ir, a.fj as u8);
        let fk = gen_float::fpr_get(self, ir, a.fk as u8);
        let fa = gen_float::fpr_get(self, ir, a.fa as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_fmadd_d as *const () as u64,
            &[fj, fk, fa],
        );
        gen_float::fpr_set(self, ir, a.fd as u8, d);
        true
    }

    fn trans_fmsub_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr4,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let fj = gen_float::fpr_get(self, ir, a.fj as u8);
        let fk = gen_float::fpr_get(self, ir, a.fk as u8);
        let fa = gen_float::fpr_get(self, ir, a.fa as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_fmsub_s as *const () as u64,
            &[fj, fk, fa],
        );
        gen_float::fpr_set(self, ir, a.fd as u8, d);
        true
    }

    fn trans_fmsub_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr4,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let fj = gen_float::fpr_get(self, ir, a.fj as u8);
        let fk = gen_float::fpr_get(self, ir, a.fk as u8);
        let fa = gen_float::fpr_get(self, ir, a.fa as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_fmsub_d as *const () as u64,
            &[fj, fk, fa],
        );
        gen_float::fpr_set(self, ir, a.fd as u8, d);
        true
    }

    fn trans_fnmadd_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr4,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let fj = gen_float::fpr_get(self, ir, a.fj as u8);
        let fk = gen_float::fpr_get(self, ir, a.fk as u8);
        let fa = gen_float::fpr_get(self, ir, a.fa as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_fnmadd_s as *const () as u64,
            &[fj, fk, fa],
        );
        gen_float::fpr_set(self, ir, a.fd as u8, d);
        true
    }

    fn trans_fnmadd_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr4,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let fj = gen_float::fpr_get(self, ir, a.fj as u8);
        let fk = gen_float::fpr_get(self, ir, a.fk as u8);
        let fa = gen_float::fpr_get(self, ir, a.fa as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_fnmadd_d as *const () as u64,
            &[fj, fk, fa],
        );
        gen_float::fpr_set(self, ir, a.fd as u8, d);
        true
    }

    fn trans_fnmsub_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr4,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let fj = gen_float::fpr_get(self, ir, a.fj as u8);
        let fk = gen_float::fpr_get(self, ir, a.fk as u8);
        let fa = gen_float::fpr_get(self, ir, a.fa as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_fnmsub_s as *const () as u64,
            &[fj, fk, fa],
        );
        gen_float::fpr_set(self, ir, a.fd as u8, d);
        true
    }

    fn trans_fnmsub_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFr4,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let fj = gen_float::fpr_get(self, ir, a.fj as u8);
        let fk = gen_float::fpr_get(self, ir, a.fk as u8);
        let fa = gen_float::fpr_get(self, ir, a.fa as u8);
        let d = ir.new_temp(Type::I64);
        ir.gen_call(
            d,
            helpers::loongarch_helper_fnmsub_d as *const () as u64,
            &[fj, fk, fa],
        );
        gen_float::fpr_set(self, ir, a.fd as u8, d);
        true
    }

    fn trans_fsel(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFsel,
    ) -> bool {
        use machina_accel::ir::Cond;
        gen_float::check_fpe(self, ir);
        let fj = gen_float::fpr_get(self, ir, a.fj as u8);
        let fk = gen_float::fpr_get(self, ir, a.fk as u8);
        let ca = gen_float::fcc_get(self, ir, a.ca as u8);
        let zero = ir.new_const(Type::I64, 0);
        let d = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, d, ca, zero, fk, fj, Cond::Ne);
        gen_float::fpr_set(self, ir, a.fd as u8, d);
        true
    }

    fn trans_bceqz(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFbranch,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        use machina_accel::ir::tb::TB_EXIT_IDX1;
        use machina_accel::ir::Cond;
        let ca = gen_float::fcc_get(self, ir, a.cj as u8);
        let zero = ir.new_const(Type::I64, 0);
        let pc = self.base.pc_next - 4;
        let target = pc.wrapping_add((a.offs21 << 2) as u64);
        let label_taken = ir.new_label();
        ir.gen_brcond(Type::I64, ca, zero, Cond::Eq, label_taken);
        let c_next = ir.new_const(Type::I64, self.base.pc_next);
        ir.gen_mov(Type::I64, self.pc, c_next);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        ir.gen_set_label(label_taken);
        let c_target = ir.new_const(Type::I64, target);
        ir.gen_mov(Type::I64, self.pc, c_target);
        ir.gen_goto_tb(1);
        ir.gen_exit_tb(TB_EXIT_IDX1);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    fn trans_bcnez(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFbranch,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        use machina_accel::ir::tb::TB_EXIT_IDX1;
        use machina_accel::ir::Cond;
        let ca = gen_float::fcc_get(self, ir, a.cj as u8);
        let zero = ir.new_const(Type::I64, 0);
        let pc = self.base.pc_next - 4;
        let target = pc.wrapping_add((a.offs21 << 2) as u64);
        let label_taken = ir.new_label();
        ir.gen_brcond(Type::I64, ca, zero, Cond::Ne, label_taken);
        let c_next = ir.new_const(Type::I64, self.base.pc_next);
        ir.gen_mov(Type::I64, self.pc, c_next);
        ir.gen_goto_tb(0);
        ir.gen_exit_tb(TB_EXIT_IDX0);
        ir.gen_set_label(label_taken);
        let c_target = ir.new_const(Type::I64, target);
        ir.gen_mov(Type::I64, self.pc, c_target);
        ir.gen_goto_tb(1);
        ir.gen_exit_tb(TB_EXIT_IDX1);
        self.base.is_jmp = DisasJumpType::NoReturn;
        true
    }

    // Remaining FCMP conditions delegate to existing helpers
    fn trans_fcmp_caf_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let z = ir.new_const(Type::I64, 0);
        gen_float::fcc_set(self, ir, a.cd as u8, z);
        true
    }
    fn trans_fcmp_caf_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        let z = ir.new_const(Type::I64, 0);
        gen_float::fcc_set(self, ir, a.cd as u8, z);
        true
    }
    fn trans_fcmp_cueq_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cueq_s,
        );
        true
    }
    fn trans_fcmp_cueq_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cueq_d,
        );
        true
    }
    fn trans_fcmp_cult_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cult_s,
        );
        true
    }
    fn trans_fcmp_cult_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cult_d,
        );
        true
    }
    fn trans_fcmp_cule_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cule_s,
        );
        true
    }
    fn trans_fcmp_cule_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cule_d,
        );
        true
    }
    fn trans_fcmp_cne_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cne_s,
        );
        true
    }
    fn trans_fcmp_cne_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cne_d,
        );
        true
    }
    fn trans_fcmp_cor_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cor_s,
        );
        true
    }
    fn trans_fcmp_cor_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cor_d,
        );
        true
    }
    fn trans_fcmp_cune_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cune_s,
        );
        true
    }
    fn trans_fcmp_cune_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        gen_float::check_fpe(self, ir);
        gen_float::gen_fp_cmp(
            self,
            ir,
            a.cd as u8,
            a.fj as u8,
            a.fk as u8,
            helpers::loongarch_helper_fcmp_cune_d,
        );
        true
    }
    // Signaling variants (same behavior for now, NaN signaling deferred to softfloat)
    fn trans_fcmp_saf_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_caf_s(ir, a)
    }
    fn trans_fcmp_saf_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_caf_d(ir, a)
    }
    fn trans_fcmp_seq_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_ceq_s(ir, a)
    }
    fn trans_fcmp_seq_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_ceq_d(ir, a)
    }
    fn trans_fcmp_slt_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_clt_s(ir, a)
    }
    fn trans_fcmp_slt_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_clt_d(ir, a)
    }
    fn trans_fcmp_sle_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cle_s(ir, a)
    }
    fn trans_fcmp_sle_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cle_d(ir, a)
    }
    fn trans_fcmp_sun_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cun_s(ir, a)
    }
    fn trans_fcmp_sun_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cun_d(ir, a)
    }
    fn trans_fcmp_sueq_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cueq_s(ir, a)
    }
    fn trans_fcmp_sueq_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cueq_d(ir, a)
    }
    fn trans_fcmp_sult_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cult_s(ir, a)
    }
    fn trans_fcmp_sult_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cult_d(ir, a)
    }
    fn trans_fcmp_sule_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cule_s(ir, a)
    }
    fn trans_fcmp_sule_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cule_d(ir, a)
    }
    fn trans_fcmp_sne_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cne_s(ir, a)
    }
    fn trans_fcmp_sne_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cne_d(ir, a)
    }
    fn trans_fcmp_sor_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cor_s(ir, a)
    }
    fn trans_fcmp_sor_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cor_d(ir, a)
    }
    fn trans_fcmp_sune_s(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cune_s(ir, a)
    }
    fn trans_fcmp_sune_d(
        &mut self,
        ir: &mut Context,
        a: &insn_decode::ArgsFcmp,
    ) -> bool {
        self.trans_fcmp_cune_d(ir, a)
    }
}

#[allow(clippy::cast_possible_truncation)]
fn gen_iocsr_rd(
    ctx: &mut LoongArchDisasContext,
    ir: &mut Context,
    a: &insn_decode::ArgsR2,
    width: u64,
) -> bool {
    use gen_common::{gpr_get, gpr_set};
    use machina_accel::ir::Cond;
    let env_tmp = ctx.env;
    let pc_val = ir.new_const(Type::I64, ctx.base.pc_next - 4);
    ir.gen_mov(Type::I64, ctx.pc, pc_val);
    let chk = ir.new_temp(Type::I64);
    ir.gen_call(
        chk,
        helpers::loongarch_helper_check_plv as *const () as u64,
        &[env_tmp],
    );
    let zero = ir.new_const(Type::I64, 0);
    let label_ok = ir.new_label();
    ir.gen_brcond(Type::I64, chk, zero, Cond::Eq, label_ok);
    ir.gen_mov(Type::I64, ctx.pc, chk);
    ir.gen_exit_tb(EXCP_ARCH_DONE);
    ir.gen_set_label(label_ok);
    let addr = gpr_get(&ctx.gpr, ir, a.rj as u8);
    let w = ir.new_const(Type::I64, width);
    let d = ir.new_temp(Type::I64);
    ir.gen_call(
        d,
        helpers::loongarch_helper_iocsrrd as *const () as u64,
        &[env_tmp, addr, w],
    );
    gpr_set(&ctx.gpr, ir, a.rd as u8, d);
    true
}

#[allow(clippy::cast_possible_truncation)]
fn gen_iocsr_wr(
    ctx: &mut LoongArchDisasContext,
    ir: &mut Context,
    a: &insn_decode::ArgsR2,
    width: u64,
) -> bool {
    use gen_common::gpr_get;
    use machina_accel::ir::Cond;
    let env_tmp = ctx.env;
    let pc_val = ir.new_const(Type::I64, ctx.base.pc_next - 4);
    ir.gen_mov(Type::I64, ctx.pc, pc_val);
    let chk = ir.new_temp(Type::I64);
    ir.gen_call(
        chk,
        helpers::loongarch_helper_check_plv as *const () as u64,
        &[env_tmp],
    );
    let zero = ir.new_const(Type::I64, 0);
    let label_ok = ir.new_label();
    ir.gen_brcond(Type::I64, chk, zero, Cond::Eq, label_ok);
    ir.gen_mov(Type::I64, ctx.pc, chk);
    ir.gen_exit_tb(EXCP_ARCH_DONE);
    ir.gen_set_label(label_ok);
    let addr = gpr_get(&ctx.gpr, ir, a.rj as u8);
    let val = gpr_get(&ctx.gpr, ir, a.rd as u8);
    let w = ir.new_const(Type::I64, width);
    let d = ir.new_temp(Type::I64);
    ir.gen_call(
        d,
        helpers::loongarch_helper_iocsrwr as *const () as u64,
        &[env_tmp, addr, val, w],
    );
    true
}

fn decode_insn(
    ctx: &mut LoongArchDisasContext,
    ir: &mut Context,
    insn: u32,
) -> bool {
    insn_decode::decode(ctx, ir, insn)
}
