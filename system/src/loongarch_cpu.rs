use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use machina_accel::ir::context::Context;
use machina_accel::GuestCpu;
use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
use machina_guest_loongarch::loongarch::csr::*;
use machina_guest_loongarch::loongarch::ext::LoongArchCfg;
use machina_guest_loongarch::loongarch::trans::{
    LoongArchDisasContext, LoongArchTranslator,
};
use machina_guest_loongarch::{DisasJumpType, TranslatorOps};

pub struct LoongArchFullSystemCpu {
    pub cpu: LoongArchCpu,
    stop_flag: Arc<AtomicBool>,
}

unsafe impl Send for LoongArchFullSystemCpu {}

impl LoongArchFullSystemCpu {
    /// # Safety
    /// `ram_ptr` must point to valid memory of `ram_size` bytes.
    pub unsafe fn new(
        mut cpu: LoongArchCpu,
        ram_ptr: *const u8,
        ram_base: u64,
        ram_size: u64,
        stop_flag: Arc<AtomicBool>,
    ) -> Self {
        cpu.set_guest_base(
            (ram_ptr as usize).wrapping_sub(ram_base as usize) as u64
        );
        cpu.set_ram_base(ram_base);
        cpu.set_ram_end(ram_base + ram_size);
        Self { cpu, stop_flag }
    }
}

impl GuestCpu for LoongArchFullSystemCpu {
    type IrContext = Context;

    fn get_pc(&self) -> u64 {
        self.cpu.pc()
    }

    fn get_flags(&self) -> u32 {
        let crmd = self.cpu.crmd();
        let plv = (crmd & CRMD_PLV_MASK) as u32;
        let da = ((crmd >> 3) & 1) as u32;
        let pg = ((crmd >> 4) & 1) as u32;
        let fpe = (self.cpu.euen() & 1) as u32;
        plv | (da << 2) | (pg << 3) | (fpe << 4)
    }

    fn gen_code(&mut self, ir: &mut Context, pc: u64, max_insns: u32) -> u32 {
        self.cpu.set_last_phys_pc(pc);
        let guest_base = self.cpu.guest_base_val() as usize as *const u8;
        let cfg = LoongArchCfg::default();
        let mut ctx = LoongArchDisasContext::new(pc, guest_base, cfg);
        ctx.base.max_insns = max_insns;

        if ir.nb_globals() == 0 {
            LoongArchTranslator::init_disas_context(&mut ctx, ir);
        } else {
            use machina_accel::ir::TempIdx;
            use machina_guest_loongarch::loongarch::cpu::NUM_GPRS;
            ctx.env = TempIdx(0);
            for i in 0..NUM_GPRS {
                ctx.gpr[i] = TempIdx((1 + i) as u32);
            }
            ctx.pc = TempIdx(33);
            ctx.llbctl = TempIdx(34);
            ctx.ll_res_addr = TempIdx(35);
            ctx.ll_res_val = TempIdx(36);
        }
        LoongArchTranslator::tb_start(&mut ctx, ir);

        loop {
            LoongArchTranslator::insn_start(&mut ctx, ir);
            LoongArchTranslator::translate_insn(&mut ctx, ir);
            if ctx.base.is_jmp != DisasJumpType::Next {
                break;
            }
            if ctx.base.num_insns >= ctx.base.max_insns {
                ctx.base.is_jmp = DisasJumpType::TooMany;
                break;
            }
        }

        LoongArchTranslator::tb_stop(&mut ctx, ir);
        ctx.base.num_insns * 4
    }

    fn env_ptr(&mut self) -> *mut u8 {
        self.cpu.env_ptr()
    }

    fn pending_interrupt(&self) -> bool {
        self.cpu.pending_interrupt()
    }

    fn has_pending_irq(&self) -> bool {
        let estat = self.cpu.estat();
        let ecfg = self.cpu.ecfg();
        (estat & ecfg & 0x1FFF) != 0
    }

    fn is_halted(&self) -> bool {
        self.cpu.is_halted()
    }

    fn set_halted(&mut self, halted: bool) {
        self.cpu.set_halted_flag(halted);
    }

    fn set_pc(&mut self, pc: u64) {
        self.cpu.set_pc(pc);
    }

    fn handle_interrupt(&mut self) {
        if !self.cpu.pending_interrupt() {
            return;
        }
        let vec = unsafe {
            machina_guest_loongarch::loongarch::trans::helpers
                ::loongarch_helper_raise_exception(
                    self.cpu.env_ptr(),
                    0, // ECODE_INT
                    0,
                )
        };
        self.cpu.set_pc(vec);
    }

    fn handle_exception(&mut self, _cause: u64, _tval: u64) {
        // LoongArch EXCP_UNDEF → raise INE (illegal instruction)
        unsafe {
            let vec = machina_guest_loongarch::loongarch::trans::helpers
                ::loongarch_helper_raise_exception(
                    self.cpu.env_ptr(),
                    0x0D, // ECODE_INE
                    0,
                );
            self.cpu.set_pc(vec);
        }
    }

    fn set_exit_request(&mut self) {
        self.cpu.set_exit_request();
    }

    fn reset_exit_request(&mut self) {
        self.cpu.reset_exit_request();
    }

    fn last_phys_pc(&self) -> u64 {
        self.cpu.last_phys_pc_val()
    }

    fn should_exit(&self) -> bool {
        !self.stop_flag.load(Ordering::Relaxed)
    }

    fn take_tb_flush_pending(&mut self) -> bool {
        self.cpu.take_tb_flush()
    }
}
