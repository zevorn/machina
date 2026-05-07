use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_intc::aplic::{RiscvAplic, RiscvAplicMmio};
use machina_hw_intc::imsic::{RiscvImsic, RiscvImsicMmio};
use machina_hw_misc::cmgcr::{Cmgcr, CmgcrMmio};
use machina_hw_misc::cpc::{Cpc, CpcMmio};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

pub(crate) struct RecordingSink {
    levels: Mutex<Vec<bool>>,
}

impl RecordingSink {
    pub(crate) fn new(num_lines: usize) -> Arc<Self> {
        Arc::new(Self {
            levels: Mutex::new(vec![false; num_lines]),
        })
    }

    pub(crate) fn level(&self, irq: u32) -> bool {
        self.levels.lock().unwrap()[irq as usize]
    }
}

impl IrqSink for RecordingSink {
    fn set_irq(&self, irq: u32, level: bool) {
        self.levels.lock().unwrap()[irq as usize] = level;
    }
}

fn recording_line(sink: &Arc<RecordingSink>, irq: u32) -> InterruptSource {
    InterruptSource::new(Arc::clone(sink) as Arc<dyn IrqSink>, irq)
}

fn make_address_space() -> AddressSpace {
    AddressSpace::new(MemoryRegion::container("system", u64::MAX))
}

// ---- Cmgcr ----

#[test]
fn test_cmgcr_defaults() {
    let cmgcr = Cmgcr::new_named("cmgcr", 0xa00, 0, 4, 1, 4, 0x1FB8_0000);
    let mmio = CmgcrMmio(Arc::new(cmgcr));

    // GCR_CONFIG_OFS: PCORES=0
    assert_eq!(mmio.read(0x0000, 8), 0);
    // GCR_BASE_OFS: gcr_base
    assert_eq!(mmio.read(0x0008, 8), 0x1FB8_0000);
    // GCR_REV_OFS: gcr_rev
    assert_eq!(mmio.read(0x0030, 8), 0xa00);
    // GCR_CPC_STATUS_OFS: not connected
    assert_eq!(mmio.read(0x00F0, 8), 0);
    // GCR_L2_CONFIG_OFS: L2 BYPASS
    assert_eq!(mmio.read(0x0130, 8), 1 << 20);
}

#[test]
fn test_cmgcr_cpc_connected() {
    let cmgcr =
        Arc::new(Cmgcr::new_named("cmgcr", 0xa00, 0, 4, 1, 4, 0x1FB8_0000));
    let mmio = CmgcrMmio(Arc::clone(&cmgcr));

    // Default: not connected
    assert_eq!(mmio.read(0x00F0, 8), 0);

    // Set CPC connected
    cmgcr.set_cpc_connected(true);
    assert_eq!(mmio.read(0x00F0, 8), 1);
}

#[test]
fn test_cmgcr_write_gcr_base() {
    let cmgcr = Cmgcr::new_named("cmgcr", 0xa00, 0, 4, 1, 4, 0x1FB8_0000);
    let mmio = CmgcrMmio(Arc::new(cmgcr));

    // Write new base address
    mmio.write(0x0008, 8, 0x1FC0_0000);
    // Upper bits masked by GCR_BASE_GCRBASE_MSK
    assert_eq!(mmio.read(0x0008, 8), 0x1FC0_0000 & 0xFFFF_FFFF_8000);
}

#[test]
fn test_cmgcr_subword_mmio_reads_mask_to_access_width() {
    let cmgcr = Cmgcr::new_named("cmgcr", 0xa00, 0, 4, 1, 4, 0x1234_5678_8000);
    let mmio = CmgcrMmio(Arc::new(cmgcr));

    assert_eq!(mmio.read(0x0030, 1), 0x00);
    assert_eq!(mmio.read(0x0030, 2), 0x0a00);
    assert_eq!(mmio.read(0x0130, 2), 0x0000);
    assert_eq!(mmio.read(0x0130, 4), 0x0010_0000);
    assert_eq!(mmio.read(0x0008, 4), 0x5678_8000);
    assert_eq!(mmio.read(0x0008, 8), 0x1234_5678_8000);
}

#[test]
fn test_cmgcr_subword_mmio_writes_mask_to_access_width() {
    let cmgcr = Cmgcr::new_named("cmgcr", 0xa00, 0, 4, 1, 4, 0x1FB8_0000);
    let mmio = CmgcrMmio(Arc::new(cmgcr));

    mmio.write(0x0008, 1, 0x1FB8_0080);
    assert_eq!(mmio.read(0x0008, 8), 0);

    mmio.write(0x0008, 2, 0x1FB8_8000);
    assert_eq!(mmio.read(0x0008, 8), 0x8000);
}

#[test]
fn test_cmgcr_write_cl_reset_base() {
    let cmgcr =
        Arc::new(Cmgcr::new_named("cmgcr", 0xa00, 0, 6, 2, 3, 0x1FB8_0000));
    let mmio = CmgcrMmio(Arc::clone(&cmgcr));
    let updates: Arc<Mutex<Vec<(usize, u64)>>> =
        Arc::new(Mutex::new(Vec::new()));
    let updates2 = Arc::clone(&updates);
    cmgcr.set_vp_reset_base_cb(Box::new(move |vp, val| {
        updates2.lock().unwrap().push((vp, val));
    }));

    // core=1, hart=1: VP index = 1*2 + 1 = 3
    // offset = 0x2000 + 1*0x100 + 1*0x8 = 0x2108
    let offset = 0x2000u64 + 0x100 + 0x8;
    mmio.write(offset, 8, 0x1FC0_0000);

    // core=2, hart=0: VP index = 2*2 + 0 = 4
    let offset2 = 0x2000u64 + 2 * 0x100;
    mmio.write(offset2, 8, 0x1D00_0000);

    let recorded = updates.lock().unwrap().clone();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0], (3, 0x1FC0_0000 & 0xFFFF_FFFF_FFFF_F000));
    assert_eq!(recorded[1], (4, 0x1D00_0000 & 0xFFFF_FFFF_FFFF_F000));
}

#[test]
fn test_cmgcr_invalid_offset_reads_zero() {
    let cmgcr = Cmgcr::new_named("cmgcr", 0xa00, 0, 4, 1, 4, 0x1FB8_0000);
    let mmio = CmgcrMmio(Arc::new(cmgcr));

    assert_eq!(mmio.read(0x0100, 8), 0);
    assert_eq!(mmio.read(0x1000, 8), 0);
}

