use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
use machina_guest_loongarch::loongarch::csr::{
    CRMD_DA, CRMD_IE, CSR_CRMD, CSR_ECFG,
};
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_intc::eiointc::{Eiointc, EiointcIrqSink, EiointcMmio};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MemoryRegion;

const HWI0: u64 = 1 << 2;
const HWI1: u64 = 1 << 3;
const HWI2: u64 = 1 << 4;

struct CpuHwiSink {
    cpu: Arc<Mutex<LoongArchCpu>>,
}

impl CpuHwiSink {
    fn new(cpu: Arc<Mutex<LoongArchCpu>>) -> Self {
        Self { cpu }
    }
}

impl IrqSink for CpuHwiSink {
    fn set_irq(&self, irq: u32, level: bool) {
        self.cpu
            .lock()
            .unwrap()
            .set_hwi_interrupt_pending(irq as u8, level);
    }
}

fn make_address_space() -> AddressSpace {
    AddressSpace::new(MemoryRegion::container("system", u64::MAX))
}

fn cpu_with_enabled_hwi(mask: u64) -> Arc<Mutex<LoongArchCpu>> {
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, mask);
    Arc::new(Mutex::new(cpu))
}

fn hwi_output(cpu: &Arc<Mutex<LoongArchCpu>>, hwi: u32) -> InterruptSource {
    InterruptSource::new(
        Arc::new(CpuHwiSink::new(Arc::clone(cpu))) as Arc<dyn IrqSink>,
        hwi,
    )
}

#[test]
fn enable_write_read_at_0x200() {
    let e = Eiointc::new();
    e.mmio_write(0x200, 0xDEAD_BEEF);
    assert_eq!(e.mmio_read(0x200), 0xDEAD_BEEF);
}

#[test]
fn enable_0x204_independent_of_0x200() {
    let e = Eiointc::new();
    e.mmio_write(0x200, 0xAAAA_AAAA);
    e.mmio_write(0x204, 0xBBBB_BBBB);
    assert_eq!(e.mmio_read(0x200), 0xAAAA_AAAA);
    assert_eq!(e.mmio_read(0x204), 0xBBBB_BBBB);
}

#[test]
fn set_irq_pending_when_enabled() {
    let e = Eiointc::new();
    e.mmio_write(0x200, 1);
    e.mmio_write(0x0C0, 0x02);
    e.set_irq(0, true);
    assert_ne!(e.pending_for_cpu(0) & (1 << 1), 0);
}

#[test]
fn set_irq_masked_when_disabled() {
    let e = Eiointc::new();
    e.set_irq(0, true);
    assert_eq!(e.pending_for_cpu(0), 0);
}

#[test]
fn ack_clears_isr_via_core_isr() {
    let e = Eiointc::new();
    e.mmio_write(0x200, 1);
    e.set_irq(0, true);
    assert_ne!(e.mmio_read(0x400) & 1, 0);
    e.mmio_write(0x400, 1);
    assert_eq!(e.mmio_read(0x400) & 1, 0);
}

#[test]
fn core_isr_read_returns_enabled_pending() {
    let e = Eiointc::new();
    e.mmio_write(0x200, 0x5);
    e.set_irq(0, true);
    e.set_irq(2, true);
    assert_eq!(e.mmio_read(0x400), 0x5);
}

#[test]
fn coremap_routes_to_specific_cpu() {
    let e = Eiointc::new_named("eiointc0", 2);
    e.mmio_write(0x200, 1);
    e.mmio_write(0x0C0, 0x02);
    e.mmio_write(0x800, 0x02);
    e.set_irq(0, true);
    assert_eq!(e.pending_for_cpu(0), 0);
    assert_ne!(e.pending_for_cpu(1) & (1 << 1), 0);
}

#[test]
fn connect_hwi_output_rebuilds_routes_for_extended_cpu() {
    let e = Arc::new(Eiointc::new_named("eiointc0", 1));
    let cpu0 = cpu_with_enabled_hwi(HWI1);
    let cpu1 = cpu_with_enabled_hwi(HWI1);

    e.connect_hwi_output(0, 1, hwi_output(&cpu0, 1));
    e.mmio_write_sized(0, 0x0c0, 4, 0x02);
    e.mmio_write_sized(0, 0x800, 1, 0x02);
    e.mmio_write_sized(0, 0x200, 4, 1);
    e.set_irq(0, true);

    assert_eq!(cpu0.lock().unwrap().pending_interrupt_line(), Some(3));

    e.connect_hwi_output(1, 1, hwi_output(&cpu1, 1));

    assert_eq!(e.pending_for_cpu(0), 0);
    assert_ne!(e.pending_for_cpu(1) & (1 << 1), 0);
    assert_eq!(cpu0.lock().unwrap().pending_interrupt_line(), None);
    assert_eq!(cpu1.lock().unwrap().pending_interrupt_line(), Some(3));
}

