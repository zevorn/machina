use machina_guest_loongarch::loongarch::insn_decode::{
    self, ArgsCf, ArgsCopRSi12, ArgsCsr, ArgsFbranch, ArgsFcmp, ArgsFr2,
    ArgsFr3, ArgsFr4, ArgsFrr, ArgsFsel, ArgsHintR3, ArgsHintRSi12, ArgsOffs26,
    ArgsR1Offs21, ArgsR1Si20, ArgsR2, ArgsR2Msbw, ArgsR2Si12, ArgsR2Si14,
    ArgsR2Si16, ArgsR2Ui6, ArgsR2Ui8, ArgsR3, ArgsR3Sa2, ArgsR3Sa3, Decode,
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
    msb: i64,
    lsb: i64,
    sa: i64,
    hint: i64,
    cop: i64,
    fa: i64,
    csr_num: i64,
    cd: i64,
    cj: i64,
    ca: i64,
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
            msb: 0,
            lsb: 0,
            sa: 0,
            hint: 0,
            cop: 0,
            fa: 0,
            csr_num: 0,
            cd: -1,
            cj: -1,
            ca: -1,
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
    fn trans_addu16i_d(&mut self, _ir: &mut (), a: &ArgsR2Si16) -> bool {
        self.name = "addu16i_d";
        self.rd = a.rd;
        self.rj = a.rj;
        self.si16 = a.si16;
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
    fn trans_ldpte(&mut self, _ir: &mut (), a: &ArgsR2Ui8) -> bool {
        self.name = "ldpte";
        self.rd = a.rd;
        self.rj = a.rj;
        self.ui8 = a.ui8;
        true
    }
    fn trans_mulw_d_w(&mut self, _ir: &mut (), a: &ArgsR3) -> bool {
        self.name = "mulw_d_w";
        self.rd = a.rd;
        self.rj = a.rj;
        self.rk = a.rk;
        true
    }
    fn trans_alsl_w(&mut self, _ir: &mut (), a: &ArgsR3Sa2) -> bool {
        self.name = "alsl_w";
        self.rd = a.rd;
        self.rj = a.rj;
        self.rk = a.rk;
        self.sa = a.sa2;
        true
    }
    fn trans_bytepick_d(&mut self, _ir: &mut (), a: &ArgsR3Sa3) -> bool {
        self.name = "bytepick_d";
        self.rd = a.rd;
        self.rj = a.rj;
        self.rk = a.rk;
        self.sa = a.sa3;
        true
    }
    fn trans_ldx_w(&mut self, _ir: &mut (), a: &ArgsR3) -> bool {
        self.name = "ldx_w";
        self.rd = a.rd;
        self.rj = a.rj;
        self.rk = a.rk;
        true
    }
    fn trans_ldptr_d(&mut self, _ir: &mut (), a: &ArgsR2Si14) -> bool {
        self.name = "ldptr_d";
        self.rd = a.rd;
        self.rj = a.rj;
        self.si14 = a.si14;
        true
    }
    fn trans_preld(&mut self, _ir: &mut (), a: &ArgsHintRSi12) -> bool {
        self.name = "preld";
        self.hint = a.hint;
        self.rj = a.rj;
        self.si12 = a.si12;
        true
    }
    fn trans_preldx(&mut self, _ir: &mut (), a: &ArgsHintR3) -> bool {
        self.name = "preldx";
        self.hint = a.hint;
        self.rj = a.rj;
        self.rk = a.rk;
        true
    }
    fn trans_amcas_db_w(&mut self, _ir: &mut (), a: &ArgsR3) -> bool {
        self.name = "amcas_db_w";
        self.rd = a.rd;
        self.rj = a.rj;
        self.rk = a.rk;
        true
    }
    fn trans_llacq_d(&mut self, _ir: &mut (), a: &ArgsR2) -> bool {
        self.name = "llacq_d";
        self.rd = a.rd;
        self.rj = a.rj;
        true
    }
    fn trans_rdtime_d(&mut self, _ir: &mut (), a: &ArgsR2) -> bool {
        self.name = "rdtime_d";
        self.rd = a.rd;
        self.rj = a.rj;
        true
    }
    fn trans_tlbclr(
        &mut self,
        _ir: &mut (),
        _a: &insn_decode::ArgsEmpty,
    ) -> bool {
        self.name = "tlbclr";
        true
    }
    fn trans_cacop(&mut self, _ir: &mut (), a: &ArgsCopRSi12) -> bool {
        self.name = "cacop";
        self.cop = a.cop;
        self.rj = a.rj;
        self.si12 = a.si12;
        true
    }
    fn trans_clz_d(&mut self, _ir: &mut (), a: &ArgsR2) -> bool {
        self.name = "clz_d";
        self.rd = a.rd;
        self.rj = a.rj;
        true
    }
    fn trans_bstrins_w(&mut self, _ir: &mut (), a: &ArgsR2Msbw) -> bool {
        self.name = "bstrins_w";
        self.rd = a.rd;
        self.rj = a.rj;
        self.msb = a.msbw;
        self.lsb = a.lsbw;
        true
    }
    fn trans_bstrpick_w(&mut self, _ir: &mut (), a: &ArgsR2Msbw) -> bool {
        self.name = "bstrpick_w";
        self.rd = a.rd;
        self.rj = a.rj;
        self.msb = a.msbw;
        self.lsb = a.lsbw;
        true
    }
    fn trans_ertn(
        &mut self,
        _ir: &mut (),
        _a: &insn_decode::ArgsEmpty,
    ) -> bool {
        self.name = "ertn";
        true
    }
    fn trans_fld_s(&mut self, _ir: &mut (), a: &ArgsR2Si12) -> bool {
        self.name = "fld_s";
        self.rd = a.rd;
        self.rj = a.rj;
        self.si12 = a.si12;
        true
    }
    fn trans_csrrd(&mut self, _ir: &mut (), a: &ArgsCsr) -> bool {
        self.name = "csrrd";
        self.csr_num = a.csr_num;
        self.rd = a.rd;
        self.rj = a.rj;
        true
    }
    fn trans_fcmp_ceq_s(&mut self, _ir: &mut (), a: &ArgsFcmp) -> bool {
        self.name = "fcmp_ceq_s";
        self.cd = a.cd;
        self.rj = a.fj;
        self.rk = a.fk;
        true
    }
    fn trans_fsel(&mut self, _ir: &mut (), a: &ArgsFsel) -> bool {
        self.name = "fsel";
        self.rd = a.fd;
        self.rj = a.fj;
        self.rk = a.fk;
        self.ca = a.ca;
        true
    }
    fn trans_bceqz(&mut self, _ir: &mut (), a: &ArgsFbranch) -> bool {
        self.name = "bceqz";
        self.cj = a.cj;
        self.offs21 = a.offs21;
        true
    }
    fn trans_ftintrm_w_s(&mut self, _ir: &mut (), a: &ArgsFr2) -> bool {
        self.name = "ftintrm_w_s";
        self.rd = a.fd;
        self.rj = a.fj;
        true
    }
    fn trans_ftintrz_w_s(&mut self, _ir: &mut (), a: &ArgsFr2) -> bool {
        self.name = "ftintrz_w_s";
        self.rd = a.fd;
        self.rj = a.fj;
        true
    }
    fn trans_ftintrne_l_d(&mut self, _ir: &mut (), a: &ArgsFr2) -> bool {
        self.name = "ftintrne_l_d";
        self.rd = a.fd;
        self.rj = a.fj;
        true
    }
    fn trans_ftint_l_d(&mut self, _ir: &mut (), a: &ArgsFr2) -> bool {
        self.name = "ftint_l_d";
        self.rd = a.fd;
        self.rj = a.fj;
        true
    }
    fn trans_frint_d(&mut self, _ir: &mut (), a: &ArgsFr2) -> bool {
        self.name = "frint_d";
        self.rd = a.fd;
        self.rj = a.fj;
        true
    }
    fn trans_fmax_s(&mut self, _ir: &mut (), a: &ArgsFr3) -> bool {
        self.name = "fmax_s";
        self.rd = a.fd;
        self.rj = a.fj;
        self.rk = a.fk;
        true
    }
    fn trans_fclass_d(&mut self, _ir: &mut (), a: &ArgsFr2) -> bool {
        self.name = "fclass_d";
        self.rd = a.fd;
        self.rj = a.fj;
        true
    }
    fn trans_fldx_d(&mut self, _ir: &mut (), a: &ArgsFrr) -> bool {
        self.name = "fldx_d";
        self.rd = a.fd;
        self.rj = a.rj;
        self.rk = a.rk;
        true
    }
    fn trans_movfr2cf(&mut self, _ir: &mut (), a: &ArgsCf) -> bool {
        self.name = "movfr2cf";
        self.cd = a.cd;
        self.rj = a.fj;
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
fn test_bstrins_w_format() {
    let mut c = FieldCapture::default();
    // BSTRINS.W rd=7, rj=13, msbw=23, lsbw=5.
    let insn: u32 =
        (0b00000000011 << 21) | (23 << 16) | (5 << 10) | (13 << 5) | 7;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "bstrins_w");
    assert_eq!(c.rd, 7);
    assert_eq!(c.rj, 13);
    assert_eq!(c.msb, 23);
    assert_eq!(c.lsb, 5);
}

#[test]
fn test_bstrpick_w_format() {
    let mut c = FieldCapture::default();
    // BSTRPICK.W rd=8, rj=14, msbw=31, lsbw=0.
    let insn: u32 =
        (0b00000000011 << 21) | (31 << 16) | (1 << 15) | (14 << 5) | 8;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "bstrpick_w");
    assert_eq!(c.rd, 8);
    assert_eq!(c.rj, 14);
    assert_eq!(c.msb, 31);
    assert_eq!(c.lsb, 0);
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
fn test_2ri8_format_ldpte() {
    let mut c = FieldCapture::default();
    // LDPTE rj=2, level=5, low bits fixed to zero.
    let insn: u32 = (0b00000110010001 << 18) | (5 << 10) | (2 << 5);
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "ldpte");
    assert_eq!(c.rd, 0);
    assert_eq!(c.rj, 2);
    assert_eq!(c.ui8, 5);
}

#[test]
fn test_reference_integer_helper_formats() {
    let mut mul = FieldCapture::default();
    let mulw: u32 = (0b00000000000111110 << 15) | (3 << 10) | (2 << 5) | 1;
    assert!(insn_decode::decode(&mut mul, &mut (), mulw));
    assert_eq!(mul.name, "mulw_d_w");
    assert_eq!(mul.rd, 1);
    assert_eq!(mul.rj, 2);
    assert_eq!(mul.rk, 3);

    let mut alsl = FieldCapture::default();
    let alsl_w: u32 =
        (0b000000000000010 << 17) | (2 << 15) | (3 << 10) | (4 << 5) | 5;
    assert!(insn_decode::decode(&mut alsl, &mut (), alsl_w));
    assert_eq!(alsl.name, "alsl_w");
    assert_eq!(alsl.rd, 5);
    assert_eq!(alsl.rj, 4);
    assert_eq!(alsl.rk, 3);
    assert_eq!(alsl.sa, 2);

    let mut bytepick = FieldCapture::default();
    let bytepick_d: u32 =
        (0b00000000000011 << 18) | (6 << 15) | (7 << 10) | (8 << 5) | 9;
    assert!(insn_decode::decode(&mut bytepick, &mut (), bytepick_d));
    assert_eq!(bytepick.name, "bytepick_d");
    assert_eq!(bytepick.rd, 9);
    assert_eq!(bytepick.rj, 8);
    assert_eq!(bytepick.rk, 7);
    assert_eq!(bytepick.sa, 6);
}

#[test]
fn test_2ri16_format_addu16i_d() {
    let mut c = FieldCapture::default();
    let insn: u32 = (0b000100 << 26) | (0xFFFC << 10) | (6 << 5) | 5;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "addu16i_d");
    assert_eq!(c.rd, 5);
    assert_eq!(c.rj, 6);
    assert_eq!(c.si16, -4);
}