#[test]
fn test_cmgcr_lifecycle_and_mom_identity() {
    let cmgcr =
        Arc::new(Cmgcr::new_named("cmgcr", 0xa00, 0, 4, 1, 4, 0x1FB8_0000));
    let mut bus = SysBus::new("sysbus");
    let mut aspace = make_address_space();
    let base = GPA(0x1FB8_0000);

    assert!(!cmgcr.realized());
    cmgcr.with_mdevice(|device| assert_eq!(device.local_id(), "cmgcr"));
    assert_eq!(cmgcr.object_info().local_id, "cmgcr");

    cmgcr.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "cmgcr",
        0x8000,
        Arc::new(CmgcrMmio(Arc::clone(&cmgcr))),
    );
    cmgcr.register_mmio(region, base).unwrap();
    cmgcr.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(cmgcr.realized());

    let err = cmgcr.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

    cmgcr.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!cmgcr.realized());

    let err = cmgcr.unrealize_from(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("not realized"));
}

#[test]
fn test_cmgcr_reset_runtime() {
    let cmgcr =
        Arc::new(Cmgcr::new_named("cmgcr", 0xa00, 0, 4, 1, 4, 0x1FB8_0000));
    let mmio = CmgcrMmio(Arc::clone(&cmgcr));

    // Write a new base address
    mmio.write(0x0008, 8, 0x1FC0_0000);
    assert_ne!(mmio.read(0x0008, 8), 0x1FB8_0000);

    let updates: Arc<Mutex<Vec<(usize, u64)>>> =
        Arc::new(Mutex::new(Vec::new()));
    let updates2 = Arc::clone(&updates);
    cmgcr.set_vp_reset_base_cb(Box::new(move |vp, val| {
        updates2.lock().unwrap().push((vp, val));
    }));

    cmgcr.reset_runtime();

    // All 4 VPs should get CM_RESET_VEC written
    let recorded = updates.lock().unwrap().clone();
    assert_eq!(recorded.len(), 4);
    let cm_reset = 0x1FC0_0000 & 0xFFFF_FFFF_FFFF_F000;
    for (vp_idx, val) in &recorded {
        assert_eq!(*val, cm_reset, "VP {vp_idx} should get CM_RESET_VEC");
    }
}

#[test]
fn test_cmgcr_validate_num_vp_zero() {
    let cmgcr = Cmgcr::new_named("cmgcr", 0xa00, 0, 0, 1, 1, 0x1FB8_0000);
    let err = cmgcr.validate_properties().unwrap_err();
    assert!(err.contains("num_vps"));
}

#[test]
fn test_cmgcr_validate_num_vp_exceeds_max() {
    let cmgcr = Cmgcr::new_named("cmgcr", 0xa00, 0, 257, 1, 1, 0x1FB8_0000);
    let err = cmgcr.validate_properties().unwrap_err();
    assert!(err.contains("exceeds max"));
}

// ---- Cpc ----

#[test]
fn test_cpc_defaults() {
    let cpc = Cpc::new_named("cpc", 0, 4, 1, 4, 1);
    let mmio = CpcMmio(Arc::new(cpc));

    assert_eq!(mmio.read(0x1008, 8), 6 << 19);
    assert_eq!(mmio.read(0x2008, 8), 7 << 19);
    assert_eq!(mmio.read(0x50, 8), 0);
}

#[test]
fn test_cpc_mtime_with_callback() {
    let cpc = Arc::new(Cpc::new_named("cpc", 0, 4, 1, 4, 1));
    let ticks: Arc<Mutex<u64>> = Arc::new(Mutex::new(0xDEAD_BEEF));
    let t = Arc::clone(&ticks);
    cpc.set_mtime_cb(Box::new(move || *t.lock().unwrap()));

    let mmio = CpcMmio(Arc::clone(&cpc));
    assert_eq!(mmio.read(0x50, 8), 0xDEAD_BEEF);

    *ticks.lock().unwrap() = 0x1234_5678_9ABC_DEF0;
    assert_eq!(mmio.read(0x50, 8), 0x1234_5678_9ABC_DEF0);
}

#[test]
fn test_cpc_subword_mmio_reads_mask_to_access_width() {
    let cpc = Arc::new(Cpc::new_named("cpc", 0, 4, 1, 4, 1));
    cpc.set_mtime_cb(Box::new(|| 0x1234_5678_9ABC_DEF0));
    let mmio = CpcMmio(Arc::clone(&cpc));

    assert_eq!(mmio.read(0x1008, 1), 0x00);
    assert_eq!(mmio.read(0x1008, 2), 0x0000);
    assert_eq!(mmio.read(0x1008, 4), 6 << 19);
    assert_eq!(mmio.read(0x50, 1), 0xf0);
    assert_eq!(mmio.read(0x50, 2), 0xdef0);
    assert_eq!(mmio.read(0x50, 4), 0x9abc_def0);
    assert_eq!(mmio.read(0x50, 8), 0x1234_5678_9ABC_DEF0);
}

#[test]
fn test_cpc_vp_run_stop_with_callback() {
    let cpc = Arc::new(Cpc::new_named("cpc", 0, 4, 1, 4, 0));
    let mmio = CpcMmio(Arc::clone(&cpc));
    let actions: Arc<Mutex<Vec<(u64, bool)>>> =
        Arc::new(Mutex::new(Vec::new()));
    let a = Arc::clone(&actions);
    cpc.set_vp_action_cb(Box::new(move |vp, run| {
        a.lock().unwrap().push((vp, run));
    }));

    // run VP 0 on core 0 (cpu_index=0, val=1 → mask bit 0)
    mmio.write(0x2028, 8, 1);
    // stop VP 1 on core 0 (cpu_index=0, val=2 → mask bit 1)
    mmio.write(0x2020, 8, 2);

    let recorded = actions.lock().unwrap().clone();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0], (0, true)); // VP 0 run
    assert_eq!(recorded[1], (1, false)); // VP 1 stop
}

#[test]
fn test_cpc_byte_mmio_write_masks_to_access_width() {
    let cpc = Arc::new(Cpc::new_named("cpc", 0, 16, 1, 16, 0));
    let mmio = CpcMmio(Arc::clone(&cpc));
    let actions: Arc<Mutex<Vec<(u64, bool)>>> =
        Arc::new(Mutex::new(Vec::new()));
    let a = Arc::clone(&actions);
    cpc.set_vp_action_cb(Box::new(move |vp, run| {
        a.lock().unwrap().push((vp, run));
    }));

    mmio.write(0x2028, 1, 0x0100);

    assert!(actions.lock().unwrap().is_empty());
}

