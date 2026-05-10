# K230 QEMU Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add QEMU `chao-k230-v7` parity for the K230 machine, including C908/T-HEAD CPU support, K230 board/device wiring, direct Linux boot, SDK U-Boot loader boot, and mtest/QEMU slice coverage.

**Architecture:** Keep board wiring in `hw/riscv`, the K230 watchdog in `hw/watchdog`, generic RISC-V extension/MMU behavior in `guest/riscv/src/riscv`, and T-HEAD vendor behavior under a dedicated vendor module. The CLI and runtime become minimally board-neutral for RISC-V so `riscv64-ref` remains unchanged while `k230` supplies its own RAM base, BootROM window, CPU profile, and boot flow.

**Tech Stack:** Rust 2021, Machina MOM/SysBus, `DeviceRegs`, `Ptimer`, `FdtBuilder` plus DTB mutation helpers, existing `cargo test`/`make fmt-check`/`make clippy`, QEMU qtest/oracle tools, and `tests/mtest` for machine-level boot slices.

---

## File Structure

- Create `guest/riscv/src/riscv/cpu_model.rs` for named RISC-V CPU models, vendor IDs, profile data, and C908 profile construction.
- Create `guest/riscv/src/riscv/vendor/mod.rs` and `guest/riscv/src/riscv/vendor/thead.rs` for T-HEAD CSR numbers, privilege checks, read/write behavior, and vendor extension predicates.
- Modify `guest/riscv/src/riscv/mod.rs`, `cpu.rs`, `csr.rs`, `ext.rs`, `mmu.rs`, and translator modules to thread profiles and extension gates without board-specific logic.
- Create `hw/watchdog/src/k230.rs` for the K230 watchdog model and MMIO wrapper.
- Modify `hw/watchdog/src/lib.rs` to export `K230Wdt` alongside `SbsaGwdt`.
- Create `hw/riscv/src/k230.rs` and `hw/riscv/src/k230_boot.rs` for K230 memmap, IRQ map, board wiring, FDT/DTB fixups, BootROM reset vector, direct boot, and SDK U-Boot loader boot.
- Create `hw/riscv/src/k230_dtb.rs` for a local FDT token parser/editor that preserves the SDK DTB structure while updating `/chosen` and disabling the SDK SDHCI nodes required by QEMU's K230 flow.
- Modify `hw/riscv/src/lib.rs` to export K230 modules.
- Modify `hw/riscv/Cargo.toml` to depend on `machina-hw-watchdog` and `machina-hw-misc`.
- Modify `core/src/machine.rs` to add `dtb` and loader specs to `MachineOpts`.
- Modify `src/main.rs` to parse `-dtb`, QEMU-style `-device loader,...`, list `k230`, instantiate the right RISC-V machine, and use a board-neutral RISC-V runtime adapter.
- Create `tests/src/riscv_cpu_model.rs`, `tests/src/riscv_thead_csr.rs`, `tests/src/hw_k230_wdt.rs`, and `tests/src/hw_k230_machine.rs`.
- Modify `tests/src/lib.rs` to include those new test modules.
- Modify `tests/mtest/Cargo.toml` to depend on Machina crates needed for machine-level tests.
- Replace `tests/mtest/src/lib.rs` with K230 boot-flow tests and helpers.

## Task 1: CPU Model Profile Plumbing

**Files:**
- Create: `guest/riscv/src/riscv/cpu_model.rs`
- Modify: `guest/riscv/src/riscv/mod.rs`
- Modify: `guest/riscv/src/riscv/cpu.rs`
- Test: `tests/src/riscv_cpu_model.rs`
- Modify: `tests/src/lib.rs`

- [ ] **Step 1: Write failing CPU profile tests**

Add `tests/src/riscv_cpu_model.rs`:

```rust
use machina_guest_riscv::riscv::cpu::RiscvCpu;
use machina_guest_riscv::riscv::cpu_model::{
    RiscvCpuModel, RiscvVendor, THEAD_VENDOR_ID, THEAD_C908_MARCHID,
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
```

Add to `tests/src/lib.rs`:

```rust
#[cfg(test)]
mod riscv_cpu_model;
```

- [ ] **Step 2: Run failing CPU profile tests**

Run:

```bash
cargo test -p machina-tests riscv_cpu_model -- --nocapture
```

Expected: compile failure because `riscv::cpu_model` and
`RiscvCpu::new_with_model` do not exist.

- [ ] **Step 3: Add CPU model types**

Create `guest/riscv/src/riscv/cpu_model.rs`:

```rust
use super::ext::{MisaExt, RiscvCfg};

pub const THEAD_VENDOR_ID: u64 = 0x5b7;
pub const THEAD_C908_MARCHID: u64 = 0x8d143000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiscvVendor {
    Generic,
    Thead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiscvCpuModel {
    GenericRv64,
    TheadC908,
}

#[derive(Clone, Copy, Debug)]
pub struct RiscvCpuProfile {
    pub name: &'static str,
    pub vendor: RiscvVendor,
    pub misa: MisaExt,
    pub cfg: RiscvCfg,
    pub mvendorid: u64,
    pub marchid: u64,
    pub max_satp_mode: u64,
}

impl RiscvCpuProfile {
    pub const fn generic_rv64() -> Self {
        Self {
            name: "rv64",
            vendor: RiscvVendor::Generic,
            misa: RiscvCfg::RV64GC.misa,
            cfg: RiscvCfg::RV64GC,
            mvendorid: 0,
            marchid: 0,
            max_satp_mode: 8,
        }
    }

    pub const fn thead_c908() -> Self {
        Self {
            name: "thead-c908",
            vendor: RiscvVendor::Thead,
            misa: MisaExt::from_bits_truncate(
                MisaExt::I.bits()
                    | MisaExt::M.bits()
                    | MisaExt::A.bits()
                    | MisaExt::F.bits()
                    | MisaExt::D.bits()
                    | MisaExt::C.bits()
                    | MisaExt::S.bits()
                    | MisaExt::U.bits(),
            ),
            cfg: RiscvCfg::RV64GC,
            mvendorid: THEAD_VENDOR_ID,
            marchid: THEAD_C908_MARCHID,
            max_satp_mode: 9,
        }
    }
}

impl RiscvCpuModel {
    pub const fn profile(self) -> RiscvCpuProfile {
        match self {
            Self::GenericRv64 => RiscvCpuProfile::generic_rv64(),
            Self::TheadC908 => RiscvCpuProfile::thead_c908(),
        }
    }
}
```

Modify `guest/riscv/src/riscv/mod.rs`:

```rust
pub mod cpu_model;
```

- [ ] **Step 4: Thread profile into `RiscvCpu`**

Modify `guest/riscv/src/riscv/cpu.rs`:

```rust
use super::cpu_model::{RiscvCpuModel, RiscvCpuProfile};
```

Add a field to `RiscvCpu`:

```rust
    /// Named CPU profile used for vendor identity, CSR hooks, and extension gates.
    pub profile: RiscvCpuProfile,
```

Change `RiscvCpu::new()` and add `new_with_model()`:

```rust
impl RiscvCpu {
    pub fn new() -> Self {
        Self::new_with_model(RiscvCpuModel::GenericRv64)
    }

    pub fn new_with_model(model: RiscvCpuModel) -> Self {
        let profile = model.profile();
        Self {
            profile,
            gpr: [0u64; NUM_GPRS],
            fpr: [0u64; NUM_FPRS],
            pc: 0,
            guest_base: 0,
            load_res: u64::MAX,
            load_val: 0,
            fflags: 0,
            frm: 0,
            ustatus: USTATUS_FS_DIRTY,
            uie: 0,
            utvec: 0,
            uscratch: 0,
            uepc: 0,
            ucause: 0,
            utval: 0,
            uip: 0,
            interrupt_request: AtomicU32::new(0),
            halted: AtomicBool::new(false),
            priv_level: PrivLevel::Machine,
            csr: CsrFile::new(),
            pmp: super::pmp::Pmp::new(),
            mmu: super::mmu::Mmu::new(),
            mem_fault_cause: 0,
            mem_fault_tval: 0,
            as_ptr: 0,
            ram_base: 0,
            ram_end: 0,
            code_pages_ptr: 0,
            code_pages_len: 0,
            tb_flush_pending: false,
            last_phys_pc: 0,
            fault_pc: 0,
            jmp_env: 0,
            neg_align: AtomicI32::new(0),
            dirty_pages: Vec::new(),
        }
    }

    pub fn profile(&self) -> &RiscvCpuProfile {
        &self.profile
    }
}
```

Add `mvendorid`, `marchid`, and `max_satp_mode` storage to `CsrFile` in Task 2.
For this task, keep the profile field and constructor in place; compile errors
from missing CSR setters are resolved in Task 2 before committing.

