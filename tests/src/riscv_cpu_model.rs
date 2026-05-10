use machina_guest_riscv::riscv::cpu::RiscvCpu;
use machina_guest_riscv::riscv::cpu_model::{
    RiscvCpuModel, RiscvVendor, THEAD_C908_MARCHID, THEAD_VENDOR_ID,
};
use machina_guest_riscv::riscv::csr::{
    PrivLevel, CSR_MARCHID, CSR_MVENDORID, CSR_SATP,
};

#[test]
fn c908_profile_has_qemu_identity() {
    let cpu = RiscvCpu::new_with_model(RiscvCpuModel::TheadC908);
    let profile = cpu.profile();
    assert_eq!(profile.name, "thead-c908");
    assert_eq!(profile.vendor, RiscvVendor::Thead);
    assert_eq!(profile.mvendorid, THEAD_VENDOR_ID);
    assert_eq!(profile.marchid, THEAD_C908_MARCHID);
    assert_eq!(profile.max_satp_mode, 9);
}

#[test]
fn generic_profile_remains_default() {
    let cpu = RiscvCpu::new();
    let profile = cpu.profile();
    assert_eq!(profile.name, "rv64");
    assert_eq!(profile.vendor, RiscvVendor::Generic);
    assert_eq!(profile.mvendorid, 0);
    assert_eq!(profile.marchid, 0);
    assert_eq!(profile.max_satp_mode, 8);
}

#[test]
fn generic_satp_warl_rejects_reserved_and_unsupported_modes() {
    let mut cpu = RiscvCpu::new();
    let sv39 = (8 << 60) | 0x1234;
    cpu.csr.write(CSR_SATP, sv39, PrivLevel::Machine).unwrap();
    assert_eq!(cpu.csr.satp, sv39);

    cpu.csr
        .write(CSR_SATP, 1 << 60, PrivLevel::Machine)
        .unwrap();
    assert_eq!(cpu.csr.satp, sv39);

    cpu.csr
        .write(CSR_SATP, 9 << 60, PrivLevel::Machine)
        .unwrap();
    assert_eq!(cpu.csr.satp, sv39);
}

#[test]
fn c908_profile_initializes_machine_id_csrs_and_sv48_satp_gate() {
    let mut cpu = RiscvCpu::new_with_model(RiscvCpuModel::TheadC908);
    assert_eq!(cpu.csr_read(CSR_MVENDORID), THEAD_VENDOR_ID);
    assert_eq!(cpu.csr_read(CSR_MARCHID), THEAD_C908_MARCHID);

    cpu.csr
        .write(CSR_SATP, 9 << 60, PrivLevel::Machine)
        .unwrap();
    assert_eq!(cpu.csr.satp >> 60, 9);
}

#[test]
fn c908_profile_sets_standard_and_thead_extension_flags() {
    let cpu = RiscvCpu::new_with_model(RiscvCpuModel::TheadC908);
    let cfg = cpu.profile().cfg;
    assert!(cfg.ext_zicsr);
    assert!(cfg.ext_zifencei);
    assert!(cfg.ext_zba);
    assert!(cfg.ext_zbb);
    assert!(cfg.ext_zbc);
    assert!(cfg.ext_zbs);
    assert!(cfg.ext_zicbom);
    assert!(cfg.ext_zicboz);
    assert!(cfg.ext_svpbmt);
    assert!(cfg.ext_ssvnapot);
    assert!(cfg.ext_svinval);
    assert!(cfg.ext_sstc);
    assert!(cfg.ext_xtheadba);
    assert!(cfg.ext_xtheadbb);
    assert!(cfg.ext_xtheadbs);
    assert!(cfg.ext_xtheadcmo);
    assert!(cfg.ext_xtheadfmv);
    assert!(cfg.ext_xtheadmaee);
    assert!(cfg.ext_xtheadsync);
}
