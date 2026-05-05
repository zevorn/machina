use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use machina_accel::ir::tb::{
    EXCP_ARCH_BASE, EXCP_LOONGARCH_DONE, EXCP_LOONGARCH_END,
    EXCP_LOONGARCH_WFI, EXCP_RISCV_EBREAK, EXCP_RISCV_ECALL, EXCP_RISCV_END,
    EXCP_RISCV_FENCE_I, EXCP_RISCV_MRET, EXCP_RISCV_PRIV_CSR,
    EXCP_RISCV_SFENCE_VMA, EXCP_RISCV_SRET, EXCP_RISCV_WFI,
};
use machina_accel::{ArchExitAction, GuestCpu};
use machina_core::address::GPA;
use machina_core::wfi::WfiWaker;
use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
use machina_guest_riscv::riscv::cpu::RiscvCpu;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MemoryRegion;
use machina_system::cpus::{FullSystemCpu, SharedMip};
use machina_system::loongarch_cpu::LoongArchFullSystemCpu;

const RISCV_RAM_BASE: u64 = 0x8000_0000;

fn riscv_cpu() -> (FullSystemCpu, Box<AddressSpace>) {
    let root = MemoryRegion::container("root", u64::MAX);
    let (ram_region, ram_block) = MemoryRegion::ram("ram", 0x1000);
    let mut addr_space = Box::new(AddressSpace::new(root));
    addr_space
        .root_mut()
        .add_subregion(ram_region, GPA::new(RISCV_RAM_BASE));
    addr_space.update_flat_view();

    let ram_ptr = ram_block.as_ptr() as *const u8;
    let shared_mip: SharedMip = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let wfi_waker = Arc::new(WfiWaker::new());
    let stop_flag = Arc::new(AtomicBool::new(true));
    let cpu = unsafe {
        FullSystemCpu::new(
            RiscvCpu::new(),
            ram_ptr,
            RISCV_RAM_BASE,
            0x1000,
            shared_mip,
            wfi_waker,
            &*addr_space as *const AddressSpace,
            stop_flag,
        )
    };
    (cpu, addr_space)
}

fn loongarch_cpu() -> LoongArchFullSystemCpu {
    let code = Box::leak(Box::new([0_u32]));
    unsafe {
        LoongArchFullSystemCpu::new(
            LoongArchCpu::new(),
            code.as_ptr().cast::<u8>(),
            0,
            4,
            0,
            Arc::new(AtomicBool::new(true)),
        )
    }
}

#[test]
fn task46_exit_namespace_partitions_architectures() {
    assert_eq!(EXCP_ARCH_BASE, 16);

    for code in [
        EXCP_RISCV_ECALL,
        EXCP_RISCV_EBREAK,
        EXCP_RISCV_MRET,
        EXCP_RISCV_SRET,
        EXCP_RISCV_WFI,
        EXCP_RISCV_SFENCE_VMA,
        EXCP_RISCV_PRIV_CSR,
        EXCP_RISCV_FENCE_I,
    ] {
        assert!((EXCP_ARCH_BASE..EXCP_RISCV_END).contains(&code));
    }

    for code in [EXCP_LOONGARCH_DONE, EXCP_LOONGARCH_WFI] {
        assert!((EXCP_RISCV_END..EXCP_LOONGARCH_END).contains(&code));
    }

    assert_ne!(EXCP_RISCV_WFI, EXCP_LOONGARCH_WFI);
    assert_ne!(EXCP_RISCV_ECALL, EXCP_LOONGARCH_DONE);
}

#[test]
fn task46_riscv_ecall_dispatch_returns_privilege_action() {
    let (mut cpu, _addr_space) = riscv_cpu();
    assert_eq!(
        cpu.handle_arch_exit(EXCP_RISCV_ECALL),
        ArchExitAction::Ecall { priv_level: 3 }
    );
}

#[test]
fn task46_loongarch_done_and_idle_dispatch_through_arch_handler() {
    let mut cpu = loongarch_cpu();
    assert_eq!(
        cpu.handle_arch_exit(EXCP_LOONGARCH_DONE),
        ArchExitAction::Continue
    );
    assert_eq!(
        cpu.handle_arch_exit(EXCP_LOONGARCH_WFI),
        ArchExitAction::Halted
    );
    assert!(!cpu.is_halted());
}
