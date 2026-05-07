use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_dma::{
    Pl080, Pl080Mmio, SifivePdma, SifivePdmaMmio, PL080_MMIO_SIZE,
    SIFIVE_PDMA_REG_SIZE,
};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MemoryRegion;

const PDMA_CONTROL: u64 = 0x000;
const PDMA_NEXT_CONFIG: u64 = 0x004;
const PDMA_NEXT_BYTES: u64 = 0x008;
const PDMA_NEXT_DST: u64 = 0x010;
const PDMA_NEXT_SRC: u64 = 0x018;
const PDMA_EXEC_CONFIG: u64 = 0x104;
const PDMA_EXEC_BYTES: u64 = 0x108;
const PDMA_EXEC_DST: u64 = 0x110;
const PDMA_EXEC_SRC: u64 = 0x118;

const PDMA_CONTROL_CLAIM: u32 = 1 << 0;
const PDMA_CONTROL_RUN: u32 = 1 << 1;
const PDMA_CONTROL_DONE_IE: u32 = 1 << 14;
const PDMA_CONTROL_DONE: u32 = 1 << 30;
const PDMA_NEXT_CONFIG_DEFAULT: u32 = (6 << 28) | (6 << 24);

const PL080_CONF_E: u32 = 1 << 0;
const PL080_CCONF_ITC: u32 = 1 << 15;
const PL080_CCONF_E: u32 = 1 << 0;
const PL080_CCTRL_I: u32 = 1 << 31;
const PL080_CCTRL_DI: u32 = 1 << 27;
const PL080_CCTRL_SI: u32 = 1 << 26;

struct RecordingSink {
    levels: Mutex<Vec<bool>>,
}

