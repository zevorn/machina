use super::cpu::{LoongArchCpu, NUM_SAVE};

pub const CSR_CRMD: u32 = 0x0;
pub const CSR_PRMD: u32 = 0x1;
pub const CSR_EUEN: u32 = 0x2;
pub const CSR_MISC: u32 = 0x3;
pub const CSR_ECFG: u32 = 0x4;
pub const CSR_ESTAT: u32 = 0x5;
pub const CSR_ERA: u32 = 0x6;
pub const CSR_BADV: u32 = 0x7;
pub const CSR_BADI: u32 = 0x8;
pub const CSR_EENTRY: u32 = 0xC;
pub const CSR_TLBIDX: u32 = 0x10;
pub const CSR_TLBEHI: u32 = 0x11;
pub const CSR_TLBELO0: u32 = 0x12;
pub const CSR_TLBELO1: u32 = 0x13;
pub const CSR_ASID: u32 = 0x18;
pub const CSR_PGDL: u32 = 0x19;
pub const CSR_PGDH: u32 = 0x1A;
pub const CSR_PGD: u32 = 0x1B;
pub const CSR_PWCL: u32 = 0x1C;
pub const CSR_PWCH: u32 = 0x1D;
pub const CSR_STLBPS: u32 = 0x1E;
pub const CSR_RVACFG: u32 = 0x1F;
pub const CSR_CPUID: u32 = 0x20;
pub const CSR_PRCFG1: u32 = 0x21;
pub const CSR_PRCFG2: u32 = 0x22;
pub const CSR_PRCFG3: u32 = 0x23;
pub const CSR_SAVE0: u32 = 0x30;
pub const CSR_SAVE_LAST: u32 = CSR_SAVE0 + NUM_SAVE as u32 - 1;
pub const CSR_TID: u32 = 0x40;
pub const CSR_TCFG: u32 = 0x41;
pub const CSR_TVAL: u32 = 0x42;
pub const CSR_CNTC: u32 = 0x43;
pub const CSR_TICLR: u32 = 0x44;
pub const CSR_LLBCTL: u32 = 0x60;
pub const CSR_TLBRENTRY: u32 = 0x88;
pub const CSR_TLBRBADV: u32 = 0x89;
pub const CSR_TLBRERA: u32 = 0x8A;
pub const CSR_TLBRSAVE: u32 = 0x8B;
pub const CSR_TLBRELO0: u32 = 0x8C;
pub const CSR_TLBRELO1: u32 = 0x8D;
pub const CSR_TLBREHI: u32 = 0x8E;
pub const CSR_TLBRPRMD: u32 = 0x8F;
pub const CSR_DMW0: u32 = 0x180;
pub const CSR_DMW1: u32 = 0x181;
pub const CSR_DMW2: u32 = 0x182;
pub const CSR_DMW3: u32 = 0x183;

pub const CRMD_PLV_MASK: u64 = 0x3;
pub const CRMD_IE: u64 = 1 << 2;
pub const CRMD_DA: u64 = 1 << 3;
pub const CRMD_PG: u64 = 1 << 4;
pub const CRMD_DATF: u64 = 0x3 << 5;
pub const CRMD_DATM: u64 = 0x3 << 7;

pub const EUEN_FPE: u64 = 1 << 0;
pub const EUEN_SXE: u64 = 1 << 1;
pub const EUEN_ASXE: u64 = 1 << 2;

pub const ESTAT_IS_MASK: u64 = 0x1FFF;

pub const CRMD_WRITE_MASK: u64 =
    CRMD_PLV_MASK | CRMD_IE | CRMD_DA | CRMD_PG | CRMD_DATF | CRMD_DATM;
