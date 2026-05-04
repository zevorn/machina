use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use machina_accel::GuestCpu;
use machina_core::address::GPA;
use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
use machina_guest_loongarch::loongarch::csr::{
    CRMD_DA, CRMD_IE, CSR_CRMD, CSR_ECFG, CSR_EENTRY, CSR_ERA, CSR_ESTAT,
    CSR_PRMD, CSR_TCFG, CSR_TICLR,
};
use machina_hw_char::uart::{Uart16550, Uart16550Mmio};
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_intc::eiointc::{Eiointc, EiointcIrqSink};
use machina_hw_intc::ipi::LoongArchIpi;
use machina_hw_intc::pch_pic::PchPic;
use machina_hw_loongarch::interrupt::{
    LoongArchInterruptCascade, LOONGARCH_DEVICE_HWI, LOONGARCH_UART_PCH_IRQ,
    LOONGARCH_VIRTIO_PCH_IRQ_BASE,
};
use machina_hw_loongarch::iocsr::VirtIocsrBus;
use machina_hw_virtio::block::VirtioBlk;
use machina_hw_virtio::mmio::VirtioMmio;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};
use machina_system::loongarch_cpu::LoongArchFullSystemCpu;

const SWI0: u64 = 1 << 0;
const SWI1: u64 = 1 << 1;
const HWI0: u64 = 1 << 2;
const HWI1: u64 = 1 << 3;
const TIMER: u64 = 1 << 11;
const IPI: u64 = 1 << 12;

const IPI_CORE_ENABLE: u64 = 0x004;
const IPI_CORE_CLEAR: u64 = 0x00c;
const IOCSR_IPI_SEND: u64 = 0x040;

const EIO_NODEMAP: u64 = 0x0a0;
const EIO_IPMAP: u64 = 0x0c0;
const EIO_ENABLE: u64 = 0x200;
const EIO_CORE_ISR: u64 = 0x400;
const EIO_COREMAP: u64 = 0x800;

const PCH_INT_MASK: u64 = 0x020;
const PCH_INT_EDGE: u64 = 0x060;
const PCH_INT_CLEAR: u64 = 0x080;
const PCH_HTMSI_VEC: u64 = 0x200;

const IOCSR_EIO_NODEMAP: u32 = 0x14a0;

struct CpuHwiSink {
    cpu: Arc<Mutex<LoongArchCpu>>,
}

impl IrqSink for CpuHwiSink {
    fn set_irq(&self, irq: u32, level: bool) {
        self.cpu
            .lock()
            .unwrap()
            .set_hwi_interrupt_pending(irq as u8, level);
    }
}

struct CpuIpiSink {
    cpu: Arc<Mutex<LoongArchCpu>>,
}

impl IrqSink for CpuIpiSink {
    fn set_irq(&self, _irq: u32, level: bool) {
        self.cpu.lock().unwrap().set_ipi_interrupt_pending(level);
    }
}

fn make_address_space() -> AddressSpace {
    AddressSpace::new(MemoryRegion::container("system", u64::MAX))
}

fn cpu_with_lie(mask: u64) -> Arc<Mutex<LoongArchCpu>> {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, mask);
    Arc::new(Mutex::new(cpu))
}

fn connect_eio_hwi(
    eiointc: &Eiointc,
    cpu_id: u32,
    hwi: u8,
    cpu: &Arc<Mutex<LoongArchCpu>>,
) {
    eiointc.connect_hwi_output(
        cpu_id,
        hwi,
        InterruptSource::new(
            Arc::new(CpuHwiSink {
                cpu: Arc::clone(cpu),
            }) as Arc<dyn IrqSink>,
            u32::from(hwi),
        ),
    );
}

