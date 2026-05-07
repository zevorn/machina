use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
use machina_guest_loongarch::loongarch::csr::{
    CRMD_DA, CRMD_IE, CSR_CRMD, CSR_ECFG, CSR_ESTAT,
};
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_intc::ipi::{LoongArchIpi, LoongArchIpiMmio};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MemoryRegion;

const IPI_LINE: u64 = 1 << 12;
const CORE_STATUS: u64 = 0x000;
const CORE_ENABLE: u64 = 0x004;
const CORE_SET: u64 = 0x008;
const CORE_CLEAR: u64 = 0x00c;
const CORE_MAILBOX0: u64 = 0x020;
const IOCSR_IPI_SEND: u64 = 0x040;
const IOCSR_MAIL_SEND: u64 = 0x048;
const IOCSR_ANY_SEND: u64 = 0x158;

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

fn cpu_with_ipi_enabled() -> Arc<Mutex<LoongArchCpu>> {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, IPI_LINE);
    Arc::new(Mutex::new(cpu))
}

fn connect_cpu(
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

fn cpu_estat(cpu: &Arc<Mutex<LoongArchCpu>>) -> u64 {
    cpu.lock().unwrap().csr_read(CSR_ESTAT)
}

#[test]
fn test_loongarch_ipi_lifecycle_and_mom_identity() {
    let ipi = Arc::new(LoongArchIpi::new_named("ipi0", 2));
    let mut bus = SysBus::new("sysbus0");
    let mut address_space = make_address_space();
    let base = GPA::new(0x1fe0_0000);
    assert!(!ipi.realized());
    ipi.with_mdevice(|device| assert_eq!(device.local_id(), "ipi0"));
    assert_eq!(ipi.object_info().local_id, "ipi0");

    ipi.register_mmio(
        MemoryRegion::io(
            "ipi0-mmio",
            0x200,
            Arc::new(LoongArchIpiMmio(Arc::clone(&ipi), 0)),
        ),
        base,
    )
    .unwrap();
    ipi.attach_to_bus(&mut bus).unwrap();
    ipi.realize_onto(&mut bus, &mut address_space).unwrap();

    assert!(ipi.realized());
    assert!(address_space.is_mapped(base, 4));

    address_space.write_u32(GPA::new(base.0 + CORE_ENABLE), 0x0000_00ff);
    assert_eq!(
        address_space.read_u32(GPA::new(base.0 + CORE_ENABLE)),
        0x0000_00ff
    );

    assert_eq!(bus.mappings().len(), 1);
    assert_eq!(bus.mappings()[0].owner, "ipi0");
    assert_eq!(bus.mappings()[0].name, "ipi0-mmio");
    assert_eq!(bus.mappings()[0].base, base);

    let err = ipi.realize_onto(&mut bus, &mut address_space).unwrap_err();
    assert!(err.to_string().contains("already realized"));

    ipi.unrealize_from(&mut bus, &mut address_space).unwrap();
    assert!(!ipi.realized());

    let err = ipi
        .unrealize_from(&mut bus, &mut address_space)
        .unwrap_err();
    assert!(err.to_string().contains("not realized"));
}

#[test]
fn task38_ipi_send_routes_nonzero_target_to_target_cpu() {
    let ipi = LoongArchIpi::new_named("ipi0", 2);
    let cpu0 = cpu_with_ipi_enabled();
    let cpu1 = cpu_with_ipi_enabled();
    connect_cpu(&ipi, 0, &cpu0);
    connect_cpu(&ipi, 1, &cpu1);
    ipi.mmio_write_sized(1, CORE_ENABLE, 4, 1 << 3);

    ipi.mmio_write_sized(0, IOCSR_IPI_SEND, 8, (1 << 16) | 3);

    assert_eq!(ipi.mmio_read_sized(1, CORE_STATUS, 4), 1 << 3);
    assert_eq!(cpu_estat(&cpu0) & IPI_LINE, 0);
    assert_eq!(cpu_estat(&cpu1) & IPI_LINE, IPI_LINE);
}

#[test]
fn task38_ipi_send_accepts_32bit_write() {
    let ipi = LoongArchIpi::new_named("ipi0", 2);
    let cpu1 = cpu_with_ipi_enabled();
    connect_cpu(&ipi, 1, &cpu1);
    ipi.mmio_write_sized(1, CORE_ENABLE, 4, 1 << 6);

    ipi.mmio_write_sized(0, IOCSR_IPI_SEND, 4, (1 << 16) | 6);

    assert_eq!(ipi.mmio_read_sized(1, CORE_STATUS, 4), 1 << 6);
    assert_eq!(cpu_estat(&cpu1) & IPI_LINE, IPI_LINE);
}

#[test]
fn task38_ipi_clear_deasserts_target_line() {
    let ipi = LoongArchIpi::new_named("ipi0", 2);
    let cpu1 = cpu_with_ipi_enabled();
    connect_cpu(&ipi, 1, &cpu1);
    ipi.mmio_write_sized(1, CORE_ENABLE, 4, 1 << 4);
    ipi.mmio_write_sized(0, IOCSR_IPI_SEND, 8, (1 << 16) | 4);
    assert_eq!(cpu_estat(&cpu1) & IPI_LINE, IPI_LINE);

    ipi.mmio_write_sized(1, CORE_CLEAR, 4, 1 << 4);

    assert_eq!(ipi.mmio_read_sized(1, CORE_STATUS, 4), 0);
    assert_eq!(cpu_estat(&cpu1) & IPI_LINE, 0);
}

#[test]
fn task38_ipi_enable_mask_controls_delivery_without_losing_status() {
    let ipi = LoongArchIpi::new_named("ipi0", 2);
    let cpu1 = cpu_with_ipi_enabled();
    connect_cpu(&ipi, 1, &cpu1);

    // Enable before send: output rises immediately.
    ipi.mmio_write_sized(1, CORE_ENABLE, 4, 1 << 2);
    ipi.mmio_write_sized(0, IOCSR_IPI_SEND, 8, (1 << 16) | 2);

    assert_eq!(ipi.mmio_read_sized(1, CORE_STATUS, 4), 1 << 2);
    assert_eq!(cpu_estat(&cpu1) & IPI_LINE, IPI_LINE);

    // QEMU: enable writes store the value but don't trigger
    // output recomputation. Status is preserved.
    ipi.mmio_write_sized(1, CORE_ENABLE, 4, 0);
    assert_eq!(ipi.mmio_read_sized(1, CORE_STATUS, 4), 1 << 2);

    // Clear deasserts the line and clears status.
    ipi.mmio_write_sized(1, CORE_CLEAR, 4, 1 << 2);
    assert_eq!(ipi.mmio_read_sized(1, CORE_STATUS, 4), 0);
    assert_eq!(cpu_estat(&cpu1) & IPI_LINE, 0);
}

#[test]
fn task38_ipi_mail_send_writes_target_mailbox_with_byte_mask() {
    let ipi = LoongArchIpi::new_named("ipi0", 2);
    ipi.mmio_write_sized(1, CORE_MAILBOX0, 4, 0xaaaa_bbbb);

    let data = 0x1122_3344u64;
    let byte_mask = 0x5u64;
    let val = (data << 32) | (byte_mask << 27) | (1 << 16);
    ipi.mmio_write_sized(0, IOCSR_MAIL_SEND, 8, val);

    assert_eq!(ipi.mmio_read_sized(1, CORE_MAILBOX0, 4), 0x11aa_33bb);
}

#[test]
fn task38_ipi_any_send_can_target_set_and_ipi_send_registers() {
    let ipi = LoongArchIpi::new_named("ipi0", 2);
    let cpu1 = cpu_with_ipi_enabled();
    connect_cpu(&ipi, 1, &cpu1);
    ipi.mmio_write_sized(1, CORE_ENABLE, 4, 0b101);

    ipi.mmio_write_sized(
        0,
        IOCSR_ANY_SEND,
        8,
        (1u64 << 32) | (1 << 16) | CORE_SET,
    );
    assert_eq!(ipi.mmio_read_sized(1, CORE_STATUS, 4), 1);

    ipi.mmio_write_sized(1, CORE_CLEAR, 4, 1);
    let nested_send = (1 << 16) | 2;
    ipi.mmio_write_sized(
        0,
        IOCSR_ANY_SEND,
        8,
        ((nested_send as u64) << 32) | IOCSR_IPI_SEND,
    );

    assert_eq!(ipi.mmio_read_sized(1, CORE_STATUS, 4), 1 << 2);
    assert_eq!(cpu_estat(&cpu1) & IPI_LINE, IPI_LINE);
}

#[test]
fn task38_ipi_32_and_64_bit_accesses_match_local_iocsr_shape() {
    let ipi = LoongArchIpi::new_named("ipi0", 1);

    ipi.mmio_write_sized(0, CORE_ENABLE, 8, (0x3u64 << 32) | 0x5);
    assert_eq!(ipi.mmio_read_sized(0, CORE_STATUS, 4), 0x3);
    assert_eq!(ipi.mmio_read_sized(0, CORE_ENABLE, 4), 0x5);
    assert_eq!(ipi.mmio_read_sized(0, CORE_STATUS, 8), (0x5u64 << 32) | 0x3);

    ipi.mmio_write_sized(0, CORE_MAILBOX0, 8, 0x1122_3344_5566_7788);
    assert_eq!(ipi.mmio_read_sized(0, CORE_MAILBOX0, 4), 0x5566_7788);
    assert_eq!(ipi.mmio_read_sized(0, CORE_MAILBOX0 + 4, 4), 0x1122_3344);
    assert_eq!(
        ipi.mmio_read_sized(0, CORE_MAILBOX0, 8),
        0x1122_3344_5566_7788
    );
}

#[test]
fn test_loongarch_ipi_core_iocsr_rejects_subword_accesses() {
    let ipi = LoongArchIpi::new_named("ipi0", 1);

    ipi.mmio_write_sized(0, CORE_ENABLE, 1, 0xff);
    ipi.mmio_write_sized(0, CORE_SET, 2, 0xffff);
    ipi.mmio_write_sized(0, CORE_MAILBOX0, 2, 0x1234);

    assert_eq!(ipi.mmio_read_sized(0, CORE_ENABLE, 4), 0);
    assert_eq!(ipi.mmio_read_sized(0, CORE_STATUS, 4), 0);
    assert_eq!(ipi.mmio_read_sized(0, CORE_MAILBOX0, 4), 0);
    assert_eq!(ipi.mmio_read_sized(0, CORE_ENABLE, 1), 0);
    assert_eq!(ipi.mmio_read_sized(0, CORE_ENABLE, 2), 0);
}