impl RecordingSink {
    fn new(count: usize) -> Self {
        Self {
            levels: Mutex::new(vec![false; count]),
        }
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

fn make_test_aspace() -> (AddressSpace, SysBus) {
    let root = MemoryRegion::container("root", 0x1_0000_0000);
    let aspace = AddressSpace::new(root);
    let bus = SysBus::new("sysbus");
    (aspace, bus)
}

fn make_ram_aspace(size: u64) -> Arc<AddressSpace> {
    let mut root = MemoryRegion::container("root", size);
    let (ram, _block) = MemoryRegion::ram("ram", size);
    root.add_subregion(ram, GPA(0));
    Arc::new(AddressSpace::new(root))
}

#[test]
fn test_pl080_reset_defaults_and_id() {
    let dma = Pl080::new();

    assert_eq!(dma.do_read(0x00, 4), 0);
    assert_eq!(dma.do_read(0x04, 4), 0);
    assert_eq!(dma.do_read(0x14, 4), 0);
    assert_eq!(dma.do_read(0x18, 4), 0);
    assert_eq!(dma.do_read(0x1c, 4), 0);
    assert_eq!(dma.do_read(0x30, 4), 0);
    assert_eq!(dma.do_read(0x34, 4), 0);

    let id = [0x80, 0x10, 0x04, 0x0a, 0x0d, 0xf0, 0x05, 0xb1];
    for (index, expected) in id.into_iter().enumerate() {
        assert_eq!(dma.do_read(0xfe0 + (index as u64 * 4), 4) as u8, expected);
    }
}

#[test]
fn test_pl080_channel_registers_and_enabled_mask() {
    let dma = Pl080::new();
    let channel2 = 0x100 + 2 * 0x20;

    dma.do_write(channel2, 4, 0x1000_0000);
    dma.do_write(channel2 + 0x04, 4, 0x2000_0000);
    dma.do_write(channel2 + 0x08, 4, 0x3000_0000);
    dma.do_write(channel2 + 0x0c, 4, 0x8000_0010);
    dma.do_write(channel2 + 0x10, 4, 0x0000_0001);

    assert_eq!(dma.do_read(channel2, 4) as u32, 0x1000_0000);
    assert_eq!(dma.do_read(channel2 + 0x04, 4) as u32, 0x2000_0000);
    assert_eq!(dma.do_read(channel2 + 0x08, 4) as u32, 0x3000_0000);
    assert_eq!(dma.do_read(channel2 + 0x0c, 4) as u32, 0x8000_0010);
    assert_eq!(dma.do_read(channel2 + 0x10, 4) as u32, 0x0000_0001);
    assert_eq!(dma.do_read(0x1c, 4) as u32, 1 << 2);
}

#[test]
fn test_pl080_reset_runtime_clears_registers() {
    let dma = Pl080::new();
    let channel0 = 0x100;

    dma.do_write(0x30, 4, 0x0000_0001);
    dma.do_write(0x34, 4, 0xffff_ffff);
    dma.do_write(channel0, 4, 0x1111_2222);
    dma.do_write(channel0 + 0x04, 4, 0x3333_4444);
    dma.do_write(channel0 + 0x10, 4, 0x0000_0001);

    dma.reset_runtime();

    assert_eq!(dma.do_read(0x30, 4), 0);
    assert_eq!(dma.do_read(0x34, 4), 0);
    assert_eq!(dma.do_read(channel0, 4), 0);
    assert_eq!(dma.do_read(channel0 + 0x04, 4), 0);
    assert_eq!(dma.do_read(channel0 + 0x10, 4), 0);
    assert_eq!(dma.do_read(0x1c, 4), 0);
}

#[test]
fn test_pl080_invalid_offsets_return_zero_and_subword_accesses_truncate() {
    let dma = Pl080::new();

    assert_eq!(dma.do_read(0x200, 4), 0);
    dma.do_write(0x100, 1, 0x0000_12aa);
    assert_eq!(dma.do_read(0x100, 1) as u32, 0x0000_00aa);
    dma.do_write(0x200, 4, 0xffff_ffff);
    assert_eq!(dma.do_read(0x100, 4) as u32, 0x0000_00aa);
}

#[test]
fn test_pl080_wide_mmio_accesses_split_into_32bit_callbacks() {
    let dma = Pl080::new();

    dma.do_write(0x100, 8, 0x1234_5678_ffff_ffff);

    assert_eq!(dma.do_read(0x100, 4) as u32, 0xffff_ffff);
    assert_eq!(dma.do_read(0x104, 4) as u32, 0x1234_5678);
    assert_eq!(dma.do_read(0x100, 8), 0x1234_5678_ffff_ffff);
}

#[test]
fn test_pl080_unaligned_wide_accesses_split_like_qemu() {
    let dma = Pl080::new();

    dma.do_write(0x030, 4, 0);
    dma.do_write(0x034, 4, 0);
    dma.do_write(0x031, 4, 0x0102_0304);

    assert_eq!(dma.do_read(0x030, 4), 0x0203);
    assert_eq!(dma.do_read(0x034, 4), 0x0001);

    assert_eq!(dma.do_read(0xfe1, 4), 0x1000_8080);
    assert_eq!(dma.do_read(0xfe2, 4), 0x0010_0080);
    assert_eq!(dma.do_read(0xfe3, 4), 0x1000_1080);
}

#[test]
fn test_pl080_lifecycle_and_mom_identity() {
    let dma = Pl080::new();
    assert!(!dma.realized());
    dma.with_mdevice(|device| assert_eq!(device.local_id(), "pl080"));
    assert_eq!(dma.object_info().local_id, "pl080");

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA(0x1000_0000);
    assert_eq!(aspace.read(GPA(base.0 + 0xfe0), 4), 0);

    dma.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "pl080",
        PL080_MMIO_SIZE,
        Arc::new(Pl080Mmio(dma.clone())),
    );
    dma.register_mmio(region, base).unwrap();
    dma.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(dma.realized());

    assert_eq!(aspace.read(GPA(base.0 + 0xfe0), 4) as u8, 0x80);
    aspace.write(GPA(base.0 + 0x100), 4, 0x1234_5678);
    assert_eq!(aspace.read(GPA(base.0 + 0x100), 4) as u32, 0x1234_5678);

    let err = dma.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

    dma.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!dma.realized());
    assert_eq!(aspace.read(GPA(base.0 + 0xfe0), 4), 0);
}

