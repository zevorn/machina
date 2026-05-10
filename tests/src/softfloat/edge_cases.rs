// IEEE 754 edge case tests: NaN, Inf, Zero, Subnormal.

use machina_softfloat::env::{ExcFlags, FloatEnv, RoundMode};
use machina_softfloat::types::Float32;

fn env() -> FloatEnv {
    FloatEnv::new(RoundMode::NearEven)
}

const POS_INF: u32 = 0x7f80_0000;
const NEG_INF: u32 = 0xff80_0000;
const QNAN: u32 = 0x7fc0_0000; // canonical QNaN
const SNAN: u32 = 0x7f80_0001; // SNaN

// ── NaN propagation ─────────────────────────────────

#[test]
fn f32_add_nan_propagates() {
    let mut e = env();
    let nan = Float32::from_bits(QNAN);
    let one = Float32::from_f32(1.0);
    let c = nan.add(one, &mut e);
    assert!(c.is_nan());
}

#[test]
fn f32_add_snan_signals_invalid() {
    let mut e = env();
    let snan = Float32::from_bits(SNAN);
    let one = Float32::from_f32(1.0);
    let c = snan.add(one, &mut e);
    assert!(c.is_nan());
    assert!(e.flags().contains(ExcFlags::INVALID));
}

#[test]
fn f32_default_nan_mode() {
    let mut e = env();
    e.set_default_nan(true);
    let nan = Float32::from_bits(0x7f80_1234); // non-canonical QNaN
    let one = Float32::from_f32(1.0);
    let c = nan.add(one, &mut e);
    // In default-NaN mode, result is canonical QNaN
    assert_eq!(c.to_bits(), QNAN);
}

// ── Infinity arithmetic ─────────────────────────────

#[test]
fn f32_inf_plus_finite() {
    let mut e = env();
    let inf = Float32::from_bits(POS_INF);
    let one = Float32::from_f32(1.0);
    let c = inf.add(one, &mut e);
    assert_eq!(c.to_bits(), POS_INF);
    assert!(e.flags().is_empty());
}

#[test]
fn f32_inf_minus_inf_is_nan() {
    let mut e = env();
    let inf = Float32::from_bits(POS_INF);
    let c = inf.sub(inf, &mut e);
    assert!(c.is_nan());
    assert!(e.flags().contains(ExcFlags::INVALID));
}

#[test]
fn f32_inf_mul_zero_is_nan() {
    let mut e = env();
    let inf = Float32::from_bits(POS_INF);
    let zero = Float32::from_f32(0.0);
    let c = inf.mul(zero, &mut e);
    assert!(c.is_nan());
    assert!(e.flags().contains(ExcFlags::INVALID));
}

#[test]
fn f32_inf_div_inf_is_nan() {
    let mut e = env();
    let inf = Float32::from_bits(POS_INF);
    let c = inf.div(inf, &mut e);
    assert!(c.is_nan());
    assert!(e.flags().contains(ExcFlags::INVALID));
}

#[test]
fn f32_zero_div_zero_is_nan() {
    let mut e = env();
    let z = Float32::from_f32(0.0);
    let c = z.div(z, &mut e);
    assert!(c.is_nan());
    assert!(e.flags().contains(ExcFlags::INVALID));
}

#[test]
fn f32_finite_div_zero_is_inf() {
    let mut e = env();
    let a = Float32::from_f32(1.0);
    let z = Float32::from_f32(0.0);
    let c = a.div(z, &mut e);
    assert_eq!(c.to_bits(), POS_INF);
    assert!(e.flags().contains(ExcFlags::DIVBYZERO));
}

#[test]
fn f32_neg_div_zero_is_neg_inf() {
    let mut e = env();
    let a = Float32::from_f32(-1.0);
    let z = Float32::from_f32(0.0);
    let c = a.div(z, &mut e);
    assert_eq!(c.to_bits(), NEG_INF);
    assert!(e.flags().contains(ExcFlags::DIVBYZERO));
}

// ── Zero arithmetic ─────────────────────────────────

#[test]
fn f32_neg_zero_plus_pos_zero() {
    let mut e = env();
    let nz = Float32::from_bits(0x8000_0000); // -0
    let pz = Float32::from_f32(0.0); // +0
    let c = nz.add(pz, &mut e);
    // IEEE 754: -0 + (+0) = +0 in RNE mode
    assert_eq!(c.to_bits(), 0x0000_0000);
}

