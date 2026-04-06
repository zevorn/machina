//! Zbb (Basic Bit Manipulation) frontend tests.

use super::*;

// ── Zbb instruction encoders ─────────────────────────────

// R-type Zbb (opcode=OP=0b0110011)
fn andn(rd: u32, rs1: u32, rs2: u32) -> u32 {
    rv_r(0b0100000, rs2, rs1, 0b111, rd, OP_REG)
}
fn orn(rd: u32, rs1: u32, rs2: u32) -> u32 {
    rv_r(0b0100000, rs2, rs1, 0b110, rd, OP_REG)
}
fn xnor(rd: u32, rs1: u32, rs2: u32) -> u32 {
    rv_r(0b0100000, rs2, rs1, 0b100, rd, OP_REG)
}
fn max(rd: u32, rs1: u32, rs2: u32) -> u32 {
    rv_r(0b0000101, rs2, rs1, 0b110, rd, OP_REG)
}
fn maxu(rd: u32, rs1: u32, rs2: u32) -> u32 {
    rv_r(0b0000101, rs2, rs1, 0b111, rd, OP_REG)
}
fn min(rd: u32, rs1: u32, rs2: u32) -> u32 {
    rv_r(0b0000101, rs2, rs1, 0b100, rd, OP_REG)
}
fn minu(rd: u32, rs1: u32, rs2: u32) -> u32 {
    rv_r(0b0000101, rs2, rs1, 0b101, rd, OP_REG)
}
fn rol(rd: u32, rs1: u32, rs2: u32) -> u32 {
    rv_r(0b0110000, rs2, rs1, 0b001, rd, OP_REG)
}
fn ror(rd: u32, rs1: u32, rs2: u32) -> u32 {
    rv_r(0b0110000, rs2, rs1, 0b101, rd, OP_REG)
}

// R-type Zbb (opcode=OP-32=0b0111011)
fn rolw(rd: u32, rs1: u32, rs2: u32) -> u32 {
    rv_r(0b0110000, rs2, rs1, 0b001, rd, OP_REG32)
}
fn rorw(rd: u32, rs1: u32, rs2: u32) -> u32 {
    rv_r(0b0110000, rs2, rs1, 0b101, rd, OP_REG32)
}

// Shift-immediate Zbb
fn rori(rd: u32, rs1: u32, shamt: u32) -> u32 {
    // funct6=011000, shamt[5:0], rs1, funct3=101, rd, op=OP-IMM
    let imm = (0b011000 << 6) | (shamt & 0x3F);
    rv_i(imm as i32, rs1, 0b101, rd, OP_IMM)
}
fn roriw(rd: u32, rs1: u32, shamt: u32) -> u32 {
    rv_r(0b0110000, shamt & 0x1F, rs1, 0b101, rd, OP_IMM32)
}

// Unary Zbb (encoded as I-type with fixed rs2 field)
fn clz_rv(rd: u32, rs1: u32) -> u32 {
    rv_r(0b0110000, 0b00000, rs1, 0b001, rd, OP_IMM)
}
fn ctz_rv(rd: u32, rs1: u32) -> u32 {
    rv_r(0b0110000, 0b00001, rs1, 0b001, rd, OP_IMM)
}
fn cpop_rv(rd: u32, rs1: u32) -> u32 {
    rv_r(0b0110000, 0b00010, rs1, 0b001, rd, OP_IMM)
}
fn sext_b(rd: u32, rs1: u32) -> u32 {
    rv_r(0b0110000, 0b00100, rs1, 0b001, rd, OP_IMM)
}
fn sext_h(rd: u32, rs1: u32) -> u32 {
    rv_r(0b0110000, 0b00101, rs1, 0b001, rd, OP_IMM)
}

// W-suffix unary
fn clzw_rv(rd: u32, rs1: u32) -> u32 {
    rv_r(0b0110000, 0b00000, rs1, 0b001, rd, OP_IMM32)
}
fn ctzw_rv(rd: u32, rs1: u32) -> u32 {
    rv_r(0b0110000, 0b00001, rs1, 0b001, rd, OP_IMM32)
}
fn cpopw_rv(rd: u32, rs1: u32) -> u32 {
    rv_r(0b0110000, 0b00010, rs1, 0b001, rd, OP_IMM32)
}

// zext.h (R-type, OP-32)
fn zext_h(rd: u32, rs1: u32) -> u32 {
    rv_r(0b0000100, 0b00000, rs1, 0b100, rd, OP_REG32)
}