#[test]
fn test_reference_memory_formats() {
    let mut indexed = FieldCapture::default();
    let ldx_w: u32 = (0b00111000000010000 << 15) | (3 << 10) | (2 << 5) | 1;
    assert!(insn_decode::decode(&mut indexed, &mut (), ldx_w));
    assert_eq!(indexed.name, "ldx_w");
    assert_eq!(indexed.rd, 1);
    assert_eq!(indexed.rj, 2);
    assert_eq!(indexed.rk, 3);

    let mut ptr = FieldCapture::default();
    let ldptr_d: u32 = (0b00100110 << 24) | (0x3F << 10) | (4 << 5) | 5;
    assert!(insn_decode::decode(&mut ptr, &mut (), ldptr_d));
    assert_eq!(ptr.name, "ldptr_d");
    assert_eq!(ptr.rd, 5);
    assert_eq!(ptr.rj, 4);
    assert_eq!(ptr.si14, 0x3F);

    let mut preld = FieldCapture::default();
    let preld_insn: u32 = (0b0010101011 << 22) | (0x123 << 10) | (9 << 5) | 7;
    assert!(insn_decode::decode(&mut preld, &mut (), preld_insn));
    assert_eq!(preld.name, "preld");
    assert_eq!(preld.hint, 7);
    assert_eq!(preld.rj, 9);
    assert_eq!(preld.si12, 0x123);

    let mut preldx = FieldCapture::default();
    let preldx_insn: u32 =
        (0b00111000001011000 << 15) | (8 << 10) | (9 << 5) | 7;
    assert!(insn_decode::decode(&mut preldx, &mut (), preldx_insn));
    assert_eq!(preldx.name, "preldx");
    assert_eq!(preldx.hint, 7);
    assert_eq!(preldx.rj, 9);
    assert_eq!(preldx.rk, 8);
}

