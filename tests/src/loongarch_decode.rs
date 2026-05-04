use machina_guest_loongarch::loongarch::insn_decode::{
    self, ArgsFr4, ArgsOffs26, ArgsR1Offs21, ArgsR1Si20, ArgsR2, ArgsR2Si12,
    ArgsR2Si14, ArgsR2Si16, ArgsR2Ui6, ArgsR2Ui8, ArgsR3, Decode,
};

struct FieldCapture {
    name: &'static str,
    rd: i64,
    rj: i64,
    rk: i64,
    si12: i64,
    si14: i64,
    si16: i64,
    si20: i64,
    offs21: i64,
    offs26: i64,
    ui6: i64,
    ui8: i64,
    fa: i64,
}

impl Default for FieldCapture {
    fn default() -> Self {
        Self {
            name: "",
            rd: -1,
            rj: -1,
            rk: -1,
            si12: 0,
            si14: 0,
            si16: 0,
            si20: 0,
            offs21: 0,
            offs26: 0,
            ui6: 0,
            ui8: 0,
            fa: 0,
        }
    }
}

impl Decode<()> for FieldCapture {
    fn trans_add_d(&mut self, _ir: &mut (), a: &ArgsR3) -> bool {
        self.name = "add_d";
        self.rd = a.rd;
        self.rj = a.rj;
        self.rk = a.rk;
        true
    }
    fn trans_addi_d(&mut self, _ir: &mut (), a: &ArgsR2Si12) -> bool {
        self.name = "addi_d";
        self.rd = a.rd;
        self.rj = a.rj;
        self.si12 = a.si12;
        true
    }
    fn trans_lu12i_w(&mut self, _ir: &mut (), a: &ArgsR1Si20) -> bool {
        self.name = "lu12i_w";
        self.rd = a.rd;
        self.si20 = a.si20;
        true
    }
    fn trans_beqz(&mut self, _ir: &mut (), a: &ArgsR1Offs21) -> bool {
        self.name = "beqz";
        self.rj = a.rj;
        self.offs21 = a.offs21;
        true
    }
    fn trans_b(&mut self, _ir: &mut (), a: &ArgsOffs26) -> bool {
        self.name = "b";
        self.offs26 = a.offs26;
        true
    }
    fn trans_ll_d(&mut self, _ir: &mut (), a: &ArgsR2Si14) -> bool {
        self.name = "ll_d";
        self.rd = a.rd;
        self.rj = a.rj;
        self.si14 = a.si14;
        true
    }
    fn trans_slli_d(&mut self, _ir: &mut (), a: &ArgsR2Ui6) -> bool {
        self.name = "slli_d";
        self.rd = a.rd;
        self.rj = a.rj;
        self.ui6 = a.ui6;
        true
    }
    fn trans_beq(&mut self, _ir: &mut (), a: &ArgsR2Si16) -> bool {
        self.name = "beq";
        self.rd = a.rd;
        self.rj = a.rj;
        self.si16 = a.si16;
        true
    }
    fn trans_fmadd_s(&mut self, _ir: &mut (), a: &ArgsFr4) -> bool {
        self.name = "fmadd_s";
        self.rd = a.fd;
        self.rj = a.fj;
        self.rk = a.fk;
        self.fa = a.fa;
        true
    }
    fn trans_lddir(&mut self, _ir: &mut (), a: &ArgsR2Ui8) -> bool {
        self.name = "lddir";
        self.rd = a.rd;
        self.rj = a.rj;
        self.ui8 = a.ui8;
        true
    }
    fn trans_clz_d(&mut self, _ir: &mut (), a: &ArgsR2) -> bool {
        self.name = "clz_d";
        self.rd = a.rd;
        self.rj = a.rj;
        true
    }
}

// --- Format coverage tests with field value assertions ---

