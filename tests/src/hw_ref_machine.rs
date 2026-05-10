use machina_core::address::GPA;
use machina_core::machine::{Machine, MachineOpts};
use machina_guest_riscv::riscv::csr::PrivLevel;
use machina_hw_core::bus::SysBus;
use machina_hw_firmware::keys;
use machina_hw_riscv::ref_machine::{
    RefMachine, RefMemMap, MROM_BASE, RAM_BASE, REF_MEMMAP,
};
use machina_hw_riscv::sifive_test::{ShutdownReason, SifiveTest};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};
use std::fs;
use std::io::Write;
use std::sync::{Arc, Mutex};

fn default_opts() -> MachineOpts {
    MachineOpts {
        ram_size: 128 * 1024 * 1024, // 128 MiB
        cpu_count: 1,
        kernel: None,
        bios: None,
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

fn make_test_aspace() -> (AddressSpace, SysBus) {
    let root = MemoryRegion::container("root", 0x1_0000_0000);
    let aspace = AddressSpace::new(root);
    let bus = SysBus::new("sysbus");
    (aspace, bus)
}

#[test]
fn test_ref_machine_init() {
    let mut m = RefMachine::new();
    assert_eq!(m.name(), "riscv64-ref");
    assert_eq!(m.machine_state().object().object_path(), Some("/machine"));
    m.init(&default_opts()).expect("init failed");
    assert_eq!(m.cpu_count(), 1);
    assert_eq!(m.ram_size(), 128 * 1024 * 1024);
}

#[test]
fn test_ref_machine_memory_map() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");
    let as_ = m.address_space();

    // Write 0xDEADBEEF at RAM_BASE, read it back.
    let ram_base = GPA::new(0x8000_0000);
    as_.write(ram_base, 4, 0xDEAD_BEEF);
    let val = as_.read(ram_base, 4);
    assert_eq!(val, 0xDEAD_BEEF);

    // Write/read at RAM_BASE + 8.
    let addr2 = GPA::new(0x8000_0008);
    as_.write(addr2, 8, 0x1234_5678_9ABC_DEF0);
    let val2 = as_.read(addr2, 8);
    assert_eq!(val2, 0x1234_5678_9ABC_DEF0);
}

#[test]
fn test_ref_machine_uart_mmio() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");
    let as_ = m.address_space();

    // Read UART LSR (offset 5). THRE and TEMT set: 0x60.
    let lsr_addr = GPA::new(0x1000_0005);
    let lsr = as_.read(lsr_addr, 1);
    assert_eq!(lsr & 0x60, 0x60, "THRE+TEMT not set");
}

#[test]
fn test_ref_machine_fw_cfg_mmio_signature() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");
    let as_ = m.address_space();
    let fw_cfg = REF_MEMMAP[RefMemMap::FwCfg as usize].base;

    as_.write(GPA::new(fw_cfg + 0x08), 2, u64::from(keys::SIGNATURE));

    assert_eq!(as_.read(GPA::new(fw_cfg), 4), 0x5145_4d55);
}

#[test]
fn test_ref_machine_fw_cfg_is_in_fdt() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    let fw_cfg = REF_MEMMAP[RefMemMap::FwCfg as usize];
    let fdt = m.fdt_blob();
    let node = format!("fw_cfg@{:x}", fw_cfg.base);
    assert!(
        fdt.windows(node.len()).any(|w| w == node.as_bytes()),
        "FDT must contain fw_cfg node {node}"
    );
    let compatible = b"qemu,fw-cfg-mmio";
    assert!(
        fdt.windows(compatible.len()).any(|w| w == compatible),
        "FDT must advertise the fw_cfg MMIO compatible"
    );

    let mut reg = Vec::new();
    reg.extend_from_slice(&0u32.to_be_bytes());
    reg.extend_from_slice(&(fw_cfg.base as u32).to_be_bytes());
    reg.extend_from_slice(&0u32.to_be_bytes());
    reg.extend_from_slice(&(fw_cfg.size as u32).to_be_bytes());
    assert!(
        fdt.windows(reg.len()).any(|w| w == reg.as_slice()),
        "FDT fw_cfg node must describe the board MMIO range"
    );
}

