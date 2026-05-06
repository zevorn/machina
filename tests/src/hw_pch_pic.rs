use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_intc::eiointc::{Eiointc, EiointcIrqSink};
use machina_hw_intc::pch_pic::{PchPic, PchPicIrqSink, PchPicMmio};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MemoryRegion;

const INT_MASK: u64 = 0x20;
const INT_EDGE: u64 = 0x60;
const INT_CLEAR: u64 = 0x80;
const ROUTE_ENTRY: u64 = 0x100;
const HTMSI_VEC: u64 = 0x200;
const INT_STATUS: u64 = 0x3a0;

struct RecordingSink {
    levels: Mutex<Vec<bool>>,
}

impl RecordingSink {
    fn new(num_lines: usize) -> Arc<Self> {
        Arc::new(Self {
            levels: Mutex::new(vec![false; num_lines]),
        })
    }

    fn level(&self, irq: usize) -> bool {
        self.levels.lock().unwrap()[irq]
    }
}

impl IrqSink for RecordingSink {
    fn set_irq(&self, irq: u32, level: bool) {
        self.levels.lock().unwrap()[irq as usize] = level;
    }
}

fn make_address_space() -> AddressSpace {
    AddressSpace::new(MemoryRegion::container("system", u64::MAX))
}

fn recording_line(sink: &Arc<RecordingSink>, irq: u32) -> InterruptSource {
    InterruptSource::new(Arc::clone(sink) as Arc<dyn IrqSink>, irq)
}

fn unmask_word(irq: u32) -> u64 {
    u64::MAX & !(1u64 << irq)
}

#[test]
fn task37_pch_pic_realizes_as_sysbus_mmio_device() {
    let pic = Arc::new(PchPic::new_named("pchpic0", 32));
    let mut bus = SysBus::new("sysbus0");
    let mut address_space = make_address_space();
    let base = GPA::new(0x1000_0000);

    pic.register_mmio(
        MemoryRegion::io(
            "pchpic0-mmio",
            0x1000,
            Arc::new(PchPicMmio(Arc::clone(&pic))),
        ),
        base,
    )
    .unwrap();
    pic.attach_to_bus(&mut bus).unwrap();
    pic.realize_onto(&mut bus, &mut address_space).unwrap();

    assert!(pic.realized());
    assert!(address_space.is_mapped(base, 4));

    address_space.write_u32(GPA::new(base.0 + HTMSI_VEC), 0x0807_0605);
    assert_eq!(
        address_space.read_u32(GPA::new(base.0 + HTMSI_VEC)),
        0x0807_0605
    );

    assert_eq!(bus.mappings().len(), 1);
    assert_eq!(bus.mappings()[0].owner, "pchpic0");
    assert_eq!(bus.mappings()[0].name, "pchpic0-mmio");
    assert_eq!(bus.mappings()[0].base, base);
}

#[test]
fn task37_pch_pic_mask_suppresses_until_unmasked() {
    let pic = Arc::new(PchPic::new_named("pchpic0", 32));
    let sink = RecordingSink::new(16);
    pic.connect_output(5, recording_line(&sink, 5));
    pic.mmio_write_sized(HTMSI_VEC + 3, 1, 5);

    PchPicIrqSink(Arc::clone(&pic)).set_irq(3, true);

    assert!(!sink.level(5));
    assert_eq!(pic.mmio_read_sized(INT_STATUS, 8) & (1 << 3), 0);

    pic.mmio_write_sized(INT_MASK, 8, unmask_word(3));

    assert!(sink.level(5));
    assert_ne!(pic.mmio_read_sized(INT_STATUS, 8) & (1 << 3), 0);
}

