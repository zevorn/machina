# K230 QEMU Parity Design

## Context

The target is to add a Machina `k230` machine based on QEMU's
`chao-k230-v7` branch. The reference branch models a Kendryte K230 SDK
compatible board, not every real K230 peripheral in full detail.

QEMU's supported K230 device list is:

- one T-HEAD C908 RISC-V hart
- CLINT/ACLINT
- PLIC
- two K230 watchdog timers
- five UARTs

The rest of the SoC address map is represented with unimplemented MMIO
devices so SDK firmware and device trees can probe known regions without
requiring all peripherals to be modeled.

Machina already has reusable infrastructure for much of this:

- `hw/riscv/src/ref_machine.rs` provides the existing RISC-V board pattern.
- `hw/intc/src/plic.rs` and `hw/intc/src/aclint.rs` provide reusable RISC-V
  interrupt controllers.
- `hw/char/src/uart.rs` provides an NS16550-compatible UART.
- `hw/misc/src/unimp.rs` provides unimplemented MMIO windows.
- `hw/timer/src/lib.rs` provides `Ptimer`.
- `core/src/device_cell.rs` provides `DeviceRegs` and `DeviceRefCell` for
  interior mutability.

The main gaps are the K230 board, the K230 watchdog model, C908 CPU identity
and extension support, T-HEAD custom CSRs, T-HEAD custom instruction
extensions, and K230 boot/FDT handling.

## Scope

This design targets QEMU `chao-k230-v7` parity plus the requested C908/T-HEAD
CPU support.

In scope:

- Add a `k230` machine type to Machina.
- Reproduce QEMU K230 memory map and IRQ map.
- Wire DDR, SRAM, BootROM, PLIC, ACLINT, five UARTs, two K230 watchdogs, and
  unimplemented SoC windows.
- Add K230 watchdog register behavior covered by QEMU's qtest.
- Add a C908 CPU model/profile for Machina RISC-V.
- Add T-HEAD vendor CSR support aligned with QEMU `target/riscv/th_csr.c`.
- Add T-HEAD extension flags and instruction decode/translation hooks in a
  vendor-isolated way.
- Add standard extension support needed by C908 where Machina currently lacks
  it, such as Sv48/Svpbmt/Svinval/Sstc gating, without making these
  T-HEAD-specific.
- Support QEMU-compatible K230 direct Linux boot:
  `-M k230 -kernel ... -dtb ... -initrd ... -append ... -nographic`.
- Support QEMU-compatible K230 SDK U-Boot boot:
  `-M k230 -bios ... -device loader,file=...,addr=...,force-raw=on`.
- Add mtest coverage for both boot flows, including QEMU differential or
  slice-level comparison where full SDK images are not available.
- Add tests that map each QEMU reference behavior to Machina evidence.

Out of scope:

- Full functional models for every real K230 peripheral such as KPU, ISP,
  CSI, VPU, GPU, USB, SDHCI, QSPI, I2C, GPIO, PWM, and codec.
- C908V vector support, unless a later boot target proves K230 needs it. QEMU
  K230 uses `thead-c908`, not `thead-c908v`.
- MAEE page-table attribute behavior. QEMU explicitly does not implement MAEE
  and leaves MAEE disabled in `mxstatus`/`sxstatus`.
- Copying QEMU source code. QEMU is the behavioral oracle and structure
  reference only.

## QEMU Reference Mapping

Primary QEMU files:

- `/home/zevorn/qemu/hw/riscv/k230.c`
- `/home/zevorn/qemu/include/hw/riscv/k230.h`
- `/home/zevorn/qemu/hw/watchdog/k230_wdt.c`
- `/home/zevorn/qemu/include/hw/watchdog/k230_wdt.h`
- `/home/zevorn/qemu/tests/qtest/k230-wdt-test.c`
- `/home/zevorn/qemu/docs/system/riscv/k230.rst`
- `/home/zevorn/qemu/target/riscv/cpu.c`
- `/home/zevorn/qemu/target/riscv/th_csr.c`
- QEMU command lines documented in `docs/system/riscv/k230.rst` for direct
  Linux boot and K230 SDK U-Boot boot.

