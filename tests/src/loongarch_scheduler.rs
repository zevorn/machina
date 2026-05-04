use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use machina_accel::exec::{ExecEnv, ExitReason};
use machina_accel::X86_64CodeGen;
use machina_core::machine::{Machine, MachineOpts};
use machina_guest_loongarch::loongarch::cpu::{
    LoongArchCpu, GUEST_BASE_CPU_OFFSET, NEG_ALIGN_CPU_OFFSET,
};
use machina_hw_loongarch::boot::KERNEL_ENTRY_DEFAULT;
use machina_hw_loongarch::virt_machine::LoongArchVirtMachine;
use machina_system::loongarch_cpu::{
    loongarch_soft_mmu_config, LoongArchFullSystemCpu,
    SharedLoongArchFullSystemCpu,
};
use machina_system::CpuManager;

const IDLE_OP: u32 = 0b00000110010010001;

fn code15_insn(op: u32, code: u32) -> u32 {
    (op << 15) | (code & 0x7FFF)
}

fn default_opts() -> MachineOpts {
    MachineOpts {
        ram_size: 64 * 1024 * 1024,
        cpu_count: 1,
        kernel: None,
        bios: None,
        bios_builtin: false,
        append: None,
        nographic: false,
        drive: None,
        initrd: None,
        netdev: None,
    }
}

fn write_raw_kernel(insns: &[u32]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for insn in insns {
        file.write_all(&insn.to_le_bytes()).unwrap();
    }
    file.flush().unwrap();
    file
}

#[test]
fn task45_loongarch_full_system_cpu_sets_address_space_pointer() {
    let code = [code15_insn(IDLE_OP, 0)];
    let cpu = unsafe {
        LoongArchFullSystemCpu::new(
            LoongArchCpu::new(),
            code.as_ptr().cast::<u8>(),
            0,
            4,
            0x1234_5678,
            Arc::new(AtomicBool::new(true)),
        )
    };
    assert_eq!(cpu.cpu.address_space_ptr(), 0x1234_5678);
}

#[test]
fn task45_loongarch_virt_cpu_runs_under_cpu_manager() {
    let kernel = write_raw_kernel(&[code15_insn(IDLE_OP, 0)]);
    let mut opts = default_opts();
    opts.kernel = Some(kernel.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).unwrap();
    machine.boot().unwrap();

    let mut backend = X86_64CodeGen::new();
    backend.set_guest_base_offset(GUEST_BASE_CPU_OFFSET);
    backend.mmio = Some(loongarch_soft_mmu_config());
    backend.neg_align_off = i32::try_from(NEG_ALIGN_CPU_OFFSET).unwrap();
    let env = ExecEnv::new(backend);
    let shared = env.shared.clone();

    let mut manager = CpuManager::new();
    let stop_flag = manager.running_flag();
    let cpu = unsafe {
        SharedLoongArchFullSystemCpu::new(
            machine.cpu(),
            machine.ram_block().as_ptr(),
            0,
            opts.ram_size,
            machine.address_space() as *const _ as u64,
            Arc::clone(&stop_flag),
        )
    };
    manager.add_loongarch_cpu(cpu);

    let exit = unsafe { manager.run(&shared) };
    assert_eq!(exit, ExitReason::Halted);

    let cpu = machine.cpu();
    let cpu = cpu.lock().unwrap();
    assert_eq!(cpu.pc(), KERNEL_ENTRY_DEFAULT + 4);
    assert_ne!(cpu.address_space_ptr(), 0);
}