#[test]
fn test_ref_machine_fw_cfg_is_realized_via_sysbus_and_mom() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    let fw_cfg = REF_MEMMAP[RefMemMap::FwCfg as usize];
    assert!(
        m.sysbus().mappings().iter().any(|mapping| {
            mapping.owner == "fw_cfg0"
                && mapping.base == GPA::new(fw_cfg.base)
                && mapping.size == fw_cfg.size
        }),
        "fw_cfg must be realized through sysbus"
    );
    assert!(
        m.lookup_object_by_local_id("fw_cfg0").is_some(),
        "fw_cfg must be visible in the MOM object tree"
    );
}

#[test]
fn test_ref_machine_pflash0_is_mapped_mmio_and_in_fdt() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    const PFLASH_BASE: u64 = 0x2000_0000;
    const PFLASH_SIZE: u64 = 32 * 1024 * 1024;

    assert!(
        m.sysbus()
            .mappings()
            .iter()
            .any(|mapping| mapping.owner == "pflash0"
                && mapping.base == GPA::new(PFLASH_BASE)
                && mapping.size == PFLASH_SIZE),
        "pflash0 must be realized through sysbus at the virt flash window"
    );
    assert!(
        m.pflash0().realized(),
        "pflash0 device state must be realized through its own MOM lifecycle"
    );

    let as_ = m.address_space();
    as_.write(GPA::new(PFLASH_BASE + 0x55), 1, 0x98);
    assert_eq!(as_.read(GPA::new(PFLASH_BASE + 0x40), 1), u64::from(b'Q'));
    assert_eq!(as_.read(GPA::new(PFLASH_BASE + 0x44), 1), u64::from(b'R'));
    assert_eq!(as_.read(GPA::new(PFLASH_BASE + 0x48), 1), u64::from(b'Y'));

    as_.write(GPA::new(PFLASH_BASE), 1, 0xff);
    as_.write(GPA::new(PFLASH_BASE), 1, 0x40);
    as_.write(GPA::new(PFLASH_BASE + 0x20), 1, 0x0f);
    as_.write(GPA::new(PFLASH_BASE), 1, 0xff);
    assert_eq!(as_.read(GPA::new(PFLASH_BASE + 0x20), 1), 0x0f);

    let fdt = m.fdt_blob();
    let node = b"flash@20000000";
    assert!(
        fdt.windows(node.len()).any(|w| w == node),
        "FDT must contain the pflash0 node"
    );
    let compatible = b"cfi-flash";
    assert!(
        fdt.windows(compatible.len()).any(|w| w == compatible),
        "FDT must advertise the CFI flash compatible"
    );
    let mut reg = Vec::new();
    reg.extend_from_slice(&0u32.to_be_bytes());
    reg.extend_from_slice(&(PFLASH_BASE as u32).to_be_bytes());
    reg.extend_from_slice(&0u32.to_be_bytes());
    reg.extend_from_slice(&(PFLASH_SIZE as u32).to_be_bytes());
    assert!(
        fdt.windows(reg.len()).any(|w| w == reg.as_slice()),
        "FDT pflash0 node must describe the board MMIO range"
    );
}

#[test]
fn test_ref_machine_rtc_is_mapped_irq_wired_and_in_fdt() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    let rtc = REF_MEMMAP[RefMemMap::Rtc as usize];
    assert!(
        m.sysbus()
            .mappings()
            .iter()
            .any(|mapping| mapping.owner == "goldfish-rtc0"
                && mapping.base == GPA::new(rtc.base)
                && mapping.size == rtc.size),
        "RTC must be realized through sysbus at the ref memmap address"
    );

    let as_ = m.address_space();
    as_.write(GPA::new(rtc.base), 4, 0x1234_5678);
    assert_eq!(as_.read(GPA::new(rtc.base), 4), 0x1234_5678);

    let plic_base = REF_MEMMAP[RefMemMap::Plic as usize].base;
    as_.write(GPA::new(plic_base + 4 * u64::from(11u32)), 4, 1);
    as_.write(GPA::new(plic_base + 0x2000), 4, 1 << 11);
    as_.write(GPA::new(rtc.base + 0x10), 4, 1);
    as_.write(GPA::new(rtc.base + 0x08), 4, 0);
    assert_ne!(
        m.plic().read(0x1000, 4) & (1 << 11),
        0,
        "RTC alarm must raise PLIC source 11"
    );

    let fdt = m.fdt_blob();
    let node = format!("rtc@{:x}", rtc.base);
    assert!(
        fdt.windows(node.len()).any(|w| w == node.as_bytes()),
        "FDT must contain RTC node {node}"
    );
    let compatible = b"google,goldfish-rtc";
    assert!(
        fdt.windows(compatible.len()).any(|w| w == compatible),
        "FDT must advertise the goldfish RTC compatible"
    );
}