- [ ] **Step 5: Run CPU profile tests**

Run:

```bash
cargo test -p machina-tests riscv_cpu_model -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit CPU profile plumbing**

```bash
git add guest/riscv/src/riscv/cpu_model.rs guest/riscv/src/riscv/mod.rs guest/riscv/src/riscv/cpu.rs tests/src/riscv_cpu_model.rs tests/src/lib.rs
git commit -s -m "target/riscv: add named CPU profiles"
```

## Task 2: T-HEAD CSR Vendor Layer

**Files:**
- Create: `guest/riscv/src/riscv/vendor/mod.rs`
- Create: `guest/riscv/src/riscv/vendor/thead.rs`
- Modify: `guest/riscv/src/riscv/mod.rs`
- Modify: `guest/riscv/src/riscv/csr.rs`
- Modify: `guest/riscv/src/riscv/cpu.rs`
- Test: `tests/src/riscv_thead_csr.rs`
- Modify: `tests/src/lib.rs`

- [ ] **Step 1: Write failing T-HEAD CSR tests**

Add `tests/src/riscv_thead_csr.rs`:

```rust
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
    let mx = cpu.csr.read_for_profile(CSR_TH_MXSTATUS, PrivLevel::Machine, cpu.profile()).unwrap();
    let sx = cpu.csr.read_for_profile(CSR_TH_SXSTATUS, PrivLevel::Supervisor, cpu.profile()).unwrap();
    assert_eq!(mx, TH_STATUS_UCME | TH_STATUS_THEADISAEE);
    assert_eq!(sx, TH_STATUS_UCME | TH_STATUS_THEADISAEE);
}

#[test]
fn c908_unimplemented_thead_csrs_read_zero() {
    let cpu = RiscvCpu::new_with_model(RiscvCpuModel::TheadC908);
    let mhcr = cpu.csr.read_for_profile(CSR_TH_MHCR, PrivLevel::Machine, cpu.profile()).unwrap();
    assert_eq!(mhcr, 0);
}

#[test]
fn generic_cpu_rejects_thead_csrs() {
    let cpu = RiscvCpu::new();
    assert!(cpu.csr.read_for_profile(CSR_TH_MXSTATUS, PrivLevel::Machine, cpu.profile()).is_err());
    assert!(cpu.csr.read_for_profile(CSR_TH_MHCR, PrivLevel::Machine, cpu.profile()).is_err());
}
```

Add to `tests/src/lib.rs`:

```rust
#[cfg(test)]
mod riscv_thead_csr;
```

- [ ] **Step 2: Run failing T-HEAD CSR tests**

Run:

```bash
cargo test -p machina-tests riscv_thead_csr -- --nocapture
```

Expected: compile failure because vendor CSR modules and profile-aware CSR
entry points do not exist.

- [ ] **Step 3: Add T-HEAD vendor CSR module**

Create `guest/riscv/src/riscv/vendor/mod.rs`:

```rust
pub mod thead;
```

Create `guest/riscv/src/riscv/vendor/thead.rs`:

```rust
use super::super::cpu_model::{RiscvCpuProfile, RiscvVendor};
use super::super::csr::PrivLevel;

pub const CSR_TH_MXSTATUS: u16 = 0x7c0;
pub const CSR_TH_MHCR: u16 = 0x7c1;
pub const CSR_TH_MCOR: u16 = 0x7c2;
pub const CSR_TH_MCCR2: u16 = 0x7c3;
pub const CSR_TH_MHINT: u16 = 0x7c5;
pub const CSR_TH_MRVBR: u16 = 0x7c7;
pub const CSR_TH_MCOUNTERWEN: u16 = 0x7c9;
pub const CSR_TH_MCOUNTERINTEN: u16 = 0x7ca;
pub const CSR_TH_MCOUNTEROF: u16 = 0x7cb;
pub const CSR_TH_MCINS: u16 = 0x7d2;
pub const CSR_TH_MCINDEX: u16 = 0x7d3;
pub const CSR_TH_MCDATA0: u16 = 0x7d4;
pub const CSR_TH_MCDATA1: u16 = 0x7d5;
pub const CSR_TH_MSMPR: u16 = 0x7f3;
pub const CSR_TH_CPUID: u16 = 0xfc0;
pub const CSR_TH_MAPBADDR: u16 = 0xfc1;
pub const CSR_TH_SXSTATUS: u16 = 0x5c0;
pub const CSR_TH_SHCR: u16 = 0x5c1;
pub const CSR_TH_SCER2: u16 = 0x5c2;
pub const CSR_TH_SCER: u16 = 0x5c3;
pub const CSR_TH_SCOUNTERINTEN: u16 = 0x5c4;
pub const CSR_TH_SCOUNTEROF: u16 = 0x5c5;
pub const CSR_TH_SCYCLE: u16 = 0x5e0;
pub const CSR_TH_SMIR: u16 = 0x9c0;
pub const CSR_TH_SMLO0: u16 = 0x9c1;
pub const CSR_TH_SMEH: u16 = 0x9c2;
pub const CSR_TH_SMCIR: u16 = 0x9c3;
pub const CSR_TH_FXCR: u16 = 0x800;

pub const TH_STATUS_UCME: u64 = 1 << 16;
pub const TH_STATUS_THEADISAEE: u64 = 1 << 22;

const CAUSE_ILLEGAL_INSN: u64 = 2;

fn is_thead(profile: &RiscvCpuProfile) -> bool {
    profile.vendor == RiscvVendor::Thead
}

fn require_priv(addr: u16, current: PrivLevel) -> Result<(), u64> {
    let required = match addr {
        CSR_TH_FXCR => PrivLevel::User,
        CSR_TH_SXSTATUS | CSR_TH_SHCR | CSR_TH_SCER2 | CSR_TH_SCER
        | CSR_TH_SCOUNTERINTEN | CSR_TH_SCOUNTEROF | CSR_TH_SCYCLE
        | CSR_TH_SMIR | CSR_TH_SMLO0 | CSR_TH_SMEH | CSR_TH_SMCIR
        | 0x5e3..=0x5ff => PrivLevel::Supervisor,
        _ => PrivLevel::Machine,
    };
    if current < required {
        Err(CAUSE_ILLEGAL_INSN)
    } else {
        Ok(())
    }
}

pub fn read(addr: u16, current: PrivLevel, profile: &RiscvCpuProfile) -> Result<u64, u64> {
    if !is_thead(profile) {
        return Err(CAUSE_ILLEGAL_INSN);
    }
    require_priv(addr, current)?;
    match addr {
        CSR_TH_MXSTATUS | CSR_TH_SXSTATUS => Ok(TH_STATUS_UCME | TH_STATUS_THEADISAEE),
        CSR_TH_MHCR | CSR_TH_MCOR | CSR_TH_MCCR2 | CSR_TH_MHINT | CSR_TH_MRVBR
        | CSR_TH_MCOUNTERWEN | CSR_TH_MCOUNTERINTEN | CSR_TH_MCOUNTEROF
        | CSR_TH_MCINS | CSR_TH_MCINDEX | CSR_TH_MCDATA0 | CSR_TH_MCDATA1
        | CSR_TH_MSMPR | CSR_TH_CPUID | CSR_TH_MAPBADDR | CSR_TH_SHCR
        | CSR_TH_SCER2 | CSR_TH_SCER | CSR_TH_SCOUNTERINTEN | CSR_TH_SCOUNTEROF
        | CSR_TH_SCYCLE | 0x5e3..=0x5ff | CSR_TH_SMIR | CSR_TH_SMLO0
        | CSR_TH_SMEH | CSR_TH_SMCIR | CSR_TH_FXCR => Ok(0),
        _ => Err(CAUSE_ILLEGAL_INSN),
    }
}

pub fn write(addr: u16, current: PrivLevel, profile: &RiscvCpuProfile) -> Result<(), u64> {
    let _ = read(addr, current, profile)?;
    Ok(())
}
```

Modify `guest/riscv/src/riscv/mod.rs`:

```rust
pub mod vendor;
```

- [ ] **Step 4: Add profile-aware CSR entry points and machine IDs**

Modify `guest/riscv/src/riscv/csr.rs`:

```rust
use super::cpu_model::RiscvCpuProfile;
use super::vendor;
```

Add fields to `CsrFile`:

```rust
    pub mvendorid: u64,
    pub marchid: u64,
    pub max_satp_mode: u64,
```

Initialize them:

```rust
            mvendorid: 0,
            marchid: 0,
            max_satp_mode: 8,
