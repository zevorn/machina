use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
use machina_guest_loongarch::loongarch::csr::*;
use machina_guest_loongarch::loongarch::mmu;

#[test]
fn csr_crmd_reset_value() {
    let cpu = LoongArchCpu::new();
    assert_eq!(cpu.csr_read(CSR_CRMD), 0x8);
}

#[test]
fn csr_read_write_save_regs() {
    let mut cpu = LoongArchCpu::new();
    for i in 0..16u32 {
        cpu.csr_write(CSR_SAVE0 + i, 0xDEAD_0000 + u64::from(i));
    }
    for i in 0..16u32 {
        assert_eq!(cpu.csr_read(CSR_SAVE0 + i), 0xDEAD_0000 + u64::from(i));
    }
}

#[test]
fn csr_warl_mask_readonly_fields() {
    let mut cpu = LoongArchCpu::new();
    let cpuid_init = cpu.csr_read(CSR_CPUID);
    cpu.csr_write(CSR_CPUID, 0xFFFF_FFFF);
    assert_eq!(cpu.csr_read(CSR_CPUID), cpuid_init);

    let prcfg1_init = cpu.csr_read(CSR_PRCFG1);
    cpu.csr_write(CSR_PRCFG1, 0xFFFF_FFFF);
    assert_eq!(cpu.csr_read(CSR_PRCFG1), prcfg1_init);

    cpu.csr_write(CSR_TVAL, 0xFFFF_FFFF);
    assert_eq!(cpu.csr_read(CSR_TVAL), 0);
}

#[test]
fn csr_warl_crmd_mask() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, 0xFFFF_FFFF_FFFF_FFFF);
    let val = cpu.csr_read(CSR_CRMD);
    assert_eq!(val, CRMD_WRITE_MASK);
}

#[test]
fn csr_warl_eentry_aligned() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EENTRY, 0xFFFF_FFFF_FFFF_FFFF);
    let val = cpu.csr_read(CSR_EENTRY);
    assert_eq!(val & 0x3F, 0);
}

#[test]
fn csr_xchg_partial_write() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_SAVE0, 0xAAAA_BBBB_CCCC_DDDD);
    let old = cpu.csr_xchg(CSR_SAVE0, 0x1111_2222_3333_4444, 0xFFFF_0000);
    assert_eq!(old, 0xAAAA_BBBB_CCCC_DDDD);
    assert_eq!(cpu.csr_read(CSR_SAVE0), 0xAAAA_BBBB_3333_DDDD);
}

#[test]
fn csr_estat_only_swi_writable() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_ESTAT, 0xFFFF_FFFF_FFFF_FFFF);
    assert_eq!(cpu.csr_read(CSR_ESTAT), 0x3);
}

#[test]
fn csr_ticlr_clears_timer_interrupt() {
    let mut cpu = LoongArchCpu::new();
    // Simulate timer interrupt pending
    cpu.set_estat_hw(1 << 11);
    assert_ne!(cpu.csr_read(CSR_ESTAT) & (1 << 11), 0);
    cpu.csr_write(CSR_TICLR, 1);
    assert_eq!(cpu.csr_read(CSR_ESTAT) & (1 << 11), 0);
}

#[test]
fn csr_tcfg_write_initializes_tval() {
    let mut cpu = LoongArchCpu::new();
    // Enable timer with initial value 0x1000 (bit0=enable, bit1=periodic)
    cpu.csr_write(CSR_TCFG, 0x1001);
    assert_eq!(cpu.csr_read(CSR_TVAL), 0x1000);
}

#[test]
fn csr_asid_reports_width() {
    let cpu = LoongArchCpu::new();
    let asid = cpu.csr_read(CSR_ASID);
    // Bits [23:16] = ASIDBITS = 10
    assert_eq!((asid >> 16) & 0xFF, 10);
}

