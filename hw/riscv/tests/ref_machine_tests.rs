use machina_core::address::GPA;
use machina_core::machine::{Machine, MachineOpts};
use machina_guest_riscv::riscv::csr::PrivLevel;
use machina_hw_riscv::ref_machine::{RefMachine, MROM_BASE, RAM_BASE};

fn default_opts() -> MachineOpts {
    MachineOpts {
        ram_size: 128 * 1024 * 1024, // 128 MiB
        cpu_count: 1,
        kernel: None,
        bios: None,
        append: None,
        nographic: false,
        drive: None,
    }
}

#[test]
fn test_ref_machine_init() {
    let mut m = RefMachine::new();
    assert_eq!(m.name(), "riscv64-ref");
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

    // Read UART LSR (offset 5). THRE and TEMT should be set
    // on a freshly initialized UART: 0x60.
    let lsr_addr = GPA::new(0x1000_0005);
    let lsr = as_.read(lsr_addr, 1);
    assert_eq!(lsr & 0x60, 0x60, "THRE+TEMT not set");
}

#[test]
fn test_ref_machine_fdt_valid() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");
    let fdt = m.fdt_blob();

    // FDT magic: 0xD00DFEED in big-endian at offset 0.
    assert!(fdt.len() >= 4, "FDT too short");
    let magic = u32::from_be_bytes([fdt[0], fdt[1], fdt[2], fdt[3]]);
    assert_eq!(magic, 0xD00D_FEED, "bad FDT magic: {magic:#010x}");

    // FDT must contain "riscv,sv39" (not sv48).
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

    // The FDT blob should contain "cpu" as part of
    // the cpu@0 node name.
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
        append: None,
        nographic: false,
        drive: None,
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
    // CPU starts at MROM base; reset vector will set
    // a0/a1/a2 at runtime.
    assert_eq!(cpu.pc, MROM_BASE);
    assert_eq!(cpu.priv_level, PrivLevel::Machine);
}

// ---- New tests ----

#[test]
fn test_ref_machine_plic_contexts_multi_hart() {
    let mut m = RefMachine::new();
    let opts = MachineOpts {
        ram_size: 128 * 1024 * 1024,
        cpu_count: 2,
        kernel: None,
        bios: None,
        append: None,
        nographic: false,
        drive: None,
    };
    m.init(&opts).expect("init failed");

    // 2 harts → 4 PLIC contexts (M+S per hart).
    // Verify by writing enable bits for context 3
    // (the last valid context) and reading back.
    let as_ = m.address_space();
    let plic_base = 0x0C00_0000u64;
    // Enable base = 0x2000, stride = 0x80 per context.
    // Context 3 enable[0] at offset 0x2000 + 3*0x80 = 0x2180.
    let ctx3_enable = GPA::new(plic_base + 0x2000 + 3 * 0x80);
    as_.write(ctx3_enable, 4, 0xFFFF_FFFF);
    let val = as_.read(ctx3_enable, 4);
    assert_eq!(
        val, 0xFFFF_FFFF,
        "PLIC context 3 enable should be writable \
         with 2 harts (4 contexts)"
    );

    // Context 4 should be out of range → read returns 0.
    let ctx4_enable = GPA::new(plic_base + 0x2000 + 4 * 0x80);
    let val4 = as_.read(ctx4_enable, 4);
    assert_eq!(val4, 0, "PLIC context 4 should be out of range");
}

#[test]
fn test_ref_machine_irq_wiring() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    // UART IRQ line exists and routes to PLIC.
    // Raise UART IRQ (source 10) → PLIC pending bit 10.
    m.uart_irq().raise();
    {
        let mut plic = m.plic().lock().unwrap();
        // Read pending register: IRQ 10 is in word 0
        // (bits 0-31 cover IRQs 0-31).
        let pending = plic.read(0x1000, 4);
        assert_ne!(
            pending & (1 << 10),
            0,
            "UART IRQ 10 should be pending in PLIC"
        );
    }

    // Lower UART IRQ → pending bit cleared.
    m.uart_irq().lower();
    {
        let mut plic = m.plic().lock().unwrap();
        let pending = plic.read(0x1000, 4);
        assert_eq!(
            pending & (1 << 10),
            0,
            "UART IRQ 10 should be cleared in PLIC"
        );
    }

    // Verify CPU mip: MEI (11) should be low (PLIC has
    // default priority=0, threshold=0, so 0 > 0 is false).
    let cpus = m.cpus_lock();
    let cpu = cpus[0].as_ref().unwrap();
    assert_eq!(cpu.csr.mip & (1 << 11), 0, "MEI should be low initially");
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
    // CPU starts at MROM; a0/a1 are set by reset vector.
    assert_eq!(cpu.pc, MROM_BASE);
    assert_eq!(cpu.priv_level, PrivLevel::Machine);
}