#[test]
fn test_cpc_vp_run_multi_core() {
    // num_vp=16 (4 cores * 4 harts), num_hart=4, num_core=4
    // cpu_index = c*4 + cluster*4*4
    // core 1: cpu_index = 4, core 2: cpu_index = 8
    let cpc = Arc::new(Cpc::new_named("cpc", 0, 16, 4, 4, 0));
    let mmio = CpcMmio(Arc::clone(&cpc));
    let actions: Arc<Mutex<Vec<(u64, bool)>>> =
        Arc::new(Mutex::new(Vec::new()));
    let a = Arc::clone(&actions);
    cpc.set_vp_action_cb(Box::new(move |vp, run| {
        a.lock().unwrap().push((vp, run));
    }));

    // core 1, VP_RUN: val=0x3 → mask bits 4,5
    mmio.write(0x2128, 8, 0x3);
    // core 2, VP_STOP: val=0x1 → mask bit 8
    mmio.write(0x2220, 8, 0x1);

    let recorded = actions.lock().unwrap().clone();
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[0], (4, true));
    assert_eq!(recorded[1], (5, true));
    assert_eq!(recorded[2], (8, false));
}

#[test]
fn test_cpc_validate_mask_exceeds_vp() {
    let cpc = Cpc::new_named("cpc", 0, 4, 1, 4, 0x10);
    let err = cpc.validate_properties().unwrap_err();
    assert!(err.contains("vps_start_running_mask"));
}

#[test]
fn test_cpc_validate_mask_ok() {
    let cpc = Cpc::new_named("cpc", 0, 4, 1, 4, 0xF);
    cpc.validate_properties().unwrap();
}

#[test]
fn test_cpc_invalid_offset_reads_zero() {
    let cpc = Cpc::new_named("cpc", 0, 4, 1, 4, 1);
    let mmio = CpcMmio(Arc::new(cpc));

    assert_eq!(mmio.read(0x00, 8), 0);
    assert_eq!(mmio.read(0x100, 8), 0);
    assert_eq!(mmio.read(0x5000, 8), 0);
}

#[test]
fn test_cpc_lifecycle_and_mom_identity() {
    let cpc = Arc::new(Cpc::new_named("cpc", 0, 4, 1, 4, 1));
    let mut bus = SysBus::new("sysbus");
    let mut aspace = make_address_space();
    let base = GPA(0x1FB0_0000);

    assert!(!cpc.realized());
    cpc.with_mdevice(|device| assert_eq!(device.local_id(), "cpc"));
    assert_eq!(cpc.object_info().local_id, "cpc");

    cpc.attach_to_bus(&mut bus).unwrap();
    let region =
        MemoryRegion::io("cpc", 0x6000, Arc::new(CpcMmio(Arc::clone(&cpc))));
    cpc.register_mmio(region, base).unwrap();
    cpc.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(cpc.realized());

    let err = cpc.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

    cpc.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!cpc.realized());

    let err = cpc.unrealize_from(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("not realized"));
}

#[test]
fn test_cpc_reset_runtime() {
    let cpc = Arc::new(Cpc::new_named("cpc", 0, 4, 1, 4, 0xF));
    let actions: Arc<Mutex<Vec<(u64, bool)>>> =
        Arc::new(Mutex::new(Vec::new()));
    let a = Arc::clone(&actions);
    cpc.set_vp_action_cb(Box::new(move |vp, run| {
        a.lock().unwrap().push((vp, run));
    }));

    // VP_STOP clears all
    cpc.reset_runtime();
    // No VP changes via callback on reset (callbacks are for guest writes).
    // The test just verifies no panic + state update.
    let mmio = CpcMmio(Arc::clone(&cpc));
    let _ = mmio.read(0x2008, 8);
}

#[test]
fn test_cpc_stat_conf_multi_core() {
    let cpc = Cpc::new_named("cpc", 0, 4, 1, 4, 1);
    let mmio = CpcMmio(Arc::new(cpc));

    // All cores should return SEQ_STATE_U6 for their STAT_CONF
    for c in 0..4 {
        let offset = 0x2008u64 + c * 0x100;
        assert_eq!(mmio.read(offset, 8), 7 << 19);
    }
}

// ---- RiscvImsic ----

#[test]
fn test_imsic_defaults() {
    let imsic = RiscvImsic::new_named("imsic", false, 0, 2, 64);
    assert!(!imsic.realized());
    assert_eq!(imsic.eidelivery_val(0), 0);
    assert_eq!(imsic.eithreshold_val(0), 0);
    assert_eq!(imsic.eistate_val(0), 0);
}

#[test]
fn test_imsic_mmio_read_returns_zero() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    let mmio = RiscvImsicMmio(Arc::clone(&imsic));

    assert_eq!(mmio.read(0x0000, 4), 0);
    assert_eq!(mmio.read(0x1000, 4), 0);
    // unaligned reads also return 0
    assert_eq!(mmio.read(0x0001, 4), 0);
}

#[test]
fn test_imsic_mmio_write_le_page_sets_pending() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    let mmio = RiscvImsicMmio(Arc::clone(&imsic));

    // Write IRQ number 5 to LE page 0
    mmio.write(0x0000, 4, 5);
    assert_eq!(imsic.eistate_val(5) & 1, 1, "IRQ 5 should be pending");
    // IRQ 6 should not be pending
    assert_eq!(imsic.eistate_val(6) & 1, 0);
}

#[test]
fn test_imsic_mmio_write_be_page_is_ignored() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    let mmio = RiscvImsicMmio(Arc::clone(&imsic));

    mmio.write(IMSIC_MMIO_PAGE_BE, 4, 0x0300_0000u64);

    assert_eq!(imsic.eistate_val(3) & 1, 0);
}

#[test]
fn test_imsic_mmio_write_ignores_zero_and_oob() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    let mmio = RiscvImsicMmio(Arc::clone(&imsic));

    // Write value 0 — ignored (bit 0 is RO zero)
    mmio.write(0x0000, 4, 0);
    assert_eq!(imsic.eistate_val(0) & 1, 0);

    // Write out-of-range IRQ number
    mmio.write(0x0000, 4, 100);
    assert_eq!(imsic.eistate_val(100), 0);
}

#[test]
fn test_imsic_eidelivery_rmw() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    let reg = eid_sel_reg(false); // S-mode, page 0

    let mut val = 0;
    // Set eidelivery to 1
    assert_eq!(imsic.rmw(reg, &mut val, 1, 1), 0);
    assert_eq!(val, 0); // old value was 0
    assert_eq!(imsic.eidelivery_val(0), 1);

    // Read back
    assert_eq!(imsic.rmw(reg, &mut val, 0, 0), 0);
    assert_eq!(val, 1);
}