```

Add methods:

```rust
    pub fn set_machine_ids(&mut self, mvendorid: u64, marchid: u64) {
        self.mvendorid = mvendorid;
        self.marchid = marchid;
    }

    pub fn set_max_satp_mode(&mut self, max_satp_mode: u64) {
        self.max_satp_mode = max_satp_mode;
    }

    pub fn read_for_profile(
        &self,
        addr: u16,
        priv_level: PrivLevel,
        profile: &RiscvCpuProfile,
    ) -> Result<u64, u64> {
        match self.read(addr, priv_level) {
            Ok(value) => Ok(value),
            Err(err) => vendor::thead::read(addr, priv_level, profile).map_err(|_| err),
        }
    }

    pub fn write_for_profile(
        &mut self,
        addr: u16,
        val: u64,
        priv_level: PrivLevel,
        profile: &RiscvCpuProfile,
    ) -> Result<(), u64> {
        match self.write(addr, val, priv_level) {
            Ok(()) => Ok(()),
            Err(err) => vendor::thead::write(addr, priv_level, profile).map_err(|_| err),
        }
    }
```

Change machine-info reads:

```rust
            CSR_MVENDORID => Ok(self.mvendorid),
            CSR_MARCHID => Ok(self.marchid),
```

Change SATP write gate:

```rust
                if mode == 0 || mode <= self.max_satp_mode {
                    self.satp = val;
                }
```

- [ ] **Step 5: Use profile-aware CSR helpers in CPU execution**

Modify the private CSR handling path in `system/src/cpus.rs` to use
`read_for_profile` and `write_for_profile` when the full-system CPU handles
CSR exits. The replacement pattern is:

```rust
let profile = self.cpu.profile();
let value = self.cpu.csr.read_for_profile(csr_addr, self.cpu.priv_level, profile)?;
```

and:

```rust
let profile = *self.cpu.profile();
self.cpu.csr.write_for_profile(csr_addr, value, self.cpu.priv_level, &profile)?;
```

Use an owned copy of `profile` before mutable CSR writes to avoid aliasing the
same `RiscvCpu`.

- [ ] **Step 6: Run CSR tests**

Run:

```bash
cargo test -p machina-tests riscv_thead_csr riscv_cpu_model -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit T-HEAD CSR support**

```bash
git add guest/riscv/src/riscv/vendor guest/riscv/src/riscv/mod.rs guest/riscv/src/riscv/csr.rs guest/riscv/src/riscv/cpu.rs system/src/cpus.rs tests/src/riscv_thead_csr.rs tests/src/lib.rs
git commit -s -m "target/riscv: add T-HEAD CSR hooks"
```

## Task 3: Standard and T-HEAD Extension Gates

**Files:**
- Modify: `guest/riscv/src/riscv/ext.rs`
- Modify: `guest/riscv/src/riscv/mmu.rs`
- Modify: `guest/riscv/src/riscv/trans/mod.rs`
- Modify: `guest/riscv/src/riscv/vendor/thead.rs`
- Test: `tests/src/riscv_cpu_model.rs`
- Test: `tests/src/riscv_mmu.rs`

- [ ] **Step 1: Extend CPU model tests for C908 cfg flags**

Append to `tests/src/riscv_cpu_model.rs`:

```rust
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
    assert!(cfg.ext_xtheadsync);
}
```

- [ ] **Step 2: Add missing cfg fields**

Modify `guest/riscv/src/riscv/ext.rs` by adding fields to `RiscvCfg`:

```rust
    pub ext_svpbmt: bool,
    pub ext_svinval: bool,
    pub ext_smepmp: bool,
    pub ext_sscofpmf: bool,
    pub ext_xtheadba: bool,
    pub ext_xtheadbb: bool,
    pub ext_xtheadbs: bool,
    pub ext_xtheadcmo: bool,
    pub ext_xtheadcondmov: bool,
    pub ext_xtheadfmv: bool,
    pub ext_xtheadfmemidx: bool,
    pub ext_xtheadmac: bool,
    pub ext_xtheadmemidx: bool,
    pub ext_xtheadmempair: bool,
    pub ext_xtheadsync: bool,
```

Set all new fields to `false` in `RV64GC`. Reuse the existing
`ext_ssvnapot` field for the supervisor Svnapot extension. Add a `THEAD_C908`
profile:

```rust
    pub const THEAD_C908: Self = Self {
        ext_zicbom: true,
        ext_zicboz: true,
        ext_zba: true,
        ext_zbb: true,
        ext_zbc: true,
        ext_zbs: true,
        ext_svpbmt: true,
        ext_ssvnapot: true,
        ext_svinval: true,
        ext_sstc: true,
        ext_smepmp: true,
        ext_sscofpmf: true,
        ext_xtheadba: true,
        ext_xtheadbb: true,
        ext_xtheadbs: true,
        ext_xtheadcmo: true,
        ext_xtheadcondmov: true,
        ext_xtheadfmv: true,
        ext_xtheadfmemidx: true,
        ext_xtheadmac: true,
        ext_xtheadmemidx: true,
        ext_xtheadmempair: true,
        ext_xtheadsync: true,
        ..Self::RV64GC
    };
```

- [ ] **Step 3: Add Sv48 and Svpbmt gates to MMU**

Change `Mmu` to store `max_satp_mode` and `ext_svpbmt`:

```rust
    max_satp_mode: u64,
    ext_svpbmt: bool,
```

Add setters:

```rust
    pub fn configure_profile(&mut self, max_satp_mode: u64, ext_svpbmt: bool) {
        self.max_satp_mode = max_satp_mode;
        self.ext_svpbmt = ext_svpbmt;
    }
```

Update `RiscvCpu::new_with_model`:

```rust
            mmu: {
                let mut mmu = super::mmu::Mmu::new();
                mmu.configure_profile(profile.max_satp_mode, profile.cfg.ext_svpbmt);
                mmu
            },
```

In `translate`, accept modes up to `max_satp_mode`:

```rust
        if mode == SATP_MODE_BARE {
            return Ok(gva);
        }
        if mode != SATP_MODE_SV39 && mode != 9 {
            return Err(page_fault(access));
        }
        if mode > self.max_satp_mode {
            return Err(page_fault(access));
        }
```

For Sv48, use four levels when `mode == 9`; otherwise use three:

```rust
let levels = if self.satp_mode() == 9 { 4 } else { LEVELS };
for level in (0..levels).rev() {
    let idx = vpn_index(gva, level);
    let pte_addr = a + idx * PTE_SIZE;
    /* existing walk body */
}
```

Reject PBMT bits when `ext_svpbmt` is false and ignore PBMT bits when true:

```rust
const PTE_PBMT: u64 = 0x6000_0000_0000_0000;

if !self.ext_svpbmt && (pte & PTE_PBMT) != 0 {
    return Err(page_fault(access));
}
```

- [ ] **Step 4: Add vendor decode scaffold**

Add a helper in `guest/riscv/src/riscv/vendor/thead.rs`:

```rust
use super::super::ext::RiscvCfg;

pub fn has_xthead(cfg: RiscvCfg) -> bool {
    cfg.ext_xtheadba
        || cfg.ext_xtheadbb
        || cfg.ext_xtheadbs
        || cfg.ext_xtheadcmo
        || cfg.ext_xtheadcondmov
        || cfg.ext_xtheadfmv
        || cfg.ext_xtheadfmemidx
        || cfg.ext_xtheadmac
        || cfg.ext_xtheadmemidx
        || cfg.ext_xtheadmempair
        || cfg.ext_xtheadsync
}
```

In `guest/riscv/src/riscv/trans/mod.rs`, add a vendor decode hook that returns
`false` before generic illegal-instruction handling:

```rust
fn decode_vendor_thead(ctx: &mut RiscvDisasContext, insn: u32) -> bool {
    if !crate::riscv::vendor::thead::has_xthead(ctx.cfg) {
        return false;
    }
    let _ = insn;
    false
}
```

Call `decode_vendor_thead(ctx, insn)` in the decode path before the final
illegal instruction case. This establishes vendor-isolated control flow without
claiming unimplemented instruction semantics.

- [ ] **Step 5: Run extension and MMU tests**

Run:

```bash
cargo test -p machina-tests riscv_cpu_model riscv_mmu -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit extension gates**

```bash
git add guest/riscv/src/riscv/ext.rs guest/riscv/src/riscv/mmu.rs guest/riscv/src/riscv/cpu.rs guest/riscv/src/riscv/trans/mod.rs guest/riscv/src/riscv/vendor/thead.rs tests/src/riscv_cpu_model.rs tests/src/riscv_mmu.rs
git commit -s -m "target/riscv: add C908 extension gates"
```

## Task 4: K230 Watchdog Device

**Files:**
- Create: `hw/watchdog/src/k230.rs`
- Modify: `hw/watchdog/src/lib.rs`
- Test: `tests/src/hw_k230_wdt.rs`
- Modify: `tests/src/lib.rs`

- [ ] **Step 1: Write failing K230 WDT tests**

Add `tests/src/hw_k230_wdt.rs`:

```rust
use machina_hw_watchdog::k230::{
    K230Wdt, K230WdtMmio, CR, CR_RMOD, CR_RPL_MASK, CR_RPL_SHIFT, CR_WDT_EN,
    CRR, CRR_RESTART, EOI, PROT_LEVEL, STAT, STAT_INT, TORR, TORR_TOP_MASK,
};
use machina_memory::region::MmioOps;

