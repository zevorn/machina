use std::io::Write;
use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_core::machine::{Machine, MachineOpts, NetdevOpts};
use machina_guest_loongarch::loongarch::csr::{
    CRMD_DA, CRMD_IE, CSR_CRMD, CSR_ECFG,
};
use machina_hw_core::bus::SysBusMapping;
use machina_hw_core::chardev::{ByteCb, CharFrontend, Chardev};
use machina_hw_loongarch::interrupt::LOONGARCH_DEVICE_HWI;
use machina_hw_loongarch::virt_machine::{
    LoongArchVirtMachine, VIRT_EIOINTC_BASE, VIRT_EIOINTC_SIZE, VIRT_IPI_BASE,
    VIRT_IPI_SIZE, VIRT_PCH_PIC_BASE, VIRT_PCH_PIC_SIZE, VIRT_RAM_BASE,
    VIRT_UART_BASE, VIRT_UART_SIZE, VIRT_VIRTIO_BASE, VIRT_VIRTIO_SIZE,
};

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

fn assert_mapping(
    mappings: &[SysBusMapping],
    owner: &str,
    base: u64,
    size: u64,
) {
    assert!(
        mappings.iter().any(|mapping| {
            mapping.owner == owner
                && mapping.base == GPA::new(base)
                && mapping.size == size
        }),
        "missing mapping {owner} @ {base:#x} size {size:#x}"
    );
}

fn enable_device_hwi(machine: &LoongArchVirtMachine) {
    let cpu = machine.cpu();
    let mut cpu = cpu.lock().unwrap();
    cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, 1 << (u32::from(LOONGARCH_DEVICE_HWI) + 2));
}

struct CapturingInputChardev {
    input_cb: Arc<Mutex<Option<ByteCb>>>,
}

impl Chardev for CapturingInputChardev {
    fn read(&mut self) -> Option<u8> {
        None
    }

    fn write(&mut self, _data: u8) {}

    fn can_read(&self) -> bool {
        true
    }

    fn start_input(&mut self, cb: ByteCb) {
        *self.input_cb.lock().unwrap() = Some(cb);
    }
}

#[test]
fn task42_virt_board_realizes_expected_mmio_map() {
    let mut machine = LoongArchVirtMachine::new();
    let opts = default_opts();
    machine.init(&opts).expect("init loongarch virt");

    assert_eq!(machine.name(), "loongarch64-virt");
    assert_eq!(machine.cpu_count(), 1);
    assert_eq!(machine.ram_size(), opts.ram_size);
    assert_eq!(VIRT_PCH_PIC_SIZE, 0x400);

    let mappings = machine.sysbus().mappings();
    assert_mapping(&mappings, "uart0", VIRT_UART_BASE, VIRT_UART_SIZE);
    assert_mapping(&mappings, "ipi0", VIRT_IPI_BASE, VIRT_IPI_SIZE);
    assert_mapping(&mappings, "eiointc0", VIRT_EIOINTC_BASE, VIRT_EIOINTC_SIZE);
    assert_mapping(&mappings, "pch-pic0", VIRT_PCH_PIC_BASE, VIRT_PCH_PIC_SIZE);

    assert!(
        machine
            .address_space()
            .is_mapped(GPA::new(VIRT_PCH_PIC_BASE + 0x3e7), 1),
        "PCH-PIC INT_POL must be inside the board mapping"
    );
    assert!(
        !machine
            .address_space()
            .is_mapped(GPA::new(VIRT_PCH_PIC_BASE + VIRT_PCH_PIC_SIZE), 1),
        "PCH-PIC mapping must not extend past the declared 0x400 region"
    );

    machine
        .address_space()
        .write(GPA::new(VIRT_RAM_BASE), 4, 0xfeed_beef);
    assert_eq!(
        machine.address_space().read(GPA::new(VIRT_RAM_BASE), 4),
        0xfeed_beef
    );

    let cpu = machine.cpu();
    let cpu = cpu.lock().unwrap();
    assert_eq!(cpu.ram_base_val(), VIRT_RAM_BASE);
    assert_eq!(cpu.ram_end_val(), VIRT_RAM_BASE + opts.ram_size);
}

#[test]
fn task87_virt_board_rejects_unsupported_virtio_net_options() {
    let mut opts = default_opts();
    opts.netdev = Some(NetdevOpts {
        id: "net0".to_string(),
        ifname: "tap0".to_string(),
        mac: Some("52:54:00:12:34:56".to_string()),
    });

    let mut machine = LoongArchVirtMachine::new();
    let err = machine
        .init(&opts)
        .expect_err("loongarch64-virt must reject unsupported virtio-net");
    let msg = err.to_string();
    assert!(
        msg.contains(
            "loongarch64-virt does not support virtio-net-device/-netdev"
        ),
        "missing virtio-net rejection message: {msg}"
    );
}