#[test]
fn test_ref_machine_uart_is_realized_via_sysbus() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    assert_eq!(m.sysbus().mappings().len(), 7);
    assert!(m
        .sysbus()
        .mappings()
        .iter()
        .any(|mapping| mapping.owner == "uart0"));
    assert!(m
        .sysbus()
        .mappings()
        .iter()
        .any(|mapping| mapping.owner == "plic0"));
    assert!(m
        .sysbus()
        .mappings()
        .iter()
        .any(|mapping| mapping.owner == "aclint0"));
    assert!(m
        .sysbus()
        .mappings()
        .iter()
        .any(|mapping| mapping.owner == "fw_cfg0"));
    assert!(m
        .sysbus()
        .mappings()
        .iter()
        .any(|mapping| mapping.owner == "goldfish-rtc0"));
    assert!(m
        .sysbus()
        .mappings()
        .iter()
        .any(|mapping| mapping.owner == "sifive-test0"));
    assert!(m
        .sysbus()
        .mappings()
        .iter()
        .any(|mapping| mapping.owner == "pflash0"));

    let uart = m.uart();
    assert_eq!(
        uart.chardev_property().as_deref(),
        Some("/machine/chardev/uart0")
    );
}

#[test]
fn test_ref_machine_virtio_is_realized_via_sysbus() {
    let mut image = tempfile::NamedTempFile::new().unwrap();
    image.write_all(&[0u8; 512]).unwrap();

    let mut m = RefMachine::new();
    let mut opts = default_opts();
    opts.drive = Some(image.path().to_path_buf());
    m.init(&opts).expect("init failed");

    assert_eq!(m.sysbus().mappings().len(), 8);
    assert!(m
        .sysbus()
        .mappings()
        .iter()
        .any(|mapping| mapping.owner == "virtio-mmio0"));
    assert_eq!(m.address_space().read(GPA::new(0x1000_1000), 4), 0x74726976);
}

#[test]
fn test_ref_machine_sysbus_owner_set_matches_migrated_devices() {
    let mut image = tempfile::NamedTempFile::new().unwrap();
    image.write_all(&[0u8; 512]).unwrap();

    let mut m = RefMachine::new();
    let mut opts = default_opts();
    opts.drive = Some(image.path().to_path_buf());
    m.init(&opts).expect("init failed");

    let mut owners = m
        .sysbus()
        .mappings()
        .iter()
        .map(|mapping| mapping.owner.as_str())
        .collect::<Vec<_>>();
    owners.sort_unstable();

    assert_eq!(
        owners,
        vec![
            "aclint0",
            "fw_cfg0",
            "goldfish-rtc0",
            "pflash0",
            "plic0",
            "sifive-test0",
            "uart0",
            "virtio-mmio0"
        ]
    );
}

#[test]
fn test_ref_machine_fdt_virtio_node_tracks_sysbus_mapping() {
    let mut image = tempfile::NamedTempFile::new().unwrap();
    image.write_all(&[0u8; 512]).unwrap();

    let mut m = RefMachine::new();
    let mut opts = default_opts();
    opts.drive = Some(image.path().to_path_buf());
    m.init(&opts).expect("init failed");

    let mapping = m
        .sysbus()
        .mappings()
        .iter()
        .find(|mapping| mapping.owner == "virtio-mmio0")
        .unwrap();
    let node_name = format!("virtio_mmio@{:x}", mapping.base.0);
    let fdt = m.fdt_blob();

    assert!(
        fdt.windows(node_name.len())
            .any(|window| window == node_name.as_bytes()),
        "FDT should use sysbus-derived virtio-mmio node name"
    );
}

