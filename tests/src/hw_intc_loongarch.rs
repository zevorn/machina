use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_intc::dintc::{Dintc, DintcMmio};
use machina_hw_intc::liointc::{Liointc, LiointcIrqSink, LiointcMmio};
use machina_hw_intc::pch_msi::{PchMsi, PchMsiMmio};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

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

    fn all_levels(&self) -> Vec<bool> {
        self.levels.lock().unwrap().clone()
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

// ---- PchMsi ----

#[test]
fn test_pch_msi_defaults() {
    let msi = PchMsi::new_named("pch_msi", 0, 32);
    assert!(!msi.realized());
}

#[test]
fn test_pch_msi_mmio_read_returns_zero() {
    let msi = PchMsi::new_named("pch_msi", 0, 32);
    let mmio = PchMsiMmio(Arc::new(msi));
    assert_eq!(mmio.read(0x00, 4), 0);
    assert_eq!(mmio.read(0x04, 4), 0);
}

#[test]
fn test_pch_msi_mmio_write_fires_output() {
    let msi = Arc::new(PchMsi::new_named("pch_msi", 0, 32));
    let sink = RecordingSink::new(32);
    msi.connect_output(5, recording_line(&sink, 5));
    msi.connect_output(10, recording_line(&sink, 10));

    let mmio = PchMsiMmio(Arc::clone(&msi));

    // Write val & 0xff = 5 -> fires output[5]
    mmio.write(0x00, 4, 5);
    assert!(sink.level(5));
    assert!(!sink.level(10));

    // Write val & 0xff = 10 -> fires output[10]
    mmio.write(0x00, 4, 10);
    assert!(sink.level(5));
    assert!(sink.level(10));
}

#[test]
fn test_pch_msi_mmio_write_with_irq_base() {
    let msi = Arc::new(PchMsi::new_named("pch_msi", 32, 64));
    let sink = RecordingSink::new(64);
    msi.connect_output(0, recording_line(&sink, 0));
    msi.connect_output(3, recording_line(&sink, 3));

    let mmio = PchMsiMmio(Arc::clone(&msi));

    // irq = 35 - 32 = 3 -> fires output[3]
    mmio.write(0x00, 4, 35);
    assert!(sink.level(3));
    assert!(!sink.level(0));

    // irq = 32 - 32 = 0 -> fires output[0]
    mmio.write(0x00, 4, 32);
    assert!(sink.level(0));
}

#[test]
fn test_pch_msi_mmio_write_out_of_range_is_noop() {
    let msi = Arc::new(PchMsi::new_named("pch_msi", 0, 8));
    let sink = RecordingSink::new(8);
    msi.connect_output(0, recording_line(&sink, 0));

    let mmio = PchMsiMmio(Arc::clone(&msi));

    // irq = 255 - 0 = 255 > irq_num(8), no output
    mmio.write(0x00, 4, 255);
    assert!(!sink.level(0));
}

#[test]
fn test_pch_msi_mmio_write_non_4byte_ignored() {
    let msi = Arc::new(PchMsi::new_named("pch_msi", 0, 8));
    let sink = RecordingSink::new(8);
    msi.connect_output(1, recording_line(&sink, 1));

    let mmio = PchMsiMmio(Arc::clone(&msi));

    mmio.write(0x00, 1, 1);
    assert!(!sink.level(1));

    mmio.write(0x00, 8, 1);
    assert!(!sink.level(1));
}

#[test]
fn test_pch_msi_mmio_write_nonzero_offset_ignored() {
    let msi = Arc::new(PchMsi::new_named("pch_msi", 0, 8));
    let sink = RecordingSink::new(8);
    msi.connect_output(1, recording_line(&sink, 1));

    let mmio = PchMsiMmio(Arc::clone(&msi));

    mmio.write(0x04, 4, 1);
    assert!(!sink.level(1));
}

#[test]
fn test_pch_msi_lifecycle() {
    let msi = Arc::new(PchMsi::new_named("pch_msi", 0, 32));
    let mut bus = SysBus::new("sysbus");
    let mut aspace = make_address_space();
    let base = GPA(0x1000_0000);

    assert!(!msi.realized());

    // Pre-realize: unmapped
    assert_eq!(aspace.read(base, 4), 0);

    msi.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "pch_msi",
        0x8,
        Arc::new(PchMsiMmio(Arc::clone(&msi))),
    );
    msi.register_mmio(region, base).unwrap();
    msi.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(msi.realized());

    // Second realize_onto fails (already realized)
    let err = msi.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

    msi.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!msi.realized());

    // Post-unrealize: unmapped
    assert_eq!(aspace.read(base, 4), 0);

    let err = msi.unrealize_from(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("not realized"));
}

