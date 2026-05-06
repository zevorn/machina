---
name: machina-qemu-oracle
description: Use when comparing Machina behavior with QEMU, using difftest/oracle tooling, or debugging CPU, interrupt, memory, or device-model parity.
license: MIT
---

# Machina QEMU Oracle

Use QEMU as a reference when behavior should match established machine or
device semantics.

## Starting Points

- Read `tests/src/oracle.rs` for the oracle contract.
- Search `tests/src/` for existing hardware regression tests.
- Check `tools/oracle/` and `tools/difftest/` before adding new tooling.
- Use `docs/en/reference.md` for test architecture notes.

## Workflow

1. Reproduce the Machina behavior with the narrowest existing test or command.
2. Run the comparable QEMU oracle path when available.
3. Capture access width, address, value, IRQ line, CPU id, and reset state.
4. Add or update a centralized test in `tests/` before changing behavior.
5. Keep device fixes local to the owning `hw/` crate unless a shared contract
   is wrong.

## Common Parity Areas

- RISC-V PLIC, ACLINT, APLIC, and IPI interrupt behavior.
- LoongArch IOCSR, EIOINTC, IPI, and PCH-PIC behavior.
- MMIO access widths and read/write side effects.
- Reset ordering and multi-CPU delivery.

## Rules

- Do not paper over a mismatch by weakening oracle assertions.
- Preserve exact QEMU observations in test names, comments, or failure output.
- Treat missing QEMU or tool dependencies as a skip only when the existing test
  harness already models that skip contract.