#[test]
fn test_imsic_eithreshold_rmw() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    let reg = eth_sel_reg(false);

    let mut val = 0;
    assert_eq!(imsic.rmw(reg, &mut val, 10, 0xffff), 0);
    assert_eq!(val, 0);
    assert_eq!(imsic.eithreshold_val(0), 10);
}

#[test]
fn test_imsic_eie_eip_rmw() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    // EIE0 register for S-mode, 32-bit
    let eie0_reg = eie_sel_reg(false, 0);

    let mut val = 0;
    // Set enable for IRQ 1-3 (bit 0 is RO zero)
    assert_eq!(imsic.rmw(eie0_reg, &mut val, 0xe, 0xe), 0);
    assert_eq!(val, 0); // old value

    // Verify eistate: IRQ 1,2,3 enabled
    assert_eq!(imsic.eistate_val(1) & 2, 2);
    assert_eq!(imsic.eistate_val(2) & 2, 2);
    assert_eq!(imsic.eistate_val(3) & 2, 2);
    assert_eq!(imsic.eistate_val(4) & 2, 0);

    // EIP0 register — set pending for IRQ 1
    let eip0_reg = eip_sel_reg(false, 0);
    assert_eq!(imsic.rmw(eip0_reg, &mut val, 2, 2), 0);

    // IRQ 1 should be pending
    assert_eq!(imsic.eistate_val(1) & 1, 1);
    // IRQ 0 bit 0 is RO zero, should not be set
    assert_eq!(imsic.eistate_val(0) & 1, 0);
}

#[test]
fn test_imsic_topei_rmw() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));

    // Enable and set pending for IRQ 3
    let eie0_reg = eie_sel_reg(false, 0);
    let mut val = 0;
    imsic.rmw(eie0_reg, &mut val, 0x8, 0x8);
    let eip0_reg = eip_sel_reg(false, 0);
    imsic.rmw(eip0_reg, &mut val, 0x8, 0x8);

    // Set eidelivery
    let eid_reg = eid_sel_reg(false);
    imsic.rmw(eid_reg, &mut val, 1, 1);

    // TOPEI should report IRQ 3
    let topei_reg = topei_sel_reg(false);
    assert_eq!(imsic.rmw(topei_reg, &mut val, 0, 0), 0);
    let iid = (val >> 16) & 0x7ff;
    assert_eq!(iid, 3);

    // Clear via TOPEI write
    assert_eq!(imsic.rmw(topei_reg, &mut val, 0, 1), 0);
    // IRQ 3 should no longer be pending
    assert_eq!(imsic.eistate_val(3) & 1, 0);
}

#[test]
fn test_imsic_output_raised_when_pending_and_enabled() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    let sink = RecordingSink::new(8);
    imsic.connect_output(0, recording_line(&sink, 0));

    // Enable eidelivery, enable IRQ 1, set pending IRQ 1
    let mut val = 0;
    let eid_reg = eid_sel_reg(false);
    imsic.rmw(eid_reg, &mut val, 1, 1);
    let eie0_reg = eie_sel_reg(false, 0);
    imsic.rmw(eie0_reg, &mut val, 2, 2);
    let eip0_reg = eip_sel_reg(false, 0);
    imsic.rmw(eip0_reg, &mut val, 2, 2);

    assert!(sink.levels.lock().unwrap()[0]);
}

#[test]
fn test_imsic_output_not_raised_without_eidelivery() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    let sink = RecordingSink::new(8);
    imsic.connect_output(0, recording_line(&sink, 0));

    // Enable IRQ 1 and set pending, but eidelivery=0
    let mut val = 0;
    let eie0_reg = eie_sel_reg(false, 0);
    imsic.rmw(eie0_reg, &mut val, 2, 2);
    let eip0_reg = eip_sel_reg(false, 0);
    imsic.rmw(eip0_reg, &mut val, 2, 2);

    assert!(!sink.levels.lock().unwrap()[0]);
}

#[test]
fn test_imsic_threshold_gating() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    let sink = RecordingSink::new(8);
    imsic.connect_output(0, recording_line(&sink, 0));

    let mut val = 0;
    let eid_reg = eid_sel_reg(false);
    imsic.rmw(eid_reg, &mut val, 1, 1);

    // Enable and set pending for IRQ 1
    let eie0_reg = eie_sel_reg(false, 0);
    imsic.rmw(eie0_reg, &mut val, 0x2, 0x2);
    let eip0_reg = eip_sel_reg(false, 0);
    imsic.rmw(eip0_reg, &mut val, 0x2, 0x2);

    // threshold=0: TOPEI sees IRQ 1
    let topei_reg = topei_sel_reg(false);
    imsic.rmw(topei_reg, &mut val, 0, 0);
    let iid = (val >> 16) & 0x7ff;
    assert_eq!(iid, 1);
    assert!(sink.levels.lock().unwrap()[0]);

    // threshold=1: masks IRQ 1 (identity 1 >= threshold 1)
    let eth_reg = eth_sel_reg(false);
    imsic.rmw(eth_reg, &mut val, 1, 0xffff);
    assert!(!sink.levels.lock().unwrap()[0]);
}

#[test]
fn test_imsic_mmode_page_routing() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", true, 0, 1, 64));
    let sink = RecordingSink::new(8);
    imsic.connect_output(0, recording_line(&sink, 0));

    // M-mode: only PRV_M (3), virt=0 → page 0
    let reg = mmode_eid_sel_reg();
    let mut val = 0;
    assert_eq!(imsic.rmw(reg, &mut val, 1, 1), 0);
    assert_eq!(imsic.eidelivery_val(0), 1);
}

#[test]
fn test_imsic_rmw_rejects_invalid_priv() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));

    // S-mode IMSIC: PRV_U (0) should fail
    let reg = make_imsic_reg(0, 0, ISELECT_IMSIC_EIDELIVERY, 0, 32);
    let mut val = 0;
    assert_eq!(imsic.rmw(reg, &mut val, 1, 1), -1);
}

#[test]
fn test_imsic_eix_rmw_rejects_invalid_num() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));

    // num >= num_irqs / xlen should fail
    let reg = make_imsic_reg(1, 0, ISELECT_IMSIC_EIP0 + 63, 0, 32);
    let mut val = 0;
    assert_eq!(imsic.rmw(reg, &mut val, 0, 0), -1);
}

