//! Zbs (Single-Bit Operations) gen helpers.

use super::super::insn_decode::*;
use super::super::RiscvDisasContext;
use machina_accel::ir::context::Context;
use machina_accel::ir::types::Type;

impl RiscvDisasContext {
    // bclr: rd = rs1 & ~(1 << (rs2 & 63))

    pub(super) fn gen_bclr(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let one = ir.new_const(Type::I64, 1);
        let bit = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, bit, one, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_andc(Type::I64, d, s1, bit);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    // bclri: rd = rs1 & ~(1 << shamt)

    pub(super) fn gen_bclri(&self, ir: &mut Context, a: &ArgsShift) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let sh = ir.new_const(Type::I64, a.shamt as u64);
        let one = ir.new_const(Type::I64, 1);
        let bit = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, bit, one, sh);
        let d = ir.new_temp(Type::I64);
        ir.gen_andc(Type::I64, d, s1, bit);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    // bext: rd = (rs1 >> (rs2 & 63)) & 1

    pub(super) fn gen_bext(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let t = ir.new_temp(Type::I64);
        ir.gen_shr(Type::I64, t, s1, s2);
        let one = ir.new_const(Type::I64, 1);
        let d = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, d, t, one);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    // bexti: rd = (rs1 >> shamt) & 1

    pub(super) fn gen_bexti(&self, ir: &mut Context, a: &ArgsShift) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let sh = ir.new_const(Type::I64, a.shamt as u64);
        let t = ir.new_temp(Type::I64);
        ir.gen_shr(Type::I64, t, s1, sh);
        let one = ir.new_const(Type::I64, 1);
        let d = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, d, t, one);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    // binv: rd = rs1 ^ (1 << (rs2 & 63))

    pub(super) fn gen_binv(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let one = ir.new_const(Type::I64, 1);
        let bit = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, bit, one, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_xor(Type::I64, d, s1, bit);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    // binvi: rd = rs1 ^ (1 << shamt)

    pub(super) fn gen_binvi(&self, ir: &mut Context, a: &ArgsShift) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let sh = ir.new_const(Type::I64, a.shamt as u64);
        let one = ir.new_const(Type::I64, 1);
        let bit = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, bit, one, sh);
        let d = ir.new_temp(Type::I64);
        ir.gen_xor(Type::I64, d, s1, bit);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    // bset: rd = rs1 | (1 << (rs2 & 63))

    pub(super) fn gen_bset(&self, ir: &mut Context, a: &ArgsR) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let s2 = self.gpr_or_zero(ir, a.rs2);
        let one = ir.new_const(Type::I64, 1);
        let bit = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, bit, one, s2);
        let d = ir.new_temp(Type::I64);
        ir.gen_or(Type::I64, d, s1, bit);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }

    // bseti: rd = rs1 | (1 << shamt)

    pub(super) fn gen_bseti(&self, ir: &mut Context, a: &ArgsShift) -> bool {
        let s1 = self.gpr_or_zero(ir, a.rs1);
        let sh = ir.new_const(Type::I64, a.shamt as u64);
        let one = ir.new_const(Type::I64, 1);
        let bit = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, bit, one, sh);
        let d = ir.new_temp(Type::I64);
        ir.gen_or(Type::I64, d, s1, bit);
        self.gen_set_gpr(ir, a.rd, d);
        true
    }
}