QEMU organization to mirror conceptually:

- Board and SoC wiring live under `hw/riscv/k230.c`.
- Board constants live under `include/hw/riscv/k230.h`.
- Watchdog device behavior is isolated under `hw/watchdog/k230_wdt.c`.
- Vendor CPU identity is represented as a named CPU type:
  `TYPE_RISCV_CPU_THEAD_C908`.
- Vendor CSRs live in `target/riscv/th_csr.c`.
- Standard extension bits remain in the shared RISC-V CPU config.
- T-HEAD instruction extensions are decoded through a vendor extension decode
  path gated by T-HEAD extension predicates.

Machina should follow the same separation:

- K230 board wiring under `hw/riscv`.
- K230 watchdog under `hw/watchdog`.
- Generic standard extensions under `guest/riscv/src/riscv`.
- T-HEAD vendor profile, CSRs, and custom extension hooks under a dedicated
  vendor module, not mixed into generic CSR/MMU/translator logic.

## Machine Model

Add `hw/riscv/src/k230.rs` with:

- `K230MemMap` enum matching QEMU's memmap order.
- `K230IrqMap` constants:
  - UART0..UART4: IRQ 16..20
  - WDT0/WDT1: IRQ 107..108
- PLIC constants:
  - 208 sources
  - 7 priorities
  - QEMU-compatible priority, pending, enable, and context offsets
- `K230Machine` implementing `machina_core::machine::Machine`.

The machine should use existing Machina board patterns:

- `MachineState` for the root object.
- `MObjectTree` tracking for machine, sysbus, chardevs, and devices.
- `SysBus` plus `MemoryRegion` mappings for MMIO windows.
- `Arc<Device>` plus interior mutability for runtime register state.

Real modeled devices:

- DDR RAM at `0x00000000`, default size `0x80000000`.
- SRAM at `0x80200000`, size `0x00200000`.
- BootROM at `0x91200000`, size `0x00010000`.
- PLIC at `0xf00000000`, size `0x00400000`.
- ACLINT/CLINT at `0xf04000000`, size `0x00400000`.
- UART0..UART4 at `0x91400000`..`0x91404000`, each with a 0x1000 SDK window.
- WDT0/WDT1 at `0x91106000` and `0x91106800`.

Unimplemented devices should be mapped for every QEMU K230 placeholder window.
For UART windows, the 16550 register subset should be mapped consistently with
QEMU's `serial_mm_init(..., regshift = 2)` while the wider SDK window remains
covered.

## K230 Watchdog

Add a K230 watchdog device in `hw/watchdog`.

The device must use:

- `#[derive(machina_hw_core::SysBusDevice)]`
- setup state behind `parking_lot::Mutex<SysBusDeviceState>`
- runtime registers behind `DeviceRegs<K230WdtRegs>`
- an IRQ output using Machina IRQ primitives
- `Ptimer` for deterministic timer tests

Registers and behavior should match QEMU:

- `CR`, `TORR`, `CCVR`, `CRR`, `STAT`, `EOI`, `PROT_LEVEL`
- component parameter registers at `0xe4..0xfc`
- default `CCVR = 0xffffffff`
- default `PROT_LEVEL = 0x2`
- `CR` mask: reset pulse length, response mode, enable
- `TORR` mask: low 4 bits
- `CRR` magic restart value: `0x76`
- interrupt mode raises IRQ and sets `STAT_INT`
- reset mode requests the Machina watchdog action/reset path
- `EOI` and valid `CRR` clear pending interrupt and lower IRQ

The tests should port QEMU qtest intent:

- register read/write masking
- counter restart
- interrupt mode
- reset mode
- timeout calculation for all TOP values
- WDT1 register mapping
- enable/disable behavior

