use machina_guest_riscv::riscv::cpu::RiscvCpu;
use machina_guest_riscv::riscv::csr::*;

// -- mstatus read/write --

#[test]
fn test_mstatus_read_write() {
    let mut cpu = RiscvCpu::new();
    assert_eq!(cpu.priv_level, PrivLevel::Machine);

    // RV64 reads back UXL=2 and SXL=2.
    let v = cpu.csr_read(CSR_MSTATUS);
    assert_eq!(
        v & ((3u64 << 32) | (3u64 << 34)),
        (2u64 << 32) | (2u64 << 34)
    );

    // Write MIE (bit 3) + MPIE (bit 7).
    cpu.csr_write(CSR_MSTATUS, (1 << 3) | (1 << 7));
    let v = cpu.csr_read(CSR_MSTATUS);
    assert_eq!(v & ((1 << 3) | (1 << 7)), (1 << 3) | (1 << 7));

    // Non-writable bits are WARL/read-only.
    cpu.csr_write(CSR_MSTATUS, 3u64 << 32);
    let v = cpu.csr_read(CSR_MSTATUS);
    assert_eq!(v & (3u64 << 32), 2u64 << 32);
}

#[test]
fn test_mstatus_sd_bit() {
    let mut cpu = RiscvCpu::new();

    // Set FS = Dirty (bits 14:13 = 0b11).
    cpu.csr_write(CSR_MSTATUS, 3u64 << 13);
    let v = cpu.csr_read(CSR_MSTATUS);
    // SD (bit 63) should be set when FS is dirty.
    assert_ne!(v & (1u64 << 63), 0);

    // Set FS = Off (bits 14:13 = 0b00).
    cpu.csr_write(CSR_MSTATUS, 0);
    let v = cpu.csr_read(CSR_MSTATUS);
    assert_eq!(v & (1u64 << 63), 0);
}

// -- Privilege checks --

#[test]
fn test_csr_privilege_check() {
    let mut cpu = RiscvCpu::new();

    // M-mode can access M-level CSRs.
    assert!(cpu.try_csr_read(CSR_MSTATUS).is_ok());
    assert!(cpu.try_csr_write(CSR_MSTATUS, 0).is_ok());

    // M-mode can access S-level CSRs.
    assert!(cpu.try_csr_read(CSR_SSTATUS).is_ok());

    // M-mode can access U-level CSRs.
    assert!(cpu.try_csr_read(CSR_FFLAGS).is_ok());

    // Drop to S-mode.
    cpu.set_priv(PrivLevel::Supervisor);
    assert!(cpu.try_csr_read(CSR_SSTATUS).is_ok());
    assert!(cpu.try_csr_read(CSR_MSTATUS).is_err());
    assert!(cpu.try_csr_write(CSR_MSTATUS, 0).is_err());

    // Drop to U-mode.
    cpu.set_priv(PrivLevel::User);
    assert!(cpu.try_csr_read(CSR_FFLAGS).is_ok());
    assert!(cpu.try_csr_read(CSR_SSTATUS).is_err());
    assert!(cpu.try_csr_read(CSR_MSTATUS).is_err());
}

#[test]
fn test_read_only_csr_write_rejected() {
    let mut cpu = RiscvCpu::new();
    // CSR_CYCLE (0xC00) has bits [11:10] = 0b11 =>
    // read-only.
    assert!(cpu.try_csr_read(CSR_CYCLE).is_ok());
    assert!(cpu.try_csr_write(CSR_CYCLE, 42).is_err());
}

// -- Delegation --

#[test]
fn test_medeleg_delegation() {
    let mut cpu = RiscvCpu::new();

    // Write all bits; only delegable exceptions survive.
    cpu.csr_write(CSR_MEDELEG, u64::MAX);
    let v = cpu.csr_read(CSR_MEDELEG);
    // Bits 10, 11, 14 are not delegable.
    assert_eq!(v & (1 << 10), 0);
    assert_eq!(v & (1 << 11), 0);
    assert_eq!(v & (1 << 14), 0);
    // Bit 2 (illegal insn) is delegable.
    assert_ne!(v & (1 << 2), 0);
}