#[test]
fn test_pch_msi_reset_runtime_lowers_outputs() {
    let msi = Arc::new(PchMsi::new_named("pch_msi", 0, 32));
    let sink = RecordingSink::new(32);
    msi.connect_output(5, recording_line(&sink, 5));

    // Fire IRQ
    let mmio = PchMsiMmio(Arc::clone(&msi));
    mmio.write(0x00, 4, 5);
    assert!(sink.level(5));

    // Reset should lower all outputs
    msi.reset_runtime();
    assert!(!sink.level(5));
}

// ---- Dintc ----

#[test]
fn test_dintc_defaults() {
    let dintc = Dintc::new_named("dintc", 4);
    assert!(!dintc.realized());
}

#[test]
fn test_dintc_mmio_read_returns_zero() {
    let dintc = Dintc::new_named("dintc", 4);
    let mmio = DintcMmio(Arc::new(dintc));
    assert_eq!(mmio.read(0x00, 4), 0);
    assert_eq!(mmio.read(0x1000, 4), 0);
}

#[test]
fn test_dintc_mmio_write_fires_cpu_output() {
    let dintc = Arc::new(Dintc::new_named("dintc", 4));
    let sink0 = RecordingSink::new(1);
    let sink1 = RecordingSink::new(1);
    dintc.connect_output(0, recording_line(&sink0, 0));
    dintc.connect_output(1, recording_line(&sink1, 0));

    let mmio = DintcMmio(Arc::clone(&dintc));

    // offset 0x2050: cpu=2, irq=5 -> fires output[2]
    // Wait, let me recalculate. VIRT_DINTC_BASE = 0x2FE00000
    // msg_addr = offset + VIRT_DINTC_BASE
    // cpu_num = (msg_addr >> 12) & 0xff
    // irq_num = (msg_addr >> 4) & 0xff
    //
    // For cpu=1: offset with bit12 set = 0x1000
    // msg_addr = 0x1000 + 0x2FE00000 = 0x2FE01000
    // cpu_num = (0x2FE01000 >> 12) & 0xff = 0x01 = 1
    //
    // For cpu=0: offset with no bits 12-19 = 0x0
    // msg_addr = 0 + 0x2FE00000 = 0x2FE00000
    // cpu_num = 0

    // Fire CPU 0
    mmio.write(0x0, 4, 0);
    assert!(sink0.level(0));
    assert!(!sink1.level(0));

    // Fire CPU 1
    mmio.write(0x1000, 4, 0);
    assert!(sink1.level(0));
}

#[test]
fn test_dintc_mmio_write_invalid_cpu_is_noop() {
    let dintc = Arc::new(Dintc::new_named("dintc", 2));
    let sink = RecordingSink::new(1);
    dintc.connect_output(0, recording_line(&sink, 0));

    let mmio = DintcMmio(Arc::clone(&dintc));

    // CPU=5 (offset 0x5000): msg_addr = 0x5000 + 0x2FE00000 = 0x2FE05000
    // cpu_num = 0x05, out of range -> noop
    mmio.write(0x5000, 4, 0);
    assert!(!sink.level(0));
}

#[test]
fn test_dintc_lifecycle() {
    let dintc = Arc::new(Dintc::new_named("dintc", 4));
    let mut bus = SysBus::new("sysbus");
    let mut aspace = make_address_space();
    let base = GPA(0x2FE0_0000);

    assert!(!dintc.realized());

    dintc.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "dintc",
        0x10_0000,
        Arc::new(DintcMmio(Arc::clone(&dintc))),
    );
    dintc.register_mmio(region, base).unwrap();
    dintc.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(dintc.realized());

    let err = dintc.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

    dintc.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!dintc.realized());

    let err = dintc.unrealize_from(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("not realized"));
}

#[test]
fn test_dintc_power_on_default_outputs_lowered() {
    let dintc = Arc::new(Dintc::new_named("dintc", 4));
    let sink = RecordingSink::new(1);
    dintc.connect_output(0, recording_line(&sink, 0));

    // Power on: outputs should be low
    assert!(!sink.level(0));
}

