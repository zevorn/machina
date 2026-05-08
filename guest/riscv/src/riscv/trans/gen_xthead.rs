//! T-HEAD/XThead vendor instruction helpers.

use super::super::ext::MisaExt;
use super::super::RiscvDisasContext;
use super::helpers;
use machina_accel::ir::context::Context;
use machina_accel::ir::types::{Cond, MemOp, Type};
use machina_accel::ir::TempIdx;

#[derive(Clone, Copy)]
enum MacAcc {
    Add,
    Sub,
}

pub(super) fn decode_xthead(
    ctx: &mut RiscvDisasContext,
    ir: &mut Context,
    insn: u32,
) -> bool {
    if (insn & 0x7f) != 0x0b {
        return false;
    }

    let rd = field(insn, 7, 5) as i64;
    let funct3 = field(insn, 12, 3);
    let rs1 = field(insn, 15, 5) as i64;
    let rs2 = field(insn, 20, 5) as i64;
    let funct7 = field(insn, 25, 7);

    match funct3 {
        0b001 => {
            if ctx.cfg.ext_xtheadba && matches!(funct7, 0..=3) {
                return ctx.gen_xthead_addsl(ir, rd, rs1, rs2, funct7);
            }
            if ctx.cfg.ext_xtheadbb && (funct7 >> 1) == 0b000100 {
                return ctx.gen_xthead_srri(ir, rd, rs1, field(insn, 20, 6));
            }
            if ctx.cfg.ext_xtheadbb && funct7 == 0b0001010 {
                return ctx.gen_xthead_srriw(ir, rd, rs1, field(insn, 20, 5));
            }
            if ctx.cfg.ext_xtheadbs && (funct7 >> 1) == 0b100010 {
                return ctx.gen_xthead_tst(ir, rd, rs1, field(insn, 20, 6));
            }
            if ctx.cfg.ext_xtheadcondmov && funct7 == 0b0100000 {
                return ctx.gen_xthead_condmov(ir, rd, rs1, rs2, Cond::Eq);
            }
            if ctx.cfg.ext_xtheadcondmov && funct7 == 0b0100001 {
                return ctx.gen_xthead_condmov(ir, rd, rs1, rs2, Cond::Ne);
            }
            if ctx.cfg.ext_xtheadbb && rs2 == 0 {
                return match funct7 {
                    0b1000010 => ctx.gen_xthead_ff0(ir, rd, rs1),
                    0b1000011 => ctx.gen_xthead_ff1(ir, rd, rs1),
                    0b1000001 => ctx.gen_xthead_rev(ir, rd, rs1),
                    0b1001000 => ctx.gen_xthead_revw(ir, rd, rs1),
                    0b1000000 => ctx.gen_xthead_tstnbz(ir, rd, rs1),
                    _ => false,
                };
            }
            if ctx.cfg.ext_xtheadmac {
                return match funct7 {
                    0b0010000 => ctx.gen_xthead_mac(
                        ir,
                        rd,
                        rs1,
                        rs2,
                        MacAcc::Add,
                        false,
                        false,
                    ),
                    0b0010100 => ctx.gen_xthead_mac(
                        ir,
                        rd,
                        rs1,
                        rs2,
                        MacAcc::Add,
                        true,
                        true,
                    ),
                    0b0010010 => ctx.gen_xthead_mac(
                        ir,
                        rd,
                        rs1,
                        rs2,
                        MacAcc::Add,
                        true,
                        false,
                    ),
                    0b0010001 => ctx.gen_xthead_mac(
                        ir,
                        rd,
                        rs1,
                        rs2,
                        MacAcc::Sub,
                        false,
                        false,
                    ),
                    0b0010101 => ctx.gen_xthead_mac(
                        ir,
                        rd,
                        rs1,
                        rs2,
                        MacAcc::Sub,
                        true,
                        true,
                    ),
                    0b0010011 => ctx.gen_xthead_mac(
                        ir,
                        rd,
                        rs1,
                        rs2,
                        MacAcc::Sub,
                        true,
                        false,
                    ),
                    _ => false,
                };
            }
            false
        }
        0b010 | 0b011 => {
            if !ctx.cfg.ext_xtheadbb {
                return false;
            }
            let lsb = field(insn, 20, 6);
            let msb = field(insn, 26, 6);
            ctx.gen_xthead_extract(ir, rd, rs1, lsb, msb, funct3 == 0b010)
        }
        0b100 | 0b101 => {
            if !ctx.cfg.ext_xtheadmemidx && !ctx.cfg.ext_xtheadmempair {
                return false;
            }
            decode_xthead_mem(ctx, ir, insn, rd, rs1, rs2, funct3 == 0b100)
        }
        0b110 | 0b111 => {
            if !ctx.cfg.ext_xtheadfmemidx {
                return false;
            }
            decode_xthead_fmemidx(ctx, ir, insn, rd, rs1, rs2, funct3 == 0b110)
        }
        _ => false,
    }
}