#[test]
fn task37_pch_pic_routes_unmasked_source_into_eiointc() {
    let pic = Arc::new(PchPic::new_named("pchpic0", 32));
    let eio = Arc::new(Eiointc::new_named("eiointc0", 1));

    eio.mmio_write_sized(0, 0x0c0, 4, 0x0202_0202);
    eio.mmio_write_sized(0, 0x200, 4, 1 << 7);
    pic.connect_output(
        7,
        InterruptSource::new(
            Arc::new(EiointcIrqSink(Arc::clone(&eio))) as Arc<dyn IrqSink>,
            7,
        ),
    );
    pic.mmio_write_sized(HTMSI_VEC + 3, 1, 7);
    pic.mmio_write_sized(INT_MASK, 8, unmask_word(3));

    pic.set_irq(3, true);

    assert_ne!(eio.pending_for_cpu(0) & (1 << 1), 0);
    assert_ne!(eio.mmio_read_sized(0, 0x400, 4) & (1 << 7), 0);
}

#[test]
fn task37_pch_pic_edge_latches_until_clear() {
    let pic = Arc::new(PchPic::new_named("pchpic0", 32));
    let sink = RecordingSink::new(16);
    pic.connect_output(4, recording_line(&sink, 4));
    pic.mmio_write_sized(HTMSI_VEC + 2, 1, 4);
    pic.mmio_write_sized(INT_EDGE, 8, 1 << 2);
    pic.mmio_write_sized(INT_MASK, 8, unmask_word(2));

    pic.set_irq(2, true);
    pic.set_irq(2, false);

    assert!(sink.level(4));
    assert_ne!(pic.mmio_read_sized(INT_STATUS, 8) & (1 << 2), 0);

    pic.mmio_write_sized(INT_CLEAR, 8, 1 << 2);

    assert!(!sink.level(4));
    assert_eq!(pic.mmio_read_sized(INT_STATUS, 8) & (1 << 2), 0);
}

#[test]
fn task37_pch_pic_level_reasserts_after_masking_while_high() {
    let pic = Arc::new(PchPic::new_named("pchpic0", 32));
    let sink = RecordingSink::new(16);
    pic.connect_output(6, recording_line(&sink, 6));
    pic.mmio_write_sized(HTMSI_VEC + 1, 1, 6);
    pic.mmio_write_sized(INT_MASK, 8, unmask_word(1));

    pic.set_irq(1, true);
    assert!(sink.level(6));

    // QEMU: mask gates ISR acceptance, not ongoing output.
    // intisr still drives the line even while masked.
    pic.mmio_write_sized(INT_MASK, 8, u64::MAX);
    assert!(sink.level(6));

    // INT_STATUS reads intisr & ~int_mask, so it shows 0.
    assert_eq!(pic.mmio_read_sized(INT_STATUS, 8) & (1 << 1), 0);

    // Unmasking has no retroactive effect on already-active IRQ.
    pic.mmio_write_sized(INT_MASK, 8, unmask_word(1));
    assert!(sink.level(6));

    // Deasserting the source clears intisr and drops the line.
    pic.set_irq(1, false);
    assert!(!sink.level(6));
    assert_eq!(pic.mmio_read_sized(INT_STATUS, 8) & (1 << 1), 0);
}

#[test]
fn task37_pch_pic_route_and_vector_bytes_are_sized_and_independent() {
    let pic = Arc::new(PchPic::new_named("pchpic0", 32));
    let sink = RecordingSink::new(16);
    pic.connect_output(7, recording_line(&sink, 7));
    pic.connect_output(8, recording_line(&sink, 8));

    pic.mmio_write_sized(ROUTE_ENTRY, 4, 0x0403_0201);
    pic.mmio_write_sized(HTMSI_VEC, 4, 0x0807_0605);
    assert_eq!(pic.mmio_read_sized(ROUTE_ENTRY, 4), 0x0403_0201);
    assert_eq!(pic.mmio_read_sized(HTMSI_VEC, 4), 0x0807_0605);

    pic.mmio_write_sized(INT_MASK, 8, unmask_word(2));
    pic.set_irq(2, true);

    assert!(sink.level(7));
    assert!(!sink.level(8));
}
