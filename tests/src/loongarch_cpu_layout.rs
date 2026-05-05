use machina_guest_loongarch::loongarch::cpu::{
    fpr_offset, gpr_offset, LoongArchCpu, BADV_OFFSET, CRMD_OFFSET,
    EENTRY_OFFSET, ERA_OFFSET, ESTAT_OFFSET, FCC_OFFSET, FCSR0_OFFSET,
    FPR_OFFSET, GPR_OFFSET, GUEST_BASE_CPU_OFFSET, GUEST_BASE_OFFSET,
    LLBCTL_OFFSET, LL_RES_ADDR_OFFSET, LL_RES_VAL_OFFSET, NEG_ALIGN_CPU_OFFSET,
    NUM_FPRS, NUM_GPRS, PC_OFFSET, RAM_BASE_OFFSET, RAM_END_OFFSET,
};

#[test]
fn task81_jit_global_offsets_match_cpu_layout() {
    let layout = LoongArchCpu::layout_offsets_for_tests();

    assert_eq!(GPR_OFFSET, layout.gpr);
    assert_eq!(PC_OFFSET, layout.pc);
    assert_eq!(GUEST_BASE_OFFSET, layout.guest_base);
    assert_eq!(FPR_OFFSET, layout.fpr);
    assert_eq!(FCSR0_OFFSET, layout.fcsr0);
    assert_eq!(FCC_OFFSET, layout.fcc);
    assert_eq!(CRMD_OFFSET, layout.crmd);
    assert_eq!(ESTAT_OFFSET, layout.estat);
    assert_eq!(ERA_OFFSET, layout.era);
    assert_eq!(BADV_OFFSET, layout.badv);
    assert_eq!(EENTRY_OFFSET, layout.eentry);
    assert_eq!(LLBCTL_OFFSET, layout.llbctl);
    assert_eq!(LL_RES_ADDR_OFFSET, layout.ll_res_addr);
    assert_eq!(LL_RES_VAL_OFFSET, layout.ll_res_val);
    assert_eq!(RAM_BASE_OFFSET, layout.ram_base);
    assert_eq!(RAM_END_OFFSET, layout.ram_end);
    assert_eq!(GUEST_BASE_CPU_OFFSET, layout.guest_base);
    assert_eq!(NEG_ALIGN_CPU_OFFSET, layout.neg_align);
}

#[test]
fn task81_register_array_offsets_are_contiguous() {
    assert_eq!(gpr_offset(0), GPR_OFFSET);
    assert_eq!(gpr_offset(NUM_GPRS - 1), GPR_OFFSET + 31 * 8);
    assert_eq!(fpr_offset(0), FPR_OFFSET);
    assert_eq!(fpr_offset(NUM_FPRS - 1), FPR_OFFSET + 31 * 8);
}