#[test]
fn test_reference_atomic_formats() {
    let mut cas = FieldCapture::default();
    let amcas_db_w: u32 =
        (0b00111000010110110 << 15) | (3 << 10) | (2 << 5) | 1;
    assert!(insn_decode::decode(&mut cas, &mut (), amcas_db_w));
    assert_eq!(cas.name, "amcas_db_w");
    assert_eq!(cas.rd, 1);
    assert_eq!(cas.rj, 2);
    assert_eq!(cas.rk, 3);

    let mut acq = FieldCapture::default();
    let llacq_d: u32 = (0b0011100001010111100010 << 10) | (4 << 5) | 5;
    assert!(insn_decode::decode(&mut acq, &mut (), llacq_d));
    assert_eq!(acq.name, "llacq_d");
    assert_eq!(acq.rd, 5);
    assert_eq!(acq.rj, 4);
}

#[test]
fn test_reference_privileged_time_tlb_cache_formats() {
    let mut rdtime = FieldCapture::default();
    let rdtime_d: u32 = (0b0000000000000000011010 << 10) | (3 << 5) | 4;
    assert!(insn_decode::decode(&mut rdtime, &mut (), rdtime_d));
    assert_eq!(rdtime.name, "rdtime_d");
    assert_eq!(rdtime.rd, 4);
    assert_eq!(rdtime.rj, 3);

    let mut tlbclr = FieldCapture::default();
    let tlbclr_insn: u32 = 0b0000011001001000001000 << 10;
    assert!(insn_decode::decode(&mut tlbclr, &mut (), tlbclr_insn));
    assert_eq!(tlbclr.name, "tlbclr");

    let mut cacop = FieldCapture::default();
    let cacop_insn: u32 = (0b0000011000 << 22) | (0x321 << 10) | (6 << 5) | 5;
    assert!(insn_decode::decode(&mut cacop, &mut (), cacop_insn));
    assert_eq!(cacop.name, "cacop");
    assert_eq!(cacop.cop, 5);
    assert_eq!(cacop.rj, 6);
    assert_eq!(cacop.si12, 0x321);
}