#[test]
fn test_mideleg_delegation() {
    let mut cpu = RiscvCpu::new();

    // Only SSIP(1), STIP(5), SEIP(9) are delegable.
    cpu.csr_write(CSR_MIDELEG, u64::MAX);
    let v = cpu.csr_read(CSR_MIDELEG);
    assert_eq!(v, (1 << 1) | (1 << 5) | (1 << 9));
}

// -- sstatus alias --

#[test]
fn test_sstatus_alias() {
    let mut cpu = RiscvCpu::new();

    // Write SIE (bit 1) via sstatus.
    cpu.csr_write(CSR_SSTATUS, 1 << 1);
    // mstatus should reflect SIE.
    let ms = cpu.csr_read(CSR_MSTATUS);
    assert_ne!(ms & (1 << 1), 0);

    // Write MIE (bit 3) via mstatus -- not visible in
    // sstatus.
    cpu.csr_write(CSR_MSTATUS, (1 << 1) | (1 << 3));
    let ss = cpu.csr_read(CSR_SSTATUS);
    assert_ne!(ss & (1 << 1), 0);
    assert_eq!(ss & (1 << 3), 0);

    // FS bits are visible in sstatus.
    cpu.csr_write(CSR_MSTATUS, 3u64 << 13);
    let ss = cpu.csr_read(CSR_SSTATUS);
    assert_eq!(ss & (3u64 << 13), 3u64 << 13);
}

// -- sip/sie alias --

#[test]
fn test_sip_sie_alias() {
    let mut cpu = RiscvCpu::new();

    // Delegate SSIP to S-mode.
    cpu.csr_write(CSR_MIDELEG, 1 << 1);

    // Write SSIP via SIP.
    cpu.csr_write(CSR_SIP, 1 << 1);
    // Should be visible in MIP.
    let mip = cpu.csr_read(CSR_MIP);
    assert_ne!(mip & (1 << 1), 0);

    // SIP read should show only delegated+pending bits.
    let sip = cpu.csr_read(CSR_SIP);
    assert_ne!(sip & (1 << 1), 0);

    // Non-delegated bit: STIP is not delegated.
    let sip_stip = cpu.csr_read(CSR_SIP);
    assert_eq!(sip_stip & (1 << 5), 0);

    // SIE follows same pattern.
    cpu.csr_write(CSR_SIE, 1 << 1);
    let mie = cpu.csr_read(CSR_MIE);
    assert_ne!(mie & (1 << 1), 0);
}

// -- FP CSRs --

#[test]
fn test_fp_csr_read_write() {
    let mut cpu = RiscvCpu::new();

    cpu.csr_write(CSR_FFLAGS, 0x1F);
    assert_eq!(cpu.csr_read(CSR_FFLAGS), 0x1F);

    // Excess bits are masked.
    cpu.csr_write(CSR_FFLAGS, 0xFF);
    assert_eq!(cpu.csr_read(CSR_FFLAGS), 0x1F);

    cpu.csr_write(CSR_FRM, 0x07);
    assert_eq!(cpu.csr_read(CSR_FRM), 0x07);

    cpu.csr_write(CSR_FRM, 0xFF);
    assert_eq!(cpu.csr_read(CSR_FRM), 0x07);
}

#[test]
fn test_fcsr_composite() {
    let mut cpu = RiscvCpu::new();

    // FCSR = frm[7:5] | fflags[4:0].
    cpu.csr_write(CSR_FCSR, 0xFF);
    assert_eq!(cpu.csr_read(CSR_FFLAGS), 0x1F);
    assert_eq!(cpu.csr_read(CSR_FRM), 0x07);
    assert_eq!(cpu.csr_read(CSR_FCSR), 0xFF);

    // Write individual fields and read back via FCSR.
    cpu.csr_write(CSR_FFLAGS, 0x05);
    cpu.csr_write(CSR_FRM, 0x03);
    assert_eq!(cpu.csr_read(CSR_FCSR), (0x03 << 5) | 0x05);
}

// -- Counter CSRs --