#[test]
fn test_pl080_run_copies_memory_and_sets_terminal_count() {
    let dma = Pl080::new();
    let aspace = make_ram_aspace(0x1000);
    dma.set_dma_address_space(Arc::clone(&aspace));

    let src = 0x180;
    let dst = 0x2c0;
    let bytes = [0xa0, 0xb1, 0xc2, 0xd3, 0xe4, 0xf5];
    for (index, byte) in bytes.iter().copied().enumerate() {
        aspace.write(GPA(src + index as u64), 1, u64::from(byte));
    }

    dma.do_write(0x100, 4, src);
    dma.do_write(0x104, 4, dst);
    dma.do_write(
        0x10c,
        4,
        u64::from(PL080_CCTRL_I | PL080_CCTRL_DI | PL080_CCTRL_SI)
            | bytes.len() as u64,
    );
    dma.do_write(0x110, 4, u64::from(PL080_CCONF_ITC | PL080_CCONF_E));
    dma.do_write(0x030, 4, u64::from(PL080_CONF_E));

    for (index, expected) in bytes.iter().copied().enumerate() {
        assert_eq!(aspace.read(GPA(dst + index as u64), 1) as u8, expected);
    }
    assert_eq!(dma.do_read(0x100, 4), src + bytes.len() as u64);
    assert_eq!(dma.do_read(0x104, 4), dst + bytes.len() as u64);
    assert_eq!(
        dma.do_read(0x10c, 4) as u32,
        PL080_CCTRL_I | PL080_CCTRL_DI | PL080_CCTRL_SI
    );
    assert_eq!(dma.do_read(0x110, 4) as u32, PL080_CCONF_ITC);
    assert_eq!(dma.do_read(0x014, 4) as u32, 1);
    assert_eq!(dma.do_read(0x004, 4) as u32, 1);

    dma.do_write(0x008, 4, 1);
    assert_eq!(dma.do_read(0x014, 4), 0);
}

#[test]
fn test_pl080_run_follows_linked_list_item() {
    let dma = Pl080::new();
    let aspace = make_ram_aspace(0x1000);
    dma.set_dma_address_space(Arc::clone(&aspace));

    let first_src = 0x180;
    let first_dst = 0x280;
    let lli = 0x380;
    let second_src = 0x480;
    let second_dst = 0x580;
    let first = [0x11, 0x22];
    let second = [0x33, 0x44, 0x55];
    for (index, byte) in first.iter().copied().enumerate() {
        aspace.write(GPA(first_src + index as u64), 1, u64::from(byte));
    }
    for (index, byte) in second.iter().copied().enumerate() {
        aspace.write(GPA(second_src + index as u64), 1, u64::from(byte));
    }
    aspace.write(GPA(lli), 4, second_src);
    aspace.write(GPA(lli + 4), 4, second_dst);
    aspace.write(GPA(lli + 8), 4, 0);
    aspace.write(
        GPA(lli + 12),
        4,
        u64::from(PL080_CCTRL_I | PL080_CCTRL_DI | PL080_CCTRL_SI)
            | second.len() as u64,
    );

    dma.do_write(0x100, 4, first_src);
    dma.do_write(0x104, 4, first_dst);
    dma.do_write(0x108, 4, lli | 0x3);
    dma.do_write(
        0x10c,
        4,
        u64::from(PL080_CCTRL_I | PL080_CCTRL_DI | PL080_CCTRL_SI)
            | first.len() as u64,
    );
    dma.do_write(0x110, 4, u64::from(PL080_CCONF_ITC | PL080_CCONF_E));
    dma.do_write(0x030, 4, u64::from(PL080_CONF_E));

    for (index, expected) in first.iter().copied().enumerate() {
        assert_eq!(
            aspace.read(GPA(first_dst + index as u64), 1) as u8,
            expected
        );
    }
    for (index, expected) in second.iter().copied().enumerate() {
        assert_eq!(
            aspace.read(GPA(second_dst + index as u64), 1) as u8,
            expected
        );
    }
    assert_eq!(dma.do_read(0x100, 4), second_src + second.len() as u64);
    assert_eq!(dma.do_read(0x104, 4), second_dst + second.len() as u64);
    assert_eq!(dma.do_read(0x108, 4), 0);
    assert_eq!(
        dma.do_read(0x10c, 4) as u32,
        PL080_CCTRL_I | PL080_CCTRL_DI | PL080_CCTRL_SI
    );
    assert_eq!(dma.do_read(0x110, 4) as u32, PL080_CCONF_ITC);
    assert_eq!(dma.do_read(0x014, 4) as u32, 1);
}