#[test]
fn test_cacop_negative_offset_sign_extends() {
    let mut c = FieldCapture::default();
    let cacop_insn: u32 = (0b0000011000 << 22) | (0xfff << 10) | (6 << 5) | 5;
    assert!(insn_decode::decode(&mut c, &mut (), cacop_insn));
    assert_eq!(c.name, "cacop");
    assert_eq!(c.cop, 5);
    assert_eq!(c.rj, 6);
    assert_eq!(c.si12, -1);
}

#[test]
fn test_reference_fpu_formats() {
    let mut fmax = FieldCapture::default();
    let fmax_s: u32 = (0b00000001000010001 << 15) | (3 << 10) | (2 << 5) | 1;
    assert!(insn_decode::decode(&mut fmax, &mut (), fmax_s));
    assert_eq!(fmax.name, "fmax_s");
    assert_eq!(fmax.rd, 1);
    assert_eq!(fmax.rj, 2);
    assert_eq!(fmax.rk, 3);

    let mut fclass = FieldCapture::default();
    let fclass_d: u32 = (0b0000000100010100001110 << 10) | (9 << 5) | 4;
    assert!(insn_decode::decode(&mut fclass, &mut (), fclass_d));
    assert_eq!(fclass.name, "fclass_d");
    assert_eq!(fclass.rd, 4);
    assert_eq!(fclass.rj, 9);

    let mut fldx = FieldCapture::default();
    let fldx_d: u32 = (0b00111000001101000 << 15) | (3 << 10) | (2 << 5) | 1;
    assert!(insn_decode::decode(&mut fldx, &mut (), fldx_d));
    assert_eq!(fldx.name, "fldx_d");
    assert_eq!(fldx.rd, 1);
    assert_eq!(fldx.rj, 2);
    assert_eq!(fldx.rk, 3);

    let mut movfr2cf = FieldCapture::default();
    let movfr2cf_insn: u32 = (0b0000000100010100110100 << 10) | (11 << 5) | 6;
    assert!(insn_decode::decode(&mut movfr2cf, &mut (), movfr2cf_insn));
    assert_eq!(movfr2cf.name, "movfr2cf");
    assert_eq!(movfr2cf.cd, 6);
    assert_eq!(movfr2cf.rj, 11);
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

#[test]
fn test_fld_s_decode() {
    let mut c = FieldCapture::default();
    // FLD.S fd=3, rj=10, si12=64: opcode=0010101100, si12=64, rj=10, rd=3
    let insn: u32 = (0b0010101100 << 22) | (64 << 10) | (10 << 5) | 3;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "fld_s");
    assert_eq!(c.rd, 3);
    assert_eq!(c.rj, 10);
    assert_eq!(c.si12, 64);
}

#[test]
fn test_csr_format_csrrd() {
    let mut c = FieldCapture::default();
    // CSRRD rd=7, csr=0x0c, fixed rj=0.
    let insn: u32 = (0b00000100 << 24) | (0x0C << 10) | 7;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "csrrd");
    assert_eq!(c.csr_num, 0x0C);
    assert_eq!(c.rd, 7);
    assert_eq!(c.rj, 0);
}

#[test]
fn test_fcmp_format_ceq_s() {
    let mut c = FieldCapture::default();
    // FCMP.CEQ.S cd=5, fj=2, fk=3.
    let insn: u32 =
        (0b000011000001 << 20) | (0b00100 << 15) | (3 << 10) | (2 << 5) | 5;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "fcmp_ceq_s");
    assert_eq!(c.cd, 5);
    assert_eq!(c.rj, 2);
    assert_eq!(c.rk, 3);
}

#[test]
fn test_fsel_format() {
    let mut c = FieldCapture::default();
    // FSEL fd=1, fj=2, fk=3, ca=6.
    let insn: u32 =
        (0b000011010000 << 20) | (6 << 15) | (3 << 10) | (2 << 5) | 1;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "fsel");
    assert_eq!(c.rd, 1);
    assert_eq!(c.rj, 2);
    assert_eq!(c.rk, 3);
    assert_eq!(c.ca, 6);
}