## C908 CPU Model

Add a named CPU model/profile instead of hardcoding board-specific behavior in
the generic RISC-V CPU.

Proposed types:

- `RiscvCpuModel`
  - `GenericRv64`
  - `TheadC908`
- `RiscvCpuProfile`
  - `name`
  - `misa`
  - `cfg`
  - `vendor`
  - `mvendorid`
  - `marchid`
  - `max_satp_mode`
- `RiscvVendor`
  - `Generic`
  - `Thead`

`RiscvCpu` should store the selected model/profile. Existing generic boards
continue to create `GenericRv64`. The K230 board creates `TheadC908`.

The C908 profile should align with QEMU:

- RV64 IMAFDC + S/U
- priv spec behavior compatible with 1.12 where Machina supports it
- `mvendorid = 0x5b7`
- `marchid = 0x8d143000`
- PMP enabled
- MMU enabled
- max SATP mode Sv48
- standard extensions enabled according to QEMU C908 config where implemented
- T-HEAD extension flags enabled according to QEMU C908 config

Unsupported C908 capabilities should be explicit. If an instruction extension
or standard feature is not implemented yet, the profile should not silently
claim full support in user-visible ISA strings until the translator/MMU support
exists.

## Vendor CSR Design

Do not place T-HEAD CSR special cases directly into generic CSR match arms as
board-specific logic.

Use a vendor CSR layer:

- Generic `CsrFile::read/write` handles standard CSR checks and standard CSRs.
- Unknown or vendor ranges are delegated to a `VendorCsr` hook selected by the
  CPU profile.
- `guest/riscv/src/riscv/vendor/thead.rs` owns T-HEAD CSR numbers, masks, and
  semantics.

T-HEAD CSR behavior should match QEMU `th_csr.c`:

- `mxstatus` returns `UCME | THEADISAEE`, with MAEE clear.
- `sxstatus` returns `UCME | THEADISAEE`, with MAEE clear.
- QEMU's listed unimplemented T-HEAD CSRs are readable and return zero when the
  selected CPU vendor is T-HEAD.
- Privilege checks follow QEMU's mmode/smode/any categories.
- These CSRs remain illegal on generic RISC-V CPU models.

This keeps vendor behavior discoverable, testable, and separate from common
RISC-V CSR semantics.

## Extension Design

Split extensions into two groups.

Standard RISC-V extensions:

- Add missing `RiscvCfg` fields for C908-relevant standard extensions.
- Implement standard behavior in generic modules with feature gates.
- Examples: Sv48 page walking, Svpbmt PTE reserved-bit handling, Svinval
  decode/traps, Sstc CSR behavior if needed by boot firmware.
- These are not T-HEAD-specific and may be reused by future CPUs.

T-HEAD vendor extensions:

- Add `xthead*` flags under `RiscvCfg` or a nested vendor extension struct.
- Keep decode and translator code under `guest/riscv/src/riscv/vendor/thead*`
  or similarly named modules.
- Generic decode calls a vendor decode table only when the selected profile
  enables T-HEAD extensions.
- Implement custom instruction semantics only in the vendor module.
- T-HEAD cache-management instructions that are no-ops in QEMU should be
  explicit no-ops, not generic cache behavior.

The immediate C908 target should prioritize the T-HEAD CSRs and the extension
instructions needed by K230 SDK firmware. Remaining T-HEAD instruction
extensions can be added in focused follow-up slices with oracle tests.

## Boot, FDT, and Loader Devices

QEMU K230 does not generate a K230 DTB. It accepts a user DTB for direct Linux
boot and applies small fixups.

Machina must support the same two user-visible boot flows.

Direct Linux boot:

```bash
machina -M k230 \
    -kernel "$SDK/images/little-core/Image" \
    -dtb "$SDK/images/little-core/k230.dtb" \
    -initrd "$SDK/images/little-core/rootfs.cpio.gz" \
    -append "console=ttyS0,115200 earlycon=sbi cma=0" \
    -nographic
```

