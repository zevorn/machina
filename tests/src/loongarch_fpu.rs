use machina_guest_loongarch::loongarch::trans::helpers;

#[test]
fn fadd_s_basic() {
    let a = f32::to_bits(1.5);
    let b = f32::to_bits(2.5);
    let r = helpers::loongarch_helper_fadd_s(u64::from(a), u64::from(b));
    assert_eq!(f32::from_bits(r as u32), 4.0);
}

#[test]
fn fadd_d_basic() {
    let a = f64::to_bits(1.5);
    let b = f64::to_bits(2.5);
    let r = helpers::loongarch_helper_fadd_d(a, b);
    assert_eq!(f64::from_bits(r), 4.0);
}

#[test]
fn fmul_s_basic() {
    let a = f32::to_bits(3.0);
    let b = f32::to_bits(4.0);
    let r = helpers::loongarch_helper_fmul_s(u64::from(a), u64::from(b));
    assert_eq!(f32::from_bits(r as u32), 12.0);
}

#[test]
fn fdiv_d_basic() {
    let a = f64::to_bits(10.0);
    let b = f64::to_bits(2.0);
    let r = helpers::loongarch_helper_fdiv_d(a, b);
    assert_eq!(f64::from_bits(r), 5.0);
}

#[test]
fn fsqrt_d_basic() {
    let a = f64::to_bits(9.0);
    let r = helpers::loongarch_helper_fsqrt_d(a);
    assert_eq!(f64::from_bits(r), 3.0);
}

#[test]
fn fmadd_s_fused() {
    let a = f32::to_bits(2.0);
    let b = f32::to_bits(3.0);
    let c = f32::to_bits(1.0);
    let r = helpers::loongarch_helper_fmadd_s(
        u64::from(a),
        u64::from(b),
        u64::from(c),
    );
    assert_eq!(f32::from_bits(r as u32), 7.0); // 2*3+1
}

#[test]
fn fmsub_d_fused() {
    let a = f64::to_bits(5.0);
    let b = f64::to_bits(4.0);
    let c = f64::to_bits(3.0);
    let r = helpers::loongarch_helper_fmsub_d(a, b, c);
    assert_eq!(f64::from_bits(r), 17.0); // 5*4-3
}

#[test]
fn fcmp_ceq_s_equal() {
    let a = f32::to_bits(1.0);
    let r = helpers::loongarch_helper_fcmp_ceq_s(u64::from(a), u64::from(a));
    assert_eq!(r, 1);
}

#[test]
fn fcmp_clt_d_less() {
    let a = f64::to_bits(1.0);
    let b = f64::to_bits(2.0);
    assert_eq!(helpers::loongarch_helper_fcmp_clt_d(a, b), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_clt_d(b, a), 0);
}

#[test]
fn fcmp_cun_s_nan() {
    let nan = f32::to_bits(f32::NAN);
    let one = f32::to_bits(1.0);
    assert_eq!(
        helpers::loongarch_helper_fcmp_cun_s(u64::from(nan), u64::from(one)),
        1,
    );
    assert_eq!(
        helpers::loongarch_helper_fcmp_cun_s(u64::from(one), u64::from(one)),
        0,
    );
}

#[test]
fn ffint_d_w_positive() {
    let r = helpers::loongarch_helper_ffint_d_w(42);
    assert_eq!(f64::from_bits(r), 42.0);
}

#[test]
fn ftintrz_w_s_truncates() {
    let a = f32::to_bits(3.7);
    let r = helpers::loongarch_helper_ftintrz_w_s(u64::from(a));
    assert_eq!(r as i32, 3);
}

#[test]
fn fcvt_s_d_converts() {
    let a = f64::to_bits(2.5);
    let r = helpers::loongarch_helper_fcvt_s_d(a);
    assert_eq!(f32::from_bits(r as u32), 2.5);
}

#[test]
fn check_fpe_disabled_raises_fpd() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
    use machina_guest_loongarch::loongarch::csr::*;

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, 0); // FPE disabled
    cpu.set_pc(0x1000);
    cpu.csr_write(CSR_EENTRY, 0x9000_0000);

    let vec = unsafe { helpers::loongarch_helper_check_fpe(cpu.env_ptr()) };

    assert_ne!(vec, 0);
    assert_eq!(vec, 0x9000_0000);
    assert_eq!((cpu.csr_read(CSR_ESTAT) >> 16) & 0x3F, 0x0F);
}