#[test]
fn test_fbranch_format_bceqz_split_offset() {
    let mut c = FieldCapture::default();
    // BCEQZ cj=6, offs21=0x3_1234 split between bits [4:0] and [25:10].
    let insn: u32 = (0b010010 << 26) | (0x1234 << 10) | (6 << 5) | 0x03;
    assert!(insn_decode::decode(&mut c, &mut (), insn));
    assert_eq!(c.name, "bceqz");
    assert_eq!(c.cj, 6);
    assert_eq!(c.offs21, 0x3_1234);
}

#[test]
fn test_fpu_rounding_conversion_opcodes() {
    let cases = [
        ("ftintrm_w_s", 0b0000000100011010000001u32),
        ("ftintrz_w_s", 0b0000000100011010100001u32),
        ("ftintrne_l_d", 0b0000000100011010111010u32),
        ("ftint_l_d", 0b0000000100011011001010u32),
        ("frint_d", 0b0000000100011110010010u32),
    ];

    for (name, opcode) in cases {
        let mut c = FieldCapture::default();
        let insn = (opcode << 10) | (9 << 5) | 4;
        assert!(insn_decode::decode(&mut c, &mut (), insn), "{name}");
        assert_eq!(c.name, name);
        assert_eq!(c.rd, 4);
        assert_eq!(c.rj, 9);
    }
}

#[test]
fn test_ftintrm_and_ftintrz_are_distinct_opcodes() {
    let mut round_down = FieldCapture::default();
    let ftintrm = (0b0000000100011010000001u32 << 10) | (1 << 5) | 2;
    assert!(insn_decode::decode(&mut round_down, &mut (), ftintrm));
    assert_eq!(round_down.name, "ftintrm_w_s");

    let mut round_zero = FieldCapture::default();
    let ftintrz = (0b0000000100011010100001u32 << 10) | (1 << 5) | 2;
    assert!(insn_decode::decode(&mut round_zero, &mut (), ftintrz));
    assert_eq!(round_zero.name, "ftintrz_w_s");
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
fn test_decode_near_miss_ertn_one_bit_flip() {
    // ERTN has a fully-fixed encoding: 0000011001001000001110 0000000000
    let valid_ertn: u32 = 0b00000110010010000011100000000000;
    let mut c = FieldCapture::default();
    // Valid ERTN must decode successfully (trans_ertn returns true)
    assert!(insn_decode::decode(&mut c, &mut (), valid_ertn));
    assert_eq!(c.name, "ertn");

    // Flip bit 10 (one of the fixed zero bits in the ERTN encoding)
    let invalid = valid_ertn ^ (1 << 10);
    let mut c2 = FieldCapture::default();
    let invalid_result = insn_decode::decode(&mut c2, &mut (), invalid);
    // The mutated encoding must fail to decode entirely
    assert!(
        !invalid_result,
        "flipping a fixed bit in ERTN must fail decode"
    );
}

#[test]
fn test_decode_near_miss_fcmp_fixed_bit_flip() {
    let valid: u32 =
        (0b000011000001 << 20) | (0b00100 << 15) | (3 << 10) | (2 << 5) | 5;
    let mut c = FieldCapture::default();
    assert!(insn_decode::decode(&mut c, &mut (), valid));
    assert_eq!(c.name, "fcmp_ceq_s");

    let invalid = valid ^ (1 << 3);
    let mut c2 = FieldCapture::default();
    assert!(!insn_decode::decode(&mut c2, &mut (), invalid));
}

#[test]
fn test_decode_near_miss_bstrins_w_prefix_bit_flip() {
    let valid: u32 =
        (0b00000000011 << 21) | (23 << 16) | (5 << 10) | (13 << 5) | 7;
    let mut c = FieldCapture::default();
    assert!(insn_decode::decode(&mut c, &mut (), valid));
    assert_eq!(c.name, "bstrins_w");

    let invalid = valid ^ (1 << 21);
    let mut c2 = FieldCapture::default();
    assert!(!insn_decode::decode(&mut c2, &mut (), invalid));
}

#[test]
fn test_decode_near_miss_ftintrz_fixed_bit_flip() {
    let valid: u32 = (0b0000000100011010100001u32 << 10) | (1 << 5) | 2;
    let mut c = FieldCapture::default();
    assert!(insn_decode::decode(&mut c, &mut (), valid));
    assert_eq!(c.name, "ftintrz_w_s");

    let invalid = valid ^ (1 << 21);
    let mut c2 = FieldCapture::default();
    assert!(!insn_decode::decode(&mut c2, &mut (), invalid));
}

#[test]
fn test_decode_near_miss_ldpte_low_fixed_bit_flip() {
    let valid: u32 = (0b00000110010001 << 18) | (5 << 10) | (2 << 5);
    let mut c = FieldCapture::default();
    assert!(insn_decode::decode(&mut c, &mut (), valid));
    assert_eq!(c.name, "ldpte");

    let invalid = valid | 1;
    let mut c2 = FieldCapture::default();
    assert!(!insn_decode::decode(&mut c2, &mut (), invalid));
}

#[test]
fn test_decode_near_miss_tlbclr_low_fixed_bit_flip() {
    let valid: u32 = 0b0000011001001000001000 << 10;
    let mut c = FieldCapture::default();
    assert!(insn_decode::decode(&mut c, &mut (), valid));
    assert_eq!(c.name, "tlbclr");

    let invalid = valid | 1;
    let mut c2 = FieldCapture::default();
    assert!(!insn_decode::decode(&mut c2, &mut (), invalid));
}

#[test]
fn test_decode_near_miss_cacop_prefix_bit_flip() {
    let valid: u32 = (0b0000011000 << 22) | (0x321 << 10) | (6 << 5) | 5;
    let mut c = FieldCapture::default();
    assert!(insn_decode::decode(&mut c, &mut (), valid));
    assert_eq!(c.name, "cacop");

    let invalid = valid ^ (1 << 22);
    let mut c2 = FieldCapture::default();
    assert!(!insn_decode::decode(&mut c2, &mut (), invalid));
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

/// Helper to translate one instruction and return number of IR ops emitted.
fn translate_one(insn: u32) -> (usize, machina_guest_loongarch::DisasJumpType) {
    use machina_accel::ir::Context;
    use machina_guest_loongarch::loongarch::ext::LoongArchCfg;
    use machina_guest_loongarch::loongarch::trans::{
        LoongArchDisasContext, LoongArchTranslator,
    };
    use machina_guest_loongarch::TranslatorOps;

    let code: [u32; 1] = [insn];
    let guest_base = code.as_ptr().cast::<u8>();
    let mut ctx =
        LoongArchDisasContext::new(0, guest_base, LoongArchCfg::default());
    let mut ir = Context::new();
    LoongArchTranslator::init_disas_context(&mut ctx, &mut ir);
    let ops_before = ir.ops().len();
    LoongArchTranslator::insn_start(&mut ctx, &mut ir);
    LoongArchTranslator::translate_insn(&mut ctx, &mut ir);
    let ops_after = ir.ops().len();
    (ops_after - ops_before, ctx.base.is_jmp)
}

fn translate_one_ir(insn: u32) -> machina_accel::ir::Context {
    use machina_accel::ir::Context;
    use machina_guest_loongarch::loongarch::ext::LoongArchCfg;
    use machina_guest_loongarch::loongarch::trans::{
        LoongArchDisasContext, LoongArchTranslator,
    };
    use machina_guest_loongarch::TranslatorOps;

    let code: [u32; 1] = [insn];
    let guest_base = code.as_ptr().cast::<u8>();
    let mut ctx =
        LoongArchDisasContext::new(0, guest_base, LoongArchCfg::default());
    let mut ir = Context::new();
    LoongArchTranslator::init_disas_context(&mut ctx, &mut ir);
    LoongArchTranslator::insn_start(&mut ctx, &mut ir);
    LoongArchTranslator::translate_insn(&mut ctx, &mut ir);
    ir
}

#[test]
fn test_alu_add_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // ADD.D rd=1, rj=2, rk=3
    let insn: u32 = (0b00000000000100001 << 15) | (3 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "ADD.D must emit IR ops (got {ops})");
}

#[test]
fn test_alu_sub_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // SUB.D rd=1, rj=2, rk=3
    let insn: u32 = (0b00000000000100011 << 15) | (3 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "SUB.D must emit IR ops (got {ops})");
}