#[test]
fn test_dintc_reset_runtime_lowers_outputs() {
    let dintc = Arc::new(Dintc::new_named("dintc", 4));
    let sink = RecordingSink::new(1);
    dintc.connect_output(0, recording_line(&sink, 0));

    let mmio = DintcMmio(Arc::clone(&dintc));
    mmio.write(0x0, 4, 0);
    assert!(sink.level(0));

    dintc.reset_runtime();
    assert!(!sink.level(0));
}

// ---- Liointc ----

#[test]
fn test_liointc_defaults() {
    let lio = Liointc::new_named("liointc");
    let mmio = LiointcMmio(Arc::new(lio));

    // ISR and IEN should be 0 at reset
    assert_eq!(mmio.read(0x20, 4), 0); // R_ISR
    assert_eq!(mmio.read(0x24, 4), 0); // R_IEN

    // Mapper should be 0 at reset
    assert_eq!(mmio.read(0x00, 1), 0);

    // per_core_isr should be 0 at reset
    assert_eq!(mmio.read(0x40, 4), 0);
    assert_eq!(mmio.read(0x48, 4), 0);
    assert_eq!(mmio.read(0x50, 4), 0);
    assert_eq!(mmio.read(0x58, 4), 0);
}

#[test]
fn test_liointc_mapper_read_write_byte() {
    let lio = Arc::new(Liointc::new_named("liointc"));
    let mmio = LiointcMmio(Arc::clone(&lio));

    // Write mapper bytes
    for i in 0..32 {
        mmio.write(i, 1, (0x10 | i) as u64);
    }

    // Read back
    for i in 0..32 {
        let val = mmio.read(i, 1) as u8;
        assert_eq!(val, 0x10 | i as u8);
    }
}

#[test]
fn test_liointc_ien_set_clear() {
    let lio = Arc::new(Liointc::new_named("liointc"));
    let mmio = LiointcMmio(Arc::clone(&lio));

    // Set bits 0, 5, 31
    mmio.write(0x28, 4, (1u32 | (1 << 5) | (1 << 31)) as u64);
    let ien = mmio.read(0x24, 4) as u32;
    assert_eq!(ien, 1 | (1 << 5) | (1 << 31));

    // Clear bit 5
    mmio.write(0x2c, 4, (1u32 << 5) as u64);
    let ien = mmio.read(0x24, 4) as u32;
    assert_eq!(ien, 1 | (1 << 31));
}

#[test]
fn test_liointc_isr_reflects_pin_state_masked_by_ien() {
    let lio = Arc::new(Liointc::new_named("liointc"));
    let mmio = LiointcMmio(Arc::clone(&lio));

    // Set IEN for IRQ 3
    mmio.write(0x28, 4, 1 << 3);

    // Set pin_state[3] via IrqSink
    LiointcIrqSink(Arc::clone(&lio)).set_irq(3, true);

    // ISR should reflect pin_state & ien
    let isr = mmio.read(0x20, 4) as u32;
    assert_eq!(isr, 1 << 3);

    // Clear pin_state[3]
    LiointcIrqSink(Arc::clone(&lio)).set_irq(3, false);
    let isr = mmio.read(0x20, 4) as u32;
    assert_eq!(isr, 0);
}

#[test]
fn test_liointc_output_routing() {
    let lio = Arc::new(Liointc::new_named("liointc"));
    let sink = RecordingSink::new(16); // 4 cores * 4 IPs
    let mmio = LiointcMmio(Arc::clone(&lio));

    // Connect all 16 parent outputs to recording sinks
    for core in 0..4u32 {
        for ip in 0..4u32 {
            let idx = core * 4 + ip;
            lio.connect_output_xy(core, ip, recording_line(&sink, idx));
        }
    }

    // Route IRQ 3 to core 0, IP 1
    // mapper[irq] byte: bits 0:3 = core mask, bits 4:7 = IP mask
    // core 0 = bit 0, IP 1 = bit 5 (4+1)
    mmio.write(3, 1, (1 << 0) | (1 << 5));

    // Enable IRQ 3
    mmio.write(0x28, 4, 1 << 3);

    // Set IRQ 3
    LiointcIrqSink(Arc::clone(&lio)).set_irq(3, true);

    // Only parent[core0,ip1] = parent[1] should be high
    let levels = sink.all_levels();
    for (idx, &level) in levels.iter().enumerate() {
        if idx == 1 {
            assert!(level, "parent[{idx}] should be high");
        } else {
            assert!(!level, "parent[{idx}] should be low");
        }
    }
}