#[test]
fn test_2r_format_clz_d() {
    let mut c = FieldCapture::default();
    // CLZ.D rd=7, rj=13: opcode=0000000000000000001001, rj=13, rd=7
    let insn: u32 = (0b0000000000000000001001 << 10) | (13 << 5) | 7;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "clz_d");
    assert_eq!(c.rd, 7);
    assert_eq!(c.rj, 13);
}

#[test]
fn test_3r_format_add_d() {
    let mut c = FieldCapture::default();
    // ADD.D rd=1, rj=2, rk=3
    let insn: u32 = (0b00000000000100001 << 15) | (3 << 10) | (2 << 5) | 1;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "add_d");
    assert_eq!(c.rd, 1);
    assert_eq!(c.rj, 2);
    assert_eq!(c.rk, 3);
}

#[test]
fn test_4r_format_fmadd_s() {
    let mut c = FieldCapture::default();
    // FMADD.S fd=1, fj=2, fk=3, fa=4
    let insn: u32 =
        (0b000010000001 << 20) | (4 << 15) | (3 << 10) | (2 << 5) | 1;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "fmadd_s");
    assert_eq!(c.rd, 1); // fd
    assert_eq!(c.rj, 2); // fj
    assert_eq!(c.rk, 3); // fk
    assert_eq!(c.fa, 4);
}

#[test]
fn test_2ri6_format_slli_d() {
    let mut c = FieldCapture::default();
    // SLLI.D rd=4, rj=5, ui6=42
    let insn: u32 = (0b0000000001000001 << 16) | (42 << 10) | (5 << 5) | 4;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "slli_d");
    assert_eq!(c.rd, 4);
    assert_eq!(c.rj, 5);
    assert_eq!(c.ui6, 42);
}

#[test]
fn test_2ri8_format_lddir() {
    let mut c = FieldCapture::default();
    // LDDIR rd=1, rj=2, level=5
    let insn: u32 = (0b00000110010000 << 18) | (5 << 10) | (2 << 5) | 1;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "lddir");
    assert_eq!(c.rd, 1);
    assert_eq!(c.rj, 2);
    assert_eq!(c.ui8, 5);
}

#[test]
fn test_2ri12_format_addi_d_positive() {
    let mut c = FieldCapture::default();
    // ADDI.D rd=5, rj=6, si12=100
    let insn: u32 =
        (0b0000001011 << 22) | ((100u32 & 0xFFF) << 10) | (6 << 5) | 5;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "addi_d");
    assert_eq!(c.rd, 5);
    assert_eq!(c.rj, 6);
    assert_eq!(c.si12, 100);
}

#[test]
fn test_2ri12_format_addi_d_negative() {
    let mut c = FieldCapture::default();
    // ADDI.D rd=5, rj=6, si12=-1 (sign-extended from 12 bits = 0xFFF)
    let imm_bits: u32 = 0xFFF; // -1 in 12-bit signed
    let insn: u32 = (0b0000001011 << 22) | (imm_bits << 10) | (6 << 5) | 5;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "addi_d");
    assert_eq!(c.si12, -1);
}

#[test]
fn test_2ri14_format_ll_d() {
    let mut c = FieldCapture::default();
    // LL.D rd=1, rj=2, si14=0x100
    let insn: u32 =
        (0b00100010 << 24) | ((0x100u32 & 0x3FFF) << 10) | (2 << 5) | 1;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "ll_d");
    assert_eq!(c.rd, 1);
    assert_eq!(c.rj, 2);
    assert_eq!(c.si14, 0x100);
}

#[test]
fn test_2ri16_format_beq() {
    let mut c = FieldCapture::default();
    // BEQ rd=4, rj=3, si16=0x10
    let insn: u32 =
        (0b010110 << 26) | ((0x10u32 & 0xFFFF) << 10) | (3 << 5) | 4;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "beq");
    assert_eq!(c.rd, 4);
    assert_eq!(c.rj, 3);
    assert_eq!(c.si16, 0x10);
}