#[test]
fn test_pl080_terminal_count_drives_combined_and_tc_irqs() {
    let dma = Pl080::new();
    let aspace = make_ram_aspace(0x1000);
    let sink = Arc::new(RecordingSink::new(3));
    dma.set_dma_address_space(Arc::clone(&aspace));
    dma.connect_irq(
        0,
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0),
    );
    dma.connect_irq(
        2,
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 2),
    );

    aspace.write(GPA(0x100), 1, 0x5a);
    dma.do_write(0x100, 4, 0x100);
    dma.do_write(0x104, 4, 0x200);
    dma.do_write(
        0x10c,
        4,
        u64::from(PL080_CCTRL_I | PL080_CCTRL_DI | PL080_CCTRL_SI | 1),
    );
    dma.do_write(0x110, 4, u64::from(PL080_CCONF_ITC | PL080_CCONF_E));
    dma.do_write(0x030, 4, u64::from(PL080_CONF_E));

    assert!(sink.level(0));
    assert!(sink.level(2));

    dma.do_write(0x008, 4, 1);

    assert!(!sink.level(0));
    assert!(!sink.level(2));
}

#[test]
fn test_sifive_pdma_defaults_and_claim_initializes_next_registers() {
    let pdma = SifivePdma::new();

    assert_eq!(pdma.do_read(PDMA_CONTROL, 4), 0);
    assert_eq!(pdma.do_read(PDMA_NEXT_CONFIG, 4), 0);
    assert_eq!(pdma.do_read(PDMA_NEXT_BYTES, 8), 0);
    assert_eq!(pdma.do_read(PDMA_NEXT_DST, 8), 0);
    assert_eq!(pdma.do_read(PDMA_NEXT_SRC, 8), 0);

    pdma.do_write(PDMA_CONTROL, 4, u64::from(PDMA_CONTROL_CLAIM));

    assert_eq!(pdma.do_read(PDMA_CONTROL, 4) as u32, PDMA_CONTROL_CLAIM);
    assert_eq!(
        pdma.do_read(PDMA_NEXT_CONFIG, 4) as u32,
        PDMA_NEXT_CONFIG_DEFAULT
    );
    assert_eq!(pdma.do_read(PDMA_NEXT_BYTES, 8), 0);
    assert_eq!(pdma.do_read(PDMA_NEXT_DST, 8), 0);
    assert_eq!(pdma.do_read(PDMA_NEXT_SRC, 8), 0);
}

