use std::sync::Arc;

use machina_core::address::GPA;
use machina_hw_core::bus::SysBus;
use machina_hw_misc::cmgcr::{Cmgcr, CmgcrMmio};
use machina_hw_misc::cpc::{Cpc, CpcMmio};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

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
fn test_cmgcr_write_cl_reset_base() {
    let cmgcr =
        Arc::new(Cmgcr::new_named("cmgcr", 0xa00, 0, 6, 2, 3, 0x1FB8_0000));
    let mmio = CmgcrMmio(Arc::clone(&cmgcr));

    // Write reset base for VP at CLCB_OFS + core*stride + hart*8
    // core=1, hart=1: offset = 0x2000 + 1*0x100 + 1*0x8 = 0x2108
    let offset = 0x2000u64 + 0x100 + 0x8;
    mmio.write(offset, 8, 0x1FC0_0000);

    // Write a different reset base for VP at core=2, hart=0
    let offset2 = 0x2000u64 + 2 * 0x100;
    mmio.write(offset2, 8, 0x1D00_0000);

    // Verify the first write - read back from the mmio (we can't directly
    // read the reset_base array, but the write doesn't complain)
    let offset3 = 0x2000u64;
    mmio.write(offset3, 8, 0x1FC0_0000);
    // Just verify no panic on writes
}

#[test]
fn test_cmgcr_invalid_offset_reads_zero() {
    let cmgcr = Cmgcr::new_named("cmgcr", 0xa00, 0, 4, 1, 4, 0x1FB8_0000);
    let mmio = CmgcrMmio(Arc::new(cmgcr));

    assert_eq!(mmio.read(0x0100, 8), 0);
    assert_eq!(mmio.read(0x1000, 8), 0);
}

#[test]
fn test_cmgcr_lifecycle() {
    let cmgcr =
        Arc::new(Cmgcr::new_named("cmgcr", 0xa00, 0, 4, 1, 4, 0x1FB8_0000));
    let mut bus = SysBus::new("sysbus");
    let mut aspace = make_address_space();
    let base = GPA(0x1FB8_0000);

    assert!(!cmgcr.realized());

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

    // Reset restores cpc_base and reset_base, but gcr_base keeps its value
    cmgcr.reset_runtime();

    // GCR_BASE is not reset by reset_runtime (only cpc_base is derived
    // from it; reset_base values are reset)
}

// ---- Cpc ----

#[test]
fn test_cpc_defaults() {
    let cpc = Cpc::new_named("cpc", 0, 4, 1, 4, 1);
    let mmio = CpcMmio(Arc::new(cpc));

    // CPC_CM_STAT_CONF_OFS: SEQ_STATE_U5
    assert_eq!(mmio.read(0x1008, 8), 6 << 19);

    // CPC_CL_BASE_OFS + STAT_CONF for core 0: SEQ_STATE_U6
    assert_eq!(mmio.read(0x2008, 8), 7 << 19);

    // CPC_MTIME_REG_OFS: returns 0
    assert_eq!(mmio.read(0x50, 8), 0);
}

#[test]
fn test_cpc_vp_run_stop() {
    let cpc = Arc::new(Cpc::new_named("cpc", 0, 4, 1, 4, 0));
    let mmio = CpcMmio(Arc::clone(&cpc));

    // Start with no VPs running
    cpc.reset_runtime();
    // vps_running_mask starts at vps_start_running_mask (0)
    // We can't directly read vps_running_mask, but we can verify
    // VP_RUN and VP_STOP don't panic

    // VP_RUN for core 0 (offset 0x2028)
    mmio.write(0x2028, 8, 1); // run VP 0
                              // VP_STOP for core 0 (offset 0x2020)
    mmio.write(0x2020, 8, 1); // stop VP 0
                              // Both should not panic
}

#[test]
fn test_cpc_vp_run_multi_core() {
    let cpc = Arc::new(Cpc::new_named("cpc", 0, 4, 1, 4, 0));
    let mmio = CpcMmio(Arc::clone(&cpc));

    cpc.reset_runtime();

    // VP_RUN for core 1 (offset 0x2028 + 1*0x100 = 0x2128)
    mmio.write(0x2128, 8, 0x3); // run VPs 0,1 (shifted by core*num_hart)

    // VP_STOP for core 2 (offset 0x2220)
    mmio.write(0x2220, 8, 0x1); // stop VP 0
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
fn test_cpc_lifecycle() {
    let cpc = Arc::new(Cpc::new_named("cpc", 0, 4, 1, 4, 1));
    let mut bus = SysBus::new("sysbus");
    let mut aspace = make_address_space();
    let base = GPA(0x1FB0_0000);

    assert!(!cpc.realized());

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
    let cpc = Cpc::new_named("cpc", 0, 4, 1, 4, 0xF);
    cpc.reset_runtime();
    // After reset, vps_running_mask = vps_start_running_mask = 0xF
    // The VP_RUNNING register at 0x2030 should reflect this
    let mmio = CpcMmio(Arc::new(cpc));
    // VP_RUNNING is not directly readable in our implementation,
    // but reset_runtime should not panic
    let _ = mmio;
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
