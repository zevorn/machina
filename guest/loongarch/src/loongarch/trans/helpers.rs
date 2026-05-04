#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::missing_const_for_fn
)]

#[no_mangle]
pub extern "C" fn loongarch_helper_div_d(a: i64, b: i64) -> i64 {
    if b == 0 || (a == i64::MIN && b == -1) {
        0
    } else {
        a.wrapping_div(b)
    }
}

#[no_mangle]
pub extern "C" fn loongarch_helper_mod_d(a: i64, b: i64) -> i64 {
    if b == 0 || (a == i64::MIN && b == -1) {
        0
    } else {
        a.wrapping_rem(b)
    }
}

#[no_mangle]
pub extern "C" fn loongarch_helper_div_du(a: u64, b: u64) -> u64 {
    if b == 0 {
        0
    } else {
        a / b
    }
}

#[no_mangle]
pub extern "C" fn loongarch_helper_mod_du(a: u64, b: u64) -> u64 {
    if b == 0 {
        0
    } else {
        a % b
    }
}

#[no_mangle]
pub extern "C" fn loongarch_helper_div_w(a: i64, b: i64) -> i64 {
    let a32 = a as i32;
    let b32 = b as i32;
    let result = if b32 == 0 || (a32 == i32::MIN && b32 == -1) {
        0i32
    } else {
        a32.wrapping_div(b32)
    };
    i64::from(result)
}

#[no_mangle]
pub extern "C" fn loongarch_helper_mod_w(a: i64, b: i64) -> i64 {
    let a32 = a as i32;
    let b32 = b as i32;
    let result = if b32 == 0 || (a32 == i32::MIN && b32 == -1) {
        0i32
    } else {
        a32.wrapping_rem(b32)
    };
    i64::from(result)
}

#[no_mangle]
pub extern "C" fn loongarch_helper_div_wu(a: u64, b: u64) -> i64 {
    let a32 = a as u32;
    let b32 = b as u32;
    let result = if b32 == 0 { 0u32 } else { a32 / b32 };
    i64::from(result as i32)
}

#[no_mangle]
pub extern "C" fn loongarch_helper_mod_wu(a: u64, b: u64) -> i64 {
    let a32 = a as u32;
    let b32 = b as u32;
    let result = if b32 == 0 { 0u32 } else { a32 % b32 };
    i64::from(result as i32)
}

#[no_mangle]
pub extern "C" fn loongarch_helper_mulh_d(a: i64, b: i64) -> i64 {
    ((i128::from(a) * i128::from(b)) >> 64) as i64
}

#[no_mangle]
pub extern "C" fn loongarch_helper_mulh_du(a: u64, b: u64) -> u64 {
    ((u128::from(a) * u128::from(b)) >> 64) as u64
}

#[no_mangle]
pub extern "C" fn loongarch_helper_mulh_w(a: i64, b: i64) -> i64 {
    let a32 = a as i32;
    let b32 = b as i32;
    let product = i64::from(a32) * i64::from(b32);
    i64::from((product >> 32) as i32)
}

#[no_mangle]
pub extern "C" fn loongarch_helper_mulh_wu(a: u64, b: u64) -> i64 {
    let a32 = a as u32;
    let b32 = b as u32;
    let product = u64::from(a32) * u64::from(b32);
    i64::from((product >> 32) as u32 as i32)
}

#[no_mangle]
pub extern "C" fn loongarch_helper_revb_2h(a: u64) -> i64 {
    let lo = a as u32;
    let swapped = ((lo & 0x00FF_00FF) << 8) | ((lo & 0xFF00_FF00) >> 8);
    i64::from(swapped as i32)
}

#[no_mangle]
pub extern "C" fn loongarch_helper_revb_4h(a: u64) -> u64 {
    ((a & 0x00FF_00FF_00FF_00FF) << 8) | ((a & 0xFF00_FF00_FF00_FF00) >> 8)
}

#[no_mangle]
pub extern "C" fn loongarch_helper_revb_2w(a: u64) -> u64 {
    let lo = (a as u32).swap_bytes();
    let hi = ((a >> 32) as u32).swap_bytes();
    u64::from(hi) << 32 | u64::from(lo)
}

#[no_mangle]
pub extern "C" fn loongarch_helper_bitrev_4b(a: u64) -> i64 {
    let mut v = a as u32;
    v = ((v & 0x5555_5555) << 1) | ((v & 0xAAAA_AAAA) >> 1);
    v = ((v & 0x3333_3333) << 2) | ((v & 0xCCCC_CCCC) >> 2);
    v = ((v & 0x0F0F_0F0F) << 4) | ((v & 0xF0F0_F0F0) >> 4);
    i64::from(v as i32)
}

#[no_mangle]
pub extern "C" fn loongarch_helper_bitrev_8b(a: u64) -> u64 {
    let mut v = a;
    v = ((v & 0x5555_5555_5555_5555) << 1) | ((v & 0xAAAA_AAAA_AAAA_AAAA) >> 1);
    v = ((v & 0x3333_3333_3333_3333) << 2) | ((v & 0xCCCC_CCCC_CCCC_CCCC) >> 2);
    v = ((v & 0x0F0F_0F0F_0F0F_0F0F) << 4) | ((v & 0xF0F0_F0F0_F0F0_F0F0) >> 4);
    v
}

