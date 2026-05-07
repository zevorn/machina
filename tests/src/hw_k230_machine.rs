use machina_core::address::GPA;
use machina_core::machine::{Machine, MachineOpts};
use machina_guest_riscv::riscv::cpu_model::RiscvVendor;
use machina_guest_riscv::riscv::csr::PrivLevel;
use machina_hw_riscv::k230::{
    K230IrqMap, K230Machine, K230MemMap, K230WdtIndex, K230_MEMMAP,
    K230_PLIC_NUM_SOURCES,
};

fn opts() -> MachineOpts {
    MachineOpts {
        ram_size: 0x8000_0000,
        cpu_count: 1,
        kernel: None,
        bios: Some("none".into()),
        bios_builtin: false,
        append: None,
        nographic: false,
        drive: None,
        initrd: None,
        dtb: None,
        loaders: Vec::new(),
        netdev: None,
    }
}

#[test]
fn k230_memmap_matches_qemu_reference_points() {
    assert_eq!(K230_MEMMAP[K230MemMap::Ddr as usize].base, 0x0000_0000);
    assert_eq!(K230_MEMMAP[K230MemMap::Sram as usize].base, 0x8020_0000);
    assert_eq!(K230_MEMMAP[K230MemMap::Bootrom as usize].base, 0x9120_0000);
    assert_eq!(
        K230_MEMMAP[K230MemMap::Plic as usize].base,
        0x000f_0000_0000
    );
    assert_eq!(
        K230_MEMMAP[K230MemMap::Clint as usize].base,
        0x000f_0400_0000
    );
    assert_eq!(K230_PLIC_NUM_SOURCES, 208);
    assert_eq!(K230IrqMap::UART0, 16);
    assert_eq!(K230IrqMap::WDT0, 107);
}

#[test]
fn k230_machine_maps_real_devices_and_unimp_windows() {
    let mut machine = K230Machine::new();
    machine.init(&opts()).unwrap();

    let sysbus = machine.sysbus();
    assert!(sysbus.mappings().iter().any(|m| m.owner == "plic0"));
    assert!(sysbus.mappings().iter().any(|m| m.owner == "aclint0"));
    assert!(sysbus.mappings().iter().any(|m| m.owner == "uart0"));
    assert!(sysbus.mappings().iter().any(|m| m.owner == "uart4"));
    assert!(sysbus.mappings().iter().any(|m| m.owner == "k230-wdt0"));
    assert!(sysbus.mappings().iter().any(|m| m.owner == "k230-wdt1"));
    assert!(sysbus.mappings().iter().any(|m| m.owner == "kpu.l2-cache"));
    assert!(machine.wdt(K230WdtIndex::Wdt0).is_some());
    assert!(machine.wdt(K230WdtIndex::Wdt1).is_some());
}

#[test]
fn k230_machine_rejects_ram_outside_fixed_ddr_window() {
    let mut small = K230Machine::new();
    let err = small
        .init(&MachineOpts {
            ram_size: 0x4000_0000,
            ..opts()
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("K230 RAM size must be exactly 0x80000000 bytes"));

    let mut large = K230Machine::new();
    let err = large
        .init(&MachineOpts {
            ram_size: 0x8000_0000 + 1,
            ..opts()
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("K230 RAM size must be exactly 0x80000000 bytes"));
}

#[test]
fn k230_machine_uses_thead_c908_cpu_profile() {
    let mut machine = K230Machine::new();
    machine.init(&opts()).unwrap();

    let cpus = machine.cpus_lock();
    let cpu = cpus[0].as_ref().unwrap();
    let profile = cpu.profile();
    assert_eq!(profile.name, "thead-c908");
    assert_eq!(profile.vendor, RiscvVendor::Thead);
}

#[test]
fn k230_boot_writes_reset_vector_and_sets_cpu_pc() {
    let mut machine = K230Machine::new();
    machine.init(&opts()).unwrap();
    machine.boot().unwrap();

    let bootrom = K230_MEMMAP[K230MemMap::Bootrom as usize];
    let cpus = machine.cpus_lock();
    let cpu = cpus[0].as_ref().unwrap();
    assert_eq!(cpu.pc, bootrom.base);

    let first_word =
        machine.address_space().read(GPA::new(bootrom.base), 4) as u32;
    assert_eq!(first_word, 0x0000_0297);
}

#[test]
fn k230_builtin_boot_enters_kernel_in_supervisor_mode() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = dir.path().join("Image");
    std::fs::write(&kernel, [0x6f, 0x00, 0x00, 0x00]).unwrap();

    let mut machine = K230Machine::new();
    machine
        .init(&MachineOpts {
            kernel: Some(kernel),
            bios: None,
            bios_builtin: true,
            ..opts()
        })
        .unwrap();
    machine.boot().unwrap();

    let cpus = machine.cpus_lock();
    let cpu = cpus[0].as_ref().unwrap();
    assert_eq!(cpu.pc, K230_MEMMAP[K230MemMap::Ddr as usize].base);
    assert_eq!(cpu.priv_level, PrivLevel::Supervisor);
    assert_eq!(cpu.gpr[10], 0);
    assert_eq!(cpu.csr.medeleg, 0xb1ff);
    assert_eq!(cpu.csr.mideleg, 0x0222);
}
