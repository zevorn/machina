#![allow(dead_code)]

pub mod loongarch;

use machina_accel::ir::Context;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisasJumpType {
    Next,
    TooMany,
    NoReturn,
}

pub struct DisasContextBase {
    pub pc_first: u64,
    pub pc_next: u64,
    pub is_jmp: DisasJumpType,
    pub num_insns: u32,
    pub max_insns: u32,
}

pub trait TranslatorOps {
    type DisasContext;

    fn init_disas_context(ctx: &mut Self::DisasContext, ir: &mut Context);
    fn tb_start(ctx: &mut Self::DisasContext, ir: &mut Context);
    fn insn_start(ctx: &mut Self::DisasContext, ir: &mut Context);
    fn translate_insn(ctx: &mut Self::DisasContext, ir: &mut Context);
    fn tb_stop(ctx: &mut Self::DisasContext, ir: &mut Context);
    fn base(ctx: &Self::DisasContext) -> &DisasContextBase;
    fn base_mut(ctx: &mut Self::DisasContext) -> &mut DisasContextBase;
}

pub fn translator_loop<T: TranslatorOps>(
    ctx: &mut T::DisasContext,
    ir: &mut Context,
) {
    T::init_disas_context(ctx, ir);
    T::tb_start(ctx, ir);

    loop {
        T::insn_start(ctx, ir);
        T::translate_insn(ctx, ir);

        let base = T::base(ctx);
        if base.is_jmp != DisasJumpType::Next {
            break;
        }
        if base.num_insns >= base.max_insns {
            T::base_mut(ctx).is_jmp = DisasJumpType::TooMany;
            break;
        }
    }

    T::tb_stop(ctx, ir);
}