#[test]
fn f32_neg_zero_sub_neg_zero() {
    let mut e = env();
    let nz = Float32::from_bits(0x8000_0000);
    let c = nz.sub(nz, &mut e);
    // IEEE 754: (-0) - (-0) = +0 in RNE mode
    assert_eq!(c.to_bits(), 0x0000_0000);
}

// ── Subnormal ───────────────────────────────────────

#[test]
fn f32_smallest_subnormal() {
    let mut e = env();
    // smallest positive subnormal: 0x00000001
    let a = Float32::from_bits(0x0000_0001);
    let b = Float32::from_bits(0x0000_0001);
    let c = a.add(b, &mut e);
    // 2 * smallest subnormal = 0x00000002
    assert_eq!(c.to_bits(), 0x0000_0002);
}

// ── Sqrt edge cases ─────────────────────────────────

#[test]
fn f32_sqrt_zero() {
    let mut e = env();
    let z = Float32::from_f32(0.0);
    let c = z.sqrt(&mut e);
    assert_eq!(c.to_bits(), 0x0000_0000);
}

#[test]
fn f32_sqrt_neg_zero() {
    let mut e = env();
    let nz = Float32::from_bits(0x8000_0000);
    let c = nz.sqrt(&mut e);
    // sqrt(-0) = -0
    assert_eq!(c.to_bits(), 0x8000_0000);
}

#[test]
fn f32_sqrt_negative_is_nan() {
    let mut e = env();
    let a = Float32::from_f32(-1.0);
    let c = a.sqrt(&mut e);
    assert!(c.is_nan());
    assert!(e.flags().contains(ExcFlags::INVALID));
}

#[test]
fn f32_sqrt_inf() {
    let mut e = env();
    let inf = Float32::from_bits(POS_INF);
    let c = inf.sqrt(&mut e);
    assert_eq!(c.to_bits(), POS_INF);
}

// ── Compare edge cases ──────────────────────────────

#[test]
fn f32_nan_not_equal_to_self() {
    let mut e = env();
    let nan = Float32::from_bits(QNAN);
    assert!(!nan.eq(nan, &mut e));
}

#[test]
fn f32_pos_zero_eq_neg_zero() {
    let mut e = env();
    let pz = Float32::from_f32(0.0);
    let nz = Float32::from_bits(0x8000_0000);
    assert!(pz.eq(nz, &mut e));
}

#[test]
fn f32_nan_compare_signals_invalid() {
    let mut e = env();
    let nan = Float32::from_bits(QNAN);
    let one = Float32::from_f32(1.0);
    // lt/le with NaN should signal INVALID
    assert!(!nan.lt(one, &mut e));
    assert!(e.flags().contains(ExcFlags::INVALID));
}

// ── Min/Max edge cases (#61) ────────────────────────

#[test]
fn f32_min_pos_zero_neg_zero_returns_neg_zero() {
    let mut e = env();
    let pz = Float32::from_f32(0.0);
    let nz = Float32::from_bits(0x8000_0000);
    // IEEE 754-2008: min(+0, -0) = -0, regardless of operand order.
    assert_eq!(pz.min(nz, &mut e).to_bits(), 0x8000_0000);
    assert_eq!(nz.min(pz, &mut e).to_bits(), 0x8000_0000);
}

#[test]
fn f32_max_pos_zero_neg_zero_returns_pos_zero() {
    let mut e = env();
    let pz = Float32::from_f32(0.0);
    let nz = Float32::from_bits(0x8000_0000);
    // IEEE 754-2008: max(+0, -0) = +0, regardless of operand order.
    assert_eq!(pz.max(nz, &mut e).to_bits(), 0x0000_0000);
    assert_eq!(nz.max(pz, &mut e).to_bits(), 0x0000_0000);
}

#[test]
fn f32_min_qnan_finite_returns_finite_no_invalid() {
    // IEEE 754-2008: min/max with one quiet NaN returns the
    // non-NaN operand and does NOT raise INVALID.
    let mut e = env();
    let nan = Float32::from_bits(QNAN);
    let one = Float32::from_f32(1.0);
    assert_eq!(nan.min(one, &mut e).to_bits(), one.to_bits());
    assert_eq!(one.min(nan, &mut e).to_bits(), one.to_bits());
    assert!(
        !e.flags().contains(ExcFlags::INVALID),
        "qNaN must not raise INVALID for min: {:?}",
        e.flags(),
    );
}

