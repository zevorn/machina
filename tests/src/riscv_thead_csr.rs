use machina_guest_riscv::riscv::cpu::RiscvCpu;
use machina_guest_riscv::riscv::cpu_model::RiscvCpuModel;
use machina_guest_riscv::riscv::csr::PrivLevel;
use machina_guest_riscv::riscv::vendor::thead::{
    CSR_TH_MHCR, CSR_TH_MXSTATUS, CSR_TH_SXSTATUS, TH_STATUS_THEADISAEE,
    TH_STATUS_UCME,
};

#[test]
fn c908_reads_thead_status_csrs_with_maee_clear() {
    let cpu = RiscvCpu::new_with_model(RiscvCpuModel::TheadC908);
    let mx = cpu
        .csr
        .read_for_profile(CSR_TH_MXSTATUS, PrivLevel::Machine, cpu.profile())
        .unwrap();
    let sx = cpu
        .csr
        .read_for_profile(CSR_TH_SXSTATUS, PrivLevel::Supervisor, cpu.profile())
        .unwrap();
    assert_eq!(mx, TH_STATUS_UCME | TH_STATUS_THEADISAEE);
    assert_eq!(sx, TH_STATUS_UCME | TH_STATUS_THEADISAEE);
}

#[test]
fn c908_unimplemented_thead_csrs_read_zero() {
    let cpu = RiscvCpu::new_with_model(RiscvCpuModel::TheadC908);
    let mhcr = cpu
        .csr
        .read_for_profile(CSR_TH_MHCR, PrivLevel::Machine, cpu.profile())
        .unwrap();
    assert_eq!(mhcr, 0);
}

#[test]
fn generic_cpu_rejects_thead_csrs() {
    let cpu = RiscvCpu::new();
    assert!(cpu
        .csr
        .read_for_profile(CSR_TH_MXSTATUS, PrivLevel::Machine, cpu.profile())
        .is_err());
    assert!(cpu
        .csr
        .read_for_profile(CSR_TH_MHCR, PrivLevel::Machine, cpu.profile())
        .is_err());
}
