use std::mem::offset_of;
use std::sync::atomic::{AtomicBool, AtomicU32};

use super::mmu::LoongArchMmu;

pub const NUM_GPRS: usize = 32;
pub const NUM_FPRS: usize = 32;
pub const NUM_FCC: usize = 8;
pub const NUM_DMW: usize = 4;
pub const NUM_SAVE: usize = 16;

#[repr(C)]
pub struct LoongArchCpu {
    pub(crate) gpr: [u64; NUM_GPRS],
    pub(crate) pc: u64,
    pub(crate) guest_base: u64,

    pub(crate) fpr: [u64; NUM_FPRS],
    pub(crate) fcsr0: u32,
    pub(crate) fcc: [u8; NUM_FCC],

    pub(crate) crmd: u64,
    pub(crate) prmd: u64,
    pub(crate) euen: u64,
    pub(crate) ecfg: u64,
    pub(crate) estat: u64,
    pub(crate) era: u64,
    pub(crate) badv: u64,
    pub(crate) badi: u64,
    pub(crate) eentry: u64,
    pub(crate) tlbidx: u64,
    pub(crate) tlbehi: u64,
    pub(crate) tlbelo0: u64,
    pub(crate) tlbelo1: u64,
    pub(crate) asid: u64,
    pub(crate) pgdl: u64,
    pub(crate) pgdh: u64,
    pub(crate) pgd: u64,
    pub(crate) pwcl: u64,
    pub(crate) pwch: u64,
    pub(crate) stlbps: u64,
    pub(crate) rvacfg: u64,
    pub(crate) cpuid: u64,
    pub(crate) prcfg1: u64,
    pub(crate) prcfg2: u64,
    pub(crate) prcfg3: u64,
    pub(crate) llbctl: u64,
    pub(crate) ll_res_addr: u64,
    pub(crate) ll_res_val: u64,
    pub(crate) tlbrentry: u64,
    pub(crate) tlbrbadv: u64,
    pub(crate) tlbrera: u64,
    pub(crate) tlbrsave: u64,
    pub(crate) tlbrelo0: u64,
    pub(crate) tlbrelo1: u64,
    pub(crate) tlbrehi: u64,
    pub(crate) tlbrprmd: u64,
    pub(crate) dmw: [u64; NUM_DMW],

    pub(crate) tcfg: u64,
    pub(crate) tval: u64,
    pub(crate) cntc: u64,
    pub(crate) ticlr: u64,
    pub(crate) tid: u64,

    pub(crate) save: [u64; NUM_SAVE],

    pub(crate) interrupt_request: AtomicU32,
    pub(crate) halted: AtomicBool,

    pub(crate) mmu: LoongArchMmu,

    pub(crate) as_ptr: u64,
    pub(crate) ram_base: u64,
    pub(crate) ram_end: u64,
    pub(crate) code_pages_ptr: u64,
    pub(crate) code_pages_len: u64,
    pub(crate) tb_flush_pending: bool,
}

pub const GPR_OFFSET: usize = offset_of!(LoongArchCpu, gpr);
pub const PC_OFFSET: usize = offset_of!(LoongArchCpu, pc);
pub const GUEST_BASE_OFFSET: usize = offset_of!(LoongArchCpu, guest_base);
pub const FPR_OFFSET: usize = offset_of!(LoongArchCpu, fpr);
pub const FCSR0_OFFSET: usize = offset_of!(LoongArchCpu, fcsr0);
pub const FCC_OFFSET: usize = offset_of!(LoongArchCpu, fcc);
pub const CRMD_OFFSET: usize = offset_of!(LoongArchCpu, crmd);
pub const ESTAT_OFFSET: usize = offset_of!(LoongArchCpu, estat);
pub const ERA_OFFSET: usize = offset_of!(LoongArchCpu, era);
pub const BADV_OFFSET: usize = offset_of!(LoongArchCpu, badv);
pub const EENTRY_OFFSET: usize = offset_of!(LoongArchCpu, eentry);
pub const LLBCTL_OFFSET: usize = offset_of!(LoongArchCpu, llbctl);
pub const LL_RES_ADDR_OFFSET: usize = offset_of!(LoongArchCpu, ll_res_addr);
pub const LL_RES_VAL_OFFSET: usize = offset_of!(LoongArchCpu, ll_res_val);
pub const RAM_BASE_OFFSET: usize = offset_of!(LoongArchCpu, ram_base);
pub const RAM_END_OFFSET: usize = offset_of!(LoongArchCpu, ram_end);
pub const GUEST_BASE_CPU_OFFSET: usize = offset_of!(LoongArchCpu, guest_base);