#[test]
fn k230_wdt_masks_writable_registers() {
    let wdt = K230Wdt::new_named("k230-wdt0");
    let mmio = K230WdtMmio(wdt);
    mmio.write(CR, 4, u64::MAX);
    assert_eq!(
        mmio.read(CR, 4),
        ((CR_RPL_MASK << CR_RPL_SHIFT) | CR_RMOD | CR_WDT_EN) as u64
    );
    mmio.write(TORR, 4, u64::MAX);
    assert_eq!(mmio.read(TORR, 4), TORR_TOP_MASK as u64);
    mmio.write(PROT_LEVEL, 4, u64::MAX);
    assert_eq!(mmio.read(PROT_LEVEL, 4), 0x7);
}

#[test]
fn k230_wdt_interrupt_mode_sets_and_clears_status() {
    let wdt = K230Wdt::new_named("k230-wdt0");
    let mmio = K230WdtMmio(wdt.clone());
    mmio.write(TORR, 4, 1);
    mmio.write(CR, 4, (CR_RMOD | CR_WDT_EN) as u64);
    wdt.step_timer(1 << 17);
    assert_eq!(mmio.read(STAT, 4) & STAT_INT as u64, STAT_INT as u64);
    mmio.write(EOI, 4, 1);
    assert_eq!(mmio.read(STAT, 4) & STAT_INT as u64, 0);
}

#[test]
fn k230_wdt_restart_magic_clears_pending_interrupt() {
    let wdt = K230Wdt::new_named("k230-wdt0");
    let mmio = K230WdtMmio(wdt.clone());
    mmio.write(TORR, 4, 1);
    mmio.write(CR, 4, (CR_RMOD | CR_WDT_EN) as u64);
    wdt.step_timer(1 << 17);
    mmio.write(CRR, 4, CRR_RESTART as u64);
    assert_eq!(mmio.read(STAT, 4) & STAT_INT as u64, 0);
}
```

Add to `tests/src/lib.rs`:

```rust
#[cfg(test)]
mod hw_k230_wdt;
```

- [ ] **Step 2: Run failing WDT tests**

Run:

```bash
cargo test -p machina-tests hw_k230_wdt -- --nocapture
```

Expected: compile failure because `machina_hw_watchdog::k230` does not exist.

- [ ] **Step 3: Implement K230 WDT**

Create `hw/watchdog/src/k230.rs` with constants and struct:

```rust
use std::sync::Arc;

use machina_core::device_cell::DeviceRegs;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_hw_timer::{Ptimer, PtimerCallback};
use machina_memory::region::MmioOps;

pub const CR: u64 = 0x00;
pub const TORR: u64 = 0x04;
pub const CCVR: u64 = 0x08;
pub const CRR: u64 = 0x0c;
pub const STAT: u64 = 0x10;
pub const EOI: u64 = 0x14;
pub const PROT_LEVEL: u64 = 0x1c;
pub const COMP_PARAM_5: u64 = 0xe4;
pub const COMP_PARAM_4: u64 = 0xe8;
pub const COMP_PARAM_3: u64 = 0xec;
pub const COMP_PARAM_2: u64 = 0xf0;
pub const COMP_PARAM_1: u64 = 0xf4;
pub const COMP_VERSION: u64 = 0xf8;
pub const COMP_TYPE: u64 = 0xfc;
pub const MMIO_SIZE: u64 = 0x100;
pub const CR_RPL_MASK: u32 = 0x7;
pub const CR_RPL_SHIFT: u32 = 2;
pub const CR_RMOD: u32 = 1 << 1;
pub const CR_WDT_EN: u32 = 1 << 0;
pub const TORR_TOP_MASK: u32 = 0xf;
pub const STAT_INT: u32 = 1 << 0;
pub const CRR_RESTART: u32 = 0x76;
pub const COMP_TYPE_VAL: u32 = 0x4457_0120;
pub const COMP_VERSION_VAL: u32 = 0x3131_302a;

#[derive(Clone, Copy)]
struct K230WdtRegs {
    cr: u32,
    torr: u32,
    current_count: u32,
    stat: u32,
    prot_level: u32,
    timeout_value: u32,
    interrupt_pending: bool,
    enabled: bool,
}