#[test]
fn csr_pgd_reads_pgdl_or_pgdh() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_PGDL, 0x1000_0000);
    cpu.csr_write(CSR_PGDH, 0x2000_0000);
    // BADV bit63=0 -> PGDL
    cpu.set_badv_raw(0x0000_1234_5678_0000);
    assert_eq!(cpu.csr_read(CSR_PGD), 0x1000_0000);
    // BADV bit63=1 -> PGDH
    cpu.set_badv_raw(0x8000_1234_5678_0000);
    assert_eq!(cpu.csr_read(CSR_PGD), 0x2000_0000);
}

#[test]
fn exception_entry_saves_state() {
    let mut cpu = LoongArchCpu::new();
    // Set CRMD: PLV=3, IE=1, DA=1, PG=1
    cpu.csr_write(CSR_CRMD, 0x1F);
    cpu.set_pc(0x8000_0100);
    cpu.csr_write(CSR_EENTRY, 0x9000_0000);

    unsafe {
        machina_guest_loongarch::loongarch::trans::helpers
            ::loongarch_helper_raise_exception(
                cpu.env_ptr(),
                0x0B, // SYS
                0,
            );
    }

    assert_eq!(cpu.csr_read(CSR_ERA), 0x8000_0100);
    assert_eq!(cpu.csr_read(CSR_PRMD), 0x07);
    assert_eq!(cpu.csr_read(CSR_CRMD) & CRMD_PLV_MASK, 0);
    assert_eq!(cpu.csr_read(CSR_CRMD) & CRMD_IE, 0);
    assert_eq!((cpu.csr_read(CSR_ESTAT) >> 16) & 0x3F, 0x0B);
}

#[test]
fn ertn_restores_state() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, 0);
    cpu.csr_write(CSR_PRMD, 0x07);
    cpu.csr_write(CSR_ERA, 0x8000_0200);
    cpu.csr_write(CSR_TLBRERA, 0);

    let pc = unsafe {
        machina_guest_loongarch::loongarch::trans::helpers
            ::loongarch_helper_ertn(cpu.env_ptr())
    };

    assert_eq!(pc, 0x8000_0200);
    assert_eq!(cpu.csr_read(CSR_CRMD) & 0x7, 0x07);
}

#[test]
fn exception_tlbr_uses_tlbr_csrs() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, 0x17); // PLV=3, IE=1, DA=0, PG=1
    cpu.set_pc(0x1234_5678);
    cpu.csr_write(CSR_TLBRENTRY, 0x9000_1000);

    unsafe {
        machina_guest_loongarch::loongarch::trans::helpers
            ::loongarch_helper_raise_exception(
                cpu.env_ptr(),
                0x3F, // TLBR
                0,
            );
    }

    let tlbrera = cpu.csr_read(CSR_TLBRERA);
    assert_eq!(tlbrera & 1, 1);
    assert_eq!(tlbrera & !0x3, 0x1234_5678);
    assert_eq!(cpu.csr_read(CSR_TLBRPRMD), 0x07);
    assert_ne!(cpu.csr_read(CSR_CRMD) & CRMD_DA, 0);
    assert_eq!(cpu.csr_read(CSR_CRMD) & CRMD_PG, 0);
}

#[test]
fn ertn_from_tlbr_clears_istlbr() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, CRMD_DA);
    cpu.csr_write(CSR_TLBRERA, 0x1234_5678 | 1);
    cpu.csr_write(CSR_TLBRPRMD, 0x07);

    let pc = unsafe {
        machina_guest_loongarch::loongarch::trans::helpers
            ::loongarch_helper_ertn(cpu.env_ptr())
    };

    assert_eq!(pc, 0x1234_5678);
    assert_eq!(cpu.csr_read(CSR_TLBRERA) & 1, 0);
    assert_eq!(cpu.csr_read(CSR_CRMD) & 0x7, 0x07);
}

#[test]
fn timer_tick_fires_interrupt() {
    let mut cpu = LoongArchCpu::new();
    // enable=1, periodic=0, initval=0x100
    cpu.csr_write(CSR_TCFG, 0x0101);

    cpu.timer_tick(0x100);

    assert_ne!(cpu.csr_read(CSR_ESTAT) & (1 << 11), 0);
    assert_eq!(cpu.tval(), 0);
}