fn decode_xthead_mem(
    ctx: &RiscvDisasContext,
    ir: &mut Context,
    insn: u32,
    rd: i64,
    rs1: i64,
    rs2: i64,
    is_load: bool,
) -> bool {
    let top5 = field(insn, 27, 5);
    let imm2 = field(insn, 25, 2);

    if ctx.cfg.ext_xtheadmempair {
        if let Some((memop, shamt)) = pair_memop(top5, is_load) {
            let rd2 = rs2;
            return if is_load {
                ctx.gen_xthead_load_pair(ir, rd, rd2, rs1, imm2, shamt, memop)
            } else {
                ctx.gen_xthead_store_pair(ir, rd, rd2, rs1, imm2, shamt, memop)
            };
        }
    }

    if !ctx.cfg.ext_xtheadmemidx {
        return false;
    }

    if let Some((memop, preinc)) = inc_memop(top5, is_load) {
        let imm5 = sign_extend(field(insn, 20, 5), 5);
        let imm = imm5 << imm2;
        return if is_load {
            ctx.gen_xthead_load_inc(ir, rd, rs1, imm, preinc, memop)
        } else {
            ctx.gen_xthead_store_inc(ir, rd, rs1, imm, preinc, memop)
        };
    }

    if let Some((memop, zext_offs)) = idx_memop(top5, is_load) {
        return if is_load {
            ctx.gen_xthead_load_idx(ir, rd, rs1, rs2, imm2, zext_offs, memop)
        } else {
            ctx.gen_xthead_store_idx(ir, rd, rs1, rs2, imm2, zext_offs, memop)
        };
    }

    false
}

fn decode_xthead_fmemidx(
    ctx: &RiscvDisasContext,
    ir: &mut Context,
    insn: u32,
    rd: i64,
    rs1: i64,
    rs2: i64,
    is_load: bool,
) -> bool {
    let top5 = field(insn, 27, 5);
    let imm2 = field(insn, 25, 2);

    if let Some((memop, zext_offs, is_single)) = fmemidx_memop(top5) {
        if is_single && !ctx.cfg.misa.contains(MisaExt::F) {
            return false;
        }
        if !is_single && !ctx.cfg.misa.contains(MisaExt::D) {
            return false;
        }
        return if is_load {
            ctx.gen_xthead_fload_idx(
                ir, rd, rs1, rs2, imm2, zext_offs, memop, is_single,
            )
        } else {
            ctx.gen_xthead_fstore_idx(
                ir, rd, rs1, rs2, imm2, zext_offs, memop, is_single,
            )
        };
    }

    false
}

fn inc_memop(top5: u32, is_load: bool) -> Option<(MemOp, bool)> {
    let preinc = matches!(top5, 13 | 9 | 25 | 5 | 21 | 1 | 17);
    let memop = match top5 {
        15 | 13 => MemOp::uq(),
        11 | 9 => MemOp::sl(),
        27 | 25 if is_load => MemOp::ul(),
        7 | 5 => MemOp::sw(),
        23 | 21 if is_load => MemOp::uw(),
        3 | 1 => MemOp::sb(),
        19 | 17 if is_load => MemOp::ub(),
        _ => return None,
    };
    Some((memop, preinc))
}

fn idx_memop(top5: u32, is_load: bool) -> Option<(MemOp, bool)> {
    let zext_offs = matches!(top5, 14 | 10 | 26 | 6 | 22 | 2 | 18);
    let memop = match top5 {
        12 | 14 => MemOp::uq(),
        8 | 10 => MemOp::sl(),
        24 | 26 if is_load => MemOp::ul(),
        4 | 6 => MemOp::sw(),
        20 | 22 if is_load => MemOp::uw(),
        0 | 2 => MemOp::sb(),
        16 | 18 if is_load => MemOp::ub(),
        _ => return None,
    };
    Some((memop, zext_offs))
}