fn connect_ipi_cpu(
    ipi: &LoongArchIpi,
    cpu_id: u32,
    cpu: &Arc<Mutex<LoongArchCpu>>,
) {
    ipi.connect_output(
        cpu_id,
        InterruptSource::new(
            Arc::new(CpuIpiSink {
                cpu: Arc::clone(cpu),
            }) as Arc<dyn IrqSink>,
            0,
        ),
    );
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

fn unmask_word(irq: u32) -> u64 {
    u64::MAX & !(1u64 << irq)
}

fn full_system_cpu(cpu: LoongArchCpu) -> LoongArchFullSystemCpu {
    let code = Box::leak(Box::new([0x0340_0000u32]));
    let ptr = code.as_ptr().cast::<u8>();
    unsafe {
        LoongArchFullSystemCpu::new(
            cpu,
            ptr,
            0,
            4,
            Arc::new(AtomicBool::new(true)),
        )
    }
}

fn interrupt_vector(base: u64, irq: u32, vs: u64) -> u64 {
    if vs == 0 {
        base
    } else {
        base.wrapping_add(u64::from(64 + irq) * ((1_u64 << vs) * 4))
    }
}

#[test]
fn task41_eiointc_nodemap_iocsr_state_is_readable() {
    let ipi = Arc::new(LoongArchIpi::new_named("ipi0", 2));
    let eiointc = Arc::new(Eiointc::new_named("eiointc0", 2));
    let bus = VirtIocsrBus::new(ipi, Arc::clone(&eiointc));

    assert_eq!(bus.read(0, IOCSR_EIO_NODEMAP, 4), Some(0));

    assert!(bus.write(0, IOCSR_EIO_NODEMAP, 4, 0x0002_0001));
    assert!(bus.write(0, IOCSR_EIO_NODEMAP + 4, 4, 0x0008_0004));

    assert_eq!(bus.read(0, IOCSR_EIO_NODEMAP, 4), Some(0x0002_0001));
    assert_eq!(bus.read(0, IOCSR_EIO_NODEMAP + 4, 4), Some(0x0008_0004));
    assert_eq!(eiointc.mmio_read_sized(0, EIO_NODEMAP, 4), 0x0002_0001);

    assert!(bus.write(0, IOCSR_EIO_NODEMAP + 1, 1, 0xaa));

    assert_eq!(bus.read(0, IOCSR_EIO_NODEMAP, 4), Some(0x0002_aa01));
}

#[test]
fn task41_software_interrupt_set_clear_matrix() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, SWI0 | SWI1);

    assert_eq!(cpu.csr_read(CSR_ESTAT) & 0x3, 0);
    assert_eq!(cpu.pending_interrupt_line(), None);

    cpu.csr_write(CSR_ESTAT, SWI0);
    assert_eq!(cpu.csr_read(CSR_ESTAT) & 0x3, SWI0);
    assert_eq!(cpu.pending_interrupt_line(), Some(0));

    cpu.csr_write(CSR_ESTAT, SWI0 | SWI1);
    assert_eq!(cpu.csr_read(CSR_ESTAT) & 0x3, SWI0 | SWI1);
    assert_eq!(cpu.pending_interrupt_line(), Some(1));

    cpu.csr_write(CSR_ESTAT, SWI1);
    assert_eq!(cpu.csr_read(CSR_ESTAT) & 0x3, SWI1);
    assert_eq!(cpu.pending_interrupt_line(), Some(1));

    cpu.csr_write(CSR_ESTAT, 0);
    assert_eq!(cpu.csr_read(CSR_ESTAT) & 0x3, 0);
    assert_eq!(cpu.pending_interrupt_line(), None);
}