#[no_mangle]
pub extern "C" fn loongarch_helper_bitrev_w(a: u64) -> i64 {
    let v = (a as u32).reverse_bits();
    i64::from(v as i32)
}

#[no_mangle]
pub extern "C" fn loongarch_helper_bitrev_d(a: u64) -> u64 {
    a.reverse_bits()
}

/// # Safety
/// `env` must point to a valid `LoongArchCpu`.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_cpucfg(
    _env: *mut u8,
    index: u64,
) -> u64 {
    match index as u32 {
        0x00 => 0x0014_C010,
        0x01 => 0x03F2_F2FE,
        // QEMU la464 value with LSX/LASX masked (bits 6,7)
        0x02 => 0x0060_C00F,
        0x03 => 0,
        0x04 => 0x05F5_E100,
        0x05 => 0x0001_0001,
        0x06 => 0,
        _ => 0,
    }
}

/// # Safety
/// `env` must point to a valid `LoongArchCpu`.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_tlbsrch(env: *mut u8) -> u64 {
    let cpu = &mut *(env.cast::<super::super::cpu::LoongArchCpu>());
    if let Some(idx) = cpu.tlb_search() {
        cpu.tlbidx = (cpu.tlbidx & !(0xFFF | (1 << 31))) | (idx as u64);
    } else {
        cpu.tlbidx |= 1 << 31;
    }
    0
}

/// # Safety
/// `env` must point to a valid `LoongArchCpu`.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_tlbrd(env: *mut u8) -> u64 {
    let cpu = &mut *(env.cast::<super::super::cpu::LoongArchCpu>());
    let idx = (cpu.tlbidx & 0xFFF) as usize;
    cpu.tlb_read(idx);
    0
}

/// # Safety
/// `env` must point to a valid `LoongArchCpu`.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_tlbwr(env: *mut u8) -> u64 {
    let cpu = &mut *(env.cast::<super::super::cpu::LoongArchCpu>());
    let idx = (cpu.tlbidx & 0xFFF) as usize;
    cpu.tlb_write(idx);
    0
}

/// # Safety
/// `env` must point to a valid `LoongArchCpu`.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_tlbfill(env: *mut u8) -> u64 {
    let cpu = &mut *(env.cast::<super::super::cpu::LoongArchCpu>());
    cpu.tlb_fill();
    0
}

/// Returns 0 on success, nonzero (exception vector) for invalid opcode.
///
/// # Safety
/// `env` must point to a valid `LoongArchCpu`.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_invtlb(
    env: *mut u8,
    op: u64,
    asid_val: u64,
    va: u64,
) -> u64 {
    let cpu = &mut *(env.cast::<super::super::cpu::LoongArchCpu>());
    if op > 6 {
        use super::super::csr::*;
        cpu.era = cpu.pc;
        cpu.prmd = cpu.crmd & 0x7;
        cpu.crmd = cpu.crmd & !CRMD_PLV_MASK & !CRMD_IE;
        cpu.estat = (cpu.estat & ESTAT_IS_MASK) | (0x0D << 16);
        return cpu.eentry;
    }
    cpu.invtlb(op as u32, asid_val as u16, va);
    0
}

/// Returns 0 if PLV==0 (privileged access OK).
/// Otherwise raises IPE exception and returns the exception vector.
///
/// # Safety
/// `env` must point to a valid `LoongArchCpu`.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_check_plv(env: *mut u8) -> u64 {
    use super::super::csr::*;
    let cpu = &mut *(env.cast::<super::super::cpu::LoongArchCpu>());
    if cpu.crmd & CRMD_PLV_MASK == 0 {
        return 0;
    }
    // Raise IPE (Instruction Privilege Error)
    let pc = cpu.pc;
    cpu.era = pc;
    cpu.prmd = cpu.crmd & 0x7;
    cpu.crmd = cpu.crmd & !CRMD_PLV_MASK & !CRMD_IE;
    cpu.estat = (cpu.estat & ESTAT_IS_MASK) | (0x0E << 16);
    cpu.eentry
}

/// # Safety
/// `env` must point to a valid `LoongArchCpu`.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_raise_exception(
    env: *mut u8,
    ecode: u64,
    esubcode: u64,
) -> u64 {
    use super::super::csr::*;
    let cpu = &mut *(env.cast::<super::super::cpu::LoongArchCpu>());
    let pc = cpu.pc;

    if ecode == 0x3F {
        // TLB refill: save to TLBR-specific CSRs
        cpu.tlbrera = (pc & !0x3) | 1; // ISTLBR=1, PC in [63:2]
        cpu.tlbrprmd = cpu.crmd & 0x7;
        // Force DA mode, PLV0, IE=0 for TLB refill handler
        cpu.crmd = (cpu.crmd & !CRMD_PLV_MASK & !CRMD_IE & !CRMD_PG) | CRMD_DA;
    } else {
        cpu.era = pc;
        cpu.prmd = cpu.crmd & 0x7;
        cpu.crmd = cpu.crmd & !CRMD_PLV_MASK & !CRMD_IE;
    }

    let estat_val = (cpu.estat & ESTAT_IS_MASK)
        | ((ecode & 0x3F) << 16)
        | ((esubcode & 0x1FF) << 22);
    cpu.estat = estat_val;

    if ecode == 0x3F {
        cpu.tlbrentry
    } else {
        cpu.eentry
    }
}