pub const PRMD_WRITE_MASK: u64 = 0x7;
pub const EUEN_WRITE_MASK: u64 = EUEN_FPE | EUEN_SXE | EUEN_ASXE;
pub const MISC_WRITE_MASK: u64 = 0;
pub const ECFG_WRITE_MASK: u64 = 0x0000_0000_0007_FFFF;
// ESTAT: only IS[1:0] (software interrupts) are writable
pub const ESTAT_WRITE_MASK: u64 = 0x3;
pub const ERA_WRITE_MASK: u64 = u64::MAX;
pub const BADV_WRITE_MASK: u64 = u64::MAX;
pub const BADI_WRITE_MASK: u64 = u64::MAX;
pub const EENTRY_WRITE_MASK: u64 = !0x3F_u64;
pub const TLBIDX_WRITE_MASK: u64 = 0xBF00_0FFF;
pub const TLBEHI_WRITE_MASK: u64 = !0x1FFF_u64;
pub const TLBELO_WRITE_MASK: u64 = 0x2000_FFFF_FFFF_FFFF;
pub const ASID_WRITE_MASK: u64 = 0x3FF;
pub const PGDL_WRITE_MASK: u64 = !0xFFF_u64;
pub const PGDH_WRITE_MASK: u64 = !0xFFF_u64;
pub const PGD_WRITE_MASK: u64 = 0;
pub const PWCL_WRITE_MASK: u64 = 0xFFFF_FFFF;
pub const PWCH_WRITE_MASK: u64 = 0x3F_FFFF_FFFF;
pub const STLBPS_WRITE_MASK: u64 = 0x3F;
pub const RVACFG_WRITE_MASK: u64 = 0;
pub const CPUID_WRITE_MASK: u64 = 0;
pub const PRCFG1_WRITE_MASK: u64 = 0;
pub const PRCFG2_WRITE_MASK: u64 = 0;
pub const PRCFG3_WRITE_MASK: u64 = 0;
pub const TID_WRITE_MASK: u64 = 0xFFFF_FFFF;
pub const TCFG_WRITE_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
pub const TVAL_WRITE_MASK: u64 = 0;
pub const CNTC_WRITE_MASK: u64 = u64::MAX;
pub const TICLR_WRITE_MASK: u64 = 0x1;
pub const LLBCTL_WRITE_MASK: u64 = 0x4;
pub const TLBRENTRY_WRITE_MASK: u64 = !0x3F_u64;
pub const TLBRBADV_WRITE_MASK: u64 = u64::MAX;
pub const TLBRERA_WRITE_MASK: u64 = u64::MAX;
pub const TLBRSAVE_WRITE_MASK: u64 = u64::MAX;
pub const TLBRELO_WRITE_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
pub const TLBREHI_WRITE_MASK: u64 = !0x3F_u64;
pub const TLBRPRMD_WRITE_MASK: u64 = 0x7;
pub const DMW_WRITE_MASK: u64 = 0xF000_0000_0E00_0039;
pub const SAVE_WRITE_MASK: u64 = u64::MAX;

