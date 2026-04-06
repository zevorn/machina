// Bitmanip (Zba/Zbb/Zbs/Zbc) disassembly tests.

use machina_disas::riscv::print_insn_riscv64;

// R-type: funct7 | rs2 | rs1 | funct3 | rd | opcode
fn rtype(f7: u32, rs2: u32, rs1: u32, f3: u32, rd: u32, op: u32) -> u32 {
    (f7 << 25) | (rs2 << 20) | (rs1 << 15) | (f3 << 12) | (rd << 7) | op
}

// I-type shift (6-bit shamt): funct6 | shamt | rs1 | funct3 | rd | op
fn ishift6(f6: u32, shamt: u32, rs1: u32, f3: u32, rd: u32, op: u32) -> u32 {
    (f6 << 26) | (shamt << 20) | (rs1 << 15) | (f3 << 12) | (rd << 7) | op
}

// I-type shift (5-bit shamt): funct7 | shamt | rs1 | funct3 | rd | op
fn ishift5(f7: u32, shamt: u32, rs1: u32, f3: u32, rd: u32, op: u32) -> u32 {
    (f7 << 25) | (shamt << 20) | (rs1 << 15) | (f3 << 12) | (rd << 7) | op
}

fn dis(insn: u32) -> String {
    let bytes = insn.to_le_bytes();
    let (text, len) = print_insn_riscv64(0, &bytes);
    assert_eq!(len, 4);
    text
}

// rd=a0(10), rs1=a1(11), rs2=a2(12)
const RD: u32 = 10;
const RS1: u32 = 11;
const RS2: u32 = 12;
const OP: u32 = 0x33;
const OP32: u32 = 0x3b;
const OP_IMM: u32 = 0x13;
const OP_IMM32: u32 = 0x1b;

// ===== Zba (OP) =====

#[test]
fn test_sh1add() {
    let insn = rtype(0x10, RS2, RS1, 2, RD, OP);
    assert_eq!(dis(insn), "sh1add a0, a1, a2");
}

#[test]
fn test_sh2add() {
    let insn = rtype(0x10, RS2, RS1, 4, RD, OP);
    assert_eq!(dis(insn), "sh2add a0, a1, a2");
}

#[test]
fn test_sh3add() {
    let insn = rtype(0x10, RS2, RS1, 6, RD, OP);
    assert_eq!(dis(insn), "sh3add a0, a1, a2");
}

// ===== Zba (OP-32) =====

#[test]
fn test_add_uw() {
    let insn = rtype(0x04, RS2, RS1, 0, RD, OP32);
    assert_eq!(dis(insn), "add.uw a0, a1, a2");
}

#[test]
fn test_sh1add_uw() {
    let insn = rtype(0x10, RS2, RS1, 2, RD, OP32);
    assert_eq!(dis(insn), "sh1add.uw a0, a1, a2");
}

#[test]
fn test_sh2add_uw() {
    let insn = rtype(0x10, RS2, RS1, 4, RD, OP32);
    assert_eq!(dis(insn), "sh2add.uw a0, a1, a2");
}

#[test]
fn test_sh3add_uw() {
    let insn = rtype(0x10, RS2, RS1, 6, RD, OP32);
    assert_eq!(dis(insn), "sh3add.uw a0, a1, a2");
}

#[test]
fn test_slli_uw() {
    // funct6=0x02, shamt=3
    let insn = ishift6(0x02, 3, RS1, 1, RD, OP_IMM32);
    assert_eq!(dis(insn), "slli.uw a0, a1, 3");
}

// ===== Zbb (OP) =====

#[test]
fn test_andn() {
    let insn = rtype(0x20, RS2, RS1, 7, RD, OP);
    assert_eq!(dis(insn), "andn a0, a1, a2");
}

#[test]
fn test_orn() {
    let insn = rtype(0x20, RS2, RS1, 6, RD, OP);
    assert_eq!(dis(insn), "orn a0, a1, a2");
}

#[test]
fn test_xnor() {
    let insn = rtype(0x20, RS2, RS1, 4, RD, OP);
    assert_eq!(dis(insn), "xnor a0, a1, a2");
}

#[test]
fn test_max() {
    let insn = rtype(0x05, RS2, RS1, 6, RD, OP);
    assert_eq!(dis(insn), "max a0, a1, a2");
}

#[test]
fn test_maxu() {
    let insn = rtype(0x05, RS2, RS1, 7, RD, OP);
    assert_eq!(dis(insn), "maxu a0, a1, a2");
}

#[test]
fn test_min() {
    let insn = rtype(0x05, RS2, RS1, 4, RD, OP);
    assert_eq!(dis(insn), "min a0, a1, a2");
}

#[test]
fn test_minu() {
    let insn = rtype(0x05, RS2, RS1, 5, RD, OP);
    assert_eq!(dis(insn), "minu a0, a1, a2");
}

#[test]
fn test_rol() {
    let insn = rtype(0x30, RS2, RS1, 1, RD, OP);
    assert_eq!(dis(insn), "rol a0, a1, a2");
}

#[test]
fn test_ror() {
    let insn = rtype(0x30, RS2, RS1, 5, RD, OP);
    assert_eq!(dis(insn), "ror a0, a1, a2");
}

// ===== Zbb (OP-32) =====

#[test]
fn test_rolw() {
    let insn = rtype(0x30, RS2, RS1, 1, RD, OP32);
    assert_eq!(dis(insn), "rolw a0, a1, a2");
}

