use crate::cpu::{ArchExitAction, GuestCpu};
use crate::ir::tb::{
    EXCP_RISCV_EBREAK, EXCP_RISCV_ECALL, EXCP_RISCV_FENCE_I, EXCP_RISCV_MRET,
    EXCP_RISCV_PRIV_CSR, EXCP_RISCV_SFENCE_VMA, EXCP_RISCV_SRET,
    EXCP_RISCV_WFI,
};

pub fn handle_riscv_arch_exit<C: GuestCpu>(
    cpu: &mut C,
    code: u64,
) -> ArchExitAction {
    match code {
        EXCP_RISCV_MRET => {
            cpu.execute_mret();
            ArchExitAction::Continue
        }
        EXCP_RISCV_SRET => {
            if !cpu.execute_sret() {
                let cur = cpu.get_pc().wrapping_sub(4);
                cpu.set_pc(cur);
                cpu.handle_exception(2, 0);
            }
            ArchExitAction::Continue
        }
        EXCP_RISCV_SFENCE_VMA => {
            if cpu.check_sfence_trap() {
                let cur = cpu.get_pc().wrapping_sub(4);
                cpu.set_pc(cur);
                cpu.handle_exception(2, 0);
                ArchExitAction::Continue
            } else {
                cpu.tlb_flush();
                ArchExitAction::FlushAllTb
            }
        }
        EXCP_RISCV_FENCE_I => {
            ArchExitAction::FlushDirtyTbPages(cpu.take_dirty_pages())
        }
        EXCP_RISCV_WFI => {
            cpu.set_halted(true);
            if !cpu.pending_wfi_wakeup() {
                if !cpu.wait_for_interrupt() {
                    cpu.set_halted(false);
                    return ArchExitAction::Halted;
                }
                if cpu.check_monitor_pause() {
                    cpu.set_halted(false);
                    return ArchExitAction::Halted;
                }
            }
            cpu.set_halted(false);
            if cpu.pending_interrupt() {
                cpu.handle_interrupt();
            }
            ArchExitAction::Continue
        }
        EXCP_RISCV_PRIV_CSR => {
            if !cpu.handle_priv_csr() {
                cpu.handle_exception(2, 0);
                return ArchExitAction::FlushPendingTbNonRetired(1);
            }
            ArchExitAction::FlushPendingTb
        }
        EXCP_RISCV_EBREAK => {
            let pc = cpu.get_pc();
            cpu.handle_exception(3, pc);
            ArchExitAction::Continue
        }
        EXCP_RISCV_ECALL => ArchExitAction::Ecall {
            priv_level: cpu.privilege_level(),
        },
        _ => ArchExitAction::Exit(code as usize),
    }
}