#[test]
fn test_ref_machine_source_has_no_direct_migrated_mmio_root_wiring() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../hw/riscv/src/ref_machine.rs"
    );
    let source = fs::read_to_string(path).expect("read ref_machine.rs");
    let compact = source
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();

    for forbidden in [
        "root.add_subregion(plic_region,GPA::new(PLIC_BASE))",
        "root.add_subregion(aclint_region,GPA::new(ACLINT_BASE))",
        "root.add_subregion(uart_region,GPA::new(UART0_BASE))",
        "root.add_subregion(virtio_region,GPA::new(VIRTIO0_BASE))",
    ] {
        assert!(
            !compact.contains(forbidden),
            "migrated devices must not bypass MOM/sysbus: found {forbidden}"
        );
    }
}

#[test]
fn test_ref_machine_fdt_valid() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");
    let fdt = m.fdt_blob();

    // FDT magic: 0xD00DFEED big-endian at offset 0.
    assert!(fdt.len() >= 4, "FDT too short");
    let magic = u32::from_be_bytes([fdt[0], fdt[1], fdt[2], fdt[3]]);
    assert_eq!(magic, 0xD00D_FEED, "bad FDT magic: {magic:#010x}");

    // FDT must contain "riscv,sv39".
    let sv39 = b"riscv,sv39";
    let has_sv39 = fdt.windows(sv39.len()).any(|w| w == sv39);
    assert!(has_sv39, "FDT missing riscv,sv39 mmu-type");

    // Must NOT contain sv48.
    let sv48 = b"riscv,sv48";
    let has_sv48 = fdt.windows(sv48.len()).any(|w| w == sv48);
    assert!(!has_sv48, "FDT still contains riscv,sv48");
}

#[test]
fn test_ref_machine_fdt_has_cpu() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");
    let fdt = m.fdt_blob();

    let has_cpu = fdt.windows(3).any(|w| w == b"cpu");
    assert!(has_cpu, "FDT does not contain 'cpu'");
}

#[test]
fn test_ref_machine_zero_ram_fails() {
    let mut m = RefMachine::new();
    let opts = MachineOpts {
        ram_size: 0,
        cpu_count: 1,
        kernel: None,
        bios: None,
        bios_builtin: false,
        append: None,
        nographic: false,
        drive: None,
        initrd: None,
        dtb: None,
        loaders: Vec::new(),
        netdev: None,
    };
    let result = m.init(&opts);
    assert!(result.is_err(), "init with 0 RAM should fail");
}

#[test]
fn test_ref_machine_boot_bios_none() {
    let mut m = RefMachine::new();
    let opts = MachineOpts {
        bios: Some("none".into()),
        ..default_opts()
    };
    m.init(&opts).expect("init failed");
    m.boot().expect("boot failed");

    let cpus = m.cpus_lock();
    let cpu = cpus[0].as_ref().unwrap();
    assert_eq!(cpu.pc, MROM_BASE);
    assert_eq!(cpu.priv_level, PrivLevel::Machine);
}

// -- New tests --

#[test]
fn test_ref_machine_plic_contexts_multi_hart() {
    let mut m = RefMachine::new();
    let opts = MachineOpts {
        ram_size: 128 * 1024 * 1024,
        cpu_count: 2,
        kernel: None,
        bios: None,
        bios_builtin: false,
        append: None,
        nographic: false,
        drive: None,
        initrd: None,
        dtb: None,
        loaders: Vec::new(),
        netdev: None,
    };
    m.init(&opts).expect("init failed");

    // 2 harts -> 4 PLIC contexts (M+S per hart).
    let as_ = m.address_space();
    let plic_base = 0x0C00_0000u64;
    let ctx3_enable = GPA::new(plic_base + 0x2000 + 3 * 0x80);
    as_.write(ctx3_enable, 4, 0xFFFF_FFFF);
    let val = as_.read(ctx3_enable, 4);
    assert_eq!(
        val, 0xFFFF_FFFF,
        "PLIC context 3 enable should be writable \
         with 2 harts (4 contexts)"
    );

    // Context 4 should be out of range.
    let ctx4_enable = GPA::new(plic_base + 0x2000 + 4 * 0x80);
    let val4 = as_.read(ctx4_enable, 4);
    assert_eq!(val4, 0, "PLIC context 4 should be out of range");
}

