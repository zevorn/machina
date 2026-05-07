use super::super::cpu_model::{RiscvCpuProfile, RiscvVendor};
use super::super::csr::PrivLevel;

pub const CSR_TH_MXSTATUS: u16 = 0x7c0;
pub const CSR_TH_MHCR: u16 = 0x7c1;
pub const CSR_TH_MCOR: u16 = 0x7c2;
pub const CSR_TH_MCCR2: u16 = 0x7c3;
pub const CSR_TH_MHINT: u16 = 0x7c5;
pub const CSR_TH_MRVBR: u16 = 0x7c7;
pub const CSR_TH_MCOUNTERWEN: u16 = 0x7c9;
pub const CSR_TH_MCOUNTERINTEN: u16 = 0x7ca;
pub const CSR_TH_MCOUNTEROF: u16 = 0x7cb;
pub const CSR_TH_MCINS: u16 = 0x7d2;
pub const CSR_TH_MCINDEX: u16 = 0x7d3;
pub const CSR_TH_MCDATA0: u16 = 0x7d4;
pub const CSR_TH_MCDATA1: u16 = 0x7d5;
pub const CSR_TH_MSMPR: u16 = 0x7f3;
pub const CSR_TH_CPUID: u16 = 0xfc0;
pub const CSR_TH_MAPBADDR: u16 = 0xfc1;
pub const CSR_TH_SXSTATUS: u16 = 0x5c0;
pub const CSR_TH_SHCR: u16 = 0x5c1;
pub const CSR_TH_SCER2: u16 = 0x5c2;
pub const CSR_TH_SCER: u16 = 0x5c3;
pub const CSR_TH_SCOUNTERINTEN: u16 = 0x5c4;
pub const CSR_TH_SCOUNTEROF: u16 = 0x5c5;
pub const CSR_TH_SCYCLE: u16 = 0x5e0;
pub const CSR_TH_SMIR: u16 = 0x9c0;
pub const CSR_TH_SMLO0: u16 = 0x9c1;
pub const CSR_TH_SMEH: u16 = 0x9c2;
pub const CSR_TH_SMCIR: u16 = 0x9c3;
pub const CSR_TH_FXCR: u16 = 0x800;

pub const TH_STATUS_UCME: u64 = 1 << 16;
pub const TH_STATUS_THEADISAEE: u64 = 1 << 22;

const CAUSE_ILLEGAL_INSN: u64 = 2;

fn is_thead(profile: &RiscvCpuProfile) -> bool {
    profile.vendor == RiscvVendor::Thead
}

fn require_priv(addr: u16, current: PrivLevel) -> Result<(), u64> {
    let required = match addr {
        CSR_TH_FXCR => PrivLevel::User,
        CSR_TH_SXSTATUS
        | CSR_TH_SHCR
        | CSR_TH_SCER2
        | CSR_TH_SCER
        | CSR_TH_SCOUNTERINTEN
        | CSR_TH_SCOUNTEROF
        | CSR_TH_SCYCLE
        | CSR_TH_SMIR
        | CSR_TH_SMLO0
        | CSR_TH_SMEH
        | CSR_TH_SMCIR
        | 0x5e3..=0x5ff => PrivLevel::Supervisor,
        _ => PrivLevel::Machine,
    };
    if current < required {
        Err(CAUSE_ILLEGAL_INSN)
    } else {
        Ok(())
    }
}

pub fn read(
    addr: u16,
    current: PrivLevel,
    profile: &RiscvCpuProfile,
) -> Result<u64, u64> {
    if !is_thead(profile) {
        return Err(CAUSE_ILLEGAL_INSN);
    }
    require_priv(addr, current)?;
    match addr {
        CSR_TH_MXSTATUS | CSR_TH_SXSTATUS => {
            Ok(TH_STATUS_UCME | TH_STATUS_THEADISAEE)
        }
        CSR_TH_MHCR
        | CSR_TH_MCOR
        | CSR_TH_MCCR2
        | CSR_TH_MHINT
        | CSR_TH_MRVBR
        | CSR_TH_MCOUNTERWEN
        | CSR_TH_MCOUNTERINTEN
        | CSR_TH_MCOUNTEROF
        | CSR_TH_MCINS
        | CSR_TH_MCINDEX
        | CSR_TH_MCDATA0
        | CSR_TH_MCDATA1
        | CSR_TH_MSMPR
        | CSR_TH_CPUID
        | CSR_TH_MAPBADDR
        | CSR_TH_SHCR
        | CSR_TH_SCER2
        | CSR_TH_SCER
        | CSR_TH_SCOUNTERINTEN
        | CSR_TH_SCOUNTEROF
        | CSR_TH_SCYCLE
        | 0x5e3..=0x5ff
        | CSR_TH_SMIR
        | CSR_TH_SMLO0
        | CSR_TH_SMEH
        | CSR_TH_SMCIR
        | CSR_TH_FXCR => Ok(0),
        _ => Err(CAUSE_ILLEGAL_INSN),
    }
}

pub fn write(
    addr: u16,
    current: PrivLevel,
    profile: &RiscvCpuProfile,
) -> Result<(), u64> {
    let _ = read(addr, current, profile)?;
    Ok(())
}