Required behavior:

- `-dtb` loads the user-provided SDK DTB.
- The DTB is handed to OpenSBI through the K230 reset vector path.
- `/chosen/bootargs` is updated from `-append`.
- `/chosen/linux,initrd-start` and `/chosen/linux,initrd-end` are updated from
  `-initrd`.
- SDK SDHCI nodes `/soc/sdhci0@91580000` and `/soc/sdhci1@91581000` are marked
  disabled.
- Bare-metal payloads, RTOS images, and firmware with embedded DTBs may omit
  `-dtb`.

K230 SDK U-Boot boot:

```bash
machina -M k230 \
    -bios "$SDK/little/uboot/u-boot" \
    -device loader,file="$FWJUMP_UIMAGE",addr=0x0c100000,force-raw=on \
    -device loader,file="$IMAGE",addr=0x08200000,force-raw=on \
    -device loader,file="$INITRD",addr=0x0a100000,force-raw=on \
    -device loader,file="$DTB",addr=0x0a000000,force-raw=on \
    -nographic
```

Required behavior:

- `-bios` starts SDK U-Boot in M-mode.
- `-device loader,file=...,addr=...,force-raw=on` loads raw bytes at the
  requested guest physical address before CPU execution.
- Multiple loader devices are supported and applied in command-line order.
- Loader ranges are bounds-checked against RAM/MMIO regions and fail fast with
  clear errors.
- The flow is compatible with the SDK `bootm` handoff documented by QEMU.
- The Linux Image still requires standard RISC-V PTE bits, matching QEMU's
  MAEE limitation.

Implementation implications:

- Extend `MachineOpts` with `dtb: Option<PathBuf>` and
  `loaders: Vec<LoaderSpec>`.
- Add CLI parsing for `-dtb`.
- Add CLI parsing for QEMU-style `-device loader,file=...,addr=...,force-raw=on`.
- Keep loader parsing generic, but only enable the loader device for machines
  whose boot path supports it.
- Add a reusable FDT loader/fixup helper rather than embedding K230-specific
  DTB mutation in generic boot code.
- Keep K230 reset vector in the K230 BootROM window.

## CLI and Runtime

Add `k230` to machine selection:

- list under `-M ?`
- accept `-M k230`
- reject options that K230 does not support yet with clear errors
- route RISC-V full-system execution through a board-agnostic path instead of
  hardcoding `RefMachine` and `RAM_BASE`
- expose parsed `-dtb` and loader devices through `MachineOpts`

The current `run_machine_cycle` path is RISC-V-ref-specific. K230 requires the
runtime to get these values from the selected machine:

- RAM base
- BootROM/MROM fetch window
- address space pointer
- CPU object
- shared MIP and WFI waker
- ACLINT timer exit request wiring

Introduce the smallest shared RISC-V machine runtime trait needed to support
both `riscv64-ref` and `k230` without a broad refactor.

## Testing Strategy

Tests live in the central `tests/` crate, following existing Machina practice.

Board tests:

- `-M ?` lists `k230`
- `K230Machine::init` maps all QEMU memmap windows
- PLIC source count and context count match QEMU
- UART0..UART4 windows and IRQs match QEMU
- WDT0/WDT1 windows and IRQs match QEMU
- SRAM, BootROM, and DDR windows match QEMU
- unimplemented devices return zero and accept writes
- MOM tree contains the K230 machine and devices

WDT tests:

- port QEMU qtest scenarios to Rust tests
- verify timer-driven IRQ behavior using deterministic `Ptimer` stepping
- verify reset clears runtime state and lowers IRQ

CPU tests:

- C908 profile exposes correct `mvendorid`, `marchid`, MISA, and enabled cfg
  bits.