#[test]
fn test_ref_machine_irq_wiring() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    // Raise UART IRQ (source 10) -> PLIC pending bit 10.
    m.uart_irq().raise();
    {
        let pending = m.plic().read(0x1000, 4);
        assert_ne!(
            pending & (1 << 10),
            0,
            "UART IRQ 10 should be pending in PLIC"
        );
    }

    // Lower UART IRQ. With edge-triggered semantics,
    // pending stays set until claimed.
    m.uart_irq().lower();
    {
        let pending = m.plic().read(0x1000, 4);
        assert_ne!(
            pending & (1 << 10),
            0,
            "UART IRQ 10 stays pending until claimed \
             (edge-triggered)"
        );
    }
}

#[test]
fn test_ref_machine_boot_cpu_state() {
    let mut m = RefMachine::new();
    let opts = MachineOpts {
        bios: Some("none".into()),
        ..default_opts()
    };
    m.init(&opts).expect("init failed");
    m.boot().expect("boot failed");

    let cpus = m.cpus_lock();
    let cpu = cpus[0].as_ref().unwrap();
    assert_eq!(cpu.pc, MROM_BASE);
    assert_eq!(cpu.priv_level, PrivLevel::Machine);
}

#[test]
fn test_uart_tx_through_machine() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    {
        let uart = m.uart();
        uart.write(0, 0x58); // 'X'
        let lsr = uart.read(5);
        assert_ne!(lsr & 0x20, 0, "THRE should remain set after TX");
    }
}

#[test]
fn test_uart_rx_irq_to_plic() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    // Enable RX interrupt on UART.
    {
        let uart = m.uart();
        uart.write(1, 0x01);
    }

    // Receive a byte.
    {
        let uart = m.uart();
        uart.receive(0x42);
        assert!(uart.irq_pending(), "UART IRQ should be pending");
    }

    // Verify PLIC has pending bit 10 set.
    {
        let pending = m.plic().read(0x1000, 4);
        assert_ne!(
            pending & (1 << 10),
            0,
            "PLIC should have UART IRQ 10 pending"
        );
    }

    // Read RBR to clear the IRQ.
    {
        let uart = m.uart();
        let _ = uart.read(0);
        assert!(!uart.irq_pending(), "UART IRQ should be cleared after read");
    }

    // With edge-triggered PLIC, pending stays set until
    // claimed even though the source was lowered.
    {
        let pending = m.plic().read(0x1000, 4);
        assert_ne!(
            pending & (1 << 10),
            0,
            "PLIC UART IRQ 10 stays pending until claimed"
        );
    }
}

#[test]
fn test_boot_sets_cpu_state() {
    let mut m = RefMachine::new();
    let opts = MachineOpts {
        bios: Some("none".into()),
        ..default_opts()
    };
    m.init(&opts).expect("init failed");
    m.boot().expect("boot failed");

    let cpus = m.cpus_lock();
    let cpu = cpus[0].as_ref().unwrap();
    assert_eq!(cpu.pc, MROM_BASE, "pc = MROM_BASE");
    assert_eq!(
        cpu.priv_level,
        PrivLevel::Machine,
        "privilege should be Machine"
    );
}

#[test]
fn test_take_cpu_preserves_boot_state() {
    let mut m = RefMachine::new();
    let opts = MachineOpts {
        bios: Some("none".into()),
        ..default_opts()
    };
    m.init(&opts).expect("init failed");
    m.boot().expect("boot failed");

    let cpu = m.take_cpu(0).expect("take_cpu failed");
    assert_eq!(cpu.pc, MROM_BASE, "pc preserved");
    assert_eq!(cpu.priv_level, PrivLevel::Machine, "priv preserved");

    let cpus = m.cpus_lock();
    assert!(cpus[0].is_none(), "cpus[0] must be None after take");

    drop(cpus);
    assert!(m.take_cpu(0).is_none(), "double take must return None");
}

#[test]
fn test_fdt_has_phandle() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");
    let fdt = m.fdt_blob();

    let needle = b"phandle";
    let found = fdt.windows(needle.len()).any(|w| w == needle);
    assert!(found, "FDT should contain phandle property");
}