impl LoongArchCpu {
    pub fn csr_read(&self, num: u32) -> u64 {
        match num {
            CSR_CRMD => self.crmd,
            CSR_PRMD => self.prmd,
            CSR_EUEN => self.euen,
            CSR_MISC => 0,
            CSR_ECFG => self.ecfg,
            CSR_ESTAT => self.estat,
            CSR_ERA => self.era,
            CSR_BADV => self.badv,
            CSR_BADI => self.badi,
            CSR_EENTRY => self.eentry,
            CSR_TLBIDX => self.tlbidx,
            CSR_TLBEHI => self.tlbehi,
            CSR_TLBELO0 => self.tlbelo0,
            CSR_TLBELO1 => self.tlbelo1,
            CSR_ASID => self.asid | (0x0A << 16),
            CSR_PGDL => self.pgdl,
            CSR_PGDH => self.pgdh,
            CSR_PGD => {
                let badv = self.badv;
                if badv & (1 << 63) != 0 {
                    self.pgdh
                } else {
                    self.pgdl
                }
            }
            CSR_PWCL => self.pwcl,
            CSR_PWCH => self.pwch,
            CSR_STLBPS => self.stlbps,
            CSR_RVACFG => self.rvacfg,
            CSR_CPUID => self.cpuid,
            CSR_PRCFG1 => self.prcfg1,
            CSR_PRCFG2 => self.prcfg2,
            CSR_PRCFG3 => self.prcfg3,
            CSR_TID => self.tid,
            CSR_TCFG => self.tcfg,
            CSR_TVAL => self.tval,
            CSR_CNTC => self.cntc,
            CSR_TICLR => 0,
            CSR_LLBCTL => self.llbctl,
            CSR_TLBRENTRY => self.tlbrentry,
            CSR_TLBRBADV => self.tlbrbadv,
            CSR_TLBRERA => self.tlbrera,
            CSR_TLBRSAVE => self.tlbrsave,
            CSR_TLBRELO0 => self.tlbrelo0,
            CSR_TLBRELO1 => self.tlbrelo1,
            CSR_TLBREHI => self.tlbrehi,
            CSR_TLBRPRMD => self.tlbrprmd,
            CSR_DMW0 => self.dmw[0],
            CSR_DMW1 => self.dmw[1],
            CSR_DMW2 => self.dmw[2],
            CSR_DMW3 => self.dmw[3],
            n if (CSR_SAVE0..=CSR_SAVE_LAST).contains(&n) => {
                self.save[(n - CSR_SAVE0) as usize]
            }
            _ => 0,
        }
    }

    pub fn csr_write(&mut self, num: u32, val: u64) {
        let mask = csr_write_mask(num);
        if mask == 0 {
            return;
        }
        self.csr_write_masked(num, val, mask);
    }

    pub fn csr_xchg(&mut self, num: u32, val: u64, mask: u64) -> u64 {
        let old = self.csr_read(num);
        let wmask = csr_write_mask(num) & mask;
        if wmask != 0 {
            let new = (old & !wmask) | (val & wmask);
            self.csr_write_raw(num, new);
        }
        old
    }

    fn csr_write_masked(&mut self, num: u32, val: u64, mask: u64) {
        let old = self.csr_read(num);
        let new = (old & !mask) | (val & mask);
        self.csr_write_raw(num, new);
    }

    fn csr_write_raw(&mut self, num: u32, val: u64) {
        match num {
            CSR_CRMD => self.crmd = val,
            CSR_PRMD => self.prmd = val,
            CSR_EUEN => self.euen = val,
            CSR_ECFG => self.ecfg = val,
            CSR_ESTAT => {
                // Only IS[1:0] writable; preserve hardware-set bits
                self.estat =
                    (self.estat & !ESTAT_WRITE_MASK) | (val & ESTAT_WRITE_MASK);
            }
            CSR_ERA => self.era = val,
            CSR_BADV => self.badv = val,
            CSR_BADI => self.badi = val,
            CSR_EENTRY => self.eentry = val,
            CSR_TLBIDX => self.tlbidx = val,
            CSR_TLBEHI => self.tlbehi = val,
            CSR_TLBELO0 => self.tlbelo0 = val,
            CSR_TLBELO1 => self.tlbelo1 = val,
            CSR_ASID => self.asid = val,
            CSR_PGDL => self.pgdl = val,
            CSR_PGDH => self.pgdh = val,
            CSR_PWCL => self.pwcl = val,
            CSR_PWCH => self.pwch = val,
            CSR_STLBPS => self.stlbps = val,
            CSR_TID => self.tid = val,
            CSR_TCFG => {
                self.tcfg = val;
                if val & 1 != 0 {
                    self.tval = val & !0x3;
                }
            }
            CSR_CNTC => self.cntc = val,
            CSR_TICLR => {
                if val & 1 != 0 {
                    self.estat &= !(1 << 11);
                }
            }
            CSR_LLBCTL => {
                if val & 0x4 != 0 {
                    self.llbctl &= !1;
                }
            }
            CSR_TLBRENTRY => self.tlbrentry = val,
            CSR_TLBRBADV => self.tlbrbadv = val,
            CSR_TLBRERA => self.tlbrera = val,
            CSR_TLBRSAVE => self.tlbrsave = val,
            CSR_TLBRELO0 => self.tlbrelo0 = val,
            CSR_TLBRELO1 => self.tlbrelo1 = val,
            CSR_TLBREHI => self.tlbrehi = val,
            CSR_TLBRPRMD => self.tlbrprmd = val,
            CSR_DMW0 => self.dmw[0] = val,
            CSR_DMW1 => self.dmw[1] = val,
            CSR_DMW2 => self.dmw[2] = val,
            CSR_DMW3 => self.dmw[3] = val,
            n if (CSR_SAVE0..=CSR_SAVE_LAST).contains(&n) => {
                self.save[(n - CSR_SAVE0) as usize] = val;
            }
            _ => {}
        }
    }
}

