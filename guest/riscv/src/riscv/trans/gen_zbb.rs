//! Zbb (Basic Bit Manipulation) gen helpers.

use super::super::insn_decode::*;
use super::super::RiscvDisasContext;
use super::helpers::helper_orc_b;
use machina_accel::ir::context::Context;
use machina_accel::ir::types::{Cond, Type};

impl RiscvDisasContext {
    // -- Logical with NOT ------------------------------------

    pub(super) fn gen_andn(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let d = ir.new_temp(Type::I64);
        ir.gen_andc(Type::I64, d, s1, s2);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_orn(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let inv = ir.new_temp(Type::I64);
        ir.gen_not(Type::I64, inv, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_or(Type::I64, d, s1, inv);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_xnor(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let t = ir.new_temp(Type::I64);
        ir.gen_xor(Type::I64, t, s1, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_not(Type::I64, d, t);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    // -- Min / Max -------------------------------------------

    pub(super) fn gen_max(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let d = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, d, s1, s2, s1, s2, Cond::Ge);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_maxu(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let d = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, d, s1, s2, s1, s2, Cond::Geu);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_min(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let d = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, d, s1, s2, s1, s2, Cond::Lt);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_minu(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let d = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, d, s1, s2, s1, s2, Cond::Ltu);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    // -- Rotate (64-bit) -------------------------------------

    pub(super) fn gen_rol(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let d = ir.new_temp(Type::I64);
        ir.gen_rotl(Type::I64, d, s1, s2);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_ror(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let d = ir.new_temp(Type::I64);
        ir.gen_rotr(Type::I64, d, s1, s2);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_rori(&self, ir: &mut Context, a: &ArgsShift) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let sh = ir.new_const(Type::I64, a.shamt as u64);
        let d = ir.new_temp(Type::I64);
        ir.gen_rotr(Type::I64, d, s1, sh);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    // -- Rotate (32-bit, W-suffix) ---------------------------

    pub(super) fn gen_rolw(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let a32 = ir.new_temp(Type::I32);
        ir.gen_extrl_i64_i32(a32, s1);
        let b32 = ir.new_temp(Type::I32);
        ir.gen_extrl_i64_i32(b32, s2);
        let d32 = ir.new_temp(Type::I32);
        ir.gen_rotl(Type::I32, d32, a32, b32);
        self.gen_set_gpr_sx32(ir, a.rd, d32);
        true
    }

    pub(super) fn gen_rorw(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let a32 = ir.new_temp(Type::I32);
        ir.gen_extrl_i64_i32(a32, s1);
        let b32 = ir.new_temp(Type::I32);
        ir.gen_extrl_i64_i32(b32, s2);
        let d32 = ir.new_temp(Type::I32);
        ir.gen_rotr(Type::I32, d32, a32, b32);
        self.gen_set_gpr_sx32(ir, a.rd, d32);
        true
    }

    pub(super) fn gen_roriw(&self, ir: &mut Context, a: &ArgsShift) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s32 = ir.new_temp(Type::I32);
        ir.gen_extrl_i64_i32(s32, s1);
        let sh = ir.new_const(Type::I32, a.shamt as u64);
        let d32 = ir.new_temp(Type::I32);
        ir.gen_rotr(Type::I32, d32, s32, sh);
        self.gen_set_gpr_sx32(ir, a.rd, d32);
        true
    }

    // -- Count leading/trailing zeros, popcount (64-bit) -----

    pub(super) fn gen_clz(&self, ir: &mut Context, a: &ArgsR2) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let fallback = ir.new_const(Type::I64, 64);
        let d = ir.new_temp(Type::I64);
        ir.gen_clz(Type::I64, d, s1, fallback);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_ctz(&self, ir: &mut Context, a: &ArgsR2) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let fallback = ir.new_const(Type::I64, 64);
        let d = ir.new_temp(Type::I64);
        ir.gen_ctz(Type::I64, d, s1, fallback);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_cpop(&self, ir: &mut Context, a: &ArgsR2) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let d = ir.new_temp(Type::I64);
        ir.gen_ctpop(Type::I64, d, s1);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    // -- Count (32-bit, W-suffix) ----------------------------

    pub(super) fn gen_clzw(&self, ir: &mut Context, a: &ArgsR2) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s32 = ir.new_temp(Type::I32);
        ir.gen_extrl_i64_i32(s32, s1);
        let fallback = ir.new_const(Type::I32, 32);
        let d32 = ir.new_temp(Type::I32);
        ir.gen_clz(Type::I32, d32, s32, fallback);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_u32_i64(d, d32);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_ctzw(&self, ir: &mut Context, a: &ArgsR2) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s32 = ir.new_temp(Type::I32);
        ir.gen_extrl_i64_i32(s32, s1);
        let fallback = ir.new_const(Type::I32, 32);
        let d32 = ir.new_temp(Type::I32);
        ir.gen_ctz(Type::I32, d32, s32, fallback);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_u32_i64(d, d32);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_cpopw(&self, ir: &mut Context, a: &ArgsR2) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s32 = ir.new_temp(Type::I32);
        ir.gen_extrl_i64_i32(s32, s1);
        let d32 = ir.new_temp(Type::I32);
        ir.gen_ctpop(Type::I32, d32, s32);
        let d = ir.new_temp(Type::I64);
        ir.gen_ext_u32_i64(d, d32);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    // -- Sign/zero extension ---------------------------------

    pub(super) fn gen_sext_b(&self, ir: &mut Context, a: &ArgsR2) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let c56 = ir.new_const(Type::I64, 56);
        let t = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, t, s1, c56);
        let d = ir.new_temp(Type::I64);
        ir.gen_sar(Type::I64, d, t, c56);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_sext_h(&self, ir: &mut Context, a: &ArgsR2) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let c48 = ir.new_const(Type::I64, 48);
        let t = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, t, s1, c48);
        let d = ir.new_temp(Type::I64);
        ir.gen_sar(Type::I64, d, t, c48);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_zext_h(&self, ir: &mut Context, a: &ArgsR2) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let mask = ir.new_const(Type::I64, 0xFFFF);
        let d = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, d, s1, mask);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    // -- Byte reverse / OR-combine ---------------------------

    pub(super) fn gen_rev8(&self, ir: &mut Context, a: &ArgsR2) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let d = ir.new_temp(Type::I64);
        ir.gen_bswap64(Type::I64, d, s1, 0);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    pub(super) fn gen_orc_b(&self, ir: &mut Context, a: &ArgsR2) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let r = ir.new_temp(Type::I64);
        ir.gen_call(r, helper_orc_b as *const () as u64, &[s1]);
        self.gen_set_gpr(ir, a.rd, r);
        true
    }
}
