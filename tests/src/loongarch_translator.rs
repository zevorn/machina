use machina_accel::ir::opcode::Opcode;
use machina_accel::ir::temp::{TempIdx, TempKind};
use machina_accel::ir::Context;
use machina_guest_loongarch::loongarch::cpu::{
    gpr_offset, NUM_GPRS, PC_OFFSET,
};
use machina_guest_loongarch::loongarch::ext::LoongArchCfg;
use machina_guest_loongarch::loongarch::trans::{
    LoongArchDisasContext, LoongArchTranslator,
};
use machina_guest_loongarch::{translator_loop, DisasJumpType, TranslatorOps};

const ADDI_D_NOP: u32 = 0b0000001011 << 22;

fn code_ptr(code: &[u32]) -> *const u8 {
    code.as_ptr().cast::<u8>()
}

#[test]
fn task81_translator_loop_registers_foundation_globals_before_locals() {
    let code = [ADDI_D_NOP];
    let mut ctx =
        LoongArchDisasContext::new(0, code_ptr(&code), LoongArchCfg::default());
    ctx.base.max_insns = 1;
    let mut ir = Context::new();

    translator_loop::<LoongArchTranslator>(&mut ctx, &mut ir);

    assert_eq!(ctx.base.pc_next, 4);
    assert_eq!(ctx.base.num_insns, 1);
    assert_eq!(ir.nb_globals(), LoongArchDisasContext::GLOBAL_COUNT);
    assert!(ir.nb_temps() > ir.nb_globals());

    assert_eq!(ctx.env, TempIdx(0));
    assert_eq!(ir.temp(ctx.env).kind, TempKind::Fixed);
    assert_eq!(ir.temp(ctx.env).name, Some("env"));
    assert_eq!(ir.temp(ctx.env).reg, Some(5));

    for i in 0..NUM_GPRS {
        let tmp = ctx.gpr[i];
        let temp = ir.temp(tmp);
        assert_eq!(tmp, TempIdx((1 + i) as u32));
        assert_eq!(temp.kind, TempKind::Global);
        assert_eq!(temp.mem_base, Some(ctx.env));
        assert_eq!(temp.mem_offset, i64::try_from(gpr_offset(i)).unwrap());
        assert_eq!(temp.name, Some("gpr"));
    }

    assert_eq!(ctx.pc, TempIdx(33));
    assert_eq!(
        ir.temp(ctx.pc).mem_offset,
        i64::try_from(PC_OFFSET).unwrap()
    );
    assert_eq!(ctx.llbctl, TempIdx(34));
    assert_eq!(ctx.ll_res_addr, TempIdx(35));
    assert_eq!(ctx.ll_res_val, TempIdx(36));

    for temp in ir.globals() {
        assert!(matches!(temp.kind, TempKind::Fixed | TempKind::Global));
    }
    for temp in &ir.temps()[ir.nb_globals() as usize..] {
        assert!(!matches!(temp.kind, TempKind::Fixed | TempKind::Global));
    }
}

#[test]
fn task81_translator_bind_existing_globals_matches_initialized_global_order() {
    let code = [ADDI_D_NOP];
    let mut initialized =
        LoongArchDisasContext::new(0, code_ptr(&code), LoongArchCfg::default());
    let mut ir = Context::new();
    LoongArchTranslator::init_disas_context(&mut initialized, &mut ir);

    let mut rebound =
        LoongArchDisasContext::new(0, code_ptr(&code), LoongArchCfg::default());
    rebound.bind_existing_globals(&ir);

    assert_eq!(rebound.env, initialized.env);
    assert_eq!(rebound.gpr, initialized.gpr);
    assert_eq!(rebound.pc, initialized.pc);
    assert_eq!(rebound.llbctl, initialized.llbctl);
    assert_eq!(rebound.ll_res_addr, initialized.ll_res_addr);
    assert_eq!(rebound.ll_res_val, initialized.ll_res_val);
}

#[test]
fn task81_translator_loop_stops_straight_line_tb_with_fallthrough_exit() {
    let code = [ADDI_D_NOP];
    let mut ctx =
        LoongArchDisasContext::new(0, code_ptr(&code), LoongArchCfg::default());
    ctx.base.max_insns = 1;
    let mut ir = Context::new();

    translator_loop::<LoongArchTranslator>(&mut ctx, &mut ir);

    assert_eq!(ctx.base.pc_next, 4);
    assert_eq!(ctx.base.is_jmp, DisasJumpType::TooMany);
    let ops = ir.ops();
    assert!(ops.len() >= 2);
    assert_eq!(ops[ops.len() - 2].opc, Opcode::GotoTb);
    assert_eq!(ops[ops.len() - 1].opc, Opcode::ExitTb);
}