impl Default for K230WdtRegs {
    fn default() -> Self {
        Self {
            cr: 0,
            torr: 0,
            current_count: u32::MAX,
            stat: 0,
            prot_level: 0x2,
            timeout_value: 0,
            interrupt_pending: false,
            enabled: false,
        }
    }
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot", irq = "manual", before_unrealize = [lower_irq, stop_timer])]
pub struct K230Wdt {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRegs<K230WdtRegs>,
    irq: parking_lot::Mutex<Option<InterruptSource>>,
    timer: Arc<Ptimer>,
}
```

Implement `new_named`, `reset_runtime`, `connect_irq`, `read`, `write`,
`step_timer`, `handle_timeout`, `raise_irq`, and `lower_irq`. Use QEMU's
timeout formula `1 << (16 + top)` for TOP values 0..15. Use `Ptimer::step` for
deterministic tests and restart the counter after timeout.

Export from `hw/watchdog/src/lib.rs`:

```rust
pub mod k230;
```

- [ ] **Step 4: Run WDT tests**

Run:

```bash
cargo test -p machina-tests hw_k230_wdt -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit K230 WDT**

```bash
git add hw/watchdog/src/k230.rs hw/watchdog/src/lib.rs tests/src/hw_k230_wdt.rs tests/src/lib.rs
git commit -s -m "hw/watchdog: add K230 watchdog"
```

## Task 5: K230 Machine Memmap and Device Wiring

**Files:**
- Create: `hw/riscv/src/k230.rs`
- Modify: `hw/riscv/src/lib.rs`
- Modify: `hw/riscv/Cargo.toml`
- Test: `tests/src/hw_k230_machine.rs`
- Modify: `tests/src/lib.rs`

- [ ] **Step 1: Write failing K230 machine tests**

Add `tests/src/hw_k230_machine.rs`:

```rust
use machina_core::machine::{Machine, MachineOpts};
use machina_hw_riscv::k230::{
    K230Machine, K230MemMap, K230IrqMap, K230WdtIndex, K230_MEMMAP,
    K230_PLIC_NUM_SOURCES,
};

fn opts() -> MachineOpts {
    MachineOpts {
        ram_size: 0x8000_0000,
        cpu_count: 1,
        kernel: None,
        bios: Some("none".into()),
        bios_builtin: false,
        append: None,
        nographic: true,
        drive: None,
        initrd: None,
        netdev: None,
    }
}

#[test]
fn k230_memmap_matches_qemu_reference_points() {
    assert_eq!(K230_MEMMAP[K230MemMap::Ddr as usize].base, 0x0000_0000);
    assert_eq!(K230_MEMMAP[K230MemMap::Sram as usize].base, 0x8020_0000);
    assert_eq!(K230_MEMMAP[K230MemMap::Bootrom as usize].base, 0x9120_0000);
    assert_eq!(K230_MEMMAP[K230MemMap::Plic as usize].base, 0x0f00_0000_00);
    assert_eq!(K230_MEMMAP[K230MemMap::Clint as usize].base, 0x0f04_0000_00);
    assert_eq!(K230_PLIC_NUM_SOURCES, 208);
    assert_eq!(K230IrqMap::UART0, 16);
    assert_eq!(K230IrqMap::WDT0, 107);
}

#[test]
fn k230_machine_maps_real_devices_and_unimp_windows() {
    let mut machine = K230Machine::new();
    machine.init(&opts()).unwrap();
    let sysbus = machine.sysbus();
    assert!(sysbus.mappings().iter().any(|m| m.owner == "plic0"));
    assert!(sysbus.mappings().iter().any(|m| m.owner == "aclint0"));
    assert!(sysbus.mappings().iter().any(|m| m.owner == "uart0"));
    assert!(sysbus.mappings().iter().any(|m| m.owner == "uart4"));
    assert!(sysbus.mappings().iter().any(|m| m.owner == "k230-wdt0"));
    assert!(sysbus.mappings().iter().any(|m| m.owner == "k230-wdt1"));
    assert!(sysbus.mappings().iter().any(|m| m.owner == "kpu.l2-cache"));
    assert!(machine.wdt(K230WdtIndex::Wdt0).is_some());
    assert!(machine.wdt(K230WdtIndex::Wdt1).is_some());
}
```

Add to `tests/src/lib.rs`:

```rust
#[cfg(test)]
mod hw_k230_machine;
```

- [ ] **Step 2: Run failing K230 machine tests**

Run:

```bash
cargo test -p machina-tests hw_k230_machine -- --nocapture
```

Expected: compile failure because `machina_hw_riscv::k230` does not exist.

- [ ] **Step 3: Add K230 module and dependencies**

Modify `hw/riscv/Cargo.toml`:

```toml
machina-hw-watchdog = { workspace = true }
machina-hw-misc = { workspace = true }
parking_lot = { workspace = true }
```

Modify `hw/riscv/src/lib.rs`:

```rust
pub mod k230;
```

- [ ] **Step 4: Implement K230 constants and machine skeleton**

Create `hw/riscv/src/k230.rs` with:

```rust
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::sync::atomic::{AtomicU64, Ordering};

use machina_core::address::GPA;
use machina_core::machine::{Machine, MachineOpts, MachineState};
use machina_core::mobject::{MObjectInfo, MObjectNode, MObjectTree};
use machina_core::wfi::WfiWaker;
use machina_guest_riscv::riscv::cpu::RiscvCpu;
use machina_guest_riscv::riscv::cpu_model::RiscvCpuModel;
use machina_hw_char::uart::{Uart16550, Uart16550Mmio};
use machina_hw_core::bus::{SysBus, SysBusMapping};
use machina_hw_core::chardev::{ChardevObject, NullChardev};
use machina_hw_core::irq::{InterruptSource, IrqLine, IrqSink};
use machina_hw_intc::aclint::{Aclint, AclintMmio};
use machina_hw_intc::plic::{Plic, PlicIrqSink, PlicMmio};
use machina_hw_misc::unimp::{Unimp, UnimpMmio};
use machina_hw_watchdog::k230::{K230Wdt, K230WdtMmio};
use machina_memory::address_space::AddressSpace;
use machina_memory::ram::RamBlock;
use machina_memory::region::{MemoryRegion, MmioOps};

#[derive(Clone, Copy)]
pub struct MemMapEntry {
    pub base: u64,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum K230MemMap {
    Ddr = 0,
    KpuL2Cache,
    Sram,
    KpuCfg,
    Fft,
    Ai2d,
    Gsdma,
    Dma,
    Gzip,
    NonAi2d,
    Isp,
    Dewarp,
    RxCsi,
    H264,
    Gpu2p5d,
    Vo,
    VoCfg,
    Engine3d,
    Pmu,
    Rtc,
    Cmu,
    Rmu,
    Boot,
    Pwr,
    Mailbox,
    Iomux,
    Timer,
    Wdt0,
    Wdt1,
    Ts,
    Hdi,
    Stc,
    Bootrom,
    Security,
    Uart0,
    Uart1,
    Uart2,
    Uart3,
    Uart4,
    I2c0,
    I2c1,
    I2c2,
    I2c3,
    I2c4,
    Pwm,
    Gpio0,
    Gpio1,
    Adc,
    Codec,
    I2s,
    Usb0,
    Usb1,
    Sd0,
    Sd1,
    Qspi0,
    Qspi1,
    Spi,
    HiSysCfg,
    DdrcCfg,
    Flash,
    Plic,
    Clint,
    Count,
}
```

Fill `K230_MEMMAP` with the QEMU base/size table from the spec. Add:

```rust
pub const K230_PLIC_NUM_SOURCES: u32 = 208;
pub const K230_PLIC_NUM_PRIORITIES: u32 = 7;
pub const K230_UART_COUNT: usize = 5;

pub struct K230IrqMap;
impl K230IrqMap {
    pub const UART0: u32 = 16;
    pub const UART4: u32 = 20;
    pub const WDT0: u32 = 107;
    pub const WDT1: u32 = 108;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum K230WdtIndex {
    Wdt0,
    Wdt1,
}
```

Implement `K230Machine` following `RefMachine` patterns, but create
`RiscvCpu::new_with_model(RiscvCpuModel::TheadC908)` for CPU0.

- [ ] **Step 5: Implement real device and unimp wiring**

Add helpers in `K230Machine`:

```rust
fn map_unimp(
    sysbus: &mut SysBus,
    root: &mut MemoryRegion,
    name: &str,
    entry: MemMapEntry,
) -> Result<Arc<Unimp>, Box<dyn std::error::Error>> {
    let dev = Unimp::new(name, entry.size);
    dev.attach_to_bus(sysbus)?;
    let region = MemoryRegion::io(name, entry.size, Arc::new(UnimpMmio(Arc::clone(&dev))));
    dev.register_mmio(region, GPA::new(entry.base))?;
    Ok(dev)
}

fn plic_irq_line(plic: &Arc<Plic>, source: u32) -> IrqLine {
    let sink = Arc::new(PlicIrqSink(Arc::clone(plic)));
    IrqLine::new(sink as Arc<dyn IrqSink>, source)
}
```

Map all real devices and placeholders. Ensure WDTs use `K230WdtMmio` with
`MMIO_SIZE` inside the 0x800 SDK WDT window and record the broader window
behavior consistently with the memory region mapper.

- [ ] **Step 6: Run K230 machine tests**

Run:

```bash
cargo test -p machina-tests hw_k230_machine -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit K230 machine wiring**

```bash
git add hw/riscv/Cargo.toml hw/riscv/src/k230.rs hw/riscv/src/lib.rs tests/src/hw_k230_machine.rs tests/src/lib.rs
git commit -s -m "hw/riscv: add K230 machine skeleton"
```

## Task 6: CLI `-dtb` and Loader Specs

**Files:**
- Modify: `core/src/machine.rs`
- Modify: `src/main.rs`
- Test: `tests/src/cli_kernel.rs`
- Test: `tests/mtest/src/lib.rs`

- [ ] **Step 1: Write failing CLI parse tests**

Add tests in `tests/mtest/src/lib.rs`:

```rust
use machina_core::machine::LoaderSpec;

#[test]
fn loader_spec_parses_qemu_loader_syntax() {
    let spec = LoaderSpec::parse("loader,file=/tmp/fw.uImage,addr=0x0c100000,force-raw=on").unwrap();
    assert_eq!(spec.file.to_str(), Some("/tmp/fw.uImage"));
    assert_eq!(spec.addr, 0x0c10_0000);
    assert!(spec.force_raw);
}

#[test]
fn loader_spec_rejects_missing_file() {
    let err = LoaderSpec::parse("loader,addr=0x1000,force-raw=on").unwrap_err();
    assert!(err.contains("missing file="));
}
```

- [ ] **Step 2: Run failing parser tests**

Run:

```bash
cargo test -p machina-mtest loader_spec -- --nocapture
```

Expected: compile failure because `LoaderSpec` does not exist.

- [ ] **Step 3: Add `LoaderSpec` and MachineOpts fields**

Modify `core/src/machine.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoaderSpec {
    pub file: PathBuf,
    pub addr: u64,
    pub force_raw: bool,
}

impl LoaderSpec {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut parts = raw.split(',');
        match parts.next() {
            Some("loader") => {}
            _ => return Err("-device: only loader devices are supported by this parser".to_string()),
        }
        let mut file = None;
        let mut addr = None;
        let mut force_raw = false;
        for part in parts {
            if let Some(value) = part.strip_prefix("file=") {
                file = Some(PathBuf::from(value));
            } else if let Some(value) = part.strip_prefix("addr=") {
                let trimmed = value.strip_prefix("0x").unwrap_or(value);
                let parsed = u64::from_str_radix(trimmed, 16)
                    .or_else(|_| value.parse::<u64>())
                    .map_err(|_| format!("loader addr is invalid: {value}"))?;
                addr = Some(parsed);
            } else if let Some(value) = part.strip_prefix("force-raw=") {
                force_raw = matches!(value, "on" | "true" | "1");
            } else {
                return Err(format!("unsupported loader option: {part}"));
            }
        }
        Ok(Self {
            file: file.ok_or("loader: missing file=".to_string())?,
            addr: addr.ok_or("loader: missing addr=".to_string())?,
            force_raw,
        })
    }
}
```

Extend `MachineOpts`:

```rust
    pub dtb: Option<PathBuf>,
    pub loaders: Vec<LoaderSpec>,
```

Update all existing `MachineOpts` constructors in tests and `src/main.rs`.

- [ ] **Step 4: Parse `-dtb` and loader devices in CLI**

Modify `src/main.rs` `Cli`:

```rust
    dtb: Option<PathBuf>,
    loaders: Vec<LoaderSpec>,
```

Add defaults:

```rust
            dtb: None,
            loaders: Vec::new(),