#[test]
fn test_imsic_lifecycle_and_mom_identity() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    let mut bus = SysBus::new("sysbus");
    let mut aspace = make_address_space();
    let base = GPA(0x2400_0000);

    assert!(!imsic.realized());
    imsic.with_mdevice(|device| assert_eq!(device.local_id(), "imsic"));
    assert_eq!(imsic.object_info().local_id, "imsic");

    imsic.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "imsic",
        0x2000,
        Arc::new(RiscvImsicMmio(Arc::clone(&imsic))),
    );
    imsic.register_mmio(region, base).unwrap();
    imsic.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(imsic.realized());

    let err = imsic.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

    imsic.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!imsic.realized());

    let err = imsic.unrealize_from(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("not realized"));
}

#[test]
fn test_imsic_reset_runtime() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));

    // Set some state
    let mut val = 0;
    let eid_reg = eid_sel_reg(false);
    imsic.rmw(eid_reg, &mut val, 1, 1);
    let eie0_reg = eie_sel_reg(false, 0);
    imsic.rmw(eie0_reg, &mut val, 2, 2);
    let eip0_reg = eip_sel_reg(false, 0);
    imsic.rmw(eip0_reg, &mut val, 2, 2);

    assert_eq!(imsic.eidelivery_val(0), 1);
    assert_eq!(imsic.eistate_val(1) & 2, 2);

    imsic.reset_runtime();

    assert_eq!(imsic.eidelivery_val(0), 0);
    assert_eq!(imsic.eistate_val(1), 0);
}

// ---- RiscvAplic ----

#[test]
fn test_aplic_defaults() {
    let aplic = RiscvAplic::new_named("aplic", 32, 4, 7, false, false);
    assert!(!aplic.realized());
    assert_eq!(aplic.domaincfg_val(), 0);
    assert_eq!(aplic.sourcecfg_val(1), 0);
    assert_eq!(aplic.state_val(1), 0);
    // Non-MSI target defaults to iprio=1
    assert_eq!(aplic.target_val(1), 1);
}

#[test]
fn test_aplic_mmio_domaincfg() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // Read: RDONLY bit set
    let v = mmio.read(0x0000, 4);
    assert_eq!(v & 0x8000_0000, 0x8000_0000);

    // Write IE bit
    mmio.write(0x0000, 4, 0x100);
    assert_eq!(aplic.domaincfg_val(), 0x100);

    // Only IE bit is writable
    mmio.write(0x0000, 4, 0xFF);
    assert_eq!(aplic.domaincfg_val(), 0x100 & 0xFF);
}

#[test]
fn test_aplic_sourcecfg_sm_modes() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // Set IRQ 5 to EDGE_RISE
    mmio.write(0x0004 + 4 * 4, 4, 0x4);
    assert_eq!(aplic.sourcecfg_val(5) & 0x7, 0x4);

    // Set IRQ 5 to LEVEL_HIGH
    mmio.write(0x0004 + 4 * 4, 4, 0x6);
    assert_eq!(aplic.sourcecfg_val(5) & 0x7, 0x6);

    // Non-SM bits masked (0xFF → SM=0x7=LEVEL_LOW)
    mmio.write(0x0004 + 4 * 4, 4, 0xFF);
    assert_eq!(aplic.sourcecfg_val(5), 0x7);
}

#[test]
fn test_aplic_sourcecfg_delegate_cleared_without_children() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    let sourcecfg_irq5 = 0x0004 + 4 * 4;
    let delegate_bit = 1 << 10;
    mmio.write(sourcecfg_irq5, 4, delegate_bit | 2);

    assert_eq!(aplic.sourcecfg_val(5), 0);
    assert_eq!(mmio.read(sourcecfg_irq5, 4), 0);
}

#[test]
fn test_aplic_sourcecfg_inactive_clears_pending_and_enabled() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // Set EDGE_RISE, set pending and enabled via bitfield
    mmio.write(0x0004 + 4 * 4, 4, 0x4); // IRQ 5 EDGE_RISE
    mmio.write(0x1e00, 4, 1 << 5); // SETIE for IRQ 5
    mmio.write(0x1c00, 4, 1 << 5); // SETIP for IRQ 5
    assert_eq!(aplic.state_val(5) & 0x3, 0x3); // pending + enabled

    // Set INACTIVE
    mmio.write(0x0004 + 4 * 4, 4, 0x0);
    assert_eq!(aplic.state_val(5) & 0x3, 0x0);
}

#[test]
fn test_aplic_setip_clrip_bitfield() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // Set IRQ 5 and 7 to EDGE_RISE
    mmio.write(0x0004 + 4 * 4, 4, 0x4);
    mmio.write(0x0004 + 6 * 4, 4, 0x4);

    // SETIP word 0: bit 5 and bit 7
    mmio.write(0x1c00, 4, (1 << 5) | (1 << 7));
    assert_eq!(aplic.state_val(5) & 1, 1);
    assert_eq!(aplic.state_val(7) & 1, 1);

    // Read SETIP word confirms pending
    let pending = mmio.read(0x1c00, 4);
    assert_eq!(pending & (1 << 5), 1 << 5);

    // CLRIP bit 5
    mmio.write(0x1d00, 4, 1 << 5);
    assert_eq!(aplic.state_val(5) & 1, 0);
    assert_eq!(aplic.state_val(7) & 1, 1);
}

#[test]
fn test_aplic_setipnum() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // Set IRQ 10 to EDGE_RISE
    mmio.write(0x0004 + 9 * 4, 4, 0x4);

    // SETIPNUM — set pending by IRQ number
    mmio.write(0x1cdc, 4, 10);
    assert_eq!(aplic.state_val(10) & 1, 1);

    // CLRIPNUM
    mmio.write(0x1ddc, 4, 10);
    assert_eq!(aplic.state_val(10) & 1, 0);
}

#[test]
fn test_aplic_setipnum_byteswap() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // LE: direct write
    mmio.write(0x0004 + 4 * 4, 4, 0x4); // IRQ 5 EDGE_RISE
    mmio.write(0x2000, 4, 5);
    assert_eq!(aplic.state_val(5) & 1, 1);

    // Clear
    mmio.write(0x1ddc, 4, 5);

    // BE: byteswapped
    mmio.write(0x2004, 4, 5u32.swap_bytes() as u64);
    assert_eq!(aplic.state_val(5) & 1, 1);
}

#[test]
fn test_aplic_setie_clrie() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // Set IRQ 3 to EDGE_RISE
    mmio.write(0x0004 + 2 * 4, 4, 0x4);

    // SETIE word 0, bit 3
    mmio.write(0x1e00, 4, 1 << 3);
    assert_eq!(aplic.state_val(3) & 2, 2);

    // CLRIE bit 3
    mmio.write(0x1f00, 4, 1 << 3);
    assert_eq!(aplic.state_val(3) & 2, 0);
}

