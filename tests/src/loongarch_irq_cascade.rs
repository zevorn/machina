use std::io::Write;
use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
use machina_guest_loongarch::loongarch::csr::{CRMD_IE, CSR_CRMD, CSR_ECFG};
use machina_hw_char::uart::{Uart16550, Uart16550Mmio};
use machina_hw_core::bus::SysBus;
use machina_hw_loongarch::interrupt::{
    LoongArchInterruptCascade, LOONGARCH_DEVICE_HWI, LOONGARCH_UART_PCH_IRQ,
    LOONGARCH_VIRTIO_PCH_IRQ_BASE,
};
use machina_hw_virtio::block::VirtioBlk;
use machina_hw_virtio::mmio::VirtioMmio;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

fn cpu_with_enabled_hwi(hwi: u8) -> Arc<Mutex<LoongArchCpu>> {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, CRMD_IE);
    cpu.csr_write(CSR_ECFG, 1 << (u32::from(hwi) + 2));
    Arc::new(Mutex::new(cpu))
}

fn make_address_space() -> AddressSpace {
    AddressSpace::new(MemoryRegion::container("system", u64::MAX))
}

fn realize_uart(uart: &Arc<Uart16550>) {
    let mut bus = SysBus::new("sysbus0");
    let mut address_space = make_address_space();
    uart.attach_to_bus(&mut bus).unwrap();
    uart.register_mmio(
        MemoryRegion::io(
            "uart0",
            0x100,
            Arc::new(Uart16550Mmio(Arc::clone(uart))),
        ),
        GPA::new(0x1fe0_01e0),
    )
    .unwrap();
    let rx_cb: Arc<Mutex<dyn FnMut(u8) + Send>> =
        Arc::new(Mutex::new(|_byte: u8| {}));
    uart.realize_onto(&mut bus, &mut address_space, rx_cb)
        .unwrap();
}

fn make_virtio(
    cascade: &LoongArchInterruptCascade,
) -> (VirtioMmio, tempfile::TempPath) {
    let mut backing = tempfile::NamedTempFile::new().unwrap();
    backing.write_all(&[0u8; 512]).unwrap();
    let path = backing.into_temp_path();
    let blk = VirtioBlk::open(path.as_ref()).unwrap();
    let mmio = VirtioMmio::new_named(
        "virtio-mmio0",
        Box::new(blk),
        cascade.virtio_irq_line(0),
        std::ptr::null_mut(),
        0x9000_0000,
        128 * 1024 * 1024,
    );
    (mmio, path)
}

#[test]
fn task40_uart_rx_cascades_through_pch_pic_and_eiointc() {
    let cascade = LoongArchInterruptCascade::new("task40-uart", 1);
    let cpu = cpu_with_enabled_hwi(LOONGARCH_DEVICE_HWI);
    cascade.connect_cpu_hwi(0, LOONGARCH_DEVICE_HWI, Arc::clone(&cpu));
    cascade.route_pch_irq_to_cpu_hwi(
        LOONGARCH_UART_PCH_IRQ,
        LOONGARCH_UART_PCH_IRQ,
        0,
        LOONGARCH_DEVICE_HWI,
    );

    let uart = Arc::new(Uart16550::new_named("uart0"));
    cascade.attach_uart(&uart).unwrap();
    realize_uart(&uart);
    uart.write(1, 0x01);

    uart.receive(0x55);

    assert_eq!(
        cpu.lock().unwrap().pending_interrupt_line(),
        Some(u32::from(LOONGARCH_DEVICE_HWI) + 2)
    );

    let got = uart.read(0);
    assert_eq!(got, 0x55);
    assert_eq!(cpu.lock().unwrap().pending_interrupt_line(), None);
}

#[test]
fn task40_virtio_irq_cascades_through_pch_pic_and_eiointc() {
    let cascade = LoongArchInterruptCascade::new("task40-virtio", 1);
    let cpu = cpu_with_enabled_hwi(LOONGARCH_DEVICE_HWI);
    cascade.connect_cpu_hwi(0, LOONGARCH_DEVICE_HWI, Arc::clone(&cpu));
    cascade.route_pch_irq_to_cpu_hwi(
        LOONGARCH_VIRTIO_PCH_IRQ_BASE,
        LOONGARCH_VIRTIO_PCH_IRQ_BASE,
        0,
        LOONGARCH_DEVICE_HWI,
    );
    let (virtio, _path) = make_virtio(&cascade);

    virtio.shared_state().lock().unwrap().inject_rx(0, 1);

    assert_eq!(
        cpu.lock().unwrap().pending_interrupt_line(),
        Some(u32::from(LOONGARCH_DEVICE_HWI) + 2)
    );
    assert_eq!(virtio.read(0x060, 4), 1);

    virtio.write(0x064, 4, 1);

    assert_eq!(virtio.read(0x060, 4), 0);
    assert_eq!(cpu.lock().unwrap().pending_interrupt_line(), None);
}

#[test]
fn task40_uart_and_virtio_routes_coexist_on_same_cascade() {
    let cascade = LoongArchInterruptCascade::new("task40-both", 1);
    let cpu = cpu_with_enabled_hwi(LOONGARCH_DEVICE_HWI);
    cascade.connect_cpu_hwi(0, LOONGARCH_DEVICE_HWI, Arc::clone(&cpu));
    cascade.route_pch_irq_to_cpu_hwi(
        LOONGARCH_UART_PCH_IRQ,
        LOONGARCH_UART_PCH_IRQ,
        0,
        LOONGARCH_DEVICE_HWI,
    );
    cascade.route_pch_irq_to_cpu_hwi(
        LOONGARCH_VIRTIO_PCH_IRQ_BASE,
        LOONGARCH_VIRTIO_PCH_IRQ_BASE,
        0,
        LOONGARCH_DEVICE_HWI,
    );

    let uart = Arc::new(Uart16550::new_named("uart0"));
    cascade.attach_uart(&uart).unwrap();
    realize_uart(&uart);
    uart.write(1, 0x01);
    let (virtio, _path) = make_virtio(&cascade);

    uart.receive(0x33);

    assert_eq!(
        cpu.lock().unwrap().pending_interrupt_line(),
        Some(u32::from(LOONGARCH_DEVICE_HWI) + 2)
    );
    assert_eq!(uart.read(0), 0x33);
    assert_eq!(cpu.lock().unwrap().pending_interrupt_line(), None);

    virtio.shared_state().lock().unwrap().inject_rx(0, 1);

    assert_eq!(
        cpu.lock().unwrap().pending_interrupt_line(),
        Some(u32::from(LOONGARCH_DEVICE_HWI) + 2)
    );
    virtio.write(0x064, 4, 1);
    assert_eq!(cpu.lock().unwrap().pending_interrupt_line(), None);
}