#[test]
fn test_fdt_has_interrupts_extended() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");
    let fdt = m.fdt_blob();

    let needle = b"interrupts-extended";
    let found = fdt.windows(needle.len()).any(|w| w == needle);
    assert!(found, "FDT should contain interrupts-extended");
}

#[test]
fn test_irq_updates_cpu_mip() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    let plic_base = 0x0C00_0000u64;
    let as_ = m.address_space();

    // Set priority for source 10 (UART) to 1.
    as_.write(GPA::new(plic_base + 4 * 10), 4, 1);

    // Enable source 10 for context 0 (M-mode hart 0).
    let ctx0_en = GPA::new(plic_base + 0x2000);
    as_.write(ctx0_en, 4, 1 << 10);

    use std::sync::atomic::Ordering;
    let mip = m.shared_mip();
    assert_eq!(
        mip.load(Ordering::SeqCst) & (1 << 11),
        0,
        "MEI should be clear before IRQ"
    );

    // Raise UART IRQ via PLIC source 10.
    m.uart_irq().raise();

    assert_ne!(
        mip.load(Ordering::SeqCst) & (1 << 11),
        0,
        "MEI should be set after PLIC source raise"
    );

    // Lower UART IRQ. Edge-triggered: pending stays, so
    // MEI remains asserted until claimed.
    m.uart_irq().lower();

    assert_ne!(
        mip.load(Ordering::SeqCst) & (1 << 11),
        0,
        "MEI stays set until claimed (edge-triggered)"
    );
}

// -- MROM / reset vector tests --

#[test]
fn test_mrom_reset_vector_content() {
    let mut m = RefMachine::new();
    let opts = MachineOpts {
        bios: Some("none".into()),
        ..default_opts()
    };
    m.init(&opts).expect("init failed");
    m.boot().expect("boot failed");

    let as_ = m.address_space();
    // First instruction: auipc t0, 0
    let insn0 = as_.read(GPA::new(MROM_BASE), 4) as u32;
    assert_eq!(insn0, 0x0000_0297, "auipc t0, 0");
    // Second: addi a2, t0, 0x28
    let insn1 = as_.read(GPA::new(MROM_BASE + 4), 4) as u32;
    assert_eq!(insn1, 0x0282_8613, "addi a2, t0, 0x28");
    // Third: csrr a0, mhartid
    let insn2 = as_.read(GPA::new(MROM_BASE + 8), 4) as u32;
    assert_eq!(insn2, 0xf140_2573, "csrr a0, mhartid");
    // Sixth: jr t0
    let insn5 = as_.read(GPA::new(MROM_BASE + 0x14), 4) as u32;
    assert_eq!(insn5, 0x0002_8067, "jr t0");

    // start_addr at offset 0x18 (dword).
    let start = as_.read(GPA::new(MROM_BASE + 0x18), 8);
    assert_eq!(start, RAM_BASE, "start_addr = RAM_BASE");

    // fdt_addr at offset 0x20 (dword): within RAM.
    let fdt = as_.read(GPA::new(MROM_BASE + 0x20), 8);
    assert!(fdt >= RAM_BASE, "fdt_addr within RAM");

    // fw_dynamic_info magic at offset 0x28.
    let magic = as_.read(GPA::new(MROM_BASE + 0x28), 8);
    assert_eq!(magic, 0x4942534f, "OSBI magic");
}

// -- SiFive Test regressions --

#[test]
fn test_sifive_test_lifecycle_and_mom_identity() {
    let dev = Arc::new(SifiveTest::new());
    assert!(!dev.realized());
    dev.with_mdevice(|device| assert_eq!(device.local_id(), "sifive-test"));
    assert_eq!(dev.object_info().local_id, "sifive-test");

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA::new(0x10_0000);

    dev.attach_to_bus(&mut bus).unwrap();
    dev.register_mmio(
        MemoryRegion::io("sifive-test", 0x1000, dev.clone()),
        base,
    )
    .unwrap();

    dev.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(dev.realized());
    assert_eq!(aspace.read(base, 4), 0);

    let err = dev.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(
        err.to_string().contains("already realized"),
        "unexpected second-realize error: {err}"
    );

    aspace.write(base, 4, 0x5555);
    assert!(dev.is_triggered());
    dev.reset_runtime();
    assert!(!dev.is_triggered());

    dev.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!dev.realized());
    assert_eq!(aspace.read(base, 4), 0);

    let err = dev.unrealize_from(&mut bus, &mut aspace).unwrap_err();
    assert!(
        err.to_string().contains("not realized"),
        "unexpected second-unrealize error: {err}"
    );
}