#[test]
fn test_rorw() {
    let insn = rtype(0x30, RS2, RS1, 5, RD, OP32);
    assert_eq!(dis(insn), "rorw a0, a1, a2");
}

#[test]
fn test_zext_h() {
    let insn = rtype(0x04, 0, RS1, 4, RD, OP32);
    assert_eq!(dis(insn), "zext.h a0, a1");
}

// ===== Zbb (OP-IMM) unary =====

#[test]
fn test_clz() {
    let insn = ishift5(0x30, 0, RS1, 1, RD, OP_IMM);
    assert_eq!(dis(insn), "clz a0, a1");
}

#[test]
fn test_ctz() {
    let insn = ishift5(0x30, 1, RS1, 1, RD, OP_IMM);
    assert_eq!(dis(insn), "ctz a0, a1");
}

#[test]
fn test_cpop() {
    let insn = ishift5(0x30, 2, RS1, 1, RD, OP_IMM);
    assert_eq!(dis(insn), "cpop a0, a1");
}

#[test]
fn test_sext_b() {
    let insn = ishift5(0x30, 4, RS1, 1, RD, OP_IMM);
    assert_eq!(dis(insn), "sext.b a0, a1");
}

#[test]
fn test_sext_h() {
    let insn = ishift5(0x30, 5, RS1, 1, RD, OP_IMM);
    assert_eq!(dis(insn), "sext.h a0, a1");
}

// ===== Zbb (OP-IMM) shifts =====

#[test]
fn test_rori() {
    // funct6=0x18, shamt=7
    let insn = ishift6(0x18, 7, RS1, 5, RD, OP_IMM);
    assert_eq!(dis(insn), "rori a0, a1, 7");
}

#[test]
fn test_rev8() {
    // funct6=0x1a, shamt=0x38
    let insn = ishift6(0x1a, 0x38, RS1, 5, RD, OP_IMM);
    assert_eq!(dis(insn), "rev8 a0, a1");
}

#[test]
fn test_orc_b() {
    // funct6=0x0a, shamt=0x07
    let insn = ishift6(0x0a, 0x07, RS1, 5, RD, OP_IMM);
    assert_eq!(dis(insn), "orc.b a0, a1");
}

// ===== Zbb (OP-IMM-32) unary =====

#[test]
fn test_clzw() {
    let insn = ishift5(0x30, 0, RS1, 1, RD, OP_IMM32);
    assert_eq!(dis(insn), "clzw a0, a1");
}

#[test]
fn test_ctzw() {
    let insn = ishift5(0x30, 1, RS1, 1, RD, OP_IMM32);
    assert_eq!(dis(insn), "ctzw a0, a1");
}

#[test]
fn test_cpopw() {
    let insn = ishift5(0x30, 2, RS1, 1, RD, OP_IMM32);
    assert_eq!(dis(insn), "cpopw a0, a1");
}

// ===== Zbb (OP-IMM-32) shifts =====

#[test]
fn test_roriw() {
    let insn = ishift5(0x30, 5, RS1, 5, RD, OP_IMM32);
    assert_eq!(dis(insn), "roriw a0, a1, 5");
}

// ===== Zbs (OP) =====

#[test]
fn test_bclr() {
    let insn = rtype(0x24, RS2, RS1, 1, RD, OP);
    assert_eq!(dis(insn), "bclr a0, a1, a2");
}

#[test]
fn test_bext() {
    let insn = rtype(0x24, RS2, RS1, 5, RD, OP);
    assert_eq!(dis(insn), "bext a0, a1, a2");
}

#[test]
fn test_binv() {
    let insn = rtype(0x34, RS2, RS1, 1, RD, OP);
    assert_eq!(dis(insn), "binv a0, a1, a2");
}

#[test]
fn test_bset() {
    let insn = rtype(0x14, RS2, RS1, 1, RD, OP);
    assert_eq!(dis(insn), "bset a0, a1, a2");
}

// ===== Zbs (OP-IMM) =====

#[test]
fn test_bclri() {
    // funct6=0x12, shamt=5
    let insn = ishift6(0x12, 5, RS1, 1, RD, OP_IMM);
    assert_eq!(dis(insn), "bclri a0, a1, 5");
}

#[test]
fn test_bexti() {
    // funct6=0x12, shamt=10
    let insn = ishift6(0x12, 10, RS1, 5, RD, OP_IMM);
    assert_eq!(dis(insn), "bexti a0, a1, 10");
}

#[test]
fn test_binvi() {
    // funct6=0x1a, shamt=3
    let insn = ishift6(0x1a, 3, RS1, 1, RD, OP_IMM);
    assert_eq!(dis(insn), "binvi a0, a1, 3");
}

#[test]
fn test_bseti() {
    // funct6=0x0a, shamt=15
    let insn = ishift6(0x0a, 15, RS1, 1, RD, OP_IMM);
    assert_eq!(dis(insn), "bseti a0, a1, 15");
}

// ===== Zbc (OP) =====

#[test]
fn test_clmul() {
    let insn = rtype(0x05, RS2, RS1, 1, RD, OP);
    assert_eq!(dis(insn), "clmul a0, a1, a2");
}

#[test]
fn test_clmulh() {
    let insn = rtype(0x05, RS2, RS1, 3, RD, OP);
    assert_eq!(dis(insn), "clmulh a0, a1, a2");
}

#[test]
fn test_clmulr() {
    let insn = rtype(0x05, RS2, RS1, 2, RD, OP);
    assert_eq!(dis(insn), "clmulr a0, a1, a2");
}
