use machina_core::address::GPA;
use machina_core::machine::{Machine, MachineOpts};
use machina_guest_riscv::riscv::cpu_model::RiscvVendor;
use machina_guest_riscv::riscv::csr::PrivLevel;
use machina_hw_core::fdt::FdtBuilder;
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

#[test]
fn k230_builtin_boot_places_kernel_in_sdk_dtb_memory_window() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = dir.path().join("Image");
    let dtb = dir.path().join("k230.dtb");
    std::fs::write(&kernel, [0x13, 0x00, 0x00, 0x00]).unwrap();
    std::fs::write(&dtb, sdk_memory_window_dtb()).unwrap();

    let mut machine = K230Machine::new();
    machine
        .init(&MachineOpts {
            kernel: Some(kernel),
            dtb: Some(dtb),
            bios: None,
            bios_builtin: true,
            ..opts()
        })
        .unwrap();
    machine.boot().unwrap();

    let cpus = machine.cpus_lock();
    let cpu = cpus[0].as_ref().unwrap();
    assert_eq!(cpu.pc, 0x0820_0000);
    assert_eq!(cpu.priv_level, PrivLevel::Supervisor);
    assert_eq!(cpu.gpr[10], 0);
    assert_eq!(cpu.gpr[11], 0x0fe0_0000);
    assert_eq!(
        machine.read_ram_bytes(0x0820_0000, 4).unwrap(),
        vec![0x13, 0x00, 0x00, 0x00]
    );
}

#[test]
fn k230_default_bios_loads_rustsbi_and_dynamic_info_for_sdk_boot() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = dir.path().join("Image");
    let dtb = dir.path().join("k230.dtb");
    std::fs::write(&kernel, [0x13, 0x00, 0x00, 0x00]).unwrap();
    std::fs::write(&dtb, sdk_memory_window_dtb()).unwrap();

    let mut machine = K230Machine::new();
    machine
        .init(&MachineOpts {
            kernel: Some(kernel),
            dtb: Some(dtb),
            bios: None,
            bios_builtin: false,
            ..opts()
        })
        .unwrap();
    machine.boot().unwrap();

    let bootrom = K230_MEMMAP[K230MemMap::Bootrom as usize].base;
    let cpus = machine.cpus_lock();
    let cpu = cpus[0].as_ref().unwrap();
    assert_eq!(cpu.pc, bootrom);
    assert_eq!(cpu.priv_level, PrivLevel::Machine);
    drop(cpus);

    assert_ne!(machine.read_ram_bytes(0, 4).unwrap(), vec![0, 0, 0, 0]);
    assert_eq!(
        machine.read_ram_bytes(0x0820_0000, 4).unwrap(),
        vec![0x13, 0x00, 0x00, 0x00]
    );
    assert_eq!(machine.address_space().read(GPA::new(bootrom + 24), 8), 0);
    assert_eq!(
        machine.address_space().read(GPA::new(bootrom + 32), 8),
        0x0fe0_0000
    );
    assert_eq!(
        machine.address_space().read(GPA::new(bootrom + 40), 8),
        0x4942_534f
    );
    assert_eq!(
        machine.address_space().read(GPA::new(bootrom + 56), 8),
        0x0820_0000
    );
    assert_eq!(machine.address_space().read(GPA::new(bootrom + 64), 8), 1);
}

fn sdk_memory_window_dtb() -> Vec<u8> {
    let mut fdt = FdtBuilder::new();
    fdt.begin_node("");
    fdt.property_u32("#address-cells", 2);
    fdt.property_u32("#size-cells", 2);

    fdt.begin_node("chosen");
    fdt.end_node();

    fdt.begin_node("memory@0");
    fdt.property_string("device_type", "memory");
    fdt.property_u32_list("reg", &[0, 0x0820_0000, 0, 0x07df_f000]);
    fdt.end_node();

    fdt.begin_node("soc");
    fdt.begin_node("sdhci0@91580000");
    fdt.property_string("status", "okay");
    fdt.end_node();
    fdt.begin_node("sdhci1@91581000");
    fdt.property_string("status", "okay");
    fdt.end_node();
    fdt.end_node();

    fdt.end_node();
    fdt.finish()
}