```

Add parser arms:

```rust
            "-dtb" => {
                i += 1;
                let s = args.get(i).ok_or("-dtb requires argument")?;
                cli.dtb = Some(PathBuf::from(s));
            }
            "-device" => {
                i += 1;
                let raw = args.get(i).ok_or("-device requires argument")?;
                if raw.starts_with("loader,") {
                    cli.loaders.push(LoaderSpec::parse(raw)?);
                } else if raw.starts_with("virtio-net-device") {
                    cli.device_net_raw = Some(raw.clone());
                } else {
                    return Err(format!("-device: unsupported device: {raw}"));
                }
            }
```

Thread into `MachineOpts`:

```rust
        dtb: cli.dtb.clone(),
        loaders: cli.loaders.clone(),
```

- [ ] **Step 5: Run parser tests**

Run:

```bash
cargo test -p machina-mtest loader_spec -- --nocapture
cargo test -p machina-tests cli_kernel -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit CLI loader plumbing**

```bash
git add core/src/machine.rs src/main.rs tests/mtest/Cargo.toml tests/mtest/src/lib.rs tests/src
git commit -s -m "machine: add DTB and loader options"
```

## Task 7: Board-Neutral RISC-V Runtime and `-M k230`

**Files:**
- Modify: `src/main.rs`
- Modify: `hw/riscv/src/ref_machine.rs`
- Modify: `hw/riscv/src/k230.rs`
- Test: `tests/src/hw_k230_machine.rs`
- Test: `tests/src/cli_kernel.rs`

- [ ] **Step 1: Write failing machine selection tests**

Add a CLI test in `tests/src/cli_kernel.rs`:

```rust
#[test]
fn machine_help_lists_k230() {
    let exe = env!("CARGO_BIN_EXE_machina");
    let output = std::process::Command::new(exe)
        .args(["-M", "?"])
        .output()
        .expect("run machina -M ?");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("k230"));
}
```

- [ ] **Step 2: Add a runtime adapter trait**

In `src/main.rs`, define a private trait:

```rust
trait RiscvRuntimeMachine: Machine {
    fn take_cpu(&self, idx: usize) -> Option<RiscvCpu>;
    fn ram_base(&self) -> u64;
    fn ram_ptr(&self) -> *const u8;
    fn bootrom_ptr(&self) -> *const u8;
    fn bootrom_base(&self) -> u64;
    fn bootrom_size(&self) -> u64;
    fn address_space(&self) -> &machina_memory::address_space::AddressSpace;
    fn shared_mip(&self) -> Arc<AtomicU64>;
    fn wfi_waker(&self) -> Arc<WfiWaker>;
    fn connect_timer_exit_request(&self, hart: u32, request: Arc<dyn Fn() + Send + Sync>);
    fn uart_for_sbi(&self) -> Option<Arc<Uart16550>>;
    fn aclint_for_sbi(&self) -> Option<Arc<Aclint>>;
}
```

Implement it for `RefMachine` and `K230Machine`. For `RefMachine`, use existing
methods and `MROM_BASE`/`MROM_SIZE`. For `K230Machine`, use K230 BootROM
base/size and its ACLINT/UART0.

- [ ] **Step 3: Genericize `run_machine_cycle`**

Replace hardcoded `RefMachine::new()` with:

```rust
enum RiscvMachineKind {
    Ref(RefMachine),
    K230(K230Machine),
}
```

Implement helper methods that delegate to `RiscvRuntimeMachine`, or use a boxed
trait object:

```rust
let mut machine: Box<dyn RiscvRuntimeMachine> = match machine_name {
    "riscv64-ref" => Box::new(RefMachine::new()),
    "k230" => Box::new(K230Machine::new()),
    other => return Some(ShutdownReason::Fail(format!("unknown machine {other}").len() as u64)),
};
```

Use `machine.ram_base()` instead of `machina_hw_riscv::ref_machine::RAM_BASE`
and use `machine.bootrom_*()` instead of hardcoded MROM access.

- [ ] **Step 4: Add `k230` to CLI selection**

Modify `src/main.rs`:

```rust
eprintln!("  k230           Kendryte K230 SDK-compatible RISC-V machine");
```

Accept it:

```rust
if cli.machine != "riscv64-ref" && cli.machine != "loongarch64-ref" && cli.machine != "k230" {
    eprintln!("machina: unknown machine: {}", cli.machine);
    restore_terminal();
    process::exit(1);
}
```

Reject unsupported features clearly:

```rust
if cli.machine == "k230" && cli.difftest {
    eprintln!("machina: --difftest is not yet supported by k230");
    restore_terminal();
    process::exit(1);
}
```

- [ ] **Step 5: Run machine selection tests**

Run:

```bash
cargo test -p machina-tests machine_help_lists_k230 hw_k230_machine -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit runtime selection**

```bash
git add src/main.rs hw/riscv/src/ref_machine.rs hw/riscv/src/k230.rs tests/src/cli_kernel.rs tests/src/hw_k230_machine.rs
git commit -s -m "riscv: add K230 machine selection"
```

## Task 8: K230 Direct Linux Boot and DTB Fixups

**Files:**
- Create: `hw/riscv/src/k230_boot.rs`
- Create: `hw/riscv/src/k230_dtb.rs`
- Modify: `hw/riscv/src/k230.rs`
- Modify: `hw/riscv/src/lib.rs`
- Modify: `hw/riscv/Cargo.toml`
- Test: `tests/mtest/src/lib.rs`

- [ ] **Step 1: Add direct boot mtest fixtures**

Extend `tests/mtest/Cargo.toml`:

```toml
machina-hw-riscv = { workspace = true }
machina-core = { workspace = true }
tempfile = { workspace = true }
```

Add `tests/mtest/src/lib.rs` tests:

```rust
use machina_core::machine::{Machine, MachineOpts};
use machina_hw_riscv::k230::K230Machine;

#[test]
fn k230_direct_boot_rejects_initrd_without_dtb() {
    let dir = tempfile::tempdir().unwrap();
    let initrd = dir.path().join("rootfs.cpio.gz");
    std::fs::write(&initrd, b"initrd").unwrap();
    let mut machine = K230Machine::new();
    machine.init(&MachineOpts {
        ram_size: 0x8000_0000,
        cpu_count: 1,
        kernel: None,
        bios: Some("none".into()),
        bios_builtin: false,
        append: None,
        nographic: true,
        drive: None,
        initrd: Some(initrd),
        netdev: None,
        dtb: None,
        loaders: Vec::new(),
    }).unwrap();
    let err = machine.boot().unwrap_err().to_string();
    assert!(err.contains("-initrd requires -dtb for the k230 machine"));
}
```

- [ ] **Step 2: Run failing direct boot mtest**

Run:

```bash
cargo test -p machina-mtest k230_direct_boot -- --nocapture
```

Expected: compile failure or test failure because `K230Machine::boot` does not
implement direct boot.

- [ ] **Step 3: Implement K230 boot helpers**

Create `hw/riscv/src/k230_boot.rs`:

```rust
use machina_core::address::GPA;
use machina_hw_core::loader;

use crate::k230::{K230Machine, K230MemMap, K230_MEMMAP};

pub const K230_BOOTROM_BASE: u64 = K230_MEMMAP[K230MemMap::Bootrom as usize].base;
pub const K230_BOOTROM_SIZE: u64 = K230_MEMMAP[K230MemMap::Bootrom as usize].size;

pub fn boot_k230(machine: &mut K230Machine) -> Result<(), Box<dyn std::error::Error>> {
    if machine.initrd_path().is_some() && machine.dtb_path().is_none() {
        return Err("-initrd requires -dtb for the k230 machine".into());
    }
    let start_addr = load_bios_or_kernel(machine)?;
    let fdt_addr = load_and_fix_user_dtb(machine)?;
    write_k230_reset_vec(machine, start_addr, fdt_addr)?;
    machine.set_boot_cpu_pc(K230_BOOTROM_BASE);
    Ok(())
}

fn load_bios_or_kernel(machine: &mut K230Machine) -> Result<u64, Box<dyn std::error::Error>> {
    if let Some(path) = machine.bios_path().filter(|p| p.to_str() != Some("none")) {
        let data = std::fs::read(path)?;
        loader::load_binary(&data, GPA::new(K230_MEMMAP[K230MemMap::Ddr as usize].base), machine.address_space())?;
        return Ok(K230_MEMMAP[K230MemMap::Ddr as usize].base);
    }
    if let Some(path) = machine.kernel_path() {
        let data = std::fs::read(path)?;
        let addr = K230_MEMMAP[K230MemMap::Ddr as usize].base;
        loader::load_binary(&data, GPA::new(addr), machine.address_space())?;
        return Ok(addr);
    }
    Ok(K230_MEMMAP[K230MemMap::Ddr as usize].base)
}
```

Create `hw/riscv/src/k230_dtb.rs` with a local FDT helper. The helper parses
the DTB header, walks the structure block tokens, resolves property names from
the strings block, and rebuilds a new blob with the requested property
replacements. It must expose this API:

```rust
pub fn fixup_k230_dtb(
    blob: &[u8],
    initrd: Option<(u64, u64)>,
    cmdline: Option<&str>,
) -> Result<Vec<u8>, String>;