/// # Safety
/// `env` must point to a valid `LoongArchCpu`.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_ertn(env: *mut u8) -> u64 {
    use super::super::csr::*;
    let cpu = &mut *(env.cast::<super::super::cpu::LoongArchCpu>());
    if cpu.tlbrera & 1 != 0 {
        // Return from TLB refill: restore PLV/IE, clear DA, set PG
        let pplv_pie = cpu.tlbrprmd & 0x7;
        cpu.crmd = (cpu.crmd & !0x7 & !CRMD_DA & !CRMD_PG) | pplv_pie | CRMD_PG;
        let pc = cpu.tlbrera & !0x3;
        cpu.tlbrera &= !1; // Clear ISTLBR
        pc
    } else {
        cpu.crmd = (cpu.crmd & !0x7) | (cpu.prmd & 0x7);
        cpu.era
    }
}

/// # Safety
/// `env` must point to a valid `LoongArchCpu`.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_idle(env: *mut u8) -> u64 {
    let cpu = &mut *(env.cast::<super::super::cpu::LoongArchCpu>());
    cpu.halted.store(true, std::sync::atomic::Ordering::Release);
    0
}

/// # Safety
/// `env` must point to a valid `LoongArchCpu`.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_csrrd(
    env: *mut u8,
    csr_num: u64,
) -> u64 {
    let cpu = &*(env.cast::<super::super::cpu::LoongArchCpu>());
    cpu.csr_read(csr_num as u32)
}

/// # Safety
/// `env` must point to a valid `LoongArchCpu`.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_csrwr(
    env: *mut u8,
    csr_num: u64,
    val: u64,
) -> u64 {
    let cpu = &mut *(env.cast::<super::super::cpu::LoongArchCpu>());
    let old = cpu.csr_read(csr_num as u32);
    cpu.csr_write(csr_num as u32, val);
    old
}

/// # Safety
/// `env` must point to a valid `LoongArchCpu`.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_csrxchg(
    env: *mut u8,
    csr_num: u64,
    val: u64,
    mask: u64,
) -> u64 {
    let cpu = &mut *(env.cast::<super::super::cpu::LoongArchCpu>());
    cpu.csr_xchg(csr_num as u32, val, mask)
}

/// # Safety
/// `env` must point to a valid `LoongArchCpu` with correct guest_base/ram fields.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_sc_w(
    env: *mut u8,
    addr: u64,
    val: u64,
) -> u64 {
    let cpu = &mut *(env.cast::<super::super::cpu::LoongArchCpu>());
    let llbit = cpu.llbctl;
    let res_addr = cpu.ll_res_addr;
    let res_val = cpu.ll_res_val;
    cpu.llbctl = 0;
    cpu.ll_res_addr = u64::MAX;
    cpu.ll_res_val = 0;
    if llbit == 0 || res_addr != addr {
        return 0;
    }
    let end = match addr.checked_add(4) {
        Some(e) if addr >= cpu.ram_base && e <= cpu.ram_end => e,
        _ => return 0,
    };
    let _ = end;
    let host_ptr = (cpu.guest_base as *const u8).add(addr as usize);
    let current = (host_ptr as *const u32).read_unaligned();
    if i64::from(current as i32) != res_val as i64 {
        return 0;
    }
    let host_wptr = (cpu.guest_base as *mut u8).add(addr as usize);
    (host_wptr as *mut u32).write_unaligned(val as u32);
    1
}

/// # Safety
/// `env` must point to a valid `LoongArchCpu` with correct guest_base/ram fields.
#[no_mangle]
pub unsafe extern "C" fn loongarch_helper_sc_d(
    env: *mut u8,
    addr: u64,
    val: u64,
) -> u64 {
    let cpu = &mut *(env.cast::<super::super::cpu::LoongArchCpu>());
    let llbit = cpu.llbctl;
    let res_addr = cpu.ll_res_addr;
    let res_val = cpu.ll_res_val;
    cpu.llbctl = 0;
    cpu.ll_res_addr = u64::MAX;
    cpu.ll_res_val = 0;
    if llbit == 0 || res_addr != addr {
        return 0;
    }
    let end = match addr.checked_add(8) {
        Some(e) if addr >= cpu.ram_base && e <= cpu.ram_end => e,
        _ => return 0,
    };
    let _ = end;
    let host_ptr = (cpu.guest_base as *const u8).add(addr as usize);
    let current = (host_ptr as *const u64).read_unaligned();
    if current != res_val {
        return 0;
    }
    let host_wptr = (cpu.guest_base as *mut u8).add(addr as usize);
    (host_wptr as *mut u64).write_unaligned(val);
    1
}