#[must_use]
pub const fn gpr_offset(i: usize) -> usize {
    GPR_OFFSET + i * 8
}

#[must_use]
pub const fn fpr_offset(i: usize) -> usize {
    FPR_OFFSET + i * 8
}

impl LoongArchCpu {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            gpr: [0; NUM_GPRS],
            pc: 0,
            guest_base: 0,
            fpr: [0; NUM_FPRS],
            fcsr0: 0,
            fcc: [0; NUM_FCC],
            crmd: 0x0000_0008,
            prmd: 0,
            euen: 0,
            ecfg: 0,
            estat: 0,
            era: 0,
            badv: 0,
            badi: 0,
            eentry: 0,
            tlbidx: 0,
            tlbehi: 0,
            tlbelo0: 0,
            tlbelo1: 0,
            asid: 0,
            pgdl: 0,
            pgdh: 0,
            pgd: 0,
            pwcl: 0,
            pwch: 0,
            stlbps: 0,
            rvacfg: 0,
            cpuid: 0,
            prcfg1: 0x72F8,
            prcfg2: 0x3FFF_F000,
            prcfg3: 0x0080_73F2,
            llbctl: 0,
            ll_res_addr: u64::MAX,
            ll_res_val: 0,
            tlbrentry: 0,
            tlbrbadv: 0,
            tlbrera: 0,
            tlbrsave: 0,
            tlbrelo0: 0,
            tlbrelo1: 0,
            tlbrehi: 0,
            tlbrprmd: 0,
            dmw: [0; NUM_DMW],
            tcfg: 0,
            tval: 0,
            cntc: 0,
            ticlr: 0,
            tid: 0,
            save: [0; NUM_SAVE],
            interrupt_request: AtomicU32::new(0),
            halted: AtomicBool::new(false),
            mmu: LoongArchMmu::new(),
            as_ptr: 0,
            ram_base: 0,
            ram_end: 0,
            code_pages_ptr: 0,
            code_pages_len: 0,
            tb_flush_pending: false,
        }
    }

    #[must_use]
    pub const fn crmd(&self) -> u64 {
        self.crmd
    }

    pub(crate) const fn set_crmd(&mut self, val: u64) {
        self.crmd = val;
    }

    #[must_use]
    pub const fn prmd(&self) -> u64 {
        self.prmd
    }

    #[must_use]
    pub const fn estat(&self) -> u64 {
        self.estat
    }

    #[must_use]
    pub const fn era(&self) -> u64 {
        self.era
    }

    pub(crate) const fn set_era(&mut self, val: u64) {
        self.era = val;
    }

    #[must_use]
    pub const fn euen(&self) -> u64 {
        self.euen
    }

    pub(crate) const fn set_euen(&mut self, val: u64) {
        self.euen = val;
    }

    #[must_use]
    pub const fn eentry(&self) -> u64 {
        self.eentry
    }

    pub(crate) const fn set_eentry(&mut self, val: u64) {
        self.eentry = val;
    }

    #[must_use]
    pub const fn ecfg(&self) -> u64 {
        self.ecfg
    }

    pub(crate) const fn set_ecfg(&mut self, val: u64) {
        self.ecfg = val;
    }

    #[must_use]
    pub const fn pc(&self) -> u64 {
        self.pc
    }

    pub const fn set_pc(&mut self, val: u64) {
        self.pc = val;
    }

    #[must_use]
    pub const fn dmw(&self, idx: usize) -> u64 {
        self.dmw[idx]
    }

    #[must_use]
    pub fn pending_interrupt(&self) -> bool {
        use super::csr::CRMD_IE;
        if self.crmd & CRMD_IE == 0 {
            return false;
        }
        (self.estat & self.ecfg & 0x1FFF) != 0
    }

    pub fn read_fcsr(&self) -> u32 {
        self.fcsr0
    }

    pub fn write_fcsr(&mut self, val: u32) {
        self.fcsr0 = val & 0x1F1F_031F;
    }

    #[must_use]
    pub fn mmu(&self) -> &LoongArchMmu {
        &self.mmu
    }

    pub fn mmu_mut(&mut self) -> &mut LoongArchMmu {
        &mut self.mmu
    }

    pub fn tlb_search(&self) -> Option<usize> {
        use super::mmu::{MTLB_SIZE, STLB_SETS, STLB_WAYS};
        let vppn = self.tlbehi >> 13;
        let asid = (self.asid & 0x3FF) as u16;
        let ps = self.stlbps as u8;
        for i in 0..MTLB_SIZE {
            let e = &self.mmu.mtlb[i];
            if !e.valid {
                continue;
            }
            if (e.g || e.asid == asid) && e.vppn == vppn {
                return Some(i);
            }
        }
        if ps > 0 {
            let vpn = self.tlbehi >> ps;
            let set_idx = (vpn as usize) & (STLB_SETS - 1);
            for w in 0..STLB_WAYS {
                let e = &self.mmu.stlb[set_idx][w];
                if !e.valid {
                    continue;
                }
                if (e.g || e.asid == asid) && e.vppn == vppn {
                    return Some(MTLB_SIZE + set_idx * STLB_WAYS + w);
                }
            }
        }
        None
    }

    pub fn tlb_read(&mut self, idx: usize) {
        use super::mmu::{MTLB_SIZE, STLB_SETS, STLB_WAYS};
        let max = MTLB_SIZE + STLB_SETS * STLB_WAYS;
        if idx >= max {
            self.tlbidx |= 1 << 31;
            return;
        }
        let e = *self.get_tlb_entry(idx);
        if !e.valid {
            self.tlbidx |= 1 << 31;
            return;
        }
        self.tlbidx = (self.tlbidx & !(0x3F << 24 | 0xFFF))
            | ((u64::from(e.page_size)) << 24)
            | (idx as u64 & 0xFFF);
        self.tlbehi = e.vppn << 13;
        self.tlbelo0 =
            self.encode_tlbelo(e.ppn0, e.v0, e.d0, e.plv0, e.mat0, e.g, e.nr0);
        self.tlbelo1 =
            self.encode_tlbelo(e.ppn1, e.v1, e.d1, e.plv1, e.mat1, e.g, e.nr1);
        self.asid = (self.asid & !0x3FF) | u64::from(e.asid);
    }

    pub fn tlb_write(&mut self, idx: usize) {
        use super::mmu::{TlbEntry, MTLB_SIZE, STLB_SETS, STLB_WAYS};
        let ps = ((self.tlbidx >> 24) & 0x3F) as u8;
        let entry = TlbEntry {
            vppn: self.tlbehi >> 13,
            page_size: ps,
            asid: (self.asid & 0x3FF) as u16,
            g: (self.tlbelo0 & self.tlbelo1 & (1 << 6)) != 0,
            valid: true,
            ppn0: (self.tlbelo0 >> 12) & 0xF_FFFF_FFFF,
            ppn1: (self.tlbelo1 >> 12) & 0xF_FFFF_FFFF,
            plv0: ((self.tlbelo0 >> 2) & 0x3) as u8,
            plv1: ((self.tlbelo1 >> 2) & 0x3) as u8,
            mat0: ((self.tlbelo0 >> 4) & 0x3) as u8,
            mat1: ((self.tlbelo1 >> 4) & 0x3) as u8,
            d0: self.tlbelo0 & (1 << 1) != 0,
            d1: self.tlbelo1 & (1 << 1) != 0,
            v0: self.tlbelo0 & 1 != 0,
            v1: self.tlbelo1 & 1 != 0,
            nr0: self.tlbelo0 & (1 << 61) != 0,
            nr1: self.tlbelo1 & (1 << 61) != 0,
        };
        if idx < MTLB_SIZE {
            self.mmu.mtlb[idx] = entry;
        } else {
            let flat = idx - MTLB_SIZE;
            let set = flat / STLB_WAYS;
            let way = flat % STLB_WAYS;
            if set < STLB_SETS && way < STLB_WAYS {
                self.mmu.stlb[set][way] = entry;
            }
        }
    }

    pub fn tlb_fill(&mut self) {
        use super::mmu::MTLB_SIZE;
        let idx = (self.tlbidx as usize) & (MTLB_SIZE - 1);
        self.tlb_write(idx);
    }

    pub fn invtlb(&mut self, op: u32, asid: u16, va: u64) {
        use super::mmu::STLB_SETS;
        match op {
            0 | 1 => {
                self.mmu.mtlb.iter_mut().for_each(|e| e.valid = false);
                self.mmu.stlb.iter_mut().for_each(|s| {
                    s.iter_mut().for_each(|e| e.valid = false);
                });
            }
            2 => {
                self.mmu.mtlb.iter_mut().for_each(|e| {
                    if e.g {
                        e.valid = false;
                    }
                });
                self.mmu.stlb.iter_mut().for_each(|s| {
                    s.iter_mut().for_each(|e| {
                        if e.g {
                            e.valid = false;
                        }
                    });
                });
            }
            3 => {
                self.mmu.mtlb.iter_mut().for_each(|e| {
                    if !e.g {
                        e.valid = false;
                    }
                });
                self.mmu.stlb.iter_mut().for_each(|s| {
                    s.iter_mut().for_each(|e| {
                        if !e.g {
                            e.valid = false;
                        }
                    });
                });
            }
            4 => {
                self.mmu.mtlb.iter_mut().for_each(|e| {
                    if !e.g && e.asid == asid {
                        e.valid = false;
                    }
                });
                self.mmu.stlb.iter_mut().for_each(|s| {
                    s.iter_mut().for_each(|e| {
                        if !e.g && e.asid == asid {
                            e.valid = false;
                        }
                    });
                });
            }
            5 => {
                let vppn = va >> 13;
                self.mmu.mtlb.iter_mut().for_each(|e| {
                    if !e.g && e.asid == asid && e.vppn == vppn {
                        e.valid = false;
                    }
                });
                if self.stlbps > 0 {
                    let vpn = va >> self.stlbps;
                    let set = (vpn as usize) & (STLB_SETS - 1);
                    self.mmu.stlb[set].iter_mut().for_each(|e| {
                        if !e.g && e.asid == asid && e.vppn == vppn {
                            e.valid = false;
                        }
                    });
                }
            }
            6 => {
                let vppn = va >> 13;
                self.mmu.mtlb.iter_mut().for_each(|e| {
                    if (e.g || e.asid == asid) && e.vppn == vppn {
                        e.valid = false;
                    }
                });
                if self.stlbps > 0 {
                    let vpn = va >> self.stlbps;
                    let set = (vpn as usize) & (STLB_SETS - 1);
                    self.mmu.stlb[set].iter_mut().for_each(|e| {
                        if (e.g || e.asid == asid) && e.vppn == vppn {
                            e.valid = false;
                        }
                    });
                }
            }
            _ => {}
        }
    }

    fn get_tlb_entry(&self, idx: usize) -> &super::mmu::TlbEntry {
        use super::mmu::{MTLB_SIZE, STLB_SETS, STLB_WAYS};
        if idx < MTLB_SIZE {
            &self.mmu.mtlb[idx]
        } else {
            let flat = idx - MTLB_SIZE;
            let set = flat / STLB_WAYS;
            let way = flat % STLB_WAYS;
            assert!(set < STLB_SETS && way < STLB_WAYS);
            &self.mmu.stlb[set][way]
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_tlbelo(
        &self,
        ppn: u64,
        v: bool,
        d: bool,
        plv: u8,
        mat: u8,
        g: bool,
        nr: bool,
    ) -> u64 {
        (u64::from(v))
            | (u64::from(d) << 1)
            | (u64::from(plv) << 2)
            | (u64::from(mat) << 4)
            | (u64::from(g) << 6)
            | (ppn << 12)
            | (u64::from(nr) << 61)
    }

    pub fn env_ptr(&mut self) -> *mut u8 {
        std::ptr::from_mut(self).cast()
    }

    pub fn set_estat_hw(&mut self, val: u64) {
        self.estat = val;
    }

    pub fn set_badv_raw(&mut self, val: u64) {
        self.badv = val;
    }

    #[must_use]
    pub fn tval(&self) -> u64 {
        self.tval
    }
}

impl LoongArchCpu {
    pub fn timer_tick(&mut self, cycles: u64) {
        if self.tcfg & 1 == 0 {
            return;
        }
        if self.tval == 0 {
            return;
        }
        if self.tval <= cycles {
            // Timer fired
            self.estat |= 1 << 11;
            let periodic = self.tcfg & 2 != 0;
            if periodic {
                let init_val = self.tcfg & !0x3;
                self.tval = init_val;
            } else {
                self.tval = 0;
            }
        } else {
            self.tval -= cycles;
        }
    }
}

impl Default for LoongArchCpu {
    fn default() -> Self {
        Self::new()
    }
}