#[test]
fn test_counter_csrs() {
    let mut cpu = RiscvCpu::new();
    cpu.csr.cycle = 42;
    cpu.csr.instret = 100;

    assert_eq!(cpu.csr_read(CSR_CYCLE), 42);
    assert_eq!(cpu.csr_read(CSR_TIME), 42);
    assert_eq!(cpu.csr_read(CSR_INSTRET), 100);
}

// -- MEPC alignment --

#[test]
fn test_mepc_alignment() {
    let mut cpu = RiscvCpu::new();

    // Bit 0 is always cleared (2-byte alignment min).
    cpu.csr_write(CSR_MEPC, 0xFFFF_FFFF_FFFF_FFFF);
    assert_eq!(cpu.csr_read(CSR_MEPC) & 1, 0);
}

// -- MISA read-only --

#[test]
fn test_misa_read_only() {
    let mut cpu = RiscvCpu::new();
    let original = cpu.csr_read(CSR_MISA);

    // MXL = 2 (64-bit).
    assert_eq!(original >> 62, 2);
    // Extensions I, M, A, F, D, C should be present.
    assert_ne!(original & (1 << 8), 0); // I
    assert_ne!(original & (1 << 12), 0); // M
    assert_ne!(original & (1 << 0), 0); // A
    assert_ne!(original & (1 << 5), 0); // F
    assert_ne!(original & (1 << 3), 0); // D
    assert_ne!(original & (1 << 2), 0); // C

    // Write attempt should be silently ignored.
    cpu.csr_write(CSR_MISA, 0);
    assert_eq!(cpu.csr_read(CSR_MISA), original);
}

// -- Unknown CSR --

#[test]
fn test_unknown_csr_returns_error() {
    let cpu = RiscvCpu::new();
    assert!(cpu.try_csr_read(0x3FF).is_err());
}

// ===== Table-driven CSR coverage (#72) =====
//
// Each table describes one orthogonal axis of CSR behaviour.
// Per-row asserts include the row label so a regression points
// at the exact CSR/bit that broke.

// (bit, mnemonic): bits in mstatus that should round-trip through
// csr_write. UXL/SXL are deliberately excluded because they are
// fixed at 0b10 (RV64) and therefore behave as WARL/read-only.
const MSTATUS_WRITABLE_BITS: &[(u32, &str)] = &[
    (1, "SIE"),
    (3, "MIE"),
    (5, "SPIE"),
    (7, "MPIE"),
    (8, "SPP"),
    (13, "FS_lo"),
    (14, "FS_hi"),
    (17, "MPRV"),
    (18, "SUM"),
    (19, "MXR"),
];

// (bit_mask, mnemonic): bits in mstatus that must NOT change
// after csr_write. UXL/SXL are pinned at 0b10 by the RV64
// hardware definition; bits 23/24 are WPRI in the RV64 priv
// spec and our MSTATUS_WRITE_MASK leaves them out.
const MSTATUS_NON_WRITABLE_FIELDS: &[(u64, &str)] = &[
    (3u64 << 32, "UXL"),
    (3u64 << 34, "SXL"),
    (1u64 << 23, "WPRI_23"),
    (1u64 << 24, "WPRI_24"),
];

#[test]
fn test_mstatus_writable_bits_table() {
    for &(bit, name) in MSTATUS_WRITABLE_BITS {
        let mut cpu = RiscvCpu::new();
        let mask = 1u64 << bit;
        cpu.csr_write(CSR_MSTATUS, mask);
        let got = cpu.csr_read(CSR_MSTATUS) & mask;
        assert_eq!(got, mask, "mstatus.{name} (bit {bit}) must be writable");
    }
}

#[test]
fn test_mstatus_warl_or_wpri_bits_table() {
    for &(field_mask, name) in MSTATUS_NON_WRITABLE_FIELDS {
        let mut cpu = RiscvCpu::new();
        let baseline = cpu.csr_read(CSR_MSTATUS) & field_mask;

        // Try to flip the bits the field covers.
        cpu.csr_write(CSR_MSTATUS, !baseline);
        let after_flip = cpu.csr_read(CSR_MSTATUS) & field_mask;
        assert_eq!(
            after_flip, baseline,
            "mstatus.{name} ({field_mask:#x}) must stay at its hardware value",
        );

        // And explicit zeroing should not change a non-zero
        // hardware-fixed value either.
        cpu.csr_write(CSR_MSTATUS, 0);
        let after_zero = cpu.csr_read(CSR_MSTATUS) & field_mask;
        assert_eq!(
            after_zero, baseline,
            "mstatus.{name} ({field_mask:#x}) must survive a 0-write",
        );
    }
}

