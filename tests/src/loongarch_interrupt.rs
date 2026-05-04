use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use machina_accel::GuestCpu;
use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
use machina_guest_loongarch::loongarch::csr::*;
use machina_system::loongarch_cpu::LoongArchFullSystemCpu;

const SWI0: u64 = 1 << 0;
const HWI0: u64 = 1 << 2;
const TIMER: u64 = 1 << 11;
const IPI: u64 = 1 << 12;

fn full_system_cpu(cpu: LoongArchCpu) -> LoongArchFullSystemCpu {
    let code = Box::leak(Box::new([0x0340_0000u32]));
    let ptr = code.as_ptr().cast::<u8>();
    unsafe {
        LoongArchFullSystemCpu::new(
            cpu,
            ptr,
            0,
            4,
            Arc::new(AtomicBool::new(true)),
        )
    }
}

fn interrupt_vector(base: u64, irq: u32, vs: u64) -> u64 {
    if vs == 0 {
        base
    } else {
        base.wrapping_add(u64::from(64 + irq) * ((1_u64 << vs) * 4))
    }
}

#[test]
fn task35_cpu_pending_line_requires_ie_and_lie() {
    let mut cpu = LoongArchCpu::new();
    cpu.set_estat_hw(TIMER);

    cpu.csr_write(CSR_CRMD, CRMD_DA);
    cpu.csr_write(CSR_ECFG, TIMER);
    assert_eq!(cpu.pending_interrupt_line(), None);
    assert!(!cpu.pending_interrupt());

    cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, 0);
    assert_eq!(cpu.pending_interrupt_line(), None);
    assert!(!cpu.pending_interrupt());

    cpu.csr_write(CSR_ECFG, TIMER);
    assert_eq!(cpu.pending_interrupt_line(), Some(11));
    assert!(cpu.pending_interrupt());
}

#[test]
fn task35_cpu_pending_line_selects_highest_enabled_source() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, SWI0 | HWI0 | TIMER | IPI);
    cpu.set_estat_hw(SWI0 | HWI0 | TIMER | IPI);

    assert_eq!(cpu.pending_interrupt_line(), Some(12));

    cpu.csr_write(CSR_ECFG, SWI0 | HWI0 | TIMER);
    assert_eq!(cpu.pending_interrupt_line(), Some(11));

    cpu.csr_write(CSR_ECFG, SWI0 | HWI0);
    assert_eq!(cpu.pending_interrupt_line(), Some(2));

    cpu.csr_write(CSR_ECFG, SWI0);
    assert_eq!(cpu.pending_interrupt_line(), Some(0));
}

#[test]
fn task35_interrupt_writes_wake_halted_and_break_chains() {
    let mut sw = LoongArchCpu::new();
    sw.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    sw.csr_write(CSR_ECFG, SWI0);
    sw.set_halted_flag(true);

    sw.csr_write(CSR_ESTAT, SWI0);

    assert_eq!(sw.pending_interrupt_line(), Some(0));
    assert!(!sw.is_halted());
    assert_eq!(sw.neg_align_val(), -1);

    let mut enable = LoongArchCpu::new();
    enable.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    enable.csr_write(CSR_ESTAT, SWI0);
    enable.set_halted_flag(true);

    enable.csr_write(CSR_ECFG, SWI0);

    assert_eq!(enable.pending_interrupt_line(), Some(0));
    assert!(!enable.is_halted());
    assert_eq!(enable.neg_align_val(), -1);

    let mut ie = LoongArchCpu::new();
    ie.csr_write(CSR_CRMD, CRMD_DA);
    ie.csr_write(CSR_ECFG, SWI0);
    ie.csr_write(CSR_ESTAT, SWI0);
    ie.set_halted_flag(true);

    ie.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);

    assert_eq!(ie.pending_interrupt_line(), Some(0));
    assert!(!ie.is_halted());
    assert_eq!(ie.neg_align_val(), -1);

    let mut hwi = LoongArchCpu::new();
    hwi.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    hwi.csr_write(CSR_ECFG, HWI0);
    hwi.set_halted_flag(true);

    hwi.set_hwi_interrupt_pending(0, true);

    assert_eq!(hwi.pending_interrupt_line(), Some(2));
    assert!(!hwi.is_halted());
    assert_eq!(hwi.neg_align_val(), -1);
}

#[test]
fn task35_timer_and_ipi_pending_transitions_wake_halted_and_break_chains() {
    let mut timer = LoongArchCpu::new();
    timer.csr_write(CSR_TCFG, 0x0101);
    timer.set_halted_flag(true);

    timer.timer_tick(0x100);

    assert_eq!(timer.csr_read(CSR_ESTAT) & TIMER, TIMER);
    assert!(!timer.is_halted());
    assert_eq!(timer.neg_align_val(), -1);

    let mut ipi = LoongArchCpu::new();
    ipi.set_halted_flag(true);

    ipi.iocsr_write(0x1004, 0x1, 4);
    ipi.iocsr_write(0x1008, 0x1, 4);

    assert_eq!(ipi.csr_read(CSR_ESTAT) & IPI, IPI);
    assert!(!ipi.is_halted());
    assert_eq!(ipi.neg_align_val(), -1);
}

#[test]
fn task35_full_system_interrupt_uses_selected_line_and_vs_vector() {
    let mut cpu = LoongArchCpu::new();
    cpu.set_pc(0x4000);
    cpu.csr_write(CSR_CRMD, 2 | CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, SWI0 | TIMER | IPI | (2 << 16));
    cpu.csr_write(CSR_EENTRY, 0x9000_0000);
    cpu.set_estat_hw(SWI0 | TIMER | IPI);
    let mut sys = full_system_cpu(cpu);

    assert_eq!(sys.cpu.pending_interrupt_line(), Some(12));
    sys.handle_interrupt();

    assert_eq!(sys.get_pc(), interrupt_vector(0x9000_0000, 12, 2));
    assert_eq!(sys.cpu.csr_read(CSR_ERA), 0x4000);
    assert_eq!(sys.cpu.csr_read(CSR_PRMD), 2 | CRMD_IE);
    assert_eq!(sys.cpu.csr_read(CSR_CRMD) & CRMD_IE, 0);
    assert_eq!(sys.cpu.csr_read(CSR_CRMD) & CRMD_PLV_MASK, 0);
}

#[test]
fn task35_full_system_interrupt_masks_higher_line_before_vectoring() {
    let mut cpu = LoongArchCpu::new();
    cpu.set_pc(0x5000);
    cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, TIMER | (1 << 16));
    cpu.csr_write(CSR_EENTRY, 0x9100_0000);
    cpu.set_estat_hw(TIMER | IPI);
    let mut sys = full_system_cpu(cpu);

    assert_eq!(sys.cpu.pending_interrupt_line(), Some(11));
    sys.handle_interrupt();

    assert_eq!(sys.get_pc(), interrupt_vector(0x9100_0000, 11, 1));
    assert_eq!(sys.cpu.csr_read(CSR_ERA), 0x5000);
}
