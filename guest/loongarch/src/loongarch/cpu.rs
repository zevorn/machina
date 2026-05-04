use std::mem::offset_of;
use std::sync::atomic::{AtomicBool, AtomicU32};

use super::csr::{
    ASID_WRITE_MASK, CRMD_DA, CRMD_IE, CRMD_PG, CRMD_PLV_MASK, ESTAT_IS_MASK,
};
use super::exception::{
    ECODE_FPE, ECODE_PIF, ECODE_PIL, ECODE_PIS, ECODE_PME, ECODE_PNR,
    ECODE_PNX, ECODE_PPI, ECODE_TLBR,
};
use super::ext::LoongArchCfg;
use super::mmu::{
    self, AccessType, LoongArchMmu, TlbLookupResult, FAST_TLB_MMIO_ADDEND,
    FAST_TLB_PAGE_MASK,
};

pub const NUM_GPRS: usize = 32;
pub const NUM_FPRS: usize = 32;
pub const NUM_FCC: usize = 8;
pub const NUM_DMW: usize = 4;
pub const NUM_SAVE: usize = 8;
const TIMER_INTERRUPT_BIT: u64 = 1 << 11;
const ECFG_VS_SHIFT: u64 = 16;
const ECFG_VS_MASK: u64 = 0x7;
const TLBENTRY_HUGE: u64 = 1 << 6;
const TLBENTRY_HGLOBAL: u64 = 1 << 12;
const TLBENTRY_LEVEL_SHIFT: u64 = 13;
const TLBENTRY_LEVEL_MASK: u64 = 0x3;
const HW_PTE_MASK_LA64: u64 = 0xE000_FFFF_FFFF_F1FF;
pub(crate) const FCSR_ENABLE_MASK: u32 = 0x0000_001F;
pub(crate) const FCSR_RM_MASK: u32 = 0x0000_0300;
pub(crate) const FCSR_FLAGS_MASK: u32 = 0x001F_0000;
pub(crate) const FCSR_CAUSE_MASK: u32 = 0x1F00_0000;
pub(crate) const FCSR_WRITE_MASK: u32 =
    FCSR_ENABLE_MASK | FCSR_RM_MASK | FCSR_FLAGS_MASK | FCSR_CAUSE_MASK;

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
    pub(crate) neg_align: i32,
    pub(crate) last_phys_pc: u64,

    pub(crate) mmu: LoongArchMmu,

    pub(crate) ipi_status: u64,
    pub(crate) ipi_enable: u64,
    pub(crate) ipi_mailbox: [u64; 4],

    pub(crate) as_ptr: u64,
    pub(crate) ram_base: u64,
    pub(crate) ram_end: u64,
    pub(crate) code_pages_ptr: u64,
    pub(crate) code_pages_len: u64,
    pub(crate) tb_flush_pending: bool,
    pub(crate) translation_fault_pending: bool,
    pub(crate) mem_fault_cause: u64,
    pub(crate) fault_pc: u64,
    pub(crate) cfg: LoongArchCfg,
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
pub const NEG_ALIGN_CPU_OFFSET: usize = offset_of!(LoongArchCpu, neg_align);
pub const FAST_TLB_PTR_OFFSET: usize =
    offset_of!(LoongArchCpu, mmu) + offset_of!(LoongArchMmu, fast_tlb);
pub const MEM_FAULT_CAUSE_OFFSET: usize =
    offset_of!(LoongArchCpu, mem_fault_cause);
pub const FAULT_PC_OFFSET: usize = offset_of!(LoongArchCpu, fault_pc);

#[must_use]
pub const fn gpr_offset(i: usize) -> usize {
    GPR_OFFSET + i * 8
}

#[must_use]
pub const fn fpr_offset(i: usize) -> usize {
    FPR_OFFSET + i * 8
}

const fn fcsr_subregister_mask(idx: u32) -> u32 {
    match idx {
        0 => FCSR_WRITE_MASK,
        1 => FCSR_ENABLE_MASK,
        2 => FCSR_FLAGS_MASK | FCSR_CAUSE_MASK,
        3 => FCSR_RM_MASK,
        _ => 0,
    }
}

impl LoongArchCpu {
    #[must_use]
    pub fn new() -> Self {
        Self::with_cfg(LoongArchCfg {
            has_fpu: true,
            has_lsx: false,
            has_lasx: false,
            has_lbt: false,
        })
    }