#[test]
fn task41_cpu_timer_ipi_priority_mask_and_clear_matrix() {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, SWI0 | HWI0 | TIMER | IPI);
    cpu.csr_write(CSR_ESTAT, SWI0);
    cpu.set_hwi_interrupt_pending(0, true);
    cpu.csr_write(CSR_TCFG, 0x0101);
    cpu.timer_tick(0x100);
    cpu.set_ipi_interrupt_pending(true);

    assert_eq!(cpu.pending_interrupt_line(), Some(12));

    cpu.set_ipi_interrupt_pending(false);
    assert_eq!(cpu.pending_interrupt_line(), Some(11));

    cpu.csr_write(CSR_TICLR, 1);
    assert_eq!(cpu.pending_interrupt_line(), Some(2));

    cpu.set_hwi_interrupt_pending(0, false);
    assert_eq!(cpu.pending_interrupt_line(), Some(0));

    cpu.csr_write(CSR_CRMD, CRMD_DA);
    assert_eq!(cpu.pending_interrupt_line(), None);

    cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, 0);
    assert_eq!(cpu.pending_interrupt_line(), None);
}

#[test]
fn task41_nested_interrupt_entry_records_handler_state() {
    let eentry = 0x9000_0000;
    let vs = 1;
    let initial_pc = 0x4000;
    let handler_pc = 0x9000_1000;
    let mut cpu = LoongArchCpu::new();
    cpu.set_pc(initial_pc);
    cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, SWI0 | IPI | (vs << 16));
    cpu.csr_write(CSR_EENTRY, eentry);
    cpu.csr_write(CSR_ESTAT, SWI0);
    let mut sys = full_system_cpu(cpu);

    assert_eq!(sys.cpu.pending_interrupt_line(), Some(0));
    sys.handle_interrupt();

    assert_eq!(sys.get_pc(), interrupt_vector(eentry, 0, vs));
    assert_eq!(sys.cpu.csr_read(CSR_ERA), initial_pc);
    assert_eq!(sys.cpu.csr_read(CSR_PRMD), CRMD_IE);
    assert_eq!(sys.cpu.csr_read(CSR_CRMD) & CRMD_IE, 0);

    sys.set_pc(handler_pc);
    sys.cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    sys.cpu.set_ipi_interrupt_pending(true);

    assert_eq!(sys.cpu.pending_interrupt_line(), Some(12));
    sys.handle_interrupt();

    assert_eq!(sys.get_pc(), interrupt_vector(eentry, 12, vs));
    assert_eq!(sys.cpu.csr_read(CSR_ERA), handler_pc);
    assert_eq!(sys.cpu.csr_read(CSR_PRMD), CRMD_IE);
    assert_eq!(sys.cpu.csr_read(CSR_CRMD) & CRMD_IE, 0);
}

#[test]
fn task41_ipi_send_clear_and_enable_mask_matrix() {
    let ipi = LoongArchIpi::new_named("ipi0", 2);
    let cpu1 = cpu_with_lie(IPI);
    connect_ipi_cpu(&ipi, 1, &cpu1);

    ipi.mmio_write_sized(0, IOCSR_IPI_SEND, 4, (1 << 16) | 5);
    assert_eq!(cpu1.lock().unwrap().pending_interrupt_line(), None);

    ipi.mmio_write_sized(1, IPI_CORE_ENABLE, 4, 1 << 5);
    assert_eq!(cpu1.lock().unwrap().pending_interrupt_line(), Some(12));

    ipi.mmio_write_sized(1, IPI_CORE_CLEAR, 4, 1 << 5);
    assert_eq!(cpu1.lock().unwrap().pending_interrupt_line(), None);
}

