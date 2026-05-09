//! RVA gen helpers: atomic load-reserved / store-conditional,
//! AMO read-modify-write, swap, min/max.

use super::super::insn_decode::*;
use super::super::RiscvDisasContext;
use super::gen_common::{TCG_BAR_LDAQ, TCG_BAR_STRL, TCG_MO_ALL};
use super::helpers::helper_sc;
use machina_accel::ir::context::Context;
use machina_accel::ir::types::{Cond, MemOp, Type};

pub(super) const AMO_SWAP: u64 = 0;
pub(super) const AMO_ADD: u64 = 1;
pub(super) const AMO_XOR: u64 = 2;
pub(super) const AMO_AND: u64 = 3;
pub(super) const AMO_OR: u64 = 4;
pub(super) const AMO_MIN: u64 = 5;
pub(super) const AMO_MAX: u64 = 6;
pub(super) const AMO_MINU: u64 = 7;
pub(super) const AMO_MAXU: u64 = 8;

impl RiscvDisasContext {
    /// LR: load-reserved.
    pub(super) fn gen_lr(
        &self,
        ir: &mut Context,
        a: &ArgsAtomic,
        memop: MemOp,
    ) -> bool {
        ir.contains_atomic = true;
        let addr = self.gpr_or_zero(ir, a.rs1);
        if a.rl != 0 {
            ir.gen_mb(TCG_MO_ALL | TCG_BAR_STRL);
        }
        let val = ir.new_temp(Type::I64);
        if self.lr_helper != 0 {
            let pc = ir.new_const(Type::I64, self.base.pc_next);
            ir.gen_mov(Type::I64, self.pc, pc);
            let size = ir.new_const(Type::I64, memop.size_bytes() as u64);
            ir.gen_call(val, self.lr_helper, &[self.env, addr, size]);
        } else {
            self.sync_pc(ir);
            ir.gen_qemu_ld(Type::I64, val, addr, memop.bits() as u32);
            ir.gen_mov(Type::I64, self.load_res, addr);
            ir.gen_mov(Type::I64, self.load_val, val);
        }
        if a.aq != 0 {
            ir.gen_mb(TCG_MO_ALL | TCG_BAR_LDAQ);
        }
        self.gen_set_gpr(ir, a.rd, val);
        true
    }

    /// SC: store-conditional via helper.
    ///
    /// Uses a helper function to atomically check
    /// reservation and conditionally store. Returns
    /// 0 on success, 1 on failure.
    pub(super) fn gen_sc(
        &self,
        ir: &mut Context,
        a: &ArgsAtomic,
        memop: MemOp,
    ) -> bool {
        ir.contains_atomic = true;
        if a.rl != 0 {
            ir.gen_mb(TCG_MO_ALL | TCG_BAR_STRL);
        }
        let addr = self.gpr_or_zero(ir, a.rs1);
        let src2 = self.gpr_or_zero(ir, a.rs2);
        let pc = ir.new_const(Type::I64, self.base.pc_next);
        ir.gen_mov(Type::I64, self.pc, pc);
        let is_word = ir.new_const(Type::I64, memop.size_bytes() as u64);
        let r = ir.new_temp(Type::I64);
        let helper = if self.sc_helper != 0 {
            self.sc_helper
        } else {
            helper_sc as *const () as u64
        };
        ir.gen_call(r, helper, &[self.env, addr, src2, is_word]);
        if a.aq != 0 {
            ir.gen_mb(TCG_MO_ALL | TCG_BAR_LDAQ);
        }
        self.gen_set_gpr(ir, a.rd, r);
        true
    }