#[test]
fn test_eiointc_lifecycle_and_mom_identity() {
    let e = Arc::new(Eiointc::new_named("eiointc0", 2));
    let mut bus = SysBus::new("sysbus0");
    let mut address_space = make_address_space();
    let base = GPA::new(0x1fe0_0000);
    assert!(!e.realized());
    e.with_mdevice(|device| assert_eq!(device.local_id(), "eiointc0"));
    assert_eq!(e.object_info().local_id, "eiointc0");

    e.register_mmio(
        MemoryRegion::io(
            "eiointc0-mmio",
            0x1000,
            Arc::new(EiointcMmio(Arc::clone(&e))),
        ),
        base,
    )
    .unwrap();
    e.attach_to_bus(&mut bus).unwrap();
    e.realize_onto(&mut bus, &mut address_space).unwrap();

    assert!(e.realized());
    assert!(address_space.is_mapped(base, 4));

    address_space.write_u32(GPA::new(base.0 + 0x200), 0x20);
    assert_eq!(address_space.read_u32(GPA::new(base.0 + 0x200)), 0x20);

    address_space.write(GPA::new(base.0 + 0x0c0), 8, 0x0706_0504_0302_0100);
    assert_eq!(
        address_space.read(GPA::new(base.0 + 0x0c0), 8),
        0x0706_0504_0302_0100
    );

    assert_eq!(bus.mappings().len(), 1);
    assert_eq!(bus.mappings()[0].owner, "eiointc0");
    assert_eq!(bus.mappings()[0].name, "eiointc0-mmio");
    assert_eq!(bus.mappings()[0].base, base);

    let err = e.realize_onto(&mut bus, &mut address_space).unwrap_err();
    assert!(err.to_string().contains("already realized"));

    e.unrealize_from(&mut bus, &mut address_space).unwrap();
    assert!(!e.realized());

    let err = e.unrealize_from(&mut bus, &mut address_space).unwrap_err();
    assert!(err.to_string().contains("not realized"));
}

#[test]
fn task36_eiointc_routes_enabled_source_to_cpu_hwi_line() {
    let e = Arc::new(Eiointc::new_named("eiointc0", 1));
    let cpu = cpu_with_enabled_hwi(HWI1);

    e.connect_hwi_output(0, 1, hwi_output(&cpu, 1));
    e.mmio_write(0x0c0, 0x02);
    e.mmio_write(0x200, 1 << 3);

    let input = EiointcIrqSink(Arc::clone(&e));
    input.set_irq(3, true);

    assert_eq!(cpu.lock().unwrap().pending_interrupt_line(), Some(3));
    assert_ne!(e.mmio_read(0x300) & (1 << 3), 0);
    assert_ne!(e.mmio_read(0x400) & (1 << 3), 0);

    e.mmio_write(0x400, 1 << 3);

    assert_eq!(cpu.lock().unwrap().pending_interrupt_line(), None);
    // ISR preserves source assertion state after ack (QEMU).
    assert_ne!(e.mmio_read(0x300) & (1 << 3), 0);
    assert_eq!(e.mmio_read(0x400) & (1 << 3), 0);
}

#[test]
fn task36_eiointc_suppresses_disabled_source_until_enabled() {
    let e = Arc::new(Eiointc::new_named("eiointc0", 1));
    let cpu = cpu_with_enabled_hwi(HWI0);

    e.connect_hwi_output(0, 0, hwi_output(&cpu, 0));
    e.mmio_write(0x0c0, 0);

    e.set_irq(0, true);

    assert_eq!(cpu.lock().unwrap().pending_interrupt_line(), None);
    assert_ne!(e.mmio_read(0x300) & 1, 0);
    assert_eq!(e.mmio_read(0x400) & 1, 0);

    e.mmio_write(0x200, 1);

    assert_eq!(cpu.lock().unwrap().pending_interrupt_line(), Some(2));
    assert_ne!(e.mmio_read(0x400) & 1, 0);
}

#[test]
fn task36_eiointc_honors_coremap_and_ipmap_routing_changes() {
    let e = Arc::new(Eiointc::new_named("eiointc0", 2));
    let cpu0 = cpu_with_enabled_hwi(HWI2);
    let cpu1 = cpu_with_enabled_hwi(HWI2);

    e.connect_hwi_output(0, 2, hwi_output(&cpu0, 2));
    e.connect_hwi_output(1, 2, hwi_output(&cpu1, 2));
    e.mmio_write(0x0c0, 0x04);
    e.mmio_write(0x800, 0x02);
    e.mmio_write(0x200, 1);
    e.set_irq(0, true);

    assert_eq!(cpu0.lock().unwrap().pending_interrupt_line(), None);
    assert_eq!(cpu1.lock().unwrap().pending_interrupt_line(), Some(4));
    assert_ne!(e.pending_for_cpu(1) & (1 << 2), 0);

    e.mmio_write(0x800, 0x01);

    assert_eq!(cpu0.lock().unwrap().pending_interrupt_line(), Some(4));
    assert_eq!(cpu1.lock().unwrap().pending_interrupt_line(), None);
    assert_ne!(e.pending_for_cpu(0) & (1 << 2), 0);
    assert_eq!(e.pending_for_cpu(1), 0);
}