#[test]
fn task41_eiointc_nodemap_route_ack_and_cpu_isolation_matrix() {
    let eiointc = Eiointc::new_named("eiointc0", 2);
    let cpu0 = cpu_with_lie(HWI1);
    let cpu1 = cpu_with_lie(HWI1);
    connect_eio_hwi(&eiointc, 0, 1, &cpu0);
    connect_eio_hwi(&eiointc, 1, 1, &cpu1);

    eiointc.mmio_write_sized(0, EIO_NODEMAP, 4, 0x0002_0001);
    eiointc.mmio_write_sized(0, EIO_IPMAP, 4, 0x0202_0202);
    eiointc.mmio_write_sized(0, EIO_COREMAP + 4, 1, 0x02);
    eiointc.mmio_write_sized(0, EIO_ENABLE, 4, 1 << 4);
    eiointc.set_irq(4, true);

    assert_eq!(eiointc.mmio_read_sized(0, EIO_NODEMAP, 4), 0x0002_0001);
    assert_eq!(cpu0.lock().unwrap().pending_interrupt_line(), None);
    assert_eq!(cpu1.lock().unwrap().pending_interrupt_line(), Some(3));
    assert_eq!(eiointc.mmio_read_sized(0, EIO_CORE_ISR, 4), 0);
    assert_eq!(eiointc.mmio_read_sized(1, EIO_CORE_ISR, 4), 1 << 4);

    eiointc.mmio_write_sized(0, EIO_CORE_ISR, 4, 1 << 4);
    assert_eq!(eiointc.mmio_read_sized(1, EIO_CORE_ISR, 4), 1 << 4);

    eiointc.mmio_write_sized(1, EIO_CORE_ISR, 4, 1 << 4);
    assert_eq!(eiointc.mmio_read_sized(1, EIO_CORE_ISR, 4), 0);
    assert_eq!(cpu1.lock().unwrap().pending_interrupt_line(), None);
}

#[test]
fn task41_pch_pic_level_edge_mask_and_eiointc_route_matrix() {
    let eiointc = Arc::new(Eiointc::new_named("eiointc0", 1));
    let pic = PchPic::new_named("pchpic0", 32);
    let cpu = cpu_with_lie(HWI1);
    connect_eio_hwi(&eiointc, 0, 1, &cpu);

    eiointc.mmio_write_sized(0, EIO_IPMAP, 4, 0x0202_0202);
    eiointc.mmio_write_sized(0, EIO_ENABLE, 4, (1 << 5) | (1 << 6));
    for eio_irq in [5, 6] {
        pic.connect_output(
            eio_irq,
            InterruptSource::new(
                Arc::new(EiointcIrqSink(Arc::clone(&eiointc)))
                    as Arc<dyn IrqSink>,
                eio_irq,
            ),
        );
    }

    pic.mmio_write_sized(PCH_HTMSI_VEC + 2, 1, 5);
    pic.mmio_write_sized(PCH_INT_MASK, 8, unmask_word(2));
    pic.set_irq(2, true);
    assert_eq!(cpu.lock().unwrap().pending_interrupt_line(), Some(3));

    pic.set_irq(2, false);
    assert_eq!(cpu.lock().unwrap().pending_interrupt_line(), None);

    pic.mmio_write_sized(PCH_HTMSI_VEC + 3, 1, 6);
    pic.mmio_write_sized(PCH_INT_EDGE, 8, 1 << 3);
    pic.mmio_write_sized(PCH_INT_MASK, 8, unmask_word(3));
    pic.set_irq(3, true);
    pic.set_irq(3, false);
    assert_eq!(cpu.lock().unwrap().pending_interrupt_line(), Some(3));

    pic.mmio_write_sized(PCH_INT_CLEAR, 8, 1 << 3);
    assert_eq!(cpu.lock().unwrap().pending_interrupt_line(), None);
}

#[test]
fn task41_uart_and_virtio_interrupt_cascade_matrix() {
    let cascade = LoongArchInterruptCascade::new("task41-cascade", 1);
    let cpu = cpu_with_lie(1 << (u32::from(LOONGARCH_DEVICE_HWI) + 2));
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

    uart.receive(0x41);
    assert_eq!(
        cpu.lock().unwrap().pending_interrupt_line(),
        Some(u32::from(LOONGARCH_DEVICE_HWI) + 2)
    );
    assert_eq!(uart.read(0), 0x41);
    assert_eq!(cpu.lock().unwrap().pending_interrupt_line(), None);

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