#[test]
fn test_sifive_test_mmio_read_returns_zero() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");
    let as_ = m.address_space();
    let val = as_.read(GPA::new(0x10_0000), 4);
    assert_eq!(val, 0, "SiFive Test MMIO read must return 0");
}

#[test]
fn test_sifive_test_ignores_writes_away_from_finisher_offset() {
    let dev = SifiveTest::new();
    let reason = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&reason);
    dev.set_shutdown_handler(Box::new(move |value| {
        *seen.lock().unwrap() = Some(value);
    }));

    dev.write(4, 4, 0x5555);

    assert!(!dev.is_triggered());
    assert_eq!(*reason.lock().unwrap(), None);
}

#[test]
fn test_sifive_test_rejects_unsupported_access_sizes() {
    let dev = SifiveTest::new();
    let reason = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&reason);
    dev.set_shutdown_handler(Box::new(move |value| {
        *seen.lock().unwrap() = Some(value);
    }));

    for size in [1_u32, 8] {
        dev.write(0, size, 0x5555);
        assert!(!dev.is_triggered(), "size {size} must be ignored");
        assert_eq!(*reason.lock().unwrap(), None);
    }
}

#[test]
fn test_sifive_test_decodes_status_and_exit_code_fields() {
    let dev = SifiveTest::new();
    let reason = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&reason);
    dev.set_shutdown_handler(Box::new(move |value| {
        *seen.lock().unwrap() = Some(value);
    }));

    dev.write(0, 4, 0x002a_5555);
    assert_eq!(*reason.lock().unwrap(), Some(ShutdownReason::Pass));

    dev.reset_runtime();
    *reason.lock().unwrap() = None;
    dev.write(0, 4, 0x0000_7777);
    assert_eq!(*reason.lock().unwrap(), Some(ShutdownReason::Reset));

    dev.reset_runtime();
    *reason.lock().unwrap() = None;
    dev.write(0, 4, 0x002b_3333);
    assert_eq!(*reason.lock().unwrap(), Some(ShutdownReason::Fail(0x2b)));
}

#[test]
fn test_sifive_test_is_realized_via_sysbus_and_mom() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    let st = REF_MEMMAP[RefMemMap::SifiveTest as usize];
    assert!(
        m.sysbus().mappings().iter().any(|mapping| {
            mapping.owner == "sifive-test0"
                && mapping.base == GPA::new(st.base)
                && mapping.size == st.size
        }),
        "SiFive Test must be realized through sysbus"
    );
    assert!(
        m.lookup_object_by_local_id("sifive-test0").is_some(),
        "SiFive Test must be visible in the MOM object tree"
    );
}

#[test]
fn test_sifive_test_dtb_has_node() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");
    let fdt = m.fdt_blob();
    let needle = b"sifive,test0";
    let found = fdt.windows(needle.len()).any(|w| w == needle);
    assert!(found, "FDT must contain 'sifive,test0' compatible");
}

#[test]
fn test_sifive_test_pass_triggers_shutdown() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");
    let st = m.sifive_test().clone();
    assert!(
        !st.is_triggered(),
        "device should not be triggered before write"
    );
    let as_ = m.address_space();
    // Write PASS (0x5555).
    as_.write(GPA::new(0x10_0000), 4, 0x5555);
    assert!(
        st.is_triggered(),
        "device must be triggered after PASS write"
    );
}

// ── VirtIO net integration (AC-4) ────────────────────

#[test]
fn test_ref_machine_no_net_device_without_netdev() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");
    let as_ = m.address_space();
    // 0x10002000 should be unmapped (no net device).
    assert!(
        !as_.is_mapped(GPA::new(0x1000_2000), 4),
        "net MMIO region should not be mapped \
         without -netdev"
    );
}