#[test]
fn task36_eiointc_decodes_linux_one_hot_ipmap_words_independently() {
    let e = Eiointc::new_named("eiointc0", 1);
    e.mmio_write_sized(0, 0x0c0, 4, 0x0202_0202);
    e.mmio_write_sized(0, 0x0c4, 4, 0x0808_0808);
    e.mmio_write_sized(0, 0x200, 4, 1);
    e.mmio_write_sized(0, 0x20c, 4, 1);
    e.mmio_write_sized(0, 0x210, 4, 1);

    e.set_irq(0, true);
    e.set_irq(96, true);
    e.set_irq(128, true);

    let pending = e.pending_for_cpu(0);
    assert_ne!(pending & (1 << 1), 0, "groups 0-3 route to HWI1");
    assert_ne!(pending & (1 << 3), 0, "groups 4-7 route to HWI3");
    assert_eq!(e.mmio_read_sized(0, 0x0c0, 4), 0x0202_0202);
    assert_eq!(e.mmio_read_sized(0, 0x0c4, 4), 0x0808_0808);
}

#[test]
fn test_eiointc_ipmap_bits_above_hwi3_decode_to_hwi0() {
    let e = Eiointc::new_named("eiointc0", 1);
    e.mmio_write_sized(0, 0x0c0, 4, 0x80);
    e.mmio_write_sized(0, 0x200, 4, 1);

    e.set_irq(0, true);

    assert_eq!(e.pending_for_cpu(0), 1);
    assert_eq!(e.mmio_read_sized(0, 0x0c0, 4), 0x80);
}

#[test]
fn task36_eiointc_decodes_linux_one_hot_coremap_words_independently() {
    let e = Eiointc::new_named("eiointc0", 2);
    e.mmio_write_sized(0, 0x0c0, 4, 0x0202_0202);
    e.mmio_write_sized(0, 0x200, 4, 0xff);
    e.mmio_write_sized(0, 0x800, 4, 0x0101_0101);
    e.mmio_write_sized(0, 0x804, 4, 0x0202_0202);

    e.set_irq(0, true);
    e.set_irq(3, true);
    e.set_irq(4, true);
    e.set_irq(7, true);

    assert_ne!(e.pending_for_cpu(0) & (1 << 1), 0);
    assert_eq!(e.mmio_read_sized(0, 0x400, 4), 0b0000_1001);
    assert_ne!(e.pending_for_cpu(1) & (1 << 1), 0);
    assert_eq!(e.mmio_read_sized(1, 0x400, 4), 0b1001_0000);
    assert_eq!(e.mmio_read_sized(0, 0x804, 4), 0x0202_0202);
}

#[test]
fn task36_eiointc_core_isr_is_cpu_specific_for_read_and_ack() {
    let e = Eiointc::new_named("eiointc0", 2);
    e.mmio_write_sized(0, 0x0c0, 4, 0x0202_0202);
    e.mmio_write_sized(0, 0x200, 4, 1 << 4);
    e.mmio_write_sized(0, 0x804, 4, 0x0000_0002);
    e.set_irq(4, true);

    assert_eq!(e.mmio_read_sized(0, 0x400, 4), 0);
    assert_eq!(e.mmio_read_sized(1, 0x400, 4), 1 << 4);

    e.mmio_write_sized(0, 0x400, 4, 1 << 4);
    assert_eq!(
        e.mmio_read_sized(1, 0x400, 4),
        1 << 4,
        "CPU0 ack must not clear CPU1-routed source"
    );

    e.mmio_write_sized(1, 0x400, 4, 1 << 4);
    assert_eq!(e.mmio_read_sized(1, 0x400, 4), 0);
    // ISR preserves source assertion state (QEMU).
    assert_ne!(e.mmio_read_sized(0, 0x300, 4) & (1 << 4), 0);
}

#[test]
fn task36_eiointc_core_isr_supports_64_bit_dispatch() {
    let e = Eiointc::new_named("eiointc0", 1);
    e.mmio_write_sized(0, 0x0c0, 4, 0x0202_0202);
    e.mmio_write_sized(0, 0x200, 4, 1);
    e.mmio_write_sized(0, 0x204, 4, 1);
    e.set_irq(0, true);
    e.set_irq(32, true);

    assert_eq!(
        e.mmio_read_sized(0, 0x400, 8),
        (1_u64 << 32) | 1,
        "64-bit core-ISR read must include both 32-bit words"
    );

    e.mmio_write_sized(0, 0x400, 8, (1_u64 << 32) | 1);

    assert_eq!(e.mmio_read_sized(0, 0x400, 8), 0);
    // ISR preserves source assertion state (QEMU).
    assert_ne!(e.mmio_read_sized(0, 0x300, 8), 0);
}