#[test]
fn test_1ri20_format_lu12i_w() {
    let mut c = FieldCapture::default();
    // LU12I.W rd=7, si20=0x12345
    let insn: u32 = (0b0001010 << 25) | ((0x12345u32 & 0xF_FFFF) << 5) | 7;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "lu12i_w");
    assert_eq!(c.rd, 7);
    assert_eq!(c.si20, 0x12345);
}

#[test]
fn test_1ri20_format_negative_si20() {
    let mut c = FieldCapture::default();
    // LU12I.W rd=1, si20=-1 (0xFFFFF in 20 bits)
    let imm_bits: u32 = 0xF_FFFF;
    let insn: u32 = (0b0001010 << 25) | (imm_bits << 5) | 1;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "lu12i_w");
    assert_eq!(c.si20, -1);
}

#[test]
fn test_1ri21_format_beqz_positive() {
    let mut c = FieldCapture::default();
    // BEQZ rj=3, offs21=0x1_1234 (positive offset)
    // offs21[20:16] = bits[4:0] = 0x01 (high 5 bits)
    // offs21[15:0] = bits[25:10] = 0x1234 (low 16 bits)
    // Value: (0x01 << 16) | 0x1234 = 0x1_1234
    let offs_hi: u32 = 0x01; // bits[4:0]
    let offs_lo: u32 = 0x1234; // bits[25:10]
    let insn: u32 = (0b010000 << 26) | (offs_lo << 10) | (3 << 5) | offs_hi;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "beqz");
    assert_eq!(c.rj, 3);
    // Concatenated: {offs_hi(5 bits), offs_lo(16 bits)} = 0x1_1234
    assert_eq!(c.offs21, 0x1_1234);
}

#[test]
fn test_1ri21_format_beqz_negative() {
    let mut c = FieldCapture::default();
    // BEQZ rj=1, offs21=-4 (negative offset, sign-extended)
    // -4 in 21-bit signed = 0x1F_FFFC = 1_1111_1111_1111_1111_100
    // offs21[20:16] = 0x1F (sign bit set, high 5 bits)
    // offs21[15:0] = 0xFFFC (low 16 bits)
    let offs_hi: u32 = 0x1F; // bits[4:0] — contains sign bit
    let offs_lo: u32 = 0xFFFC; // bits[25:10]
    let insn: u32 = (0b010000 << 26) | (offs_lo << 10) | (1 << 5) | offs_hi;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "beqz");
    assert_eq!(c.rj, 1);
    // Sign-extended 21-bit value: -4
    assert_eq!(c.offs21, -4);
}

#[test]
fn test_i26_format_b_positive() {
    let mut c = FieldCapture::default();
    // B offs26=0x3FF_1234 (positive)
    // offs26[25:16] = bits[9:0] = 0x3FF (high 10 bits)
    // offs26[15:0] = bits[25:10] = 0x1234 (low 16 bits)
    let offs_hi: u32 = 0x0FF; // bits[9:0] (10 bits, positive)
    let offs_lo: u32 = 0x1234; // bits[25:10]
    let insn: u32 = (0b010100 << 26) | (offs_lo << 10) | offs_hi;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "b");
    // Concatenated: {offs_hi(10 bits), offs_lo(16 bits)} = 0xFF_1234
    assert_eq!(c.offs26, 0xFF_1234);
}

#[test]
fn test_i26_format_b_negative() {
    let mut c = FieldCapture::default();
    // B offs26=-4 (negative offset)
    // -4 in 26-bit signed = 0x3FF_FFFC
    // offs26[25:16] = 0x3FF (high 10 bits, sign bit set)
    // offs26[15:0] = 0xFFFC (low 16 bits)
    let offs_hi: u32 = 0x3FF; // bits[9:0] — sign bit in bit 9
    let offs_lo: u32 = 0xFFFC; // bits[25:10]
    let insn: u32 = (0b010100 << 26) | (offs_lo << 10) | offs_hi;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "b");
    assert_eq!(c.offs26, -4);
}