#[test]
fn check_fpe_enabled_returns_zero() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
    use machina_guest_loongarch::loongarch::csr::*;

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);
    cpu.set_pc(0x2000);

    let vec = unsafe { helpers::loongarch_helper_check_fpe(cpu.env_ptr()) };
    assert_eq!(vec, 0);
}

#[test]
fn fcmp_cueq_true_for_nan() {
    let nan = u64::from(f32::NAN.to_bits());
    let one = u64::from(1.0f32.to_bits());
    assert_eq!(helpers::loongarch_helper_fcmp_cueq_s(nan, one), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cueq_s(one, one), 1);
    assert_eq!(
        helpers::loongarch_helper_fcmp_cueq_s(one, u64::from(2.0f32.to_bits())),
        0
    );
}

#[test]
fn fcmp_cult_true_for_nan() {
    let nan = u64::from(f32::NAN.to_bits());
    let one = u64::from(1.0f32.to_bits());
    let two = u64::from(2.0f32.to_bits());
    assert_eq!(helpers::loongarch_helper_fcmp_cult_s(nan, one), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cult_s(one, two), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cult_s(two, one), 0);
}

#[test]
fn fcmp_cne_ordered_not_equal() {
    let one = u64::from(1.0f32.to_bits());
    let two = u64::from(2.0f32.to_bits());
    let nan = u64::from(f32::NAN.to_bits());
    assert_eq!(helpers::loongarch_helper_fcmp_cne_s(one, two), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cne_s(one, one), 0);
    assert_eq!(helpers::loongarch_helper_fcmp_cne_s(nan, one), 0);
}

#[test]
fn fcmp_cor_ordered() {
    let one = u64::from(1.0f32.to_bits());
    let nan = u64::from(f32::NAN.to_bits());
    assert_eq!(helpers::loongarch_helper_fcmp_cor_s(one, one), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cor_s(nan, one), 0);
}

#[test]
fn fcmp_cune_unordered_or_ne() {
    let one = u64::from(1.0f32.to_bits());
    let two = u64::from(2.0f32.to_bits());
    let nan = u64::from(f32::NAN.to_bits());
    assert_eq!(helpers::loongarch_helper_fcmp_cune_s(nan, one), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cune_s(one, two), 1);
    assert_eq!(helpers::loongarch_helper_fcmp_cune_s(one, one), 0);
}

#[test]
fn fcsr_read_write() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;

    let mut cpu = LoongArchCpu::new();
    assert_eq!(cpu.read_fcsr(), 0);
    cpu.write_fcsr(0x0300); // RM = RTZ (bits[9:8]=3)
    assert_eq!(cpu.read_fcsr() & 0x300, 0x300);
    cpu.write_fcsr(0xFFFF_FFFF);
    assert_eq!(cpu.read_fcsr(), 0x1F1F_031F);
}

#[test]
fn fcsr_helper_roundtrip() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
    use machina_guest_loongarch::loongarch::csr::*;

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_EUEN, EUEN_FPE);

    unsafe {
        helpers::loongarch_helper_movgr2fcsr(cpu.env_ptr(), 0x0200);
    }
    let val = unsafe { helpers::loongarch_helper_movfcsr2gr(cpu.env_ptr()) };
    assert_eq!(val, 0x0200);
}

#[test]
fn cpu_pending_interrupt_ie_disabled() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
    use machina_guest_loongarch::loongarch::csr::*;

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, 0); // IE=0
    cpu.csr_write(CSR_ECFG, 0x1FFF);
    cpu.set_estat_hw(1 << 11); // timer pending
    assert!(!cpu.pending_interrupt());
}

#[test]
fn cpu_pending_interrupt_ie_enabled_masked() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
    use machina_guest_loongarch::loongarch::csr::*;

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, CRMD_IE | CRMD_DA);
    cpu.csr_write(CSR_ECFG, 0); // all masked
    cpu.set_estat_hw(1 << 11);
    assert!(!cpu.pending_interrupt());
}

#[test]
fn cpu_pending_interrupt_fires() {
    use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
    use machina_guest_loongarch::loongarch::csr::*;

    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_CRMD, CRMD_IE | CRMD_DA);
    cpu.csr_write(CSR_ECFG, 1 << 11); // timer enabled
    cpu.set_estat_hw(1 << 11); // timer pending
    assert!(cpu.pending_interrupt());
}