pub const fn csr_write_mask(num: u32) -> u64 {
    match num {
        CSR_CRMD => CRMD_WRITE_MASK,
        CSR_PRMD => PRMD_WRITE_MASK,
        CSR_EUEN => EUEN_WRITE_MASK,
        CSR_MISC => MISC_WRITE_MASK,
        CSR_ECFG => ECFG_WRITE_MASK,
        CSR_ESTAT => ESTAT_WRITE_MASK,
        CSR_ERA => ERA_WRITE_MASK,
        CSR_BADV => BADV_WRITE_MASK,
        CSR_BADI => BADI_WRITE_MASK,
        CSR_EENTRY => EENTRY_WRITE_MASK,
        CSR_TLBIDX => TLBIDX_WRITE_MASK,
        CSR_TLBEHI => TLBEHI_WRITE_MASK,
        CSR_TLBELO0 | CSR_TLBELO1 => TLBELO_WRITE_MASK,
        CSR_ASID => ASID_WRITE_MASK,
        CSR_PGDL => PGDL_WRITE_MASK,
        CSR_PGDH => PGDH_WRITE_MASK,
        CSR_PGD => PGD_WRITE_MASK,
        CSR_PWCL => PWCL_WRITE_MASK,
        CSR_PWCH => PWCH_WRITE_MASK,
        CSR_STLBPS => STLBPS_WRITE_MASK,
        CSR_RVACFG => RVACFG_WRITE_MASK,
        CSR_CPUID => CPUID_WRITE_MASK,
        CSR_PRCFG1 => PRCFG1_WRITE_MASK,
        CSR_PRCFG2 => PRCFG2_WRITE_MASK,
        CSR_PRCFG3 => PRCFG3_WRITE_MASK,
        CSR_TID => TID_WRITE_MASK,
        CSR_TCFG => TCFG_WRITE_MASK,
        CSR_TVAL => TVAL_WRITE_MASK,
        CSR_CNTC => CNTC_WRITE_MASK,
        CSR_TICLR => TICLR_WRITE_MASK,
        CSR_LLBCTL => LLBCTL_WRITE_MASK,
        CSR_TLBRENTRY => TLBRENTRY_WRITE_MASK,
        CSR_TLBRBADV => TLBRBADV_WRITE_MASK,
        CSR_TLBRERA => TLBRERA_WRITE_MASK,
        CSR_TLBRSAVE => TLBRSAVE_WRITE_MASK,
        CSR_TLBRELO0 | CSR_TLBRELO1 => TLBRELO_WRITE_MASK,
        CSR_TLBREHI => TLBREHI_WRITE_MASK,
        CSR_TLBRPRMD => TLBRPRMD_WRITE_MASK,
        CSR_DMW0 | CSR_DMW1 | CSR_DMW2 | CSR_DMW3 => DMW_WRITE_MASK,
        n if n >= CSR_SAVE0 && n <= CSR_SAVE_LAST => SAVE_WRITE_MASK,
        _ => 0,
    }
}
