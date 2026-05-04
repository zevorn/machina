use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use machina_accel::ir::context::Context;
use machina_accel::GuestCpu;
use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
use machina_guest_loongarch::loongarch::csr::*;
use machina_system::loongarch_cpu::{
    LoongArchFullSystemCpu, LOONGARCH_TB_FLAG_DA, LOONGARCH_TB_FLAG_FPE,
    LOONGARCH_TB_FLAG_PG, LOONGARCH_TB_FLAG_PLV_MASK,
};

fn make_cpu(code: &[u32]) -> LoongArchFullSystemCpu {
    let ptr = code.as_ptr().cast::<u8>();
    let size = (code.len() * 4) as u64;
    let stop = Arc::new(AtomicBool::new(true));
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, CRMD_DA);
    unsafe { LoongArchFullSystemCpu::new(cpu, ptr, 0, size, 0, stop) }
}

#[test]
fn gen_code_returns_byte_span_for_normal_tb() {
    // Two NOP instructions: ANDI r0, r0, 0 = 0x03400000
    let code: [u32; 2] = [0x0340_0000, 0x0340_0000];
    let mut cpu = make_cpu(&code);
    let mut ir = Context::new();
    let size = cpu.gen_code(&mut ir, 0, 2);
    assert_eq!(size, 8);
}

#[test]
fn gen_code_returns_nonzero_for_illegal_insn() {
    let code: [u32; 1] = [0x0000_0000]; // invalid
    let mut cpu = make_cpu(&code);
    let mut ir = Context::new();
    let size = cpu.gen_code(&mut ir, 0, 512);
    assert_eq!(size, 4);
}

#[test]
fn ir_globals_stable_across_translations() {
    let code: [u32; 2] = [0x0340_0000, 0x0340_0000];
    let mut cpu = make_cpu(&code);
    let mut ir = Context::new();

    cpu.gen_code(&mut ir, 0, 1);
    let globals_after_first = ir.nb_globals();

    ir.reset();
    cpu.gen_code(&mut ir, 0, 1);
    let globals_after_second = ir.nb_globals();

    assert_eq!(globals_after_first, globals_after_second);
    assert!(globals_after_first > 0);
}

#[test]
fn handle_interrupt_sets_pc_to_eentry() {
    let code: [u32; 1] = [0x0340_0000];
    let mut cpu = make_cpu(&code);
    cpu.cpu.set_pc(0x1000);
    cpu.cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.cpu.csr_write(CSR_ECFG, 1 << 11);
    cpu.cpu.set_estat_hw(1 << 11);
    cpu.cpu.csr_write(CSR_EENTRY, 0x9000_0000);

    assert!(cpu.pending_interrupt());
    cpu.handle_interrupt();

    assert_eq!(cpu.get_pc(), 0x9000_0000);
    assert_eq!(cpu.cpu.csr_read(CSR_ERA), 0x1000);
}

#[test]
fn has_pending_irq_ignores_ie() {
    let code: [u32; 1] = [0x0340_0000];
    let mut cpu = make_cpu(&code);
    cpu.cpu.csr_write(CSR_CRMD, CRMD_DA); // IE=0
    cpu.cpu.csr_write(CSR_ECFG, 1 << 11);
    cpu.cpu.set_estat_hw(1 << 11);

    assert!(!cpu.pending_interrupt());
    assert!(cpu.has_pending_irq());
}

#[test]
fn handle_exception_raises_ine_for_illegal_insn() {
    let code: [u32; 1] = [0x0340_0000];
    let mut cpu = make_cpu(&code);
    cpu.cpu.set_pc(0x100);
    cpu.cpu.csr_write(CSR_EENTRY, 0x9000_0000);

    cpu.handle_exception(2, 0);

    assert_eq!(cpu.get_pc(), 0x9000_0000);
    assert_eq!((cpu.cpu.csr_read(CSR_ESTAT) >> 16) & 0x3F, 0x0D);
    assert_eq!(cpu.cpu.csr_read(CSR_ERA), 0x100);
}

#[test]
fn syscall_helper_preserves_sys_ecode() {
    use machina_guest_loongarch::loongarch::trans::helpers;

    let code: [u32; 1] = [0x0340_0000];
    let mut cpu = make_cpu(&code);
    cpu.cpu.set_pc(0x200);
    cpu.cpu.csr_write(CSR_EENTRY, 0x9000_0000);

    let vec = unsafe {
        helpers::loongarch_helper_raise_exception(
            cpu.cpu.env_ptr(),
            0x0B, // SYS
            0,
        )
    };
    cpu.cpu.set_pc(vec);

    // SYS ecode should be preserved, not overwritten by INE
    assert_eq!((cpu.cpu.csr_read(CSR_ESTAT) >> 16) & 0x3F, 0x0B);
    assert_eq!(cpu.get_pc(), 0x9000_0000);
    assert_eq!(cpu.cpu.csr_read(CSR_ERA), 0x200);
}

#[test]
fn get_flags_encodes_direct_address_reset_state() {
    let code: [u32; 1] = [0x0340_0000];
    let cpu = make_cpu(&code);

    assert_eq!(cpu.get_flags(), LOONGARCH_TB_FLAG_DA);
}

#[test]
fn get_flags_encodes_plv_da_pg_and_fpe_bits() {
    let code: [u32; 1] = [0x0340_0000];
    let mut cpu = make_cpu(&code);

    cpu.cpu.csr_write(CSR_CRMD, 3 | CRMD_DA | CRMD_PG | CRMD_IE);
    cpu.cpu.csr_write(CSR_EUEN, EUEN_FPE);

    assert_eq!(
        cpu.get_flags(),
        (3 & LOONGARCH_TB_FLAG_PLV_MASK)
            | LOONGARCH_TB_FLAG_DA
            | LOONGARCH_TB_FLAG_PG
            | LOONGARCH_TB_FLAG_FPE
    );
}

#[test]
fn get_flags_tracks_addressing_and_fpu_changes_independently() {
    let code: [u32; 1] = [0x0340_0000];
    let mut cpu = make_cpu(&code);

    cpu.cpu.csr_write(CSR_CRMD, 2 | CRMD_PG);
    cpu.cpu.csr_write(CSR_EUEN, 0);
    assert_eq!(cpu.get_flags(), 2 | LOONGARCH_TB_FLAG_PG);

    cpu.cpu.csr_write(CSR_CRMD, 2 | CRMD_DA);
    cpu.cpu.csr_write(CSR_EUEN, EUEN_FPE);
    assert_eq!(
        cpu.get_flags(),
        2 | LOONGARCH_TB_FLAG_DA | LOONGARCH_TB_FLAG_FPE
    );
}