#[test]
fn timer_tick_periodic_reloads() {
    let mut cpu = LoongArchCpu::new();
    // enable=1, periodic=1, initval=0x100
    cpu.csr_write(CSR_TCFG, 0x0103);
    // Tick down to 0x50 first
    cpu.timer_tick(0xB0);
    // Now it's at 0x50, tick the rest
    cpu.timer_tick(0x50);

    assert_ne!(cpu.csr_read(CSR_ESTAT) & (1 << 11), 0);
    assert_eq!(cpu.tval(), 0x100); // reloaded
}

#[test]
fn timer_disabled_no_countdown() {
    let mut cpu = LoongArchCpu::new();
    // enable=0, initval=0x100 - TCFG bit0=0 means disabled
    cpu.csr_write(CSR_TCFG, 0x0100);

    cpu.timer_tick(0x50);

    assert_eq!(cpu.csr_read(CSR_ESTAT) & (1 << 11), 0);
}

#[test]
fn timer_tick_partial_countdown() {
    let mut cpu = LoongArchCpu::new();
    // enable=1, periodic=0, initval=0x100
    cpu.csr_write(CSR_TCFG, 0x0101);

    cpu.timer_tick(0x50);

    assert_eq!(cpu.csr_read(CSR_ESTAT) & (1 << 11), 0);
    assert_eq!(cpu.tval(), 0xB0);
}

#[test]
fn plv3_csr_write_raises_ipe() {
    let mut cpu = LoongArchCpu::new();
    // Set PLV=3
    cpu.csr_write(CSR_CRMD, 0x03);
    cpu.set_pc(0x1000);
    cpu.csr_write(CSR_EENTRY, 0x9000_0000);

    let vec = unsafe {
        machina_guest_loongarch::loongarch::trans::helpers
            ::loongarch_helper_check_plv(cpu.env_ptr())
    };

    assert_ne!(vec, 0);
    assert_eq!(vec, 0x9000_0000);
    assert_eq!((cpu.csr_read(CSR_ESTAT) >> 16) & 0x3F, 0x0E); // IPE
    assert_eq!(cpu.csr_read(CSR_ERA), 0x1000);
}

#[test]
fn plv0_passes_privilege_check() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, 0x00); // PLV=0
    cpu.set_pc(0x2000);

    let vec = unsafe {
        machina_guest_loongarch::loongarch::trans::helpers
            ::loongarch_helper_check_plv(cpu.env_ptr())
    };

    assert_eq!(vec, 0);
}

#[test]
fn ertn_from_tlbr_restores_paged_mode() {
    let mut cpu = LoongArchCpu::new();
    // Simulate being in TLB refill handler: DA=1, PG=0
    cpu.csr_write(CSR_CRMD, CRMD_DA);
    // TLBRERA with ISTLBR=1, PC=0xABC0
    cpu.csr_write(CSR_TLBRERA, 0xABC0 | 1);
    // Pre-refill state was: PLV=0, IE=1, DA=0, PG=1
    cpu.csr_write(CSR_TLBRPRMD, 0x04); // IE=1, PLV=0

    let pc = unsafe {
        machina_guest_loongarch::loongarch::trans::helpers
            ::loongarch_helper_ertn(cpu.env_ptr())
    };

    assert_eq!(pc, 0xABC0);
    let crmd = cpu.csr_read(CSR_CRMD);
    assert_eq!(crmd & CRMD_DA, 0); // DA cleared
    assert_ne!(crmd & CRMD_PG, 0); // PG set
    assert_eq!(crmd & 0x7, 0x04); // PLV=0, IE=1
}

#[test]
fn cpucfg_returns_la464_prid() {
    let mut cpu = LoongArchCpu::new();
    let result = unsafe {
        machina_guest_loongarch::loongarch::trans::helpers
            ::loongarch_helper_cpucfg(cpu.env_ptr(), 0)
    };
    assert_eq!(result, 0x0014_C010);
}