fn pair_memop(top5: u32, is_load: bool) -> Option<(MemOp, u32)> {
    match top5 {
        31 => Some((MemOp::uq(), 4)),
        28 => Some((MemOp::sl(), 3)),
        30 if is_load => Some((MemOp::ul(), 3)),
        _ => None,
    }
}

fn fmemidx_memop(top5: u32) -> Option<(MemOp, bool, bool)> {
    match top5 {
        12 => Some((MemOp::uq(), false, false)),
        8 => Some((MemOp::ul(), false, true)),
        14 => Some((MemOp::uq(), true, false)),
        10 => Some((MemOp::ul(), true, true)),
        _ => None,
    }
}

fn field(insn: u32, shift: u32, bits: u32) -> u32 {
    (insn >> shift) & ((1u32 << bits) - 1)
}

fn sign_extend(value: u32, bits: u32) -> i64 {
    let shift = 64 - bits;
    ((u64::from(value) << shift) as i64) >> shift
}

fn sx32(ir: &mut Context, src: TempIdx) -> TempIdx {
    let lo = ir.new_temp(Type::I32);
    ir.gen_extrl_i64_i32(lo, src);
    let dst = ir.new_temp(Type::I64);
    ir.gen_ext_i32_i64(dst, lo);
    dst
}

