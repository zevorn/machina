use std::mem::offset_of;
use std::sync::atomic::{AtomicBool, AtomicU32};

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
            prcfg1: 0,
            prcfg2: 0,
            prcfg3: 0,
            llbctl: 0,
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

    pub(crate) const fn set_pc(&mut self, val: u64) {
        self.pc = val;
    }

    pub(crate) const fn env_ptr(&mut self) -> *mut u8 {
        std::ptr::from_mut(self).cast()
    }
}

impl Default for LoongArchCpu {
    fn default() -> Self {
        Self::new()
    }
}