#[test]
fn test_alu_and_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // AND rd=4, rj=5, rk=6
    let insn: u32 = (0b00000000000101001 << 15) | (6 << 10) | (5 << 5) | 4;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "AND must emit IR ops (got {ops})");
}

#[test]
fn test_alu_slt_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // SLT rd=1, rj=2, rk=3
    let insn: u32 = (0b00000000000100100 << 15) | (3 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "SLT must emit IR ops (got {ops})");
}

#[test]
fn test_alu_sll_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // SLL.D rd=1, rj=2, rk=3
    let insn: u32 = (0b00000000000110001 << 15) | (3 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "SLL.D must emit IR ops (got {ops})");
}

#[test]
fn test_alu_r0_dest_suppressed() {
    use machina_guest_loongarch::DisasJumpType;
    // ADD.D rd=0, rj=2, rk=3 — writing to r0 should still succeed
    // but produce fewer ops (no gen_mov to global)
    let insn: u32 = (0b00000000000100001 << 15) | (3 << 10) | (2 << 5) | 0;
    let (ops_r0, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    // Compare with rd=1
    let insn_r1: u32 = (0b00000000000100001 << 15) | (3 << 10) | (2 << 5) | 1;
    let (ops_r1, _) = translate_one(insn_r1);
    // rd=0 should emit fewer ops (no write-back mov)
    assert!(
        ops_r0 < ops_r1,
        "r0 dest should suppress write: r0={ops_r0} vs r1={ops_r1}"
    );
}

#[test]
fn test_alu_mul_w_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // MUL.W rd=1, rj=2, rk=3: opcode=00000000000111000
    let insn: u32 = (0b00000000000111000 << 15) | (3 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "MUL.W must emit IR ops (got {ops})");
}

