use super::cpu::LoongArchCpu;
use super::csr::*;

pub const MTLB_SIZE: usize = 64;
pub const STLB_SETS: usize = 256;
pub const STLB_WAYS: usize = 8;

#[derive(Clone, Copy, Default)]
pub struct TlbEntry {
    pub vppn: u64,
    pub page_size: u8,
    pub asid: u16,
    pub g: bool,
    pub valid: bool,
    pub ppn0: u64,
    pub ppn1: u64,
    pub plv0: u8,
    pub plv1: u8,
    pub mat0: u8,
    pub mat1: u8,
    pub d0: bool,
    pub d1: bool,
    pub v0: bool,
    pub v1: bool,
    pub nr0: bool,
    pub nr1: bool,
}

pub struct LoongArchMmu {
    pub mtlb: [TlbEntry; MTLB_SIZE],
    pub stlb: [[TlbEntry; STLB_WAYS]; STLB_SETS],
}

impl LoongArchMmu {
    pub const fn new() -> Self {
        const EMPTY: TlbEntry = TlbEntry {
            vppn: 0,
            page_size: 0,
            asid: 0,
            g: false,
            valid: false,
            ppn0: 0,
            ppn1: 0,
            plv0: 0,
            plv1: 0,
            mat0: 0,
            mat1: 0,
            d0: false,
            d1: false,
            v0: false,
            v1: false,
            nr0: false,
            nr1: false,
        };
        Self {
            mtlb: [EMPTY; MTLB_SIZE],
            stlb: [[EMPTY; STLB_WAYS]; STLB_SETS],
        }
    }
}

impl Default for LoongArchMmu {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlbLookupResult {
    Hit { pa: u64, mat: u8 },
    Miss,
    Invalid,
    Dirty,
    PrivViolation,
    ExecProtect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    Load,
    Store,
    Fetch,
}

pub fn dmw_match(cpu: &LoongArchCpu, va: u64) -> Option<u64> {
    let plv = (cpu.crmd() & CRMD_PLV_MASK) as u8;
    for i in 0..4 {
        let dmw = cpu.dmw(i);
        if dmw == 0 {
            continue;
        }
        let plv_match = match plv {
            0 => dmw & (1 << 0) != 0,
            3 => dmw & (1 << 3) != 0,
            _ => false,
        };
        if !plv_match {
            continue;
        }
        let vseg = (dmw >> 60) & 0xF;
        let va_vseg = (va >> 60) & 0xF;
        if vseg == va_vseg {
            let pseg = (dmw >> 25) & 0x7;
            let pa = (pseg << 60) | (va & 0x0FFF_FFFF_FFFF_FFFF);
            return Some(pa);
        }
    }
    None
}

impl LoongArchMmu {
    pub fn tlb_lookup(
        &self,
        va: u64,
        asid: u16,
        stlb_ps: u8,
        access: AccessType,
        plv: u8,
    ) -> TlbLookupResult {
        if let Some(r) = self.search_mtlb(va, asid, access, plv) {
            return r;
        }
        self.search_stlb(va, asid, stlb_ps, access, plv)
    }

    fn search_mtlb(
        &self,
        va: u64,
        asid: u16,
        access: AccessType,
        plv: u8,
    ) -> Option<TlbLookupResult> {
        for entry in &self.mtlb {
            if !entry.valid {
                continue;
            }
            if let Some(r) = check_entry(entry, va, asid, access, plv) {
                return Some(r);
            }
        }
        None
    }

    fn search_stlb(
        &self,
        va: u64,
        asid: u16,
        stlb_ps: u8,
        access: AccessType,
        plv: u8,
    ) -> TlbLookupResult {
        if stlb_ps == 0 {
            return TlbLookupResult::Miss;
        }
        let vpn = va >> stlb_ps;
        let set_idx = (vpn as usize) & (STLB_SETS - 1);
        for entry in &self.stlb[set_idx] {
            if !entry.valid {
                continue;
            }
            if let Some(r) = check_entry(entry, va, asid, access, plv) {
                return r;
            }
        }
        TlbLookupResult::Miss
    }
}

fn check_entry(
    entry: &TlbEntry,
    va: u64,
    asid: u16,
    access: AccessType,
    plv: u8,
) -> Option<TlbLookupResult> {
    let ps = entry.page_size;
    let page_mask = (1u64 << ps) - 1;
    let vppn_mask = !((1u64 << (ps + 1)) - 1);
    if (va & vppn_mask) != (entry.vppn << 12 & vppn_mask) {
        return None;
    }
    if !entry.g && entry.asid != asid {
        return None;
    }
    let odd = (va >> ps) & 1 != 0;
    let (ppn, v, d, plv_ok, nr) = if odd {
        (entry.ppn1, entry.v1, entry.d1, entry.plv1, entry.nr1)
    } else {
        (entry.ppn0, entry.v0, entry.d0, entry.plv0, entry.nr0)
    };
    if !v {
        return Some(TlbLookupResult::Invalid);
    }
    if plv > plv_ok {
        return Some(TlbLookupResult::PrivViolation);
    }
    if access == AccessType::Store && !d {
        return Some(TlbLookupResult::Dirty);
    }
    if access == AccessType::Fetch && nr {
        return Some(TlbLookupResult::ExecProtect);
    }
    let pa = (ppn << 12) | (va & page_mask);
    let mat = if odd { entry.mat1 } else { entry.mat0 };
    Some(TlbLookupResult::Hit { pa, mat })
}