// rev8 / orc.b (full 12-bit immediate)
fn rev8(rd: u32, rs1: u32) -> u32 {
    // RV64 imm12=0b011010_111000 = 0x6B8 (shamt=56)
    rv_i(0x6B8, rs1, 0b101, rd, OP_IMM)
}
fn orc_b(rd: u32, rs1: u32) -> u32 {
    // imm12=0b001010_000111 = 0x287
    rv_i(0x287, rs1, 0b101, rd, OP_IMM)
}

// ── Zbb config helper ────────────────────────────────────

fn cfg_zbb() -> RiscvCfg {
    RiscvCfg {
        ext_zbb: true,
        ..cfg_rv64i_only()
    }
}

// ── Tests: Logical with NOT ──────────────────────────────

#[test]
fn test_andn() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0xFF00_FF00;
    cpu.gpr[3] = 0x0F0F_0F0F;
    run_rv_with_cfg(&mut cpu, andn(1, 2, 3), cfg_zbb());
    // 0xFF00_FF00 & ~0x0F0F_0F0F = 0xFF00_FF00 & 0xF0F0_F0F0
    assert_eq!(cpu.gpr[1], 0xF000_F000);
}

#[test]
fn test_orn() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x00FF;
    cpu.gpr[3] = 0x0F0F_0F0F;
    run_rv_with_cfg(&mut cpu, orn(1, 2, 3), cfg_zbb());
    // 0x00FF | ~0x0F0F0F0F
    let expected = 0x00FFu64 | !0x0F0F_0F0Fu64;
    assert_eq!(cpu.gpr[1], expected);
}

#[test]
fn test_xnor() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0xAAAA;
    cpu.gpr[3] = 0xAAAA;
    run_rv_with_cfg(&mut cpu, xnor(1, 2, 3), cfg_zbb());
    assert_eq!(cpu.gpr[1], !0u64); // ~(A ^ A) = ~0
}

#[test]
fn test_andn_rejected_without_zbb() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0xFF;
    cpu.gpr[3] = 0x0F;
    let exit = run_rv_with_cfg(&mut cpu, andn(1, 2, 3), cfg_rv64i_only());
    assert_eq!(exit, EXCP_UNDEF as usize);
}

// ── Tests: Min / Max ─────────────────────────────────────

#[test]
fn test_max_positive() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 10;
    cpu.gpr[3] = 20;
    run_rv_with_cfg(&mut cpu, max(1, 2, 3), cfg_zbb());
    assert_eq!(cpu.gpr[1], 20);
}

#[test]
fn test_max_negative() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = (-5i64) as u64;
    cpu.gpr[3] = 3;
    run_rv_with_cfg(&mut cpu, max(1, 2, 3), cfg_zbb());
    assert_eq!(cpu.gpr[1], 3);
}

#[test]
fn test_maxu() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = (-1i64) as u64; // large unsigned
    cpu.gpr[3] = 3;
    run_rv_with_cfg(&mut cpu, maxu(1, 2, 3), cfg_zbb());
    assert_eq!(cpu.gpr[1], u64::MAX);
}

#[test]
fn test_min_positive() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 10;
    cpu.gpr[3] = 20;
    run_rv_with_cfg(&mut cpu, min(1, 2, 3), cfg_zbb());
    assert_eq!(cpu.gpr[1], 10);
}

#[test]
fn test_min_negative() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = (-5i64) as u64;
    cpu.gpr[3] = 3;
    run_rv_with_cfg(&mut cpu, min(1, 2, 3), cfg_zbb());
    assert_eq!(cpu.gpr[1], (-5i64) as u64);
}

#[test]
fn test_minu() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = (-1i64) as u64;
    cpu.gpr[3] = 3;
    run_rv_with_cfg(&mut cpu, minu(1, 2, 3), cfg_zbb());
    assert_eq!(cpu.gpr[1], 3);
}

// ── Tests: Rotate (64-bit) ──────────────────────────────

#[test]
fn test_rol() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x8000_0000_0000_0001;
    cpu.gpr[3] = 1;
    run_rv_with_cfg(&mut cpu, rol(1, 2, 3), cfg_zbb());
    assert_eq!(cpu.gpr[1], 0x0000_0000_0000_0003);
}

#[test]
fn test_ror() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x0000_0000_0000_0003;
    cpu.gpr[3] = 1;
    run_rv_with_cfg(&mut cpu, ror(1, 2, 3), cfg_zbb());
    assert_eq!(cpu.gpr[1], 0x8000_0000_0000_0001);
}

#[test]
fn test_rori() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x0000_0000_0000_0003;
    run_rv_with_cfg(&mut cpu, rori(1, 2, 1), cfg_zbb());
    assert_eq!(cpu.gpr[1], 0x8000_0000_0000_0001);
}

