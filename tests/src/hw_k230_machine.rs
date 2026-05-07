use machina_core::machine::{Machine, MachineOpts};
use machina_guest_riscv::riscv::cpu_model::RiscvVendor;
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
fn k230_machine_uses_thead_c908_cpu_profile() {
    let mut machine = K230Machine::new();
    machine.init(&opts()).unwrap();

    let cpus = machine.cpus_lock();
    let cpu = cpus[0].as_ref().unwrap();
    let profile = cpu.profile();
    assert_eq!(profile.name, "thead-c908");
    assert_eq!(profile.vendor, RiscvVendor::Thead);
}