    #[must_use]
    pub fn with_cfg(cfg: LoongArchCfg) -> Self {
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
            prcfg2: 0x4020_5000,
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
            neg_align: 0,
            last_phys_pc: 0,
            mmu: LoongArchMmu::new(),
            ipi_status: 0,
            ipi_enable: 0,
            ipi_mailbox: [0; 4],
            as_ptr: 0,
            ram_base: 0,
            ram_end: 0,
            code_pages_ptr: 0,
            code_pages_len: 0,
            tb_flush_pending: false,
            translation_fault_pending: false,
            mem_fault_cause: 0,
            fault_pc: 0,
            cfg,
        }
    }

    #[must_use]
    pub const fn crmd(&self) -> u64 {
        self.crmd
    }

    pub(crate) fn set_crmd(&mut self, val: u64) {
        let old = self.crmd;
        self.crmd = val;
        if (old ^ val) & (CRMD_PLV_MASK | CRMD_DA | CRMD_PG) != 0 {
            self.flush_fast_tlb();
        }
    }

    pub(crate) fn set_asid_low(&mut self, val: u64) {
        let old = self.asid;
        self.asid = (self.asid & !ASID_WRITE_MASK) | (val & ASID_WRITE_MASK);
        if (old ^ self.asid) & ASID_WRITE_MASK != 0 {
            self.invalidate_tlb_translations();
        }
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
    pub const fn read_gpr(&self, idx: usize) -> u64 {
        self.gpr[idx]
    }

    pub fn write_gpr(&mut self, idx: usize, val: u64) {
        if idx != 0 {
            self.gpr[idx] = val;
        }
    }

    #[must_use]
    pub const fn read_fpr(&self, idx: usize) -> u64 {
        self.fpr[idx]
    }

    pub const fn write_fpr(&mut self, idx: usize, val: u64) {
        self.fpr[idx] = val;
    }

    #[must_use]
    pub const fn read_fcc(&self, idx: usize) -> u8 {
        self.fcc[idx]
    }

    pub const fn write_fcc(&mut self, idx: usize, val: u8) {
        self.fcc[idx] = val & 1;
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
        self.fcsr0 = val & FCSR_WRITE_MASK;
    }

    #[must_use]
    pub(crate) const fn fcsr_rounding_mode(&self) -> u32 {
        (self.fcsr0 & FCSR_RM_MASK) >> 8
    }

    #[must_use]
    pub(crate) const fn read_fcsr_subregister(&self, idx: u32) -> u32 {
        self.fcsr0 & fcsr_subregister_mask(idx)
    }

    pub(crate) fn write_fcsr_subregister(&mut self, idx: u32, val: u32) {
        let mask = fcsr_subregister_mask(idx);
        self.fcsr0 = (self.fcsr0 & !mask) | (val & mask);
    }

    pub(crate) fn update_fcsr_exception(
        &mut self,
        flags: u32,
        fault_pc: u64,
    ) -> Option<u64> {
        let flags = flags & FCSR_ENABLE_MASK;
        self.fcsr0 = (self.fcsr0 & !FCSR_CAUSE_MASK) | (flags << 24);
        if flags == 0 {
            return None;
        }
        if self.fcsr0 & flags != 0 {
            self.pc = fault_pc;
            return Some(self.enter_exception(u64::from(ECODE_FPE), 0, None));
        }
        self.fcsr0 |= flags << 16;
        None
    }

    #[must_use]
    pub fn mmu(&self) -> &LoongArchMmu {
        &self.mmu
    }

    pub fn mmu_mut(&mut self) -> &mut LoongArchMmu {
        &mut self.mmu
    }

    #[must_use]
    pub fn translate_address(
        &self,
        va: u64,
        access: AccessType,
    ) -> TlbLookupResult {
        if let Some(pa) = mmu::direct_map_address(self, va) {
            return TlbLookupResult::Hit { pa, mat: 0 };
        }
        if self.crmd & super::csr::CRMD_PG == 0 {
            return TlbLookupResult::Miss;
        }
        self.mmu.tlb_lookup(
            va,
            (self.asid & 0x3FF) as u16,
            self.stlbps as u8,
            access,
            (self.crmd & super::csr::CRMD_PLV_MASK) as u8,
        )
    }

    pub fn translate_address_and_cache(
        &mut self,
        va: u64,
        access: AccessType,
    ) -> TlbLookupResult {
        let result = self.translate_address(va, access);
        if let TlbLookupResult::Hit { pa, .. } = result {
            let addend = self.fast_tlb_addend(va, pa);
            self.mmu.fill_fast_tlb(va, access, pa, addend);
        }
        result
    }

    pub fn translate_address_or_exception(
        &mut self,
        va: u64,
        access: AccessType,
        fault_pc: u64,
    ) -> Result<u64, u64> {
        match self.translate_address(va, access) {
            TlbLookupResult::Hit { pa, .. } => Ok(pa),
            fault => Err(self.enter_address_translation_exception(
                va, access, fault, fault_pc,
            )),
        }
    }

    pub fn enter_address_translation_exception(
        &mut self,
        va: u64,
        access: AccessType,
        fault: TlbLookupResult,
        fault_pc: u64,
    ) -> u64 {
        self.pc = fault_pc;
        if fault == TlbLookupResult::Miss {
            self.tlbrehi = (self.tlbrehi & 0x3F) | (va & !0x1FFF);
        } else {
            self.tlbehi = va & !0x1FFF;
        }
        let ecode = match fault {
            TlbLookupResult::Miss => ECODE_TLBR,
            TlbLookupResult::Invalid => match access {
                AccessType::Load => ECODE_PIL,
                AccessType::Store => ECODE_PIS,
                AccessType::Fetch => ECODE_PIF,
            },
            TlbLookupResult::Dirty => ECODE_PME,
            TlbLookupResult::PrivViolation => ECODE_PPI,
            TlbLookupResult::ReadProtect => ECODE_PNR,
            TlbLookupResult::ExecProtect => ECODE_PNX,
            TlbLookupResult::Hit { .. } => unreachable!("hit is not a fault"),
        };
        self.enter_exception(u64::from(ecode), 0, Some(va))
    }

    pub(crate) fn enter_exception(
        &mut self,
        ecode: u64,
        esubcode: u64,
        badv: Option<u64>,
    ) -> u64 {
        let pc = self.pc;
        if ecode == u64::from(ECODE_TLBR) {
            self.tlbrera = (pc & !0x3) | 1;
            self.tlbrprmd = self.crmd & 0x7;
            if let Some(badv) = badv {
                self.tlbrbadv = badv;
            }
            self.set_crmd(
                (self.crmd & !CRMD_PLV_MASK & !CRMD_IE & !CRMD_PG) | CRMD_DA,
            );
            return self.tlbrentry;
        }

        self.era = pc;
        self.prmd = self.crmd & 0x7;
        if let Some(badv) = badv {
            self.badv = badv;
        }
        self.set_crmd(self.crmd & !CRMD_PLV_MASK & !CRMD_IE);
        self.estat = (self.estat & ESTAT_IS_MASK)
            | ((ecode & 0x3F) << 16)
            | ((esubcode & 0x1FF) << 22);
        self.exception_vector(ecode)
    }

    fn exception_vector(&self, ecode: u64) -> u64 {
        if ecode == u64::from(ECODE_TLBR) {
            return self.tlbrentry;
        }

        let vs = (self.ecfg >> ECFG_VS_SHIFT) & ECFG_VS_MASK;
        if vs == 0 {
            self.eentry
        } else {
            self.eentry
                .wrapping_add((ecode & 0x3F).wrapping_mul((1_u64 << vs) * 4))
        }
    }

    #[must_use]
    pub fn fast_tlb_lookup_addend(
        &self,
        va: u64,
        access: AccessType,
    ) -> Option<usize> {
        self.mmu.fast_tlb_lookup_addend(va, access)
    }

    pub(crate) fn flush_fast_tlb(&mut self) {
        self.mmu.flush_fast_tlb();
    }

    pub(crate) fn invalidate_tlb_translations(&mut self) {
        self.flush_fast_tlb();
        self.tb_flush_pending = true;
        self.set_exit_request();
    }

    fn fast_tlb_addend(&self, va: u64, pa: u64) -> usize {
        if pa >= self.ram_base && pa < self.ram_end {
            self.guest_base
                .wrapping_add(pa & FAST_TLB_PAGE_MASK)
                .wrapping_sub(va & FAST_TLB_PAGE_MASK) as usize
        } else {
            FAST_TLB_MMIO_ADDEND
        }
    }

    pub fn tlb_search(&self) -> Option<usize> {
        use super::mmu::{
            mtlb_flat_index, stlb_flat_index, stlb_set_index, MTLB_SIZE,
            STLB_WAYS,
        };
        let entryhi = if self.tlbrera & 1 != 0 {
            self.tlbrehi
        } else {
            self.tlbehi
        };
        let asid = (self.asid & 0x3FF) as u16;
        let ps = self.stlbps as u8;
        for i in 0..MTLB_SIZE {
            let e = &self.mmu.mtlb[i];
            if Self::tlb_entry_matches_va(e, entryhi, asid) {
                return mtlb_flat_index(i);
            }
        }
        if let Some(set_idx) = stlb_set_index(entryhi, ps) {
            for w in 0..STLB_WAYS {
                let e = &self.mmu.stlb[set_idx][w];
                if Self::tlb_entry_matches_va(e, entryhi, asid) {
                    return stlb_flat_index(set_idx, w);
                }
            }
        }
        None
    }

    fn tlb_entry_matches_va(
        entry: &super::mmu::TlbEntry,
        va: u64,
        asid: u16,
    ) -> bool {
        if !entry.valid || (!entry.g && entry.asid != asid) {
            return false;
        }
        let ps = u32::from(entry.page_size);
        if ps >= 63 {
            return false;
        }
        let pair_mask = !((1_u64 << (ps + 1)) - 1);
        (va & super::mmu::TARGET_VIRT_MASK & pair_mask)
            == ((entry.vppn << 13) & super::mmu::TARGET_VIRT_MASK & pair_mask)
    }

    fn page_walk_dir_base_width(&self, level: u64) -> (u64, u64) {
        match level {
            1 => ((self.pwcl >> 10) & 0x1F, (self.pwcl >> 15) & 0x1F),
            2 => ((self.pwcl >> 20) & 0x1F, (self.pwcl >> 25) & 0x1F),
            3 => (self.pwch & 0x3F, (self.pwch >> 6) & 0x3F),
            4 => ((self.pwch >> 12) & 0x3F, (self.pwch >> 18) & 0x3F),
            _ => (self.pwcl & 0x1F, (self.pwcl >> 5) & 0x1F),
        }
    }

    fn read_phys_u64(&self, pa: u64) -> u64 {
        if self.guest_base == 0 {
            return 0;
        }
        if pa < self.ram_base
            || pa.checked_add(8).is_none_or(|end| end > self.ram_end)
        {
            return 0;
        }
        let ptr = (self.guest_base + pa) as *const u64;
        // SAFETY: bounds were checked against the RAM window above.
        unsafe { ptr.read_unaligned() }
    }

    fn sanitize_page_walk_pte(&self, pte: u64) -> u64 {
        pte & HW_PTE_MASK_LA64
    }

    pub fn lddir(&self, base: u64, level: u64) -> u64 {
        if level == 0 || level > 4 {
            return base;
        }

        if base & TLBENTRY_HUGE != 0 {
            if level == 4
                || ((base >> TLBENTRY_LEVEL_SHIFT) & TLBENTRY_LEVEL_MASK) != 0
            {
                return base;
            }
            return base
                | ((level & TLBENTRY_LEVEL_MASK) << TLBENTRY_LEVEL_SHIFT);
        }

        let badv = self.tlbrbadv;
        let (dir_base, dir_width) = self.page_walk_dir_base_width(level);
        if dir_width == 0 || dir_width >= 64 {
            return 0;
        }
        let index = (badv >> dir_base) & ((1_u64 << dir_width) - 1);
        let phys = (base & super::mmu::TARGET_VIRT_MASK) | (index << 3);
        self.read_phys_u64(phys) & super::mmu::TARGET_VIRT_MASK
    }

    pub fn ldpte(&mut self, base: u64, odd: u64) {
        let tmp;
        let ps;
        if base & TLBENTRY_HUGE != 0 {
            let level = (base >> TLBENTRY_LEVEL_SHIFT) & TLBENTRY_LEVEL_MASK;
            let (dir_base, dir_width) = self.page_walk_dir_base_width(level);
            let mut huge = base & !TLBENTRY_HUGE;
            huge &= !(TLBENTRY_LEVEL_MASK << TLBENTRY_LEVEL_SHIFT);
            if huge & TLBENTRY_HGLOBAL != 0 {
                huge &= !TLBENTRY_HGLOBAL;
                huge |= 1 << 6;
            }
            ps = dir_base + dir_width - 1;
            tmp = self.sanitize_page_walk_pte(if odd != 0 {
                huge.wrapping_add(1_u64 << ps)
            } else {
                huge
            });
        } else {
            let ptbase = self.pwcl & 0x1F;
            let ptwidth = (self.pwcl >> 5) & 0x1F;
            if ptwidth == 0 || ptwidth >= 64 {
                return;
            }
            let badv = self.tlbrbadv;
            let ptindex = ((badv >> ptbase) & ((1_u64 << ptwidth) - 1)) & !1;
            let offset = if odd != 0 { ptindex + 1 } else { ptindex } << 3;
            let phys = (base & super::mmu::TARGET_VIRT_MASK) | offset;
            tmp = self.sanitize_page_walk_pte(self.read_phys_u64(phys));
            ps = ptbase;
        }

        if odd != 0 {
            self.tlbrelo1 = tmp;
        } else {
            self.tlbrelo0 = tmp;
        }
        self.tlbrehi = (self.tlbrehi & !0x3F) | (ps & 0x3F);
    }

    pub fn tlb_read(&mut self, idx: usize) {
        let Some(e) = self.mmu.entry(idx).copied() else {
            self.read_invalid_tlb_entry();
            return;
        };
        if !e.valid {
            self.read_invalid_tlb_entry();
            return;
        }
        self.tlbidx = (self.tlbidx & !(1 << 31 | 0x3F << 24 | 0xFFF))
            | ((u64::from(e.page_size)) << 24)
            | (idx as u64 & 0xFFF);
        self.tlbehi = e.vppn << 13;
        self.tlbelo0 = self.encode_tlbelo(
            e.ppn0, e.v0, e.d0, e.plv0, e.mat0, e.g, e.nr0, e.nx0, e.rplv0,
        );
        self.tlbelo1 = self.encode_tlbelo(
            e.ppn1, e.v1, e.d1, e.plv1, e.mat1, e.g, e.nr1, e.nx1, e.rplv1,
        );
        self.set_asid_low(u64::from(e.asid));
    }

    fn read_invalid_tlb_entry(&mut self) {
        self.tlbidx = (self.tlbidx & !(0x3F << 24)) | (1 << 31);
        self.tlbehi = 0;
        self.tlbelo0 = 0;
        self.tlbelo1 = 0;
        self.set_asid_low(0);
    }

    pub fn tlb_write(&mut self, idx: usize) {
        if self.tlbidx & (1 << 31) != 0 {
            if self.mmu.write_entry(idx, super::mmu::TlbEntry::default()) {
                self.invalidate_tlb_translations();
            }
            return;
        }
        let (entryhi, elo0, elo1, ps) = self.tlb_entry_source_csrs();
        let entry = self.tlb_entry_from_csrs(entryhi, elo0, elo1, ps);
        if self.mmu.write_entry(idx, entry) {
            self.invalidate_tlb_translations();
        }
    }

    fn tlb_entry_source_csrs(&self) -> (u64, u64, u64, u8) {
        if self.tlbrera & 1 != 0 {
            (
                self.tlbrehi,
                self.tlbrelo0,
                self.tlbrelo1,
                (self.tlbrehi & 0x3F) as u8,
            )
        } else {
            (
                self.tlbehi,
                self.tlbelo0,
                self.tlbelo1,
                ((self.tlbidx >> 24) & 0x3F) as u8,
            )
        }
    }

    fn tlb_entry_from_csrs(
        &self,
        entryhi: u64,
        elo0: u64,
        elo1: u64,
        ps: u8,
    ) -> super::mmu::TlbEntry {
        super::mmu::TlbEntry {
            vppn: entryhi >> 13,
            page_size: ps,
            asid: (self.asid & 0x3FF) as u16,
            g: (elo0 & elo1 & (1 << 6)) != 0,
            valid: true,
            ppn0: (elo0 >> 12) & 0xF_FFFF_FFFF,
            ppn1: (elo1 >> 12) & 0xF_FFFF_FFFF,
            plv0: ((elo0 >> 2) & 0x3) as u8,
            plv1: ((elo1 >> 2) & 0x3) as u8,
            mat0: ((elo0 >> 4) & 0x3) as u8,
            mat1: ((elo1 >> 4) & 0x3) as u8,
            d0: elo0 & (1 << 1) != 0,
            d1: elo1 & (1 << 1) != 0,
            v0: elo0 & 1 != 0,
            v1: elo1 & 1 != 0,
            nr0: elo0 & (1 << 61) != 0,
            nr1: elo1 & (1 << 61) != 0,
            nx0: elo0 & (1 << 62) != 0,
            nx1: elo1 & (1 << 62) != 0,
            rplv0: elo0 & (1 << 63) != 0,
            rplv1: elo1 & (1 << 63) != 0,
        }
    }

    pub fn tlb_fill(&mut self) {
        use super::mmu::{
            mtlb_flat_index, stlb_flat_index, stlb_set_index, MTLB_SIZE,
        };
        let (entryhi, elo0, elo1, ps) = self.tlb_entry_source_csrs();
        let entry = self.tlb_entry_from_csrs(entryhi, elo0, elo1, ps);
        let idx = if ps == self.stlbps as u8 {
            stlb_set_index(entryhi, ps).and_then(|set| stlb_flat_index(set, 0))
        } else {
            let raw_idx = (self.tlbidx as usize) & (MTLB_SIZE - 1);
            mtlb_flat_index(raw_idx)
        };
        if let Some(idx) = idx {
            if self.mmu.write_entry(idx, entry) {
                self.invalidate_tlb_translations();
            }
        }
    }

    pub fn invtlb(&mut self, op: u32, asid: u16, va: u64) {
        let should_flush = matches!(op, 0..=6);
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
                self.mmu.mtlb.iter_mut().for_each(|e| {
                    if !e.g && Self::tlb_entry_matches_va(e, va, asid) {
                        e.valid = false;
                    }
                });
                self.mmu.stlb.iter_mut().for_each(|s| {
                    s.iter_mut().for_each(|e| {
                        if !e.g && Self::tlb_entry_matches_va(e, va, asid) {
                            e.valid = false;
                        }
                    });
                });
            }
            6 => {
                self.mmu.mtlb.iter_mut().for_each(|e| {
                    if Self::tlb_entry_matches_va(e, va, asid) {
                        e.valid = false;
                    }
                });
                self.mmu.stlb.iter_mut().for_each(|s| {
                    s.iter_mut().for_each(|e| {
                        if Self::tlb_entry_matches_va(e, va, asid) {
                            e.valid = false;
                        }
                    });
                });
            }
            _ => {}
        }
        if should_flush {
            self.invalidate_tlb_translations();
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
        nx: bool,
        rplv: bool,
    ) -> u64 {
        (u64::from(v))
            | (u64::from(d) << 1)
            | (u64::from(plv) << 2)
            | (u64::from(mat) << 4)
            | (u64::from(g) << 6)
            | (ppn << 12)
            | (u64::from(nr) << 61)
            | (u64::from(nx) << 62)
            | (u64::from(rplv) << 63)
    }

    pub fn env_ptr(&mut self) -> *mut u8 {
        std::ptr::from_mut(self).cast()
    }

    pub fn set_estat_hw(&mut self, val: u64) {
        self.estat = val;
    }

    #[must_use]
    pub fn is_halted(&self) -> bool {
        self.halted.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn set_halted_flag(&mut self, val: bool) {
        self.halted.store(val, std::sync::atomic::Ordering::Release);
    }

    pub fn take_tb_flush(&mut self) -> bool {
        let p = self.tb_flush_pending;
        self.tb_flush_pending = false;
        p
    }

    pub fn set_translation_fault_pending(&mut self) {
        self.translation_fault_pending = true;
        self.mem_fault_cause = 1;
        self.fault_pc = self.pc;
    }

    pub fn set_memory_fault_pending(&mut self, fault_pc: u64) {
        self.mem_fault_cause = 1;
        self.fault_pc = fault_pc;
    }

    pub fn take_translation_fault_pending(&mut self) -> bool {
        let pending =
            self.translation_fault_pending || self.mem_fault_cause != 0;
        self.translation_fault_pending = false;
        self.mem_fault_cause = 0;
        self.fault_pc = 0;
        pending
    }

    #[must_use]
    pub fn guest_base_val(&self) -> u64 {
        self.guest_base
    }

    pub fn set_guest_base(&mut self, val: u64) {
        self.guest_base = val;
    }

    #[must_use]
    pub fn ram_base_val(&self) -> u64 {
        self.ram_base
    }

    pub fn set_ram_base(&mut self, val: u64) {
        self.ram_base = val;
    }

    #[must_use]
    pub fn ram_end_val(&self) -> u64 {
        self.ram_end
    }

    pub fn set_ram_end(&mut self, val: u64) {
        self.ram_end = val;
    }

    #[must_use]
    pub fn address_space_ptr(&self) -> u64 {
        self.as_ptr
    }

    pub fn set_address_space_ptr(&mut self, ptr: u64) {
        self.as_ptr = ptr;
    }

    #[must_use]
    pub fn fault_pc_val(&self) -> u64 {
        self.fault_pc
    }

    pub fn set_exit_request(&mut self) {
        self.neg_align = -1;
    }

    pub fn reset_exit_request(&mut self) {
        self.neg_align = 0;
    }

    #[must_use]
    pub fn neg_align_val(&self) -> i32 {
        self.neg_align
    }

    pub fn set_last_phys_pc(&mut self, val: u64) {
        self.last_phys_pc = val;
    }

    #[must_use]
    pub fn last_phys_pc_val(&self) -> u64 {
        self.last_phys_pc
    }

    pub fn set_badv_raw(&mut self, val: u64) {
        self.badv = val;
    }

    #[must_use]
    pub fn tval(&self) -> u64 {
        self.tval
    }

    pub fn iocsr_read(&self, addr: u32, width: u32) -> u64 {
        let mask: u64 = match width {
            1 => 0xFF,
            2 => 0xFFFF,
            4 => 0xFFFF_FFFF,
            _ => u64::MAX,
        };
        let (reg_val, byte_off) = self.iocsr_reg_read(addr);
        let shift = u64::from(byte_off) * 8;
        (reg_val >> shift) & mask
    }

    fn iocsr_reg_read(&self, addr: u32) -> (u64, u32) {
        let base = addr & !0x7;
        let off = addr & 0x7;
        let val = match base {
            0x1000 => self.ipi_status | (self.ipi_enable << 32),
            0x1008 => 0,
            0x1020 => self.ipi_mailbox[0],
            0x1028 => self.ipi_mailbox[1],
            0x1030 => self.ipi_mailbox[2],
            0x1038 => self.ipi_mailbox[3],
            _ => 0,
        };
        (val, off)
    }

    pub fn iocsr_write(&mut self, addr: u32, val: u64, width: u32) {
        let mask: u64 = match width {
            1 => 0xFF,
            2 => 0xFFFF,
            4 => 0xFFFF_FFFF,
            _ => u64::MAX,
        };
        let masked_val = val & mask;
        match addr {
            a if (0x1000..0x1004).contains(&a) => {
                // Status: read-only, writes ignored
            }
            a if (0x1004..0x1008).contains(&a) => {
                let off = a - 0x1004;
                let shift = u64::from(off) * 8;
                let wmask = mask << shift;
                let wval = masked_val << shift;
                self.ipi_enable = (self.ipi_enable & !wmask) | wval;
                self.update_ipi_interrupt();
            }
            a if (0x1008..0x100C).contains(&a) => {
                let off = a - 0x1008;
                let shift = u64::from(off) * 8;
                self.ipi_status |= masked_val << shift;
                self.update_ipi_interrupt();
            }
            a if (0x100C..0x1010).contains(&a) => {
                let off = a - 0x100C;
                let shift = u64::from(off) * 8;
                self.ipi_status &= !(masked_val << shift);
                self.update_ipi_interrupt();
            }
            0x1040 => {
                let target = (masked_val >> 16) & 0x3FF;
                let vector = (masked_val & 0x1F) as u32;
                if target == 0 {
                    self.ipi_status |= 1u64 << vector;
                    self.update_ipi_interrupt();
                }
            }
            0x1048 => {
                let dest = 0x1020 + (masked_val as u32 & 0x1C);
                self.send_ipi_data(dest, masked_val);
            }
            0x1158 => {
                let dest = (masked_val & 0xFFFF) as u32;
                self.send_ipi_data(dest, masked_val);
            }
            a if (0x1020..0x1040).contains(&a) => {
                let mb_off = a - 0x1020;
                let mb_idx = (mb_off / 8) as usize;
                let byte_off = mb_off % 8;
                if mb_idx < 4 {
                    let shift = u64::from(byte_off) * 8;
                    let wmask = mask << shift;
                    let wval = masked_val << shift;
                    let old = self.ipi_mailbox[mb_idx];
                    self.ipi_mailbox[mb_idx] = (old & !wmask) | wval;
                }
            }
            _ => {}
        }
    }

    fn send_ipi_data(&mut self, dest_addr: u32, val: u64) {
        let target = (val >> 16) & 0x3FF;
        if target != 0 {
            return;
        }
        let data = (val >> 32) as u32;
        let byte_mask = ((val >> 27) & 0xF) as u32;
        let merged = self.merge_ipi_word(dest_addr, data, byte_mask);
        self.write_local_iocsr_word(dest_addr, merged);
    }

    fn merge_ipi_word(&self, dest_addr: u32, data: u32, byte_mask: u32) -> u32 {
        let old = self.read_local_iocsr_word(dest_addr);
        let mut word = old;
        for b in 0..4u32 {
            if byte_mask & (1 << b) == 0 {
                let s = b * 8;
                word = (word & !(0xFF << s)) | ((data >> s & 0xFF) << s);
            }
        }
        word
    }

    fn read_local_iocsr_word(&self, addr: u32) -> u32 {
        match addr {
            a if (0x1000..0x1004).contains(&a) => self.ipi_status as u32,
            a if (0x1004..0x1008).contains(&a) => self.ipi_enable as u32,
            a if (0x1020..0x1040).contains(&a) => {
                let off = a - 0x1020;
                let idx = (off / 8) as usize;
                let hi = (off / 4) & 1 != 0;
                if idx < 4 {
                    let shift = if hi { 32 } else { 0 };
                    ((self.ipi_mailbox[idx] >> shift) & 0xFFFF_FFFF) as u32
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    fn write_local_iocsr_word(&mut self, addr: u32, val: u32) {
        match addr {
            a if (0x1000..0x1004).contains(&a) => {}
            a if (0x1004..0x1008).contains(&a) => {
                self.ipi_enable = u64::from(val);
                self.update_ipi_interrupt();
            }
            a if (0x1008..0x100C).contains(&a) => {
                self.ipi_status |= u64::from(val);
                self.update_ipi_interrupt();
            }
            a if (0x100C..0x1010).contains(&a) => {
                self.ipi_status &= !u64::from(val);
                self.update_ipi_interrupt();
            }
            a if (0x1020..0x1040).contains(&a) => {
                let off = a - 0x1020;
                let idx = (off / 8) as usize;
                let hi = (off / 4) & 1 != 0;
                if idx < 4 {
                    let shift = if hi { 32 } else { 0 };
                    let cleared =
                        self.ipi_mailbox[idx] & !(0xFFFF_FFFF_u64 << shift);
                    self.ipi_mailbox[idx] = cleared | (u64::from(val) << shift);
                }
            }
            0x1040 => {
                let target = (val >> 16) & 0x3FF;
                if target == 0 {
                    let vector = val & 0x1F;
                    self.ipi_status |= 1u64 << vector;
                    self.update_ipi_interrupt();
                }
            }
            _ => {}
        }
    }

    fn update_ipi_interrupt(&mut self) {
        let was_pending = self.estat & (1 << 12) != 0;
        if self.ipi_status & self.ipi_enable != 0 {
            self.estat |= 1 << 12;
            if !was_pending {
                self.halted
                    .store(false, std::sync::atomic::Ordering::Release);
                self.neg_align = -1;
            }
        } else {
            self.estat &= !(1 << 12);
        }
    }

    pub(crate) fn set_timer_interrupt_pending(&mut self, pending: bool) {
        let was_pending = self.estat & TIMER_INTERRUPT_BIT != 0;
        if pending {
            self.estat |= TIMER_INTERRUPT_BIT;
            if !was_pending {
                self.halted
                    .store(false, std::sync::atomic::Ordering::Release);
                self.neg_align = -1;
            }
        } else {
            self.estat &= !TIMER_INTERRUPT_BIT;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jit_global_offsets_match_cpu_layout() {
        assert_eq!(GPR_OFFSET, offset_of!(LoongArchCpu, gpr));
        assert_eq!(PC_OFFSET, offset_of!(LoongArchCpu, pc));
        assert_eq!(GUEST_BASE_OFFSET, offset_of!(LoongArchCpu, guest_base));
        assert_eq!(FPR_OFFSET, offset_of!(LoongArchCpu, fpr));
        assert_eq!(FCSR0_OFFSET, offset_of!(LoongArchCpu, fcsr0));
        assert_eq!(FCC_OFFSET, offset_of!(LoongArchCpu, fcc));
        assert_eq!(CRMD_OFFSET, offset_of!(LoongArchCpu, crmd));
        assert_eq!(ESTAT_OFFSET, offset_of!(LoongArchCpu, estat));
        assert_eq!(ERA_OFFSET, offset_of!(LoongArchCpu, era));
        assert_eq!(BADV_OFFSET, offset_of!(LoongArchCpu, badv));
        assert_eq!(EENTRY_OFFSET, offset_of!(LoongArchCpu, eentry));
        assert_eq!(LLBCTL_OFFSET, offset_of!(LoongArchCpu, llbctl));
        assert_eq!(LL_RES_ADDR_OFFSET, offset_of!(LoongArchCpu, ll_res_addr));
        assert_eq!(LL_RES_VAL_OFFSET, offset_of!(LoongArchCpu, ll_res_val));
        assert_eq!(RAM_BASE_OFFSET, offset_of!(LoongArchCpu, ram_base));
        assert_eq!(RAM_END_OFFSET, offset_of!(LoongArchCpu, ram_end));
        assert_eq!(GUEST_BASE_CPU_OFFSET, offset_of!(LoongArchCpu, guest_base));
        assert_eq!(NEG_ALIGN_CPU_OFFSET, offset_of!(LoongArchCpu, neg_align));
    }

    #[test]
    fn register_array_offsets_are_contiguous() {
        assert_eq!(gpr_offset(0), GPR_OFFSET);
        assert_eq!(gpr_offset(NUM_GPRS - 1), GPR_OFFSET + 31 * 8);
        assert_eq!(fpr_offset(0), FPR_OFFSET);
        assert_eq!(fpr_offset(NUM_FPRS - 1), FPR_OFFSET + 31 * 8);
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
            self.set_timer_interrupt_pending(true);
            let periodic = self.tcfg & 2 != 0;
            if periodic {
                let init_val = self.tcfg & !0x3;
                self.tval = init_val;
            } else {
                self.tval = 0;
                self.tcfg &= !1;
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