// (csr, m, s, u): expected access result at each privilege.
// Ok = readable, NotOk = traps illegal.
struct PrivCase {
    csr: u16,
    name: &'static str,
    m: bool,
    s: bool,
    u: bool,
}

const PRIV_TABLE: &[PrivCase] = &[
    PrivCase {
        csr: CSR_MSTATUS,
        name: "mstatus",
        m: true,
        s: false,
        u: false,
    },
    PrivCase {
        csr: CSR_MTVEC,
        name: "mtvec",
        m: true,
        s: false,
        u: false,
    },
    PrivCase {
        csr: CSR_MEPC,
        name: "mepc",
        m: true,
        s: false,
        u: false,
    },
    PrivCase {
        csr: CSR_SSTATUS,
        name: "sstatus",
        m: true,
        s: true,
        u: false,
    },
    PrivCase {
        csr: CSR_STVEC,
        name: "stvec",
        m: true,
        s: true,
        u: false,
    },
    PrivCase {
        csr: CSR_FFLAGS,
        name: "fflags",
        m: true,
        s: true,
        u: true,
    },
    PrivCase {
        csr: CSR_FCSR,
        name: "fcsr",
        m: true,
        s: true,
        u: true,
    },
];

#[test]
fn test_csr_privilege_access_table() {
    for case in PRIV_TABLE {
        for (priv_level, expected) in [
            (PrivLevel::Machine, case.m),
            (PrivLevel::Supervisor, case.s),
            (PrivLevel::User, case.u),
        ] {
            let mut cpu = RiscvCpu::new();
            cpu.set_priv(priv_level);
            let got = cpu.try_csr_read(case.csr).is_ok();
            assert_eq!(
                got, expected,
                "{} read at {:?}: expected {expected}, got {got}",
                case.name, priv_level,
            );
        }
    }
}

// CSRs whose access bits encode read-only (top two bits = 0b11):
// writes must trap and the read value must be preserved.
const READ_ONLY_CSRS: &[(u16, &str)] = &[
    (CSR_CYCLE, "cycle"),
    (CSR_INSTRET, "instret"),
    (CSR_TIME, "time"),
    (CSR_MHARTID, "mhartid"),
    (CSR_MVENDORID, "mvendorid"),
    (CSR_MARCHID, "marchid"),
    (CSR_MIMPID, "mimpid"),
];

#[test]
fn test_read_only_csr_write_table() {
    for &(addr, name) in READ_ONLY_CSRS {
        let mut cpu = RiscvCpu::new();
        let before = cpu.try_csr_read(addr).unwrap_or_else(|_| {
            panic!("{name} ({addr:#x}) should be readable")
        });

        let result = cpu.try_csr_write(addr, !before);
        assert!(
            result.is_err(),
            "{name} ({addr:#x}) write must return Err, not silently mask",
        );

        let after = cpu.try_csr_read(addr).unwrap();
        assert_eq!(
            before, after,
            "{name} ({addr:#x}) value must not change after rejected write",
        );
    }
}

// Invalid / unassigned CSR numbers: read AND write must error.
const INVALID_CSRS: &[u16] = &[
    0x3FE, // unassigned in M-mode area
    0x4FF, // unassigned in U/S area
    0x800, // unassigned U area
    0xCFF, // unassigned counter-shadow area
    0xFFF, // top of address space
];

#[test]
fn test_invalid_csr_addresses_table() {
    let mut cpu = RiscvCpu::new();
    for &addr in INVALID_CSRS {
        assert!(
            cpu.try_csr_read(addr).is_err(),
            "read of unassigned CSR {addr:#x} must trap",
        );
        assert!(
            cpu.try_csr_write(addr, 0).is_err(),
            "write to unassigned CSR {addr:#x} must trap",
        );
    }
}