- T-HEAD CSRs are readable on C908 and illegal on generic RV64.
- `mxstatus`/`sxstatus` return QEMU-compatible values with MAEE clear.
- Standard extension tests verify feature gates independently of T-HEAD.
- T-HEAD instruction tests compare implemented semantics against QEMU where
  possible.

Boot tests:

- BootROM reset vector points to the expected start address.
- CPU reset PC is the K230 BootROM base.
- DTB fixups disable the same SDHCI nodes as QEMU.
- Direct Linux boot mtest uses small fixture kernel/initrd/DTB artifacts to
  verify:
  - `-dtb` is required for Linux-style handoff but optional for bare-metal and
    embedded-DTB firmware cases.
  - `/chosen/bootargs` is updated from `-append`.
  - initrd start/end properties are inserted and match loaded RAM addresses.
  - QEMU and Machina apply the same K230 DTB SDHCI disable fixups.
- U-Boot boot mtest verifies:
  - `-bios` loads at the same effective start path as QEMU.
  - each `-device loader` payload appears at the requested guest physical
    address.
  - loader order and overlap errors are deterministic.
  - the SDK documented addresses
    `0x0c100000`, `0x08200000`, `0x0a100000`, and `0x0a000000` are accepted.
- QEMU slice comparison tests should compare DTB mutation, payload placement,
  reset PC, and visible boot handoff registers without requiring a full SDK
  Linux boot in normal CI.
- Full SDK boot smoke tests may be opt-in behind environment variables such as
  `MACHINA_K230_SDK` and `QEMU_SYSTEM_RISCV64`; they must skip explicitly when
  artifacts are unavailable.

Mtest placement:

- Add machine-level tests under `tests/mtest` for the two K230 boot flows.
- Keep device-level K230 WDT and CPU CSR tests in the central `tests/` crate
  when that is more consistent with existing hardware tests.
- Reuse the oracle/qtest tooling for QEMU-observable slices instead of baking
  QEMU-derived expected data into implementation code.

## Implementation Slices

1. Add CPU profile plumbing and keep `riscv64-ref` behavior unchanged.
2. Add C908 profile identity and T-HEAD CSR stubs with tests.
3. Add missing standard extension config gates needed by C908.
4. Add T-HEAD extension decode/translation scaffolding with no generic
   coupling.
5. Add K230 watchdog device and tests.
6. Add K230 machine memmap, real device wiring, and unimplemented windows.
7. Add `-dtb`, loader-device parsing, and `MachineOpts` plumbing.
8. Add `-M k230` CLI/runtime support.
9. Add K230 direct Linux boot support and mtest/QEMU slice tests.
10. Add K230 SDK U-Boot loader boot support and mtest/QEMU slice tests.
11. Fill remaining C908/T-HEAD instruction support in oracle-backed slices.

Each slice should build and test independently.

## Acceptance Criteria

The work is complete when:

- `machina -M ?` lists `k230`.
- `machina -M k230` constructs a K230 machine with QEMU-compatible memmap and
  IRQ topology.
- K230 WDT behavior passes the Rust port of QEMU's qtest coverage.
- C908 CPU profile is selected by K230 and does not affect generic RV64.
- T-HEAD custom CSRs match QEMU behavior and remain illegal on generic RV64.
- Standard extensions and T-HEAD extensions are gated by CPU profile, not by
  board name.
- K230 boot path reaches a reset vector in the K230 BootROM window.
- Direct Linux boot supports the QEMU-compatible `-kernel`, `-dtb`, `-initrd`,
  `-append`, and `-nographic` flow.
- K230 SDK U-Boot boot supports `-bios` plus multiple QEMU-compatible
  `-device loader,file=...,addr=...,force-raw=on` options.
- mtest covers both K230 boot flows with deterministic fixture tests and QEMU
  slice comparison where available.
- Existing `riscv64-ref` and `loongarch64-ref` tests still pass.
- New tests provide direct evidence for each QEMU parity item above.
