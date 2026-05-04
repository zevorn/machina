use machina_accel::ir::{Context, TempIdx, Type};

use crate::loongarch::cpu::NUM_GPRS;

#[must_use]
pub fn gpr_get(
    gpr: &[TempIdx; NUM_GPRS],
    ir: &mut Context,
    reg: u8,
) -> TempIdx {
    if reg == 0 {
        ir.new_const(Type::I64, 0)
    } else {
        gpr[reg as usize]
    }
}

pub fn gpr_set(
    gpr: &[TempIdx; NUM_GPRS],
    ir: &mut Context,
    reg: u8,
    val: TempIdx,
) {
    if reg != 0 {
        ir.gen_mov(Type::I64, gpr[reg as usize], val);
    }
}