#[test]
fn task88_virtio_dma_uses_guest_low_ram_base() {
    let mut image = tempfile::NamedTempFile::new().unwrap();
    image.write_all(&[0u8; 512]).unwrap();

    let mut opts = default_opts();
    opts.drive = Some(image.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).expect("init loongarch virt with drive");

    let (_ram_ptr, ram_base, ram_size) = machine
        .virtio_mmio()
        .expect("virtio-mmio0")
        .shared_state()
        .lock()
        .unwrap()
        .ram_info();
    assert_eq!(
        ram_base, 0,
        "virtio DMA must use the low physical RAM base visible to Linux"
    );
    assert_eq!(ram_size, opts.ram_size);
}

#[test]
fn task42_virt_board_installs_iocsr_and_uart_cascade() {
    let mut machine = LoongArchVirtMachine::new();
    machine.init(&default_opts()).expect("init loongarch virt");

    assert!(
        machine.iocsr_bus().write(0, 0x14c0, 4, 0x0202_0202),
        "board IOCSR bus must accept EIOINTC aliases"
    );
    assert_eq!(
        machine.cpu().lock().unwrap().iocsr_read(0x14c0, 4),
        0x0202_0202,
        "CPU IOCSR dispatcher must read through the board bus"
    );
    assert_eq!(
        machine.eiointc().mmio_read_sized(0, 0x0c0, 4),
        0x0202_0202,
        "CPU IOCSR dispatcher must route EIOINTC aliases to the board device"
    );

    enable_device_hwi(&machine);
    machine
        .address_space()
        .write(GPA::new(VIRT_UART_BASE + 1), 1, 1);
    machine.uart().receive(0x41);

    let expected_line = Some(u32::from(LOONGARCH_DEVICE_HWI) + 2);
    assert_eq!(
        machine.cpu().lock().unwrap().pending_interrupt_line(),
        expected_line
    );
    assert_eq!(
        machine.address_space().read(GPA::new(VIRT_UART_BASE), 1),
        0x41
    );
    assert_eq!(machine.cpu().lock().unwrap().pending_interrupt_line(), None);
}

#[test]
fn task82_virt_board_chardev_input_reaches_uart_rx_fifo() {
    let input_cb = Arc::new(Mutex::new(None));
    let frontend = CharFrontend::new(Box::new(CapturingInputChardev {
        input_cb: Arc::clone(&input_cb),
    }));

    let mut machine = LoongArchVirtMachine::new();
    machine.set_uart_chardev(frontend).unwrap();
    machine.init(&default_opts()).expect("init loongarch virt");

    enable_device_hwi(&machine);
    machine
        .address_space()
        .write(GPA::new(VIRT_UART_BASE + 1), 1, 1);

    let cb = input_cb
        .lock()
        .unwrap()
        .as_ref()
        .expect("UART realize must start chardev input")
        .clone();
    cb.lock().unwrap()(0x5A);

    let expected_line = Some(u32::from(LOONGARCH_DEVICE_HWI) + 2);
    assert_eq!(
        machine.cpu().lock().unwrap().pending_interrupt_line(),
        expected_line
    );
    assert_eq!(
        machine.address_space().read(GPA::new(VIRT_UART_BASE), 1),
        0x5A
    );
    assert_eq!(machine.cpu().lock().unwrap().pending_interrupt_line(), None);
}

#[test]
fn task42_virt_board_realizes_optional_virtio_cascade() {
    let mut image = tempfile::NamedTempFile::new().unwrap();
    image.write_all(&[0u8; 512]).unwrap();

    let mut opts = default_opts();
    opts.drive = Some(image.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).expect("init loongarch virt");

    assert_mapping(
        &machine.sysbus().mappings(),
        "virtio-mmio0",
        VIRT_VIRTIO_BASE,
        VIRT_VIRTIO_SIZE,
    );
    assert_eq!(
        machine.address_space().read(GPA::new(VIRT_VIRTIO_BASE), 4),
        0x7472_6976
    );

    enable_device_hwi(&machine);
    machine
        .virtio_mmio()
        .expect("virtio-mmio0")
        .shared_state()
        .lock()
        .unwrap()
        .inject_rx(0, 1);

    let expected_line = Some(u32::from(LOONGARCH_DEVICE_HWI) + 2);
    assert_eq!(
        machine.cpu().lock().unwrap().pending_interrupt_line(),
        expected_line
    );
    assert_eq!(
        machine
            .address_space()
            .read(GPA::new(VIRT_VIRTIO_BASE + 0x060), 4),
        1
    );

    machine
        .address_space()
        .write(GPA::new(VIRT_VIRTIO_BASE + 0x064), 4, 1);

    assert_eq!(
        machine
            .address_space()
            .read(GPA::new(VIRT_VIRTIO_BASE + 0x060), 4),
        0
    );
    assert_eq!(machine.cpu().lock().unwrap().pending_interrupt_line(), None);
}