impl RiscvDisasContext {
    fn gen_xthead_addsl(
        &self,
        ir: &mut Context,
        rd: i64,
        rs1: i64,
        rs2: i64,
        shift: u32,
    ) -> bool {
        let s1 = self.gpr_or_zero(ir, rs1);
        let s2 = self.gpr_or_zero(ir, rs2);
        let sh = ir.new_const(Type::I64, u64::from(shift));
        let shifted = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, shifted, s2, sh);
        let dst = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, dst, s1, shifted);
        self.gen_set_gpr(ir, rd, dst);
        true
    }

    fn gen_xthead_srri(
        &self,
        ir: &mut Context,
        rd: i64,
        rs1: i64,
        shamt: u32,
    ) -> bool {
        let src = self.gpr_or_zero(ir, rs1);
        let sh = ir.new_const(Type::I64, u64::from(shamt));
        let dst = ir.new_temp(Type::I64);
        ir.gen_rotr(Type::I64, dst, src, sh);
        self.gen_set_gpr(ir, rd, dst);
        true
    }

    fn gen_xthead_srriw(
        &self,
        ir: &mut Context,
        rd: i64,
        rs1: i64,
        shamt: u32,
    ) -> bool {
        let src = self.gpr_or_zero(ir, rs1);
        let src32 = ir.new_temp(Type::I32);
        ir.gen_extrl_i64_i32(src32, src);
        let sh = ir.new_const(Type::I32, u64::from(shamt));
        let dst32 = ir.new_temp(Type::I32);
        ir.gen_rotr(Type::I32, dst32, src32, sh);
        self.gen_set_gpr_sx32(ir, rd, dst32);
        true
    }

    fn gen_xthead_tst(
        &self,
        ir: &mut Context,
        rd: i64,
        rs1: i64,
        shamt: u32,
    ) -> bool {
        let src = self.gpr_or_zero(ir, rs1);
        let sh = ir.new_const(Type::I64, u64::from(shamt));
        let shifted = ir.new_temp(Type::I64);
        ir.gen_shr(Type::I64, shifted, src, sh);
        let one = ir.new_const(Type::I64, 1);
        let dst = ir.new_temp(Type::I64);
        ir.gen_and(Type::I64, dst, shifted, one);
        self.gen_set_gpr(ir, rd, dst);
        true
    }

    fn gen_xthead_extract(
        &self,
        ir: &mut Context,
        rd: i64,
        rs1: i64,
        lsb: u32,
        msb: u32,
        signed: bool,
    ) -> bool {
        if lsb > msb {
            return true;
        }
        let src = self.gpr_or_zero(ir, rs1);
        let dst = ir.new_temp(Type::I64);
        let len = msb - lsb + 1;
        if signed {
            let left = ir.new_const(Type::I64, u64::from(63 - msb));
            let shifted = ir.new_temp(Type::I64);
            ir.gen_shl(Type::I64, shifted, src, left);
            let right = ir.new_const(Type::I64, u64::from(64 - len));
            ir.gen_sar(Type::I64, dst, shifted, right);
        } else {
            let shifted = if lsb == 0 {
                src
            } else {
                let right = ir.new_const(Type::I64, u64::from(lsb));
                let tmp = ir.new_temp(Type::I64);
                ir.gen_shr(Type::I64, tmp, src, right);
                tmp
            };
            if len == 64 {
                ir.gen_mov(Type::I64, dst, shifted);
            } else {
                let mask = ir.new_const(Type::I64, (1u64 << len) - 1);
                ir.gen_and(Type::I64, dst, shifted, mask);
            }
        }
        self.gen_set_gpr(ir, rd, dst);
        true
    }

    fn gen_xthead_ff0(&self, ir: &mut Context, rd: i64, rs1: i64) -> bool {
        let src = self.gpr_or_zero(ir, rs1);
        let inv = ir.new_temp(Type::I64);
        ir.gen_not(Type::I64, inv, src);
        let fallback = ir.new_const(Type::I64, 64);
        let dst = ir.new_temp(Type::I64);
        ir.gen_clz(Type::I64, dst, inv, fallback);
        self.gen_set_gpr(ir, rd, dst);
        true
    }

    fn gen_xthead_ff1(&self, ir: &mut Context, rd: i64, rs1: i64) -> bool {
        let src = self.gpr_or_zero(ir, rs1);
        let fallback = ir.new_const(Type::I64, 64);
        let dst = ir.new_temp(Type::I64);
        ir.gen_clz(Type::I64, dst, src, fallback);
        self.gen_set_gpr(ir, rd, dst);
        true
    }

    fn gen_xthead_rev(&self, ir: &mut Context, rd: i64, rs1: i64) -> bool {
        let src = self.gpr_or_zero(ir, rs1);
        let dst = ir.new_temp(Type::I64);
        ir.gen_bswap64(Type::I64, dst, src, 0);
        self.gen_set_gpr(ir, rd, dst);
        true
    }

    fn gen_xthead_revw(&self, ir: &mut Context, rd: i64, rs1: i64) -> bool {
        let src = self.gpr_or_zero(ir, rs1);
        let dst = ir.new_temp(Type::I64);
        ir.gen_bswap32(Type::I64, dst, src, 4);
        self.gen_set_gpr(ir, rd, dst);
        true
    }

    fn gen_xthead_tstnbz(&self, ir: &mut Context, rd: i64, rs1: i64) -> bool {
        let src = self.gpr_or_zero(ir, rs1);
        let orc = ir.new_temp(Type::I64);
        ir.gen_call(orc, helpers::helper_orc_b as *const () as u64, &[src]);
        let dst = ir.new_temp(Type::I64);
        ir.gen_not(Type::I64, dst, orc);
        self.gen_set_gpr(ir, rd, dst);
        true
    }

    fn gen_xthead_condmov(
        &self,
        ir: &mut Context,
        rd: i64,
        rs1: i64,
        rs2: i64,
        cond: Cond,
    ) -> bool {
        let src1 = self.gpr_or_zero(ir, rs1);
        let src2 = self.gpr_or_zero(ir, rs2);
        let old = self.gpr_or_zero(ir, rd);
        let zero = ir.new_const(Type::I64, 0);
        let dst = ir.new_temp(Type::I64);
        ir.gen_movcond(Type::I64, dst, src2, zero, src1, old, cond);
        self.gen_set_gpr(ir, rd, dst);
        true
    }

    fn gen_xthead_mac(
        &self,
        ir: &mut Context,
        rd: i64,
        rs1: i64,
        rs2: i64,
        acc: MacAcc,
        word_result: bool,
        half_operands: bool,
    ) -> bool {
        let old = self.gpr_or_zero(ir, rd);
        let s1 = self.gpr_or_zero(ir, rs1);
        let s2 = self.gpr_or_zero(ir, rs2);

        let product = if half_operands {
            let lhs = self.sx16(ir, s1);
            let rhs = self.sx16(ir, s2);
            let tmp = ir.new_temp(Type::I64);
            ir.gen_mul(Type::I64, tmp, lhs, rhs);
            tmp
        } else if word_result {
            let lhs = ir.new_temp(Type::I32);
            let rhs = ir.new_temp(Type::I32);
            ir.gen_extrl_i64_i32(lhs, s1);
            ir.gen_extrl_i64_i32(rhs, s2);
            let tmp32 = ir.new_temp(Type::I32);
            ir.gen_mul(Type::I32, tmp32, lhs, rhs);
            let tmp64 = ir.new_temp(Type::I64);
            ir.gen_ext_i32_i64(tmp64, tmp32);
            tmp64
        } else {
            let tmp = ir.new_temp(Type::I64);
            ir.gen_mul(Type::I64, tmp, s1, s2);
            tmp
        };

        let acc_val = match acc {
            MacAcc::Add => {
                let tmp = ir.new_temp(Type::I64);
                ir.gen_add(Type::I64, tmp, old, product);
                tmp
            }
            MacAcc::Sub => {
                let tmp = ir.new_temp(Type::I64);
                ir.gen_sub(Type::I64, tmp, old, product);
                tmp
            }
        };

        let dst = if word_result {
            sx32(ir, acc_val)
        } else {
            acc_val
        };
        self.gen_set_gpr(ir, rd, dst);
        true
    }

    fn sx16(&self, ir: &mut Context, src: TempIdx) -> TempIdx {
        let sh = ir.new_const(Type::I64, 48);
        let shifted = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, shifted, src, sh);
        let dst = ir.new_temp(Type::I64);
        ir.gen_sar(Type::I64, dst, shifted, sh);
        dst
    }

    fn gen_xthead_load_inc(
        &self,
        ir: &mut Context,
        rd: i64,
        rs1: i64,
        imm: i64,
        preinc: bool,
        memop: MemOp,
    ) -> bool {
        if rd == rs1 {
            return false;
        }
        let base = self.gpr_or_zero(ir, rs1);
        let addr = self.addr_with_imm(ir, base, if preinc { imm } else { 0 });
        let new_base = self.addr_with_imm(ir, base, imm);
        self.sync_pc(ir);
        let dst = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, dst, addr, memop.bits() as u32);
        self.gen_set_gpr(ir, rd, dst);
        self.gen_set_gpr(ir, rs1, new_base);
        true
    }

    fn gen_xthead_store_inc(
        &self,
        ir: &mut Context,
        rd: i64,
        rs1: i64,
        imm: i64,
        preinc: bool,
        memop: MemOp,
    ) -> bool {
        let base = self.gpr_or_zero(ir, rs1);
        let addr = self.addr_with_imm(ir, base, if preinc { imm } else { 0 });
        let new_base = self.addr_with_imm(ir, base, imm);
        let data = self.gpr_or_zero(ir, rd);
        self.sync_pc(ir);
        ir.gen_qemu_st(Type::I64, data, addr, memop.bits() as u32);
        self.gen_set_gpr(ir, rs1, new_base);
        true
    }

    fn gen_xthead_load_idx(
        &self,
        ir: &mut Context,
        rd: i64,
        rs1: i64,
        rs2: i64,
        imm2: u32,
        zext_offs: bool,
        memop: MemOp,
    ) -> bool {
        let addr = self.xthead_indexed_addr(ir, rs1, rs2, imm2, zext_offs);
        self.sync_pc(ir);
        let dst = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, dst, addr, memop.bits() as u32);
        self.gen_set_gpr(ir, rd, dst);
        true
    }

    fn gen_xthead_store_idx(
        &self,
        ir: &mut Context,
        rd: i64,
        rs1: i64,
        rs2: i64,
        imm2: u32,
        zext_offs: bool,
        memop: MemOp,
    ) -> bool {
        let addr = self.xthead_indexed_addr(ir, rs1, rs2, imm2, zext_offs);
        let data = self.gpr_or_zero(ir, rd);
        self.sync_pc(ir);
        ir.gen_qemu_st(Type::I64, data, addr, memop.bits() as u32);
        true
    }

    fn gen_xthead_fload_idx(
        &self,
        ir: &mut Context,
        rd: i64,
        rs1: i64,
        rs2: i64,
        imm2: u32,
        zext_offs: bool,
        memop: MemOp,
        is_single: bool,
    ) -> bool {
        self.gen_fp_check(ir);
        self.gen_set_fs_dirty(ir);
        let addr = self.xthead_indexed_addr(ir, rs1, rs2, imm2, zext_offs);
        self.sync_pc(ir);
        let val = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, val, addr, memop.bits() as u32);
        if is_single {
            let mask = ir.new_const(Type::I64, 0xffff_ffff_0000_0000u64);
            let boxed = ir.new_temp(Type::I64);
            ir.gen_or(Type::I64, boxed, val, mask);
            self.fpr_store(ir, rd, boxed);
        } else {
            self.fpr_store(ir, rd, val);
        }
        true
    }

    fn gen_xthead_fstore_idx(
        &self,
        ir: &mut Context,
        rd: i64,
        rs1: i64,
        rs2: i64,
        imm2: u32,
        zext_offs: bool,
        memop: MemOp,
        is_single: bool,
    ) -> bool {
        self.gen_fp_check(ir);
        let addr = self.xthead_indexed_addr(ir, rs1, rs2, imm2, zext_offs);
        let val = self.fpr_load(ir, rd);
        let store_val = if is_single {
            let lo32 = ir.new_temp(Type::I32);
            ir.gen_extrl_i64_i32(lo32, val);
            let lo64 = ir.new_temp(Type::I64);
            ir.gen_ext_u32_i64(lo64, lo32);
            lo64
        } else {
            val
        };
        self.sync_pc(ir);
        ir.gen_qemu_st(Type::I64, store_val, addr, memop.bits() as u32);
        true
    }

    fn gen_xthead_load_pair(
        &self,
        ir: &mut Context,
        rd1: i64,
        rd2: i64,
        rs: i64,
        sh2: u32,
        shamt: u32,
        memop: MemOp,
    ) -> bool {
        if rs == rd1 || rs == rd2 || rd1 == rd2 {
            return false;
        }
        let imm = i64::from(sh2 << shamt);
        let base = self.gpr_or_zero(ir, rs);
        let addr1 = self.addr_with_imm(ir, base, imm);
        let addr2 =
            self.addr_with_imm(ir, base, imm + i64::from(memop.size_bytes()));
        self.sync_pc(ir);
        let val1 = ir.new_temp(Type::I64);
        let val2 = ir.new_temp(Type::I64);
        ir.gen_qemu_ld(Type::I64, val1, addr1, memop.bits() as u32);
        ir.gen_qemu_ld(Type::I64, val2, addr2, memop.bits() as u32);
        self.gen_set_gpr(ir, rd1, val1);
        self.gen_set_gpr(ir, rd2, val2);
        true
    }

    fn gen_xthead_store_pair(
        &self,
        ir: &mut Context,
        rd1: i64,
        rd2: i64,
        rs: i64,
        sh2: u32,
        shamt: u32,
        memop: MemOp,
    ) -> bool {
        let imm = i64::from(sh2 << shamt);
        let base = self.gpr_or_zero(ir, rs);
        let addr1 = self.addr_with_imm(ir, base, imm);
        let addr2 =
            self.addr_with_imm(ir, base, imm + i64::from(memop.size_bytes()));
        let val1 = self.gpr_or_zero(ir, rd1);
        let val2 = self.gpr_or_zero(ir, rd2);
        self.sync_pc(ir);
        ir.gen_qemu_st(Type::I64, val1, addr1, memop.bits() as u32);
        ir.gen_qemu_st(Type::I64, val2, addr2, memop.bits() as u32);
        true
    }

    fn xthead_indexed_addr(
        &self,
        ir: &mut Context,
        rs1: i64,
        rs2: i64,
        imm2: u32,
        zext_offs: bool,
    ) -> TempIdx {
        let base = self.gpr_or_zero(ir, rs1);
        let src = self.gpr_or_zero(ir, rs2);
        let offs_src = if zext_offs {
            let lo = ir.new_temp(Type::I32);
            ir.gen_extrl_i64_i32(lo, src);
            let ext = ir.new_temp(Type::I64);
            ir.gen_ext_u32_i64(ext, lo);
            ext
        } else {
            src
        };
        let sh = ir.new_const(Type::I64, u64::from(imm2));
        let offs = ir.new_temp(Type::I64);
        ir.gen_shl(Type::I64, offs, offs_src, sh);
        let addr = ir.new_temp(Type::I64);
        ir.gen_add(Type::I64, addr, base, offs);
        addr
    }
}
