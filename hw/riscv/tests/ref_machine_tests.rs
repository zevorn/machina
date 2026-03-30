use machina_core::address::GPA;
use machina_core::machine::{Machine, MachineOpts};
use machina_hw_riscv::boot;
use machina_hw_riscv::ref_machine::RefMachine;

fn default_opts() -> MachineOpts {
    MachineOpts {
        ram_size: 128 * 1024 * 1024, // 128 MiB
        cpu_count: 1,
        kernel: None,
        bios: None,
        append: None,
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
    };
    let result = m.init(&opts);
    assert!(result.is_err(), "init with 0 RAM should fail");
}

#[test]
fn test_ref_machine_boot_setup() {
    let mut m = RefMachine::new();
    m.init(&default_opts()).expect("init failed");

    let bios = [0x13u8; 64]; // NOP sled
    let kernel = [0xAAu8; 128];

    let info = boot::setup_boot(&m, Some(&bios), Some(&kernel))
        .expect("setup_boot failed");

    // Entry PC should be RAM_BASE.
    assert_eq!(info.entry_pc, 0x8000_0000);

    // FDT address should be within RAM range.
    let ram_end = 0x8000_0000u64 + m.ram_size();
    assert!(
        info.fdt_addr >= 0x8000_0000 && info.fdt_addr < ram_end,
        "fdt_addr {:#x} out of RAM range",
        info.fdt_addr
    );

    // Verify BIOS bytes were written at RAM_BASE.
    let as_ = m.address_space();
    let first_word = as_.read(GPA::new(0x8000_0000), 4);
    assert_eq!(first_word, 0x13131313, "bios data mismatch");

    // Verify kernel bytes at RAM_BASE + 0x20_0000.
    let kernel_word = as_.read(GPA::new(0x8020_0000), 4);
    assert_eq!(kernel_word, 0xAAAAAAAA, "kernel data mismatch");
}