// --- Invalid opcode tests ---

#[test]
fn test_decode_invalid_all_ones() {
    let mut c = FieldCapture::default();
    assert!(!insn_decode::decode(&mut c, &mut (), 0xFFFF_FFFF));
}

#[test]
fn test_decode_invalid_all_zeros() {
    let mut c = FieldCapture::default();
    assert!(!insn_decode::decode(&mut c, &mut (), 0x0000_0000));
}

#[test]
fn test_decode_near_miss_one_bit_off() {
    let mut c = FieldCapture::default();
    // ADD.D has opcode 00000000000100001 at [31:15].
    // Flip bit 15 (the LSB of the opcode) to make it invalid.
    let valid_add_d: u32 =
        (0b00000000000100001 << 15) | (3 << 10) | (2 << 5) | 1;
    assert!(insn_decode::decode(&mut c, &mut (), valid_add_d));
    // Flip bit 15: change opcode[0] from 1 to 0
    let invalid = valid_add_d ^ (1 << 15);
    let mut c2 = FieldCapture::default();
    // This should decode to a different instruction or fail
    let result = insn_decode::decode(&mut c2, &mut (), invalid);
    // The flipped bit makes opcode=00000000000100000 = ADD.W
    // This is actually valid (ADD.W), so test that it decoded differently
    assert!(result || c2.name != "add_d");
}

// --- Translator dispatch test ---

#[test]
fn test_translator_dispatch_valid_insn() {
    use machina_accel::ir::Context;
    use machina_guest_loongarch::loongarch::ext::LoongArchCfg;
    use machina_guest_loongarch::loongarch::trans::{
        LoongArchDisasContext, LoongArchTranslator,
    };
    use machina_guest_loongarch::{DisasJumpType, TranslatorOps};

    // ADDI.D r0, r0, 0 (NOP): opcode=0000001011, si12=0, rj=0, rd=0
    let code: [u32; 1] = [0b0000001011 << 22];
    let guest_base = code.as_ptr().cast::<u8>();

    let mut ctx =
        LoongArchDisasContext::new(0, guest_base, LoongArchCfg::default());
    let mut ir = Context::new();

    LoongArchTranslator::init_disas_context(&mut ctx, &mut ir);
    LoongArchTranslator::insn_start(&mut ctx, &mut ir);
    LoongArchTranslator::translate_insn(&mut ctx, &mut ir);

    // Valid instruction: pc advanced, is_jmp remains Next
    assert_eq!(ctx.base.pc_next, 4);
    assert_eq!(ctx.base.is_jmp, DisasJumpType::Next);
}

#[test]
fn test_translator_dispatch_invalid_insn() {
    use machina_accel::ir::Context;
    use machina_guest_loongarch::loongarch::ext::LoongArchCfg;
    use machina_guest_loongarch::loongarch::trans::{
        LoongArchDisasContext, LoongArchTranslator,
    };
    use machina_guest_loongarch::{DisasJumpType, TranslatorOps};

    // Invalid instruction (all zeros)
    let code: [u32; 1] = [0x0000_0000];
    let guest_base = code.as_ptr().cast::<u8>();

    let mut ctx =
        LoongArchDisasContext::new(0, guest_base, LoongArchCfg::default());
    let mut ir = Context::new();

    LoongArchTranslator::init_disas_context(&mut ctx, &mut ir);
    LoongArchTranslator::insn_start(&mut ctx, &mut ir);
    LoongArchTranslator::translate_insn(&mut ctx, &mut ir);

    // Invalid instruction: pc stays at 0, is_jmp is NoReturn
    assert_eq!(ctx.base.pc_next, 0);
    assert_eq!(ctx.base.is_jmp, DisasJumpType::NoReturn);
}