#[test]
fn test_alu_sll_w_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // SLL.W rd=1, rj=2, rk=3: opcode=00000000000101110
    let insn: u32 = (0b00000000000101110 << 15) | (3 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "SLL.W must emit IR ops (got {ops})");
}

#[test]
fn test_alu_rotr_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // ROTR.D rd=1, rj=2, rk=3: opcode=00000000000110111
    let insn: u32 = (0b00000000000110111 << 15) | (3 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "ROTR.D must emit IR ops (got {ops})");
}

#[test]
fn test_bstrpick_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // BSTRPICK.D rd=1, rj=2, msbd=15, lsbd=8:
    // opcode=0000000011, msbd(6)=001111, lsbd(6)=001000, rj=2, rd=1
    let insn: u32 =
        (0b0000000011 << 22) | (15 << 16) | (8 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "BSTRPICK.D must emit IR ops (got {ops})");
}

#[test]
fn test_bstrins_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // BSTRINS.D rd=1, rj=2, msbd=7, lsbd=0:
    // opcode=0000000010, msbd(6)=000111, lsbd(6)=000000, rj=2, rd=1
    let insn: u32 = (0b0000000010 << 22) | (7 << 16) | (0 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "BSTRINS.D must emit IR ops (got {ops})");
}

#[test]
fn test_div_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // DIV.D rd=1, rj=2, rk=3: opcode=00000000001000100
    let insn: u32 = (0b00000000001000100 << 15) | (3 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(
        ops > 1,
        "DIV.D must emit IR ops via helper call (got {ops})"
    );
}

#[test]
fn test_mulh_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // MULH.D rd=1, rj=2, rk=3: opcode=00000000000111100
    let insn: u32 = (0b00000000000111100 << 15) | (3 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "MULH.D must emit IR (got {ops})");
}

#[test]
fn test_slli_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // SLLI.D rd=1, rj=2, ui6=10: opcode=0000000001000001
    let insn: u32 = (0b0000000001000001 << 16) | (10 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "SLLI.D must emit IR (got {ops})");
}

#[test]
fn test_clz_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // CLZ.D rd=1, rj=2: opcode=0000000000000000001001
    let insn: u32 = (0b0000000000000000001001 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "CLZ.D must emit IR (got {ops})");
}

#[test]
fn test_revb_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // REVB.D rd=1, rj=2: opcode=0000000000000000001111
    let insn: u32 = (0b0000000000000000001111 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "REVB.D must emit IR (got {ops})");
}

#[test]
fn test_bitrev_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // BITREV.D rd=1, rj=2: opcode=0000000000000000010101
    let insn: u32 = (0b0000000000000000010101 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 1, "BITREV.D must emit IR (got {ops})");
}

#[test]
fn test_helper_bitrev_4b_sign_extends() {
    use machina_guest_loongarch::loongarch::trans::helpers::loongarch_helper_bitrev_4b;
    // Input 0x0100_0000: byte 3 = 0x01, reversed = 0x80
    // Result low 32 = 0x8000_0000, sign-extended = 0xFFFF_FFFF_8000_0000
    let result = loongarch_helper_bitrev_4b(0x0100_0000);
    assert_eq!(result, -0x8000_0000_i64, "BITREV.4B must sign-extend");
}

#[test]
fn test_helper_revb_2h_sign_extends() {
    use machina_guest_loongarch::loongarch::trans::helpers::loongarch_helper_revb_2h;
    // Input 0x0000_0000_0080_1234:
    // halfword[1] = 0x0080 → swapped = 0x8000
    // halfword[0] = 0x1234 → swapped = 0x3412
    // Low 32 = 0x8000_3412, bit 31 set → sign-extend
    let result = loongarch_helper_revb_2h(0x0080_1234);
    assert_eq!(
        result as u64, 0xFFFF_FFFF_8000_3412,
        "REVB.2H must sign-extend when bit 31 is set"
    );
}

#[test]
fn test_helper_revb_2h_ignores_high_bits() {
    use machina_guest_loongarch::loongarch::trans::helpers::loongarch_helper_revb_2h;
    // High 32 bits of input should be ignored
    let result = loongarch_helper_revb_2h(0xDEAD_BEEF_0001_0002);
    // halfword[1]=0x0001→0x0100, halfword[0]=0x0002→0x0200
    // Low 32 = 0x0100_0200, bit 31 clear → zero-extend
    assert_eq!(result, 0x0100_0200);
}

#[test]
fn test_ld_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // LD.D rd=1, rj=2, si12=8: opcode=0010100011
    let insn: u32 = (0b0010100011 << 22) | (8 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 2, "LD.D must emit address calc + QemuLd (got {ops})");
}

#[test]
fn test_beq_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // BEQ rd=3, rj=4, si16=2: opcode=010110, offset=2<<2=8
    let insn: u32 = (0b010110 << 26) | (2 << 10) | (4 << 5) | 3;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::NoReturn);
    assert!(ops > 5, "BEQ must emit brcond + both paths (got {ops})");
}