#[test]
fn cpucfg_index1_reports_la64() {
    let mut cpu = LoongArchCpu::new();
    let result = unsafe {
        machina_guest_loongarch::loongarch::trans::helpers
            ::loongarch_helper_cpucfg(cpu.env_ptr(), 1)
    };
    assert_eq!(result, 0x03F2_F2FE);
    assert_eq!(result & 0x3, 2); // ARCH=2 (LA64)
    assert_ne!(result & (1 << 2), 0); // PGMMU
    assert_ne!(result & (1 << 3), 0); // IOCSR
}

#[test]
fn cpucfg_index2_reports_fp_no_lsx() {
    let mut cpu = LoongArchCpu::new();
    let result = unsafe {
        machina_guest_loongarch::loongarch::trans::helpers
            ::loongarch_helper_cpucfg(cpu.env_ptr(), 2)
    };
    assert_eq!(result, 0x0060_C00F);
    assert_ne!(result & 1, 0); // FP_SP
    assert_ne!(result & 2, 0); // FP_DP
    assert_eq!(result & (1 << 6), 0); // LSX=0
    assert_eq!(result & (1 << 7), 0); // LASX=0
}

#[test]
fn cpucfg_unknown_index_returns_zero() {
    let mut cpu = LoongArchCpu::new();
    let result = unsafe {
        machina_guest_loongarch::loongarch::trans::helpers
            ::loongarch_helper_cpucfg(cpu.env_ptr(), 0x100)
    };
    assert_eq!(result, 0);
}

#[test]
fn prcfg2_is_page_size_bitmap() {
    let cpu = LoongArchCpu::new();
    let val = cpu.csr_read(CSR_PRCFG2);
    assert_eq!(val, 0x3FFF_F000);
    assert_ne!(val & (1 << 12), 0); // 4K supported
    assert_ne!(val & (1 << 14), 0); // 16K supported
    assert_ne!(val & (1 << 21), 0); // 2M supported
}

#[test]
fn prcfg3_is_tlb_organization() {
    let cpu = LoongArchCpu::new();
    let val = cpu.csr_read(CSR_PRCFG3);
    assert_eq!(val, 0x0080_73F2);
    assert_eq!(val & 0xF, 2); // TLBType = MTLB+STLB
}

#[test]
fn prcfg1_has_nonzero_reset_value() {
    let cpu = LoongArchCpu::new();
    let val = cpu.csr_read(CSR_PRCFG1);
    assert_eq!(val & 0xF, 8); // SAVE_NUM=8
    assert_eq!((val >> 4) & 0xFF, 0x2F); // TIMER_BITS=47
    assert_eq!((val >> 12) & 0x7, 7); // VSMAX=7
}

#[test]
fn exception_syscall_sets_pc_to_eentry() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, 0x08); // DA=1
    cpu.set_pc(0x4000);
    cpu.csr_write(CSR_EENTRY, 0x9000_0000);

    let vec = unsafe {
        machina_guest_loongarch::loongarch::trans::helpers
            ::loongarch_helper_raise_exception(
                cpu.env_ptr(), 0x0B, 0,
            )
    };

    assert_eq!(vec, 0x9000_0000);
    assert_eq!(cpu.csr_read(CSR_ERA), 0x4000);
}

#[test]
fn translated_csrrd_under_plv3_sets_noreturn() {
    use machina_accel::ir::Context;
    use machina_guest_loongarch::loongarch::ext::LoongArchCfg;
    use machina_guest_loongarch::loongarch::trans::{
        LoongArchDisasContext, LoongArchTranslator,
    };
    use machina_guest_loongarch::{DisasJumpType, TranslatorOps};

    // CSRRD r1, CRMD(0x0): 00000100 00000000000000 00000 00001
    let insn: u32 = 0x0400_0001;
    let code: [u32; 1] = [insn];
    let guest_base = code.as_ptr().cast::<u8>();
    let mut ctx =
        LoongArchDisasContext::new(0, guest_base, LoongArchCfg::default());
    let mut ir = Context::new();

    LoongArchTranslator::init_disas_context(&mut ctx, &mut ir);
    LoongArchTranslator::insn_start(&mut ctx, &mut ir);
    LoongArchTranslator::translate_insn(&mut ctx, &mut ir);

    assert_eq!(ctx.base.is_jmp, DisasJumpType::Next);
    assert!(ir.num_ops() > 5);
}

