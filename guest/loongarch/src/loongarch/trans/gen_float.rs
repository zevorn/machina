#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use machina_accel::ir::tb::EXCP_UNDEF;
use machina_accel::ir::{Cond, Context, TempIdx, Type};

use super::helpers;
use super::LoongArchDisasContext;

use super::super::cpu::{FCC_OFFSET, FPR_OFFSET};

pub fn fpr_get(
    ctx: &LoongArchDisasContext,
    ir: &mut Context,
    reg: u8,
) -> TempIdx {
    let d = ir.new_temp(Type::I64);
    ir.gen_ld(
        Type::I64,
        d,
        ctx.env,
        i64::try_from(FPR_OFFSET + usize::from(reg) * 8).unwrap(),
    );
    d
}

pub fn fpr_set(
    ctx: &LoongArchDisasContext,
    ir: &mut Context,
    reg: u8,
    val: TempIdx,
) {
    ir.gen_st(
        Type::I64,
        val,
        ctx.env,
        i64::try_from(FPR_OFFSET + usize::from(reg) * 8).unwrap(),
    );
}

pub fn fcc_get(
    ctx: &LoongArchDisasContext,
    ir: &mut Context,
    idx: u8,
) -> TempIdx {
    let d = ir.new_temp(Type::I64);
    ir.gen_ld8u(
        Type::I64,
        d,
        ctx.env,
        i64::try_from(FCC_OFFSET + usize::from(idx)).unwrap(),
    );
    d
}

pub fn fcc_set(
    ctx: &LoongArchDisasContext,
    ir: &mut Context,
    idx: u8,
    val: TempIdx,
) {
    ir.gen_st8(
        Type::I64,
        val,
        ctx.env,
        i64::try_from(FCC_OFFSET + usize::from(idx)).unwrap(),
    );
}

pub fn check_fpe(ctx: &mut LoongArchDisasContext, ir: &mut Context) -> bool {
    let env_tmp = ctx.env;
    let pc_val = ir.new_const(Type::I64, ctx.base.pc_next - 4);
    ir.gen_mov(Type::I64, ctx.pc, pc_val);
    let chk = ir.new_temp(Type::I64);
    ir.gen_call(
        chk,
        helpers::loongarch_helper_check_fpe as *const () as u64,
        &[env_tmp],
    );
    let zero = ir.new_const(Type::I64, 0);
    let label_ok = ir.new_label();
    ir.gen_brcond(Type::I64, chk, zero, Cond::Eq, label_ok);
    ir.gen_mov(Type::I64, ctx.pc, chk);
    ir.gen_exit_tb(EXCP_UNDEF);
    ir.gen_set_label(label_ok);
    true
}

pub fn gen_fp_arith_s(
    ctx: &mut LoongArchDisasContext,
    ir: &mut Context,
    fd: u8,
    fj: u8,
    fk: u8,
    helper: unsafe extern "C" fn(u64, u64) -> u64,
) {
    let a = fpr_get(ctx, ir, fj);
    let b = fpr_get(ctx, ir, fk);
    let d = ir.new_temp(Type::I64);
    ir.gen_call(d, helper as *const () as u64, &[a, b]);
    fpr_set(ctx, ir, fd, d);
}

pub fn gen_fp_unary(
    ctx: &mut LoongArchDisasContext,
    ir: &mut Context,
    fd: u8,
    fj: u8,
    helper: unsafe extern "C" fn(u64) -> u64,
) {
    let a = fpr_get(ctx, ir, fj);
    let d = ir.new_temp(Type::I64);
    ir.gen_call(d, helper as *const () as u64, &[a]);
    fpr_set(ctx, ir, fd, d);
}

pub fn gen_fp_cmp(
    ctx: &mut LoongArchDisasContext,
    ir: &mut Context,
    cd: u8,
    fj: u8,
    fk: u8,
    helper: unsafe extern "C" fn(u64, u64) -> u64,
) {
    let a = fpr_get(ctx, ir, fj);
    let b = fpr_get(ctx, ir, fk);
    let d = ir.new_temp(Type::I64);
    ir.gen_call(d, helper as *const () as u64, &[a, b]);
    fcc_set(ctx, ir, cd, d);
}