#[test]
fn f32_max_qnan_finite_returns_finite_no_invalid() {
    let mut e = env();
    let nan = Float32::from_bits(QNAN);
    let one = Float32::from_f32(1.0);
    assert_eq!(nan.max(one, &mut e).to_bits(), one.to_bits());
    assert_eq!(one.max(nan, &mut e).to_bits(), one.to_bits());
    assert!(
        !e.flags().contains(ExcFlags::INVALID),
        "qNaN must not raise INVALID for max: {:?}",
        e.flags(),
    );
}

#[test]
fn f32_min_snan_finite_signals_invalid() {
    // IEEE 754-2008: any signalling NaN operand to min/max raises
    // INVALID. The non-NaN value is returned.
    let mut e = env();
    let snan = Float32::from_bits(SNAN);
    let one = Float32::from_f32(1.0);
    let r = snan.min(one, &mut e);
    assert_eq!(r.to_bits(), one.to_bits());
    assert!(
        e.flags().contains(ExcFlags::INVALID),
        "sNaN min must signal INVALID: {:?}",
        e.flags(),
    );
}

#[test]
fn f32_max_snan_finite_signals_invalid() {
    let mut e = env();
    let snan = Float32::from_bits(SNAN);
    let one = Float32::from_f32(1.0);
    let r = snan.max(one, &mut e);
    assert_eq!(r.to_bits(), one.to_bits());
    assert!(
        e.flags().contains(ExcFlags::INVALID),
        "sNaN max must signal INVALID: {:?}",
        e.flags(),
    );
}

#[test]
fn f32_min_both_qnan_returns_nan_without_invalid() {
    let mut e = env();
    let nan = Float32::from_bits(QNAN);
    let r = nan.min(nan, &mut e);
    assert!(r.is_nan());
    assert!(
        !e.flags().contains(ExcFlags::INVALID),
        "two qNaNs to min must not raise INVALID: {:?}",
        e.flags(),
    );
}

#[test]
fn f32_min_max_signed_ordering() {
    let mut e = env();

    // Mixed sign: negative is smaller than positive.
    let a = Float32::from_f32(-3.0);
    let b = Float32::from_f32(5.0);
    assert_eq!(a.min(b, &mut e).to_bits(), a.to_bits());
    assert_eq!(b.min(a, &mut e).to_bits(), a.to_bits());
    assert_eq!(a.max(b, &mut e).to_bits(), b.to_bits());
    assert_eq!(b.max(a, &mut e).to_bits(), b.to_bits());

    // Both negative: more-negative magnitude is smaller.
    let p = Float32::from_f32(-1.0);
    let q = Float32::from_f32(-2.0);
    assert_eq!(p.min(q, &mut e).to_bits(), q.to_bits());
    assert_eq!(p.max(q, &mut e).to_bits(), p.to_bits());

    // Both positive: smaller magnitude is smaller.
    let r = Float32::from_f32(0.5);
    let s = Float32::from_f32(1.0);
    assert_eq!(r.min(s, &mut e).to_bits(), r.to_bits());
    assert_eq!(r.max(s, &mut e).to_bits(), s.to_bits());
}

#[test]
fn f32_min_max_with_infinities() {
    let mut e = env();
    let pinf = Float32::from_bits(POS_INF);
    let ninf = Float32::from_bits(NEG_INF);
    let v = Float32::from_f32(100.0);
    assert_eq!(ninf.min(v, &mut e).to_bits(), NEG_INF);
    assert_eq!(pinf.max(v, &mut e).to_bits(), POS_INF);
    assert_eq!(ninf.min(pinf, &mut e).to_bits(), NEG_INF);
    assert_eq!(ninf.max(pinf, &mut e).to_bits(), POS_INF);
}

// ── Float64 min/max smoke ──────────────────────────

#[test]
fn f64_min_pos_neg_zero_returns_neg_zero() {
    use machina_softfloat::types::Float64;
    let mut e = env();
    let pz = Float64::from_f64(0.0);
    let nz = Float64::from_bits(0x8000_0000_0000_0000);
    assert_eq!(pz.min(nz, &mut e).to_bits(), 0x8000_0000_0000_0000);
    assert_eq!(nz.min(pz, &mut e).to_bits(), 0x8000_0000_0000_0000);
}

#[test]
fn f64_max_pos_neg_zero_returns_pos_zero() {
    use machina_softfloat::types::Float64;
    let mut e = env();
    let pz = Float64::from_f64(0.0);
    let nz = Float64::from_bits(0x8000_0000_0000_0000);
    assert_eq!(pz.max(nz, &mut e).to_bits(), 0x0000_0000_0000_0000);
    assert_eq!(nz.max(pz, &mut e).to_bits(), 0x0000_0000_0000_0000);
}