#[test]
fn test_uart_tx_through_machine() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    // Write 'X' to UART THR via direct lock.
    // The attached chardev is NullChardev; verify no
    // panic and THRE stays set.
    {
        let mut uart = m.uart().lock().unwrap();
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
        let mut uart = m.uart().lock().unwrap();
        uart.write(1, 0x01); // IER: enable RX avail
    }

    // Receive a byte. UART's attached IrqLine should
    // route to PLIC set_irq(10, true).
    {
        let mut uart = m.uart().lock().unwrap();
        uart.receive(0x42);
        assert!(uart.irq_pending(), "UART IRQ should be pending");
    }

    // Verify PLIC has pending bit 10 set.
    {
        let mut plic = m.plic().lock().unwrap();
        let pending = plic.read(0x1000, 4);
        assert_ne!(
            pending & (1 << 10),
            0,
            "PLIC should have UART IRQ 10 pending"
        );
    }

    // Read RBR to clear the IRQ.
    {
        let mut uart = m.uart().lock().unwrap();
        let _ = uart.read(0);
        assert!(!uart.irq_pending(), "UART IRQ should be cleared after read");
    }

    // PLIC pending bit should now be clear.
    {
        let mut plic = m.plic().lock().unwrap();
        let pending = plic.read(0x1000, 4);
        assert_eq!(
            pending & (1 << 10),
            0,
            "PLIC UART IRQ 10 should be cleared"
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

    // The string "phandle" should appear in the DTB
    // strings block.
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
    // Priority offset = 4 * source.
    as_.write(GPA::new(plic_base + 4 * 10), 4, 1);

    // Enable source 10 for context 0 (M-mode hart 0).
    // Enable base = 0x2000, context 0, word 0, bit 10.
    let ctx0_en = GPA::new(plic_base + 0x2000);
    as_.write(ctx0_en, 4, 1 << 10);

    // Threshold for context 0 stays at 0 (default).
    // priority(1) > threshold(0) → active.

    // SharedMip should be clear before IRQ.
    use std::sync::atomic::Ordering;
    let mip = m.shared_mip();
    assert_eq!(
        mip.load(Ordering::SeqCst) & (1 << 11),
        0,
        "MEI should be clear before IRQ"
    );

    // Raise UART IRQ via PLIC source 10.
    m.uart_irq().raise();

    // SharedMip should now have MEI (bit 11) set.
    assert_ne!(
        mip.load(Ordering::SeqCst) & (1 << 11),
        0,
        "MEI should be set after PLIC source raise"
    );

    // Lower UART IRQ.
    m.uart_irq().lower();

    // MEI should be cleared.
    assert_eq!(
        mip.load(Ordering::SeqCst) & (1 << 11),
        0,
        "MEI should be cleared after IRQ lower"
    );
}

// ---- MROM / reset vector tests ----

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
    // First instruction: auipc t0, 0 → 0x00000297.
    let insn0 = as_.read(GPA::new(MROM_BASE), 4) as u32;
    assert_eq!(insn0, 0x0000_0297, "auipc t0, 0");
    // Second: addi a2, t0, 0x28 → 0x02828613.
    let insn1 = as_.read(GPA::new(MROM_BASE + 4), 4) as u32;
    assert_eq!(insn1, 0x0282_8613, "addi a2, t0, 0x28");
    // Third: csrr a0, mhartid → 0xf1402573.
    let insn2 = as_.read(GPA::new(MROM_BASE + 8), 4) as u32;
    assert_eq!(insn2, 0xf140_2573, "csrr a0, mhartid");
    // Sixth: jr t0 → 0x00028067.
    let insn5 = as_.read(GPA::new(MROM_BASE + 0x14), 4) as u32;
    assert_eq!(insn5, 0x0002_8067, "jr t0");

    // start_addr at offset 0x18 (dword): should be
    // RAM_BASE for -bios none.
    let start = as_.read(GPA::new(MROM_BASE + 0x18), 8);
    assert_eq!(start, RAM_BASE, "start_addr = RAM_BASE");

    // fdt_addr at offset 0x20 (dword): within RAM.
    let fdt = as_.read(GPA::new(MROM_BASE + 0x20), 8);
    assert!(fdt >= RAM_BASE, "fdt_addr within RAM");

    // fw_dynamic_info magic at offset 0x28.
    let magic = as_.read(GPA::new(MROM_BASE + 0x28), 8);
    assert_eq!(magic, 0x4942534f, "OSBI magic");
}

// ---- SiFive Test regressions ----

#[test]
fn test_sifive_test_mmio_read_returns_zero() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");
    let as_ = m.address_space();
    // Read from SiFive Test register @ 0x10_0000.
    let val = as_.read(GPA::new(0x10_0000), 4);
    assert_eq!(val, 0, "SiFive Test MMIO read must return 0");
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