// ── Tests: Rotate (32-bit, W-suffix) ────────────────────

#[test]
fn test_rolw() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x8000_0001;
    cpu.gpr[3] = 1;
    run_rv_with_cfg(&mut cpu, rolw(1, 2, 3), cfg_zbb());
    // 32-bit rotate left 1: 0x8000_0001 -> 0x0000_0003
    assert_eq!(cpu.gpr[1], 3);
}

#[test]
fn test_rorw() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x0000_0003;
    cpu.gpr[3] = 1;
    run_rv_with_cfg(&mut cpu, rorw(1, 2, 3), cfg_zbb());
    // 32-bit rotate right 1: 0x0000_0003 -> 0x8000_0001
    // sign-extended: 0xFFFFFFFF_80000001
    assert_eq!(cpu.gpr[1], 0xFFFF_FFFF_8000_0001);
}

#[test]
fn test_roriw() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x0000_0003;
    run_rv_with_cfg(&mut cpu, roriw(1, 2, 1), cfg_zbb());
    assert_eq!(cpu.gpr[1], 0xFFFF_FFFF_8000_0001);
}

// ── Tests: CLZ / CTZ / CPOP ─────────────────────────────

#[test]
fn test_clz() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x0000_0000_0000_0001;
    run_rv_with_cfg(&mut cpu, clz_rv(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 63);
}

#[test]
fn test_clz_zero() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0;
    run_rv_with_cfg(&mut cpu, clz_rv(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 64);
}

#[test]
fn test_ctz() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x8000_0000_0000_0000;
    run_rv_with_cfg(&mut cpu, ctz_rv(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 63);
}

#[test]
fn test_ctz_zero() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0;
    run_rv_with_cfg(&mut cpu, ctz_rv(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 64);
}

#[test]
fn test_cpop() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0xFF00_FF00_FF00_FF00;
    run_rv_with_cfg(&mut cpu, cpop_rv(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 32);
}

// ── Tests: CLZ/CTZ/CPOP W-suffix ────────────────────────

#[test]
fn test_clzw() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x0000_0001;
    run_rv_with_cfg(&mut cpu, clzw_rv(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 31);
}

#[test]
fn test_clzw_zero() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0;
    run_rv_with_cfg(&mut cpu, clzw_rv(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 32);
}

#[test]
fn test_ctzw() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x8000_0000;
    run_rv_with_cfg(&mut cpu, ctzw_rv(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 31);
}

#[test]
fn test_cpopw() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0xFFFF_FFFF_0000_00FF;
    run_rv_with_cfg(&mut cpu, cpopw_rv(1, 2), cfg_zbb());
    // Low 32 bits = 0x0000_00FF → popcount = 8
    assert_eq!(cpu.gpr[1], 8);
}

// ── Tests: Sign/zero extension ──────────────────────────

#[test]
fn test_sext_b_positive() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x7F;
    run_rv_with_cfg(&mut cpu, sext_b(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 0x7F);
}

#[test]
fn test_sext_b_negative() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x80;
    run_rv_with_cfg(&mut cpu, sext_b(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 0xFFFF_FFFF_FFFF_FF80);
}

#[test]
fn test_sext_h_positive() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x7FFF;
    run_rv_with_cfg(&mut cpu, sext_h(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 0x7FFF);
}

#[test]
fn test_sext_h_negative() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x8000;
    run_rv_with_cfg(&mut cpu, sext_h(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 0xFFFF_FFFF_FFFF_8000);
}

#[test]
fn test_zext_h() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0xFFFF_FFFF_FFFF_ABCD;
    run_rv_with_cfg(&mut cpu, zext_h(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 0xABCD);
}

// ── Tests: rev8 / orc.b ─────────────────────────────────

#[test]
fn test_rev8() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x0102_0304_0506_0708;
    run_rv_with_cfg(&mut cpu, rev8(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 0x0807_0605_0403_0201);
}

#[test]
fn test_orc_b() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0x0001_0000_0100_0001;
    run_rv_with_cfg(&mut cpu, orc_b(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 0x00FF_0000_FF00_00FF);
}

#[test]
fn test_orc_b_all_zero() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0;
    run_rv_with_cfg(&mut cpu, orc_b(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 0);
}

#[test]
fn test_orc_b_all_set() {
    let mut cpu = RiscvCpu::new();
    cpu.gpr[2] = 0xFFFF_FFFF_FFFF_FFFF;
    run_rv_with_cfg(&mut cpu, orc_b(1, 2), cfg_zbb());
    assert_eq!(cpu.gpr[1], 0xFFFF_FFFF_FFFF_FFFF);
}