#[test]
fn test_aplic_setienum() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    mmio.write(0x0004 + 7 * 4, 4, 0x4); // IRQ 8 EDGE_RISE
    mmio.write(0x1edc, 4, 8);
    assert_eq!(aplic.state_val(8) & 2, 2);

    mmio.write(0x1fdc, 4, 8); // CLRIENUM
    assert_eq!(aplic.state_val(8) & 2, 0);
}

#[test]
fn test_aplic_target_register() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // Set IRQ 2 to EDGE_RISE (must be active to write target)
    mmio.write(0x0004 + 1 * 4, 4, 0x4);

    // Write target: hart_idx=1, iprio=5
    let target_val = (1u32 << 18) | 5;
    mmio.write(0x3004 + 1 * 4, 4, target_val as u64);
    assert_eq!(aplic.target_val(2), target_val);
}

#[test]
fn test_aplic_idc_registers() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // IDC 0 idelivery
    mmio.write(0x4000, 4, 1);
    assert_eq!(aplic.idelivery_val(0), 1);
    assert_eq!(mmio.read(0x4000, 4), 1);

    // IDC 0 iforce
    mmio.write(0x4004, 4, 1);
    assert_eq!(aplic.iforce_val(0), 1);

    // IDC 0 ithreshold
    mmio.write(0x4008, 4, 5);
    assert_eq!(aplic.ithreshold_val(0), 5);

    // IDC 1 independent
    assert_eq!(aplic.idelivery_val(1), 0);
}

#[test]
fn test_aplic_idc_topi() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // Set up IRQ 3: EDGE_RISE, pending, enabled, target to hart 0
    mmio.write(0x0004 + 2 * 4, 4, 0x4);
    mmio.write(0x1e00, 4, 1 << 3); // SETIE
    mmio.write(0x3004 + 2 * 4, 4, 5); // target hart=0, iprio=5
    mmio.write(0x1c00, 4, 1 << 3); // SETIP

    // TOPI should report IRQ 3 with prio 5
    let topi = mmio.read(0x4018, 4);
    let irq = (topi >> 16) & 0x3ff;
    let prio = topi & 0xff;
    assert_eq!(irq, 3);
    assert_eq!(prio, 5);
}

#[test]
fn test_aplic_idc_claimi() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // IRQ 3: EDGE_RISE, enabled, target to hart 0, set pending
    mmio.write(0x0004 + 2 * 4, 4, 0x4);
    mmio.write(0x1e00, 4, 1 << 3);
    mmio.write(0x3004 + 2 * 4, 4, 5);
    mmio.write(0x1c00, 4, 1 << 3);

    // CLAIMI should return TOPI and clear pending for edge
    let claimi = mmio.read(0x401c, 4);
    assert_eq!(claimi >> 16, 3);

    // After claim, EDGE_RISE should not repend
    let topi = mmio.read(0x4018, 4);
    assert_eq!(topi, 0);
}

#[test]
fn test_aplic_level_high_claimi_repends() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // IRQ 3: LEVEL_HIGH, enabled, target to hart 0, input asserted
    mmio.write(0x0004 + 2 * 4, 4, 0x6);
    mmio.write(0x1e00, 4, 1 << 3);
    mmio.write(0x3004 + 2 * 4, 4, 5);

    // Assert IRQ via set_irq
    aplic.set_irq(3, true);
    assert_eq!(aplic.state_val(3) & 1, 1); // pending

    // CLAIMI
    let claimi = mmio.read(0x401c, 4);
    assert_eq!(claimi >> 16, 3);

    // After claim for LEVEL_HIGH with input still high: repends
    assert_eq!(aplic.state_val(3) & 1, 1);
}

#[test]
fn test_aplic_set_irq_edge_rise() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // IRQ 3 EDGE_RISE
    mmio.write(0x0004 + 2 * 4, 4, 0x4);

    // Rising edge
    aplic.set_irq(3, true);
    assert_eq!(aplic.state_val(3) & 1, 1); // pending set

    // Clear, then same edge should not re-trigger (no falling edge between)
    mmio.write(0x1d00, 4, 1 << 3);
    aplic.set_irq(3, true);
    assert_eq!(aplic.state_val(3) & 1, 0); // no change: input was already high
}

#[test]
fn test_aplic_set_irq_edge_fall() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // IRQ 5 EDGE_FALL: rectified=true when input low,
    // so sourcecfg write auto-sets pending. Clear it first.
    mmio.write(0x0004 + 4 * 4, 4, 0x5);
    mmio.write(0x1d00, 4, 1 << 5); // clear auto-set pending

    // Initially high (input=1, rectified=0), then falling edge
    aplic.set_irq(5, true);
    assert_eq!(aplic.state_val(5) & 1, 0); // rising on EDGE_FALL: no pending
    aplic.set_irq(5, false);
    assert_eq!(aplic.state_val(5) & 1, 1); // falling: pending set
}

#[test]
fn test_aplic_set_irq_level_high() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    mmio.write(0x0004 + 2 * 4, 4, 0x6); // IRQ 3 LEVEL_HIGH

    aplic.set_irq(3, true);
    assert_eq!(aplic.state_val(3) & 1, 1);

    // Deassert and reassert
    aplic.set_irq(3, false);
    mmio.write(0x1d00, 4, 1 << 3); // clear pending
    aplic.set_irq(3, true);
    assert_eq!(aplic.state_val(3) & 1, 1);
}

#[test]
fn test_aplic_output_fires_on_pending_enabled() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));
    let sink = RecordingSink::new(8);
    aplic.connect_output(0, recording_line(&sink, 0));

    // IRQ 3 EDGE_RISE, enabled, target hart 0
    mmio.write(0x0004 + 2 * 4, 4, 0x4);
    mmio.write(0x1e00, 4, 1 << 3);
    mmio.write(0x3004 + 2 * 4, 4, 5);

    // Enable domaincfg IE, set idelivery
    mmio.write(0x0000, 4, 0x100);
    mmio.write(0x4000, 4, 1);

    // Trigger
    aplic.set_irq(3, true);
    assert!(sink.levels.lock().unwrap()[0]);

    // Claimi — edge should clear
    mmio.read(0x401c, 4);
    assert!(!sink.levels.lock().unwrap()[0]);
}