    /// AMO: atomic read-modify-write
    /// (single-thread: ld+op+st).
    pub(super) fn gen_amo(
        &self,
        ir: &mut Context,
        a: &ArgsAtomic,
        op: u64,
        memop: MemOp,
    ) -> bool {
        ir.contains_atomic = true;
        let addr = self.gpr_or_zero(ir, a.rs1);
        if a.rl != 0 {
            ir.gen_mb(TCG_MO_ALL | TCG_BAR_STRL);
        }
        if self.amo_helper != 0 {
            self.gen_amo_helper(ir, a, addr, op, memop);
            return true;
        }
        self.sync_pc(ir);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, memop.bits() as u32);
        let src2 = self.gpr_or_zero(ir, a.rs2);
        let new = ir.new_temp(Type::I64);
        match op {
            AMO_ADD => ir.gen_add(Type::I64, new, old, src2),
            AMO_XOR => ir.gen_xor(Type::I64, new, old, src2),
            AMO_AND => ir.gen_and(Type::I64, new, old, src2),
            AMO_OR => ir.gen_or(Type::I64, new, old, src2),
            _ => unreachable!("unsupported AMO op"),
        };
        ir.gen_qemu_st(Type::I64, new, addr, memop.bits() as u32);
        if a.aq != 0 {
            ir.gen_mb(TCG_MO_ALL | TCG_BAR_LDAQ);
        }
        self.gen_set_gpr(ir, a.rd, old);
        true
    }

    /// AMO swap: store rs2, return old value.
    pub(super) fn gen_amo_swap(
        &self,
        ir: &mut Context,
        a: &ArgsAtomic,
        memop: MemOp,
    ) -> bool {
        ir.contains_atomic = true;
        let addr = self.gpr_or_zero(ir, a.rs1);
        if a.rl != 0 {
            ir.gen_mb(TCG_MO_ALL | TCG_BAR_STRL);
        }
        if self.amo_helper != 0 {
            self.gen_amo_helper(ir, a, addr, AMO_SWAP, memop);
            return true;
        }
        self.sync_pc(ir);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, memop.bits() as u32);
        let src2 = self.gpr_or_zero(ir, a.rs2);
        ir.gen_qemu_st(Type::I64, src2, addr, memop.bits() as u32);
        if a.aq != 0 {
            ir.gen_mb(TCG_MO_ALL | TCG_BAR_LDAQ);
        }
        self.gen_set_gpr(ir, a.rd, old);
        true
    }

    /// AMO min/max: conditional select via movcond.
    pub(super) fn gen_amo_minmax(
        &self,
        ir: &mut Context,
        a: &ArgsAtomic,
        cond: Cond,
        memop: MemOp,
    ) -> bool {
        ir.contains_atomic = true;
        let addr = self.gpr_or_zero(ir, a.rs1);
        if a.rl != 0 {
            ir.gen_mb(TCG_MO_ALL | TCG_BAR_STRL);
        }
        if self.amo_helper != 0 {
            let op = match cond {
                Cond::Lt => AMO_MIN,
                Cond::Gt => AMO_MAX,
                Cond::Ltu => AMO_MINU,
                Cond::Gtu => AMO_MAXU,
                _ => unreachable!("unsupported AMO min/max condition"),
            };
            self.gen_amo_helper(ir, a, addr, op, memop);
            return true;
        }
        self.sync_pc(ir);
        let old = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, old, addr, memop.bits() as u32);
        let src2 = self.gpr_or_zero(ir, a.rs2);

        // For 32-bit AMO, truncate src2 to 32 bits and
        // extend to match the loaded value's width.
        let is_32 = memop.size_bytes() == 4;
        let cmp_src2 = if is_32 {
            let t = ir.new_temp(Type::I64);
            let t32 = ir.new_temp(Type::I32);
            ir.gen_extrl_i64_i32(t32, src2);
            // Signed cond: sign-extend; unsigned: zero.
            if cond == Cond::Lt || cond == Cond::Gt {
                ir.gen_ext_i32_i64(t, t32);
            } else {
                ir.gen_ext_u32_i64(t, t32);
            }
            t
        } else {
            src2
        };

        // For unsigned 32-bit cmp, also zero-extend old
        // (which was sign-extended by the load).
        let cmp_old = if is_32 && (cond == Cond::Ltu || cond == Cond::Gtu) {
            let t = ir.new_temp(Type::I64);
            let t32 = ir.new_temp(Type::I32);
            ir.gen_extrl_i64_i32(t32, old);
            ir.gen_ext_u32_i64(t, t32);
            t
        } else {
            old
        };

        let new = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, new, cmp_old, cmp_src2, old, src2, cond);
        ir.gen_qemu_st(Type::I64, new, addr, memop.bits() as u32);
        if a.aq != 0 {
            ir.gen_mb(TCG_MO_ALL | TCG_BAR_LDAQ);
        }
        self.gen_set_gpr(ir, a.rd, old);
        true
    }

    fn gen_amo_helper(
        &self,
        ir: &mut Context,
        a: &ArgsAtomic,
        addr: machina_accel::ir::TempIdx,
        op: u64,
        memop: MemOp,
    ) {
        let pc = ir.new_const(Type::I64, self.base.pc_next);
        ir.gen_mov(Type::I64, self.pc, pc);
        let src2 = self.gpr_or_zero(ir, a.rs2);
        let size = ir.new_const(Type::I64, memop.size_bytes() as u64);
        let op = ir.new_const(Type::I64, op);
        let old = ir.new_temp(Type::I64);
        ir.gen_call(old, self.amo_helper, &[self.env, addr, src2, size, op]);
        if a.aq != 0 {
            ir.gen_mb(TCG_MO_ALL | TCG_BAR_LDAQ);
        }
        self.gen_set_gpr(ir, a.rd, old);
    }
}