#[test]
fn test_translator_tb_stop_emits_goto_and_exit() {
    use machina_accel::ir::opcode::Opcode;
    use machina_accel::ir::Context;
    use machina_guest_loongarch::loongarch::ext::LoongArchCfg;
    use machina_guest_loongarch::loongarch::trans::{
        LoongArchDisasContext, LoongArchTranslator,
    };
    use machina_guest_loongarch::{DisasJumpType, TranslatorOps};

    let code: [u32; 1] = [0b0000001011 << 22]; // ADDI.D NOP
    let guest_base = code.as_ptr().cast::<u8>();

    let mut ctx =
        LoongArchDisasContext::new(0, guest_base, LoongArchCfg::default());
    let mut ir = Context::new();

    LoongArchTranslator::init_disas_context(&mut ctx, &mut ir);
    LoongArchTranslator::insn_start(&mut ctx, &mut ir);
    LoongArchTranslator::translate_insn(&mut ctx, &mut ir);
    assert_eq!(ctx.base.is_jmp, DisasJumpType::Next);

    // Call tb_stop for fall-through
    LoongArchTranslator::tb_stop(&mut ctx, &mut ir);

    // Inspect emitted IR ops: should end with GotoTb + ExitTb
    let ops = ir.ops();
    let len = ops.len();
    assert!(len >= 2, "tb_stop must emit at least GotoTb + ExitTb");
    assert_eq!(ops[len - 2].opc, Opcode::GotoTb);
    assert_eq!(ops[len - 1].opc, Opcode::ExitTb);
}

#[test]
fn test_ll_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // LL.D rd=1, rj=2, si14=4: opcode=00100010, si14=4, rj=2, rd=1
    let insn: u32 = (0b00100010 << 24) | (4 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(
        ops > 3,
        "LL.D must emit QemuLd + reservation writes (got {ops})"
    );
}

#[test]
fn test_sc_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // SC.D rd=1, rj=2, si14=4: opcode=00100011, si14=4, rj=2, rd=1
    let insn: u32 = (0b00100011 << 24) | (4 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 2, "SC.D must emit helper call (got {ops})");
}

#[test]
fn test_dbar_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // DBAR hint=0: opcode=00111000011100100, code15=0
    let insn: u32 = 0b00111000011100100_000000000000000;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 0, "DBAR must emit barrier (got {ops})");
}

#[test]
fn test_amadd_d_translation() {
    use machina_guest_loongarch::DisasJumpType;
    // AMADD.D rd=1, rj=2, rk=3: opcode=00111000011000010
    let insn: u32 = (0b00111000011000010 << 15) | (3 << 10) | (2 << 5) | 1;
    let (ops, jmp) = translate_one(insn);
    assert_eq!(jmp, DisasJumpType::Next);
    assert!(ops > 3, "AMADD.D must emit QemuLd+Add+QemuSt (got {ops})");
}
#[test]
fn test_all_atomic_handlers_set_contains_atomic() {
    let cases: &[(&str, u32)] = &[
        ("LL.W", (0b00100000u32 << 24) | (1 << 10) | (2 << 5) | 3),
        ("LL.D", (0b00100010u32 << 24) | (1 << 10) | (2 << 5) | 3),
        ("SC.W", (0b00100001u32 << 24) | (1 << 10) | (2 << 5) | 3),
        ("SC.D", (0b00100011u32 << 24) | (1 << 10) | (2 << 5) | 3),
        (
            "AMADD.W",
            (0b00111000011000001u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMADD.D",
            (0b00111000011000010u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMSWAP.W",
            (0b00111000011000000u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMSWAP.D",
            (0b00111000011000011u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMAND.W",
            (0b00111000011000100u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMAND.D",
            (0b00111000011000101u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMOR.W",
            (0b00111000011000110u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMOR.D",
            (0b00111000011000111u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMXOR.W",
            (0b00111000011001000u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMXOR.D",
            (0b00111000011001001u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMMAX.W",
            (0b00111000011001010u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMMAX.D",
            (0b00111000011001011u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMMIN.W",
            (0b00111000011001100u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMMIN.D",
            (0b00111000011001101u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMMAX.WU",
            (0b00111000011001110u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMMAX.DU",
            (0b00111000011001111u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMMIN.WU",
            (0b00111000011010000u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
        (
            "AMMIN.DU",
            (0b00111000011010001u32 << 15) | (3 << 10) | (2 << 5) | 1,
        ),
    ];
    for (name, insn) in cases {
        let ir = translate_one_ir(*insn);
        assert!(ir.contains_atomic, "{name} must set contains_atomic",);
    }
}

#[test]
fn test_privacy_fixture_fails_to_compile() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/loongarch_privacy_fail");
    let output = std::process::Command::new("cargo")
        .args(["check", "--manifest-path"])
        .arg(fixture_dir.join("Cargo.toml"))
        .output()
        .expect("failed to run cargo check on privacy fixture");

    assert!(
        !output.status.success(),
        "privacy fixture must fail to compile"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("is private"),
        "error must mention private fields, got: {stderr}"
    );
}