#[test]
fn test_liointc_output_routing_multi_core() {
    let lio = Arc::new(Liointc::new_named("liointc"));
    let sink = RecordingSink::new(16);
    let mmio = LiointcMmio(Arc::clone(&lio));

    for core in 0..4u32 {
        for ip in 0..4u32 {
            lio.connect_output_xy(
                core,
                ip,
                recording_line(&sink, core * 4 + ip),
            );
        }
    }

    // Route IRQ 7 to: core 1 bit 1, core 3 bit 3, IP 0 bit 4, IP 2 bit 6
    mmio.write(7, 1, (1 << 1) | (1 << 3) | (1 << 4) | (1 << 6));

    // Enable IRQ 7
    mmio.write(0x28, 4, 1 << 7);

    LiointcIrqSink(Arc::clone(&lio)).set_irq(7, true);

    // Expected: per_core_isr[1] and per_core_isr[3] nonzero
    // per_ip_isr[0] and per_ip_isr[2] nonzero
    // -> parent[core1,ip0]=4, parent[core1,ip2]=6, parent[core3,ip0]=12, parent[core3,ip2]=14
    let levels = sink.all_levels();
    let expected = [4usize, 6, 12, 14];
    for (idx, &level) in levels.iter().enumerate() {
        if expected.contains(&idx) {
            assert!(level, "parent[{idx}] should be high");
        } else {
            assert!(!level, "parent[{idx}] should be low");
        }
    }
}

#[test]
fn test_liointc_output_deasserts_when_ien_cleared() {
    let lio = Arc::new(Liointc::new_named("liointc"));
    let sink = RecordingSink::new(16);
    let mmio = LiointcMmio(Arc::clone(&lio));

    lio.connect_output_xy(0, 0, recording_line(&sink, 0));

    // Route IRQ 1 to core0:ip0
    mmio.write(1, 1, (1 << 0) | (1 << 4));
    // Enable IRQ 1
    mmio.write(0x28, 4, 1 << 1);

    LiointcIrqSink(Arc::clone(&lio)).set_irq(1, true);
    assert!(sink.level(0));

    // Clear IEN for IRQ 1
    mmio.write(0x2c, 4, 1 << 1);
    assert!(!sink.level(0));
}

#[test]
fn test_liointc_invalid_offset_reads_zero() {
    let lio = Arc::new(Liointc::new_named("liointc"));
    let mmio = LiointcMmio(Arc::clone(&lio));

    // Invalid offset
    assert_eq!(mmio.read(0x30, 4), 0);
    assert_eq!(mmio.read(0x100, 4), 0);
}

#[test]
fn test_liointc_non_4byte_aligned_access_returns_zero() {
    let lio = Arc::new(Liointc::new_named("liointc"));
    let mmio = LiointcMmio(Arc::clone(&lio));

    // Non-aligned 4-byte access outside mapper range should return 0
    assert_eq!(mmio.read(0x22, 4), 0);

    // Non-4-byte access to non-mapper range returns 0
    assert_eq!(mmio.read(0x20, 1), 0);
}

#[test]
fn test_liointc_lifecycle() {
    let lio = Arc::new(Liointc::new_named("liointc"));
    let mut bus = SysBus::new("sysbus");
    let mut aspace = make_address_space();
    let base = GPA(0x1000_0000);

    assert!(!lio.realized());

    lio.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "liointc",
        0x60,
        Arc::new(LiointcMmio(Arc::clone(&lio))),
    );
    lio.register_mmio(region, base).unwrap();
    lio.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(lio.realized());

    let err = lio.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

    lio.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!lio.realized());

    let err = lio.unrealize_from(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("not realized"));
}

#[test]
fn test_liointc_reset_runtime() {
    let lio = Arc::new(Liointc::new_named("liointc"));
    let sink = RecordingSink::new(16);
    let mmio = LiointcMmio(Arc::clone(&lio));

    lio.connect_output_xy(0, 0, recording_line(&sink, 0));

    // Set up mapper, IEN, and fire IRQ
    mmio.write(1, 1, (1 << 0) | (1 << 4));
    mmio.write(0x28, 4, 1 << 1);
    LiointcIrqSink(Arc::clone(&lio)).set_irq(1, true);
    assert!(sink.level(0));

    // Reset runtime
    lio.reset_runtime();

    // Check defaults restored and outputs lowered
    assert!(!sink.level(0));
    assert_eq!(mmio.read(0x20, 4), 0); // ISR
    assert_eq!(mmio.read(0x24, 4), 0); // IEN
    assert_eq!(mmio.read(0x40, 4), 0); // per_core_isr[0]
}