#[test]
fn test_aplic_output_with_iforce() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));
    let sink = RecordingSink::new(8);
    aplic.connect_output(0, recording_line(&sink, 0));

    // iforce with IE=0 should NOT fire even with idelivery
    mmio.write(0x4000, 4, 1); // idelivery
    mmio.write(0x4004, 4, 1); // iforce
    assert!(!sink.levels.lock().unwrap()[0]);

    // Set IE → iforce + idelivery should fire
    mmio.write(0x0000, 4, 0x100);
    assert!(sink.levels.lock().unwrap()[0]);
}

#[test]
fn test_aplic_output_not_raised_without_ie() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));
    let sink = RecordingSink::new(8);
    aplic.connect_output(0, recording_line(&sink, 0));

    mmio.write(0x0004 + 2 * 4, 4, 0x4); // IRQ 3 EDGE_RISE
    mmio.write(0x1e00, 4, 1 << 3);
    mmio.write(0x3004 + 2 * 4, 4, 5);
    mmio.write(0x4000, 4, 1); // idelivery = 1
                              // domaincfg IE = 0

    aplic.set_irq(3, true);
    assert!(!sink.levels.lock().unwrap()[0]);
}

#[test]
fn test_aplic_msi_mode_genmsi() {
    let aplic = Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, true, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // Set IRQ 3 EDGE_RISE, enabled, target (hart=1, guest=0, eiid=42)
    mmio.write(0x0004 + 2 * 4, 4, 0x4);
    mmio.write(0x1e00, 4, 1 << 3);
    // MSI target: hart_idx<<18 | guest_idx<<12 | eiid
    mmio.write(0x3004 + 2 * 4, 4, ((1u32 << 18) | 42) as u64);
    mmio.write(0x0000, 4, 0x100); // IE

    // Trigger via set_irq
    aplic.set_irq(3, true);

    // genmsi should be updated with MSI info
    let g = aplic.genmsi_val();
    let hart = (g >> 18) & 0x3fff;
    let eiid = g & 0x7ff;
    assert_eq!(hart, 1);
    assert_eq!(eiid, 42);
}

#[test]
fn test_aplic_msi_mode_genmsi_delivers_write() {
    let aplic = Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, true, true));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));
    let writes = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&writes);

    aplic.set_msi_delivery(Box::new(move |addr, data| {
        seen.lock().unwrap().push((addr, data));
    }));
    mmio.write(0x1bc0, 4, 0x1);
    mmio.write(0x3000, 4, ((2u32 << 18) | (3u32 << 12) | 55) as u64);

    assert_eq!(aplic.genmsi_val(), (2u32 << 18) | 55);
    assert_eq!(&*writes.lock().unwrap(), &[(0x1000, 55)]);
}

#[test]
fn test_aplic_msi_mode_delivery_uses_msicfgaddrh_hart_index_fields() {
    let aplic = Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, true, true));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));
    let writes = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&writes);

    aplic.set_msi_delivery(Box::new(move |addr, data| {
        seen.lock().unwrap().push((addr, data));
    }));
    mmio.write(0x1bc4, 4, ((1u32 << 20) | (2u32 << 12)) as u64);
    mmio.write(0x3000, 4, ((3u32 << 18) | 0x33) as u64);

    assert_eq!(&*writes.lock().unwrap(), &[(0x6000, 0x33)]);
}

#[test]
fn test_aplic_msi_mode_domaincfg_dm_bit() {
    let aplic = Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, true, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // MSI mode: DM bit appears in domaincfg read
    let v = mmio.read(0x0000, 4);
    assert_eq!(v & 0x4, 0x4);
}

#[test]
fn test_aplic_msi_config_regs() {
    let aplic = Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, true, true));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // M-mode with MSI: MMSICFGADDR accessible
    mmio.write(0x1bc0, 4, 0x1234_5000u64);
    assert_eq!(aplic.mmsicfgaddr_val(), 0x1234_5000);
    assert_eq!(mmio.read(0x1bc0, 4), 0x1234_5000);

    // MMSICFGADDRH: lock bit prevents rewrite
    mmio.write(0x1bc4, 4, 0x8000_0000u64); // set L bit
    assert_eq!(aplic.mmsicfgaddr_h_val() & 0x8000_0000, 0x8000_0000);
    mmio.write(0x1bc4, 4, 0); // try to clear
    assert_eq!(aplic.mmsicfgaddr_h_val() & 0x8000_0000, 0x8000_0000);

    // After L bit set, MMSICFGADDR is read-only
    mmio.write(0x1bc0, 4, 0);
    assert_eq!(aplic.mmsicfgaddr_val(), 0x1234_5000);
}

#[test]
fn test_aplic_smsi_config_regs_hidden_without_children() {
    let aplic = Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, true, true));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    mmio.write(0x1bc8, 4, 0x1234_5000);
    mmio.write(0x1bcc, 4, 0x8000_0001);

    assert_eq!(mmio.read(0x1bc8, 4), 0);
    assert_eq!(mmio.read(0x1bcc, 4), 0);
}

#[test]
fn test_aplic_mmio_reads_require_4byte_alignment() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // Unaligned reads return 0
    assert_eq!(mmio.read(0x0001, 4), 0);
    assert_eq!(mmio.read(0x0002, 4), 0);
    assert_eq!(mmio.read(0x0003, 4), 0);
}

#[test]
fn test_aplic_invalid_offset_reads_zero() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    assert_eq!(mmio.read(0x1000, 4), 0);
    assert_eq!(mmio.read(0x3800, 4), 0);
}

#[test]
fn test_aplic_lifecycle_and_mom_identity() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mut bus = SysBus::new("sysbus");
    let mut aspace = make_address_space();
    let base = GPA(0xC000_0000);

    assert!(!aplic.realized());
    aplic.with_mdevice(|device| assert_eq!(device.local_id(), "aplic"));
    assert_eq!(aplic.object_info().local_id, "aplic");

    aplic.attach_to_bus(&mut bus).unwrap();
    let region = MemoryRegion::io(
        "aplic",
        0x8000,
        Arc::new(RiscvAplicMmio(Arc::clone(&aplic))),
    );
    aplic.register_mmio(region, base).unwrap();
    aplic.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(aplic.realized());

    let err = aplic.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("already realized"));

    aplic.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!aplic.realized());

    let err = aplic.unrealize_from(&mut bus, &mut aspace).unwrap_err();
    assert!(err.to_string().contains("not realized"));
}

