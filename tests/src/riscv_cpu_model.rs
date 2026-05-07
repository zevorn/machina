use machina_guest_riscv::riscv::cpu::RiscvCpu;
use machina_guest_riscv::riscv::cpu_model::{
    RiscvCpuModel, RiscvVendor, THEAD_C908_MARCHID, THEAD_VENDOR_ID,
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