#[test]
fn test_sifive_pdma_32_and_64_bit_next_register_access() {
    let pdma = SifivePdma::new();
    let channel1 = 0x1000;

    pdma.do_write(channel1 + PDMA_NEXT_BYTES, 8, 0x1122_3344_5566_7788);
    assert_eq!(
        pdma.do_read(channel1 + PDMA_NEXT_BYTES, 8),
        0x1122_3344_5566_7788
    );
    assert_eq!(
        pdma.do_read(channel1 + PDMA_NEXT_BYTES, 4) as u32,
        0x5566_7788
    );
    assert_eq!(
        pdma.do_read(channel1 + PDMA_NEXT_BYTES + 4, 4) as u32,
        0x1122_3344
    );

    pdma.do_write(channel1 + PDMA_NEXT_DST, 4, 0xaabb_ccdd);
    pdma.do_write(channel1 + PDMA_NEXT_DST + 4, 4, 0xeeff_0011);
    assert_eq!(
        pdma.do_read(channel1 + PDMA_NEXT_DST, 8),
        0xeeff_0011_aabb_ccdd
    );

    pdma.do_write(channel1 + PDMA_NEXT_SRC, 8, 0x0123_4567_89ab_cdef);
    assert_eq!(
        pdma.do_read(channel1 + PDMA_NEXT_SRC, 8),
        0x0123_4567_89ab_cdef
    );
}

#[test]
fn test_sifive_pdma_unaligned_qword_accesses_split_like_qemu() {
    let pdma = SifivePdma::new();

    pdma.do_write(PDMA_NEXT_BYTES, 8, 0x1122_3344_5566_7788);
    pdma.do_write(PDMA_NEXT_DST, 8, 0x99aa_bbcc_ddee_ff00);

    assert_eq!(pdma.do_read(PDMA_NEXT_BYTES + 1, 4), 0);
    assert_eq!(pdma.do_read(PDMA_NEXT_BYTES + 1, 8), 0x0011_2233_4400_0000);

    pdma.do_write(PDMA_NEXT_BYTES, 8, 0);
    pdma.do_write(PDMA_NEXT_DST, 8, 0);
    pdma.do_write(PDMA_NEXT_BYTES + 1, 4, 0x0102_0304);
    assert_eq!(pdma.do_read(PDMA_NEXT_BYTES, 8), 0);

    pdma.do_write(PDMA_NEXT_BYTES + 1, 8, 0x0102_0304_0506_0708);
    assert_eq!(pdma.do_read(PDMA_NEXT_BYTES, 8), 0x0203_0405_0000_0000);
    assert_eq!(pdma.do_read(PDMA_NEXT_DST, 8), 0);
}

#[test]
fn test_sifive_pdma_exec_registers_are_read_only() {
    let pdma = SifivePdma::new();

    pdma.do_write(PDMA_EXEC_CONFIG, 4, 0xffff_ffff);
    pdma.do_write(PDMA_EXEC_BYTES, 8, 0xffff_ffff_ffff_ffff);
    pdma.do_write(PDMA_EXEC_DST, 8, 0xffff_ffff_ffff_ffff);
    pdma.do_write(PDMA_EXEC_SRC, 8, 0xffff_ffff_ffff_ffff);

    assert_eq!(pdma.do_read(PDMA_EXEC_CONFIG, 4), 0);
    assert_eq!(pdma.do_read(PDMA_EXEC_BYTES, 8), 0);
    assert_eq!(pdma.do_read(PDMA_EXEC_DST, 8), 0);
    assert_eq!(pdma.do_read(PDMA_EXEC_SRC, 8), 0);
}

#[test]
fn test_sifive_pdma_zero_byte_run_sets_done_and_done_irq() {
    let pdma = SifivePdma::new();
    let sink = Arc::new(RecordingSink::new(8));
    pdma.connect_irq(
        0,
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0),
    );

    pdma.do_write(PDMA_CONTROL, 4, u64::from(PDMA_CONTROL_CLAIM));
    pdma.do_write(
        PDMA_CONTROL,
        4,
        u64::from(PDMA_CONTROL_CLAIM | PDMA_CONTROL_RUN | PDMA_CONTROL_DONE_IE),
    );

    assert_eq!(
        pdma.do_read(PDMA_CONTROL, 4) as u32,
        PDMA_CONTROL_CLAIM | PDMA_CONTROL_DONE_IE | PDMA_CONTROL_DONE
    );
    assert!(sink.level(0));
}