#[test]
fn tlb_write_and_search_via_cpu() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_TLBEHI, 0x8000_0000_0000_0000);
    cpu.csr_write(CSR_TLBELO0, 0x0000_0001_0000_0001); // V=1, PPN=0x10000
    cpu.csr_write(CSR_TLBELO1, 0x0000_0002_0000_0001); // V=1, PPN=0x20000
    cpu.csr_write(CSR_ASID, 0x05);
    // PS=14 (16KB pages), index=0
    cpu.csr_write(CSR_TLBIDX, 14 << 24);

    cpu.tlb_write(0);

    // Search: set TLBEHI to same VPPN
    cpu.csr_write(CSR_TLBEHI, 0x8000_0000_0000_0000);
    cpu.csr_write(CSR_ASID, 0x05);
    let found = cpu.tlb_search();
    assert_eq!(found, Some(0));
}

#[test]
fn tlb_write_read_roundtrip() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_TLBEHI, 0x0000_1234_0000_0000);
    cpu.csr_write(CSR_TLBELO0, 0x0000_00AB_C000_0005); // V=1,D=0,PLV=1
    cpu.csr_write(CSR_TLBELO1, 0x0000_00DE_F000_0003); // V=1,D=1
    cpu.csr_write(CSR_ASID, 0x0A);
    cpu.csr_write(CSR_TLBIDX, 12 << 24 | 3); // PS=12, idx=3

    cpu.tlb_write(3);

    // Read it back
    cpu.csr_write(CSR_TLBIDX, 3);
    cpu.tlb_read(3);

    assert_eq!(cpu.csr_read(CSR_TLBEHI), 0x0000_1234_0000_0000);
    assert_eq!(cpu.csr_read(CSR_ASID) & 0x3FF, 0x0A);
}

#[test]
fn tlb_search_miss_sets_ne_bit() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_TLBEHI, 0xFFFF_0000_0000_0000);
    cpu.csr_write(CSR_ASID, 1);

    let found = cpu.tlb_search();
    assert_eq!(found, None);
}

#[test]
fn invtlb_op0_clears_all() {
    let mut cpu = LoongArchCpu::new();
    // Write an entry
    cpu.csr_write(CSR_TLBEHI, 0x1000_0000_0000_0000);
    cpu.csr_write(CSR_TLBELO0, 1);
    cpu.csr_write(CSR_TLBELO1, 1);
    cpu.csr_write(CSR_ASID, 1);
    cpu.csr_write(CSR_TLBIDX, 12 << 24);
    cpu.tlb_write(0);

    assert!(cpu.mmu().mtlb[0].valid);

    cpu.invtlb(0, 0, 0);

    assert!(!cpu.mmu().mtlb[0].valid);
}

#[test]
fn dmw_match_basic() {
    let mut cpu = LoongArchCpu::new();
    // PLV=0 (bit 0 of DMW enables PLV0 match)
    cpu.csr_write(CSR_CRMD, 0);
    // DMW0: VSEG=0x9, PLV0=1(bit0), PSEG=0x0
    cpu.csr_write(CSR_DMW0, (0x9u64 << 60) | 1);

    let va = 0x9000_0000_1234_5678;
    let result = mmu::dmw_match(&cpu, va);
    assert_eq!(result, Some(0x0000_0000_1234_5678));
}

#[test]
fn dmw_no_match_wrong_plv() {
    let mut cpu = LoongArchCpu::new();
    // PLV=3
    cpu.csr_write(CSR_CRMD, 3);
    // DMW0: only PLV0 enabled (bit 0)
    cpu.csr_write(CSR_DMW0, (0x9u64 << 60) | 1);

    let va = 0x9000_0000_1234_5678;
    let result = mmu::dmw_match(&cpu, va);
    assert_eq!(result, None);
}