pub fn dtb_node_status(blob: &[u8], path: &str) -> Result<Option<String>, String>;

#[cfg(test)]
pub fn test_fixture_dtb_with_sdhci_nodes() -> Vec<u8>;
```

`fixup_k230_dtb` must preserve all nodes from the SDK DTB and apply these
exact changes:

```text
/chosen/bootargs = cmdline.unwrap_or("")
/chosen/linux,initrd-start = initrd.map(|(start, _)| start)
/chosen/linux,initrd-end = initrd.map(|(_, end)| end)
/soc/sdhci0@91580000/status = "disabled"
/soc/sdhci1@91581000/status = "disabled"
```

Use `machina_hw_core::fdt::FdtBuilder` only for the `#[cfg(test)]` fixture
builder. Do not generate a new K230 DTB for normal boot; the machine must load
the user-provided SDK DTB, mutate it with `fixup_k230_dtb`, and place the fixed
blob in RAM.

Modify `hw/riscv/src/lib.rs`:

```rust
pub mod k230_boot;
pub mod k230_dtb;
```

The public machine behavior must expose testable methods:

```rust
impl K230Machine {
    pub fn dtb_blob(&self) -> Option<&[u8]>;
    pub fn dtb_path(&self) -> Option<&std::path::PathBuf>;
    pub fn initrd_path(&self) -> Option<&std::path::PathBuf>;
    pub fn kernel_path(&self) -> Option<&std::path::PathBuf>;
    pub fn bios_path(&self) -> Option<&std::path::PathBuf>;
}
```

- [ ] **Step 4: Add DTB chosen/fixup tests**

Add fixture tests in `tests/mtest/src/lib.rs` that call a K230 helper with a
small valid DTB blob:

```rust
#[test]
fn k230_dtb_fixup_disables_sdk_sdhci_nodes() {
    let mut blob = machina_hw_riscv::k230_dtb::test_fixture_dtb_with_sdhci_nodes();
    blob = machina_hw_riscv::k230_dtb::fixup_k230_dtb(
        &blob,
        Some((0x0a10_0000, 0x0a20_0000)),
        Some("console=ttyS0,115200 earlycon=sbi cma=0"),
    ).unwrap();
    assert_eq!(
        machina_hw_riscv::k230_dtb::dtb_node_status(&blob, "/soc/sdhci0@91580000").unwrap(),
        Some("disabled".to_string()),
    );
    assert_eq!(
        machina_hw_riscv::k230_dtb::dtb_node_status(&blob, "/soc/sdhci1@91581000").unwrap(),
        Some("disabled".to_string()),
    );
    assert!(blob.windows(b"console=ttyS0,115200 earlycon=sbi cma=0".len()).any(
        |w| w == b"console=ttyS0,115200 earlycon=sbi cma=0"
    ));
}
```

- [ ] **Step 5: Run direct boot tests**

Run:

```bash
cargo test -p machina-mtest k230_direct_boot k230_dtb_fixup -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit direct boot**

```bash
git add hw/riscv/Cargo.toml hw/riscv/src/k230_boot.rs hw/riscv/src/k230_dtb.rs hw/riscv/src/k230.rs hw/riscv/src/lib.rs tests/mtest/Cargo.toml tests/mtest/src/lib.rs
git commit -s -m "hw/riscv: add K230 direct boot"
```

## Task 9: K230 SDK U-Boot Loader Boot

**Files:**
- Modify: `hw/riscv/src/k230_boot.rs`
- Modify: `hw/riscv/src/k230.rs`
- Test: `tests/mtest/src/lib.rs`

- [ ] **Step 1: Write loader placement mtests**

Add to `tests/mtest/src/lib.rs`:

```rust
use machina_core::machine::LoaderSpec;

#[test]
fn k230_loader_boot_places_sdk_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let fw = dir.path().join("fw.uImage");
    let image = dir.path().join("Image");
    let initrd = dir.path().join("rootfs.cpio.gz");
    let dtb = dir.path().join("k230.dtb");
    std::fs::write(&fw, [0x11, 0x22, 0x33, 0x44]).unwrap();
    std::fs::write(&image, [0x55, 0x66, 0x77, 0x88]).unwrap();
    std::fs::write(&initrd, [0xaa, 0xbb, 0xcc, 0xdd]).unwrap();
    std::fs::write(&dtb, [0xde, 0xad, 0xbe, 0xef]).unwrap();

    let mut machine = K230Machine::new();
    machine.init(&MachineOpts {
        ram_size: 0x8000_0000,
        cpu_count: 1,
        kernel: None,
        bios: Some("none".into()),
        bios_builtin: false,
        append: None,
        nographic: true,
        drive: None,
        initrd: None,
        netdev: None,
        dtb: None,
        loaders: vec![
            LoaderSpec { file: fw.clone(), addr: 0x0c10_0000, force_raw: true },
            LoaderSpec { file: image.clone(), addr: 0x0820_0000, force_raw: true },
            LoaderSpec { file: initrd.clone(), addr: 0x0a10_0000, force_raw: true },
            LoaderSpec { file: dtb.clone(), addr: 0x0a00_0000, force_raw: true },
        ],
    }).unwrap();
    machine.boot().unwrap();
    assert_eq!(machine.read_ram_bytes(0x0c10_0000, 4).unwrap(), vec![0x11, 0x22, 0x33, 0x44]);
    assert_eq!(machine.read_ram_bytes(0x0820_0000, 4).unwrap(), vec![0x55, 0x66, 0x77, 0x88]);
    assert_eq!(machine.read_ram_bytes(0x0a10_0000, 4).unwrap(), vec![0xaa, 0xbb, 0xcc, 0xdd]);
    assert_eq!(machine.read_ram_bytes(0x0a00_0000, 4).unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
}
```

- [ ] **Step 2: Run failing loader placement mtests**

Run:

```bash
cargo test -p machina-mtest k230_loader_boot -- --nocapture
```

Expected: failure because loader specs are not applied by K230 boot yet.

- [ ] **Step 3: Apply loader specs before CPU execution**

Modify `hw/riscv/src/k230_boot.rs`:

```rust
fn apply_loaders(machine: &K230Machine) -> Result<(), Box<dyn std::error::Error>> {
    for loader_spec in machine.loaders() {
        if !loader_spec.force_raw {
            return Err("k230 loader requires force-raw=on".into());
        }
        let data = std::fs::read(&loader_spec.file)?;
        loader::load_binary(&data, GPA::new(loader_spec.addr), machine.address_space())?;
    }
    Ok(())
}
```

Call `apply_loaders(machine)?` in `boot_k230()` before reset vector setup.

Add accessors to `K230Machine`:

```rust
pub fn loaders(&self) -> &[machina_core::machine::LoaderSpec] {
    &self.loaders
}

pub fn read_ram_bytes(&self, gpa: u64, len: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let base = K230_MEMMAP[K230MemMap::Ddr as usize].base;
    let offset = gpa.checked_sub(base).ok_or("address below K230 DDR")?;
    let block = self.ram_block();
    if offset + len as u64 > block.size() {
        return Err("read exceeds K230 DDR".into());
    }
    let mut out = vec![0u8; len];
    unsafe {
        std::ptr::copy_nonoverlapping(block.as_ptr().add(offset as usize), out.as_mut_ptr(), len);
    }
    Ok(out)
}
```

The `unsafe` block matches existing RAM copy patterns and must stay in board
test helper code, not in device models.

- [ ] **Step 4: Run loader boot tests**

Run:

```bash
cargo test -p machina-mtest k230_loader_boot -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit loader boot**

```bash
git add hw/riscv/src/k230_boot.rs hw/riscv/src/k230.rs tests/mtest/src/lib.rs
git commit -s -m "hw/riscv: add K230 loader boot"
```

## Task 10: QEMU Slice Comparison Tests

**Files:**
- Modify: `tests/mtest/Cargo.toml`
- Modify: `tests/mtest/src/lib.rs`

- [ ] **Step 1: Add QEMU availability helper**

Add to `tests/mtest/Cargo.toml`:

```toml
machina-oracle = { workspace = true }
```

Add to `tests/mtest/src/lib.rs`:

```rust
fn qemu_system_riscv64() -> Option<String> {
    machina_oracle::qemu::find_qemu("riscv64")
}
```

- [ ] **Step 2: Add WDT qtest slice comparison**

Add a test that probes QEMU's K230 WDT masking with qtest when QEMU exists:

```rust
#[test]
fn qemu_k230_wdt_register_mask_slice_matches_machina() {
    use machina_hw_watchdog::k230::{
        K230Wdt, K230WdtMmio, CR, PROT_LEVEL, TORR,
    };
    use machina_memory::region::MmioOps;
    use machina_oracle::qemu::QemuProbe;

    let Some(qemu) = qemu_system_riscv64() else {
        eprintln!("skip: MACHINA_QEMU_SYSTEM_RISCV64/qemu-system-riscv64 not available");
        return;
    };
    let extra = vec![
        "-accel".to_string(),
        "qtest".to_string(),
        "-bios".to_string(),
        "none".to_string(),
    ];
    let mut probe = match QemuProbe::spawn(&qemu, "k230", &extra) {
        Ok(probe) => probe,
        Err(error) => {
            eprintln!("skip: {error}");
            return;
        }
    };
    let base = 0x9110_6000;
    probe.send_write(base + CR, 4, u64::MAX).unwrap();
    probe.send_read(base + CR, 4).unwrap();
    probe.send_write(base + TORR, 4, u64::MAX).unwrap();
    probe.send_read(base + TORR, 4).unwrap();
    probe.send_write(base + PROT_LEVEL, 4, u64::MAX).unwrap();
    probe.send_read(base + PROT_LEVEL, 4).unwrap();
    let qemu_values = match probe.finish() {
        Ok(values) => values,
        Err(error) => {
            eprintln!("skip: qtest probe failed: {error}");
            return;
        }
    };
    assert_eq!(qemu_values.len(), 3);

    let wdt = K230Wdt::new_named("k230-wdt0");
    let mmio = K230WdtMmio(wdt);
    mmio.write(CR, 4, u64::MAX);
    let machina_cr = mmio.read(CR, 4);
    mmio.write(TORR, 4, u64::MAX);
    let machina_torr = mmio.read(TORR, 4);
    mmio.write(PROT_LEVEL, 4, u64::MAX);
    let machina_prot = mmio.read(PROT_LEVEL, 4);

    assert_eq!(qemu_values, vec![machina_cr, machina_torr, machina_prot]);
}
```

This uses the existing public `machina_oracle::qemu::QemuProbe` API. Keep
`tools/oracle/src/qemu.rs` unchanged in this task.

- [ ] **Step 3: Add boot handoff slice comparison contract**

Add a skipped-by-default test that requires `MACHINA_K230_SDK`:

```rust
#[test]
fn k230_sdk_boot_artifacts_are_discovered_for_opt_in_smoke() {
    let Some(sdk) = std::env::var_os("MACHINA_K230_SDK").map(std::path::PathBuf::from) else {
        eprintln!("skip: MACHINA_K230_SDK not set");
        return;
    };
    assert!(sdk.join("images/little-core/Image").is_file());
    assert!(sdk.join("images/little-core/k230.dtb").is_file());
    assert!(sdk.join("images/little-core/rootfs.cpio.gz").is_file());
}
```

This test is an opt-in artifact gate. Normal CI uses deterministic fixture
tests from Tasks 8 and 9.

- [ ] **Step 4: Run mtest suite**

Run:

```bash
cargo test -p machina-mtest -- --nocapture
```

Expected: PASS, with explicit skip messages when QEMU or SDK artifacts are not
configured.

- [ ] **Step 5: Commit QEMU slice tests**

```bash
git add tests/mtest/Cargo.toml tests/mtest/src/lib.rs
git commit -s -m "tests: add K230 QEMU slice coverage"
```

## Task 11: Final Integration Gates

**Files:**
- Modify: `docs/en/getting-started.md`
- Modify: `docs/zh/getting-started.md`
- Modify: `docs/en/architecture.md`
- Modify: `docs/zh/architecture.md`
- No source changes expected after this task starts.

- [ ] **Step 1: Run targeted tests**

Run:

```bash
cargo test -p machina-tests riscv_cpu_model riscv_thead_csr hw_k230_wdt hw_k230_machine -- --nocapture
cargo test -p machina-mtest -- --nocapture
```

Expected: PASS.

- [ ] **Step 2: Run workspace gates**

Run:

```bash
make fmt-check
make clippy
make test
```

Expected: all commands return zero. `make clippy` must have no warnings under
the repository's configured flags.

- [ ] **Step 3: Audit prompt-to-artifact coverage**

Create a local checklist in the final response, not as a committed file:

```text
QEMU k230 machine: hw/riscv/src/k230.rs + hw_k230_machine tests
C908 CPU: cpu_model.rs + riscv_cpu_model tests
T-HEAD CSR: vendor/thead.rs + riscv_thead_csr tests
K230 WDT: hw/watchdog/src/k230.rs + hw_k230_wdt tests
Direct boot: k230_boot.rs + mtest direct boot tests
U-Boot loader boot: LoaderSpec + k230 loader tests
QEMU slices: mtest qemu slice tests or explicit skip evidence
```

- [ ] **Step 4: Document K230 machine support**

In `docs/en/getting-started.md`, change the machine type introduction to
include K230:

```markdown
Machina currently exposes three user-facing machines:
```

Add this row to the machine table:

```markdown
| `k230` | RISC-V 64-bit | Kendryte K230 SDK-compatible platform with C908 CPU profile, PLIC, ACLINT, UARTs, WDTs, direct Linux boot with `-dtb`, and SDK U-Boot loader boot |
```

In `docs/zh/getting-started.md`, change the machine type introduction to:

```markdown
Machina 当前暴露三个用户可见机器：
```

Add this row to the machine table:

```markdown
| `k230` | RISC-V 64-bit | Kendryte K230 SDK 兼容平台，包含 C908 CPU profile、PLIC、ACLINT、UART、WDT、带 `-dtb` 的 Linux 直接启动和 SDK U-Boot loader 启动 |
```

In `docs/en/architecture.md`, update the tree line:

```text
+-- hw/riscv/       # RISC-V machine definitions: riscv64-ref, k230
```

Add a short subsection after the `riscv64-ref` section:

```markdown
#### k230 SDK-Compatible Machine

`hw/riscv/src/k230.rs` defines the `k230` machine modeled after QEMU's K230
SDK-compatible board. It wires one T-HEAD C908 CPU profile, PLIC, ACLINT, five
UARTs, two K230 watchdogs, and unimplemented SDK address windows so Linux,
OpenSBI, and SDK U-Boot see the expected memory map.
```

In `docs/zh/architecture.md`, update the tree line:

```text
+-- hw/riscv/       # RISC-V 机器定义：riscv64-ref, k230
```

Add the matching subsection after the `riscv64-ref` section:

```markdown
#### k230 SDK 兼容机器

`hw/riscv/src/k230.rs` 定义 `k230` machine，对齐 QEMU 的 K230 SDK
兼容板级模型。它装配一个 T-HEAD C908 CPU profile、PLIC、ACLINT、5 个
UART、2 个 K230 watchdog，以及 SDK 地址图中的 unimplemented window，使
Linux、OpenSBI 和 SDK U-Boot 看到预期内存布局。
```

Run:

```bash
git diff -- docs/en/getting-started.md docs/zh/getting-started.md docs/en/architecture.md docs/zh/architecture.md
```

Expected: the diff contains only the K230 documentation additions above.

- [ ] **Step 5: Commit documentation updates**

```bash
git add docs/en/getting-started.md docs/zh/getting-started.md docs/en/architecture.md docs/zh/architecture.md
git commit -s -m "docs: document K230 machine support"
```

- [ ] **Step 6: Report completion**

After all gates pass, report the commit range and the exact commands that
passed. If any opt-in QEMU/SDK smoke tests skipped, report the environment
variables required to run them.

## Self-Review

Spec coverage:

- K230 QEMU parity is covered by Tasks 5, 7, 8, 9, and 10.
- K230 WDT qtest-equivalent behavior is covered by Task 4 and compared in Task
  10.
- C908 CPU identity is covered by Task 1.
- T-HEAD CSR separation is covered by Task 2.
- Standard and T-HEAD extension gates are covered by Task 3.
- Direct Linux boot with `-dtb`, `-initrd`, and `-append` is covered by Task 8.
- SDK U-Boot boot with `-bios` and multiple `-device loader` entries is covered
  by Task 9.
- mtest and QEMU slice coverage is covered by Task 10.
- Final verification and completion audit are covered by Task 11.

Placeholder scan:

- The plan contains no `TBD`, `TODO`, or unspecified acceptance gates.
- QEMU and SDK availability are represented as explicit skip contracts with
  environment variables.

Type consistency:

- `RiscvCpuModel`, `RiscvCpuProfile`, `RiscvVendor`, `LoaderSpec`,
  `K230Machine`, `K230Wdt`, and `K230WdtMmio` are introduced before later tasks
  use them.
- `MachineOpts` gains `dtb` and `loaders` before K230 boot tasks rely on them.