#[test]
fn test_sifive_pdma_run_copies_memory_and_updates_exec_registers() {
    let pdma = SifivePdma::new();
    let aspace = make_ram_aspace(0x1000);
    let sink = Arc::new(RecordingSink::new(8));
    pdma.connect_irq(
        0,
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0),
    );
    pdma.set_dma_address_space(Arc::clone(&aspace));

    let src = 0x100;
    let dst = 0x240;
    let bytes = [0x10, 0x22, 0x34, 0x48, 0x5a, 0x6c, 0x7e, 0x80, 0x91];
    for (index, byte) in bytes.iter().copied().enumerate() {
        aspace.write(GPA(src + index as u64), 1, u64::from(byte));
    }

    pdma.do_write(PDMA_CONTROL, 4, u64::from(PDMA_CONTROL_CLAIM));
    pdma.do_write(PDMA_NEXT_SRC, 8, src);
    pdma.do_write(PDMA_NEXT_DST, 8, dst);
    pdma.do_write(PDMA_NEXT_BYTES, 8, bytes.len() as u64);
    pdma.do_write(
        PDMA_CONTROL,
        4,
        u64::from(PDMA_CONTROL_CLAIM | PDMA_CONTROL_RUN | PDMA_CONTROL_DONE_IE),
    );

    for (index, expected) in bytes.iter().copied().enumerate() {
        assert_eq!(aspace.read(GPA(dst + index as u64), 1) as u8, expected);
    }
    assert_eq!(pdma.do_read(PDMA_EXEC_BYTES, 8), 0);
    assert_eq!(pdma.do_read(PDMA_EXEC_SRC, 8), src + bytes.len() as u64);
    assert_eq!(pdma.do_read(PDMA_EXEC_DST, 8), dst + bytes.len() as u64);
    assert_eq!(
        pdma.do_read(PDMA_CONTROL, 4) as u32,
        PDMA_CONTROL_CLAIM | PDMA_CONTROL_DONE_IE | PDMA_CONTROL_DONE
    );
    assert!(sink.level(0));
}

#[test]
fn test_sifive_pdma_unclaimed_run_is_ignored_and_invalid_channel_is_zero() {
    let pdma = SifivePdma::new();

    pdma.do_write(PDMA_CONTROL, 4, u64::from(PDMA_CONTROL_RUN));
    assert_eq!(pdma.do_read(PDMA_CONTROL, 4), 0);

    let invalid_channel = 4 * 0x1000;
    pdma.do_write(invalid_channel + PDMA_NEXT_BYTES, 8, 0xffff);
    assert_eq!(pdma.do_read(invalid_channel + PDMA_NEXT_BYTES, 8), 0);
    assert_eq!(pdma.do_read(SIFIVE_PDMA_REG_SIZE, 4), 0);
}

#[test]
fn test_sifive_pdma_lifecycle_and_mom_identity() {
    let pdma = SifivePdma::new();
    assert!(!pdma.realized());
    pdma.with_mdevice(|device| assert_eq!(device.local_id(), "sifive-pdma"));
    assert_eq!(pdma.object_info().local_id, "sifive-pdma");

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA(0x2000_0000);
    pdma.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "sifive-pdma",
        SIFIVE_PDMA_REG_SIZE,
        Arc::new(SifivePdmaMmio(pdma.clone())),
    );
    pdma.register_mmio(region, base).unwrap();
    pdma.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(pdma.realized());

    aspace.write(GPA(base.0 + PDMA_NEXT_BYTES), 8, 0x1234);
    assert_eq!(aspace.read(GPA(base.0 + PDMA_NEXT_BYTES), 8), 0x1234);

    let err = pdma.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

    pdma.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!pdma.realized());
    assert_eq!(aspace.read(GPA(base.0 + PDMA_NEXT_BYTES), 8), 0);
}