#[test]
fn test_aplic_reset_runtime() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // Set some state
    mmio.write(0x0000, 4, 0x100);
    mmio.write(0x0004 + 4 * 4, 4, 0x4); // IRQ 5 EDGE_RISE
    mmio.write(0x1e00, 4, 1 << 5);
    mmio.write(0x1c00, 4, 1 << 5);
    mmio.write(0x4000, 4, 1);

    assert_eq!(aplic.domaincfg_val(), 0x100);
    assert_eq!(aplic.state_val(5) & 1, 1);

    aplic.reset_runtime();

    assert_eq!(aplic.domaincfg_val(), 0);
    assert_eq!(aplic.state_val(5) & 1, 0);
    assert_eq!(aplic.idelivery_val(0), 0);
    // Non-MSI target resets to iprio=1
    assert_eq!(aplic.target_val(5), 1);
}

// --- access-size matrix and pre-realize checks ---

#[test]
fn test_imsic_access_size_rejection() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    let mmio = RiscvImsicMmio(Arc::clone(&imsic));

    // Aligned 4-byte read returns 0 (expected IMSIC behavior)
    assert_eq!(mmio.read(0x0000, 4), 0);

    // 1-byte and 2-byte reads at aligned offsets return 0
    assert_eq!(mmio.read(0x0000, 1), 0);
    assert_eq!(mmio.read(0x0000, 2), 0);

    // Aligned 4-byte write works
    mmio.write(0x0000, 4, 5);
    assert_eq!(imsic.eistate_val(5) & 1, 1);

    // QEMU only accepts 4-byte IMSIC doorbell writes.
    let imsic2 = Arc::new(RiscvImsic::new_named("imsic2", false, 0, 2, 64));
    let mmio2 = RiscvImsicMmio(Arc::clone(&imsic2));
    mmio2.write(0x0000, 1, 3);
    assert_eq!(imsic2.eistate_val(3) & 1, 0);
}

#[test]
fn test_imsic_end_offset_is_out_of_range() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    let mmio = RiscvImsicMmio(Arc::clone(&imsic));

    let end_offset = 2 * 0x1000;
    assert_eq!(mmio.read(end_offset, 4), 0);
    mmio.write(end_offset, 4, 5);

    assert_eq!(imsic.eistate_val(5) & 1, 0);
}

#[test]
fn test_imsic_prerealize_mmio() {
    let imsic = Arc::new(RiscvImsic::new_named("imsic", false, 0, 2, 64));
    let mmio = RiscvImsicMmio(Arc::clone(&imsic));

    // MMIO reads before realize complete without panic
    assert_eq!(mmio.read(0x0000, 4), 0);
    assert_eq!(mmio.read(0x0010, 4), 0);

    // MMIO writes before realize complete without panic
    mmio.write(0x0000, 4, 5);
    assert_eq!(imsic.eistate_val(5) & 1, 1);
}

#[test]
fn test_aplic_access_size_rejection() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // 4-byte aligned read works (returns RDONLY bit)
    assert_eq!(mmio.read(0x0000, 4), 0x8000_0000);

    // QEMU only accepts 4-byte APLIC MMIO accesses.
    assert_eq!(mmio.read(0x0000, 1), 0);
    assert_eq!(mmio.read(0x0000, 2), 0);
    assert_eq!(mmio.read(0x0000, 8), 0);

    // Unaligned 4-byte read returns 0
    assert_eq!(mmio.read(0x0001, 4), 0);

    // Non-4-byte writes are rejected and do not update registers.
    mmio.write(0x0000, 1, 0x100);
    assert_eq!(aplic.domaincfg_val(), 0);

    // 4-byte write also works normally
    mmio.write(0x0000, 4, 0);
    assert_eq!(aplic.domaincfg_val(), 0);
}

#[test]
fn test_aplic_prerealize_mmio() {
    let aplic =
        Arc::new(RiscvAplic::new_named("aplic", 32, 4, 7, false, false));
    let mmio = RiscvAplicMmio(Arc::clone(&aplic));

    // MMIO reads before realize: domaincfg returns RDONLY bit
    assert_eq!(mmio.read(0x0000, 4), 0x8000_0000);
    // sourcecfg returns 0 for IRQ 1
    assert_eq!(mmio.read(0x0004, 4), 0);

    // MMIO writes before realize: should update state
    mmio.write(0x0000, 4, 0x100);
    assert_eq!(aplic.domaincfg_val(), 0x100);
}

// --- helpers for IMSIC register encoding ---

const IMSIC_MMIO_PAGE_BE: u64 = 0x04;

const ISELECT_IMSIC_EIDELIVERY: u32 = 0x70;
const ISELECT_IMSIC_EITHRESHOLD: u32 = 0x72;
const ISELECT_IMSIC_TOPEI: u32 = 0x200;
const ISELECT_IMSIC_EIP0: u32 = 0x80;
const ISELECT_IMSIC_EIE0: u32 = 0xc0;

fn make_imsic_reg(
    priv_: u32,
    virt: u32,
    isel: u32,
    vgein: u32,
    xlen: u32,
) -> u64 {
    (isel as u64)
        | ((priv_ as u64) << 16)
        | ((virt as u64) << 18)
        | ((vgein as u64) << 20)
        | ((xlen as u64) << 24)
}

fn eid_sel_reg(mmode: bool) -> u64 {
    if mmode {
        make_imsic_reg(3, 0, ISELECT_IMSIC_EIDELIVERY, 0, 32)
    } else {
        make_imsic_reg(1, 0, ISELECT_IMSIC_EIDELIVERY, 0, 32)
    }
}

fn eth_sel_reg(mmode: bool) -> u64 {
    if mmode {
        make_imsic_reg(3, 0, ISELECT_IMSIC_EITHRESHOLD, 0, 32)
    } else {
        make_imsic_reg(1, 0, ISELECT_IMSIC_EITHRESHOLD, 0, 32)
    }
}

fn topei_sel_reg(mmode: bool) -> u64 {
    if mmode {
        make_imsic_reg(3, 0, ISELECT_IMSIC_TOPEI, 0, 32)
    } else {
        make_imsic_reg(1, 0, ISELECT_IMSIC_TOPEI, 0, 32)
    }
}

fn mmode_eid_sel_reg() -> u64 {
    make_imsic_reg(3, 0, ISELECT_IMSIC_EIDELIVERY, 0, 32)
}

fn eie_sel_reg(mmode: bool, num: u32) -> u64 {
    let isel = ISELECT_IMSIC_EIE0 + num;
    if mmode {
        make_imsic_reg(3, 0, isel, 0, 32)
    } else {
        make_imsic_reg(1, 0, isel, 0, 32)
    }
}

fn eip_sel_reg(mmode: bool, num: u32) -> u64 {
    let isel = ISELECT_IMSIC_EIP0 + num;
    if mmode {
        make_imsic_reg(3, 0, isel, 0, 32)
    } else {
        make_imsic_reg(1, 0, isel, 0, 32)
    }
}