#[test]
fn test_ref_machine_blk_device_unaffected() {
    let dir = tempfile::tempdir().unwrap();
    let disk = dir.path().join("disk.raw");
    {
        let mut f = fs::File::create(&disk).unwrap();
        f.write_all(&[0u8; 512]).unwrap();
    }
    let mut m = RefMachine::new();
    let opts = MachineOpts {
        drive: Some(disk),
        ..default_opts()
    };
    m.init(&opts).expect("init failed");
    let as_ = m.address_space();
    // Block device at 0x10001000 should still work.
    assert!(as_.is_mapped(GPA::new(0x1000_1000), 4),);
    let magic = as_.read(GPA::new(0x1000_1000), 4);
    assert_eq!(magic, 0x74726976); // "virt"
}

#[cfg(unix)]
#[test]
fn test_ref_machine_net_via_pipe_backend() {
    use machina_hw_virtio::net::{PipeBackend, VirtioNet};
    use std::sync::Arc;

    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    let pipe = PipeBackend::new().unwrap();
    let net = VirtioNet::new_default(Arc::new(pipe));
    m.add_virtio_net(net).expect("add_virtio_net failed");

    let as_ = m.address_space();
    assert!(
        as_.is_mapped(GPA::new(0x1000_2000), 4),
        "net MMIO should be mapped at 0x10002000"
    );
    let magic = as_.read(GPA::new(0x1000_2000), 4);
    assert_eq!(magic, 0x74726976);
    let dev_id = as_.read(GPA::new(0x1000_2000 + 0x008), 4);
    assert_eq!(dev_id, 1); // net device

    assert!(m
        .sysbus()
        .mappings()
        .iter()
        .any(|m| m.owner == "virtio-mmio1"));
}

#[cfg(unix)]
#[test]
fn test_ref_machine_net_fdt_node() {
    use machina_hw_virtio::net::{PipeBackend, VirtioNet};
    use std::sync::Arc;

    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    let pipe = PipeBackend::new().unwrap();
    let net = VirtioNet::new_default(Arc::new(pipe));
    m.add_virtio_net(net).expect("add_virtio_net failed");

    let fdt = m.fdt_blob();
    let node = b"virtio_mmio@10002000";
    assert!(
        fdt.windows(node.len()).any(|w| w == node),
        "FDT should contain virtio_mmio@10002000"
    );

    // Verify IRQ 12 is present in the FDT (u32 BE).
    let irq12_be = 12u32.to_be_bytes();
    assert!(
        fdt.windows(4).any(|w| w == irq12_be),
        "FDT net node should contain IRQ 12"
    );
}

#[cfg(unix)]
#[test]
fn test_ref_machine_blk_and_net_coexist() {
    use machina_hw_virtio::net::{PipeBackend, VirtioNet};
    use std::sync::Arc;

    let dir = tempfile::tempdir().unwrap();
    let disk = dir.path().join("disk.raw");
    {
        let mut f = fs::File::create(&disk).unwrap();
        f.write_all(&[0u8; 512]).unwrap();
    }
    let mut m = RefMachine::new();
    let opts = MachineOpts {
        drive: Some(disk),
        ..default_opts()
    };
    m.init(&opts).expect("init failed");

    let pipe = PipeBackend::new().unwrap();
    let net = VirtioNet::new_default(Arc::new(pipe));
    m.add_virtio_net(net).expect("add_virtio_net failed");

    let as_ = m.address_space();
    // Block device at 0x10001000 still works.
    assert!(as_.is_mapped(GPA::new(0x1000_1000), 4));
    assert_eq!(as_.read(GPA::new(0x1000_1000), 4), 0x74726976);
    // Net device at 0x10002000 also works.
    assert!(as_.is_mapped(GPA::new(0x1000_2000), 4));
    assert_eq!(as_.read(GPA::new(0x1000_2000), 4), 0x74726976);
    // Different device IDs.
    let blk_id = as_.read(GPA::new(0x1000_1000 + 8), 4);
    let net_id = as_.read(GPA::new(0x1000_2000 + 8), 4);
    assert_eq!(blk_id, 2); // block
    assert_eq!(net_id, 1); // net
}
