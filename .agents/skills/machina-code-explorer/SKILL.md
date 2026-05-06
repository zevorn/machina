---
name: machina-code-explorer
description: Use when finding Machina definitions, call sites, crate boundaries, device models, target code, or QEMU-inspired subsystem mappings.
license: MIT
---

# Machina Code Explorer

Machina is a Rust workspace that reimplements core QEMU concepts. Search with
Rust crate boundaries in mind.

## Search Order

1. Use `rg` or `rg --files` from the repository root.
2. Use `cargo metadata --no-deps` when crate ownership or package names matter.
3. Use `git log -- <path>` for subsystem history before changing behavior.
4. Use the docs under `docs/en/` and `docs/zh/` to confirm terminology.

## Repository Map

| Path | Purpose |
|------|---------|
| `accel/` | Execution loop, DBT control, CPU run state |
| `core/` | Core traits and shared emulator interfaces |
| `decode/` | Guest instruction decoding |
| `disas/` | Disassembly helpers |
| `gdbstub/` | GDB remote protocol support |
| `guest/` | Guest architecture support code |
| `hw/` | Device models and machine-specific hardware |
| `memory/` | Address spaces, regions, and memory transactions |
| `monitor/` | QMP/HMP monitor handling |
| `softfloat/` | Floating-point helpers |
| `system/` | Machine assembly and CPU orchestration |
| `tests/` | Centralized `machina-tests` crate |
| `tools/` | Developer and oracle tooling |
| `util/` | Shared low-level utilities |

## QEMU Concept Mapping

- TCG-like translation and execution lives mostly under `accel/`, `decode/`,
  `guest/`, and target-specific support.
- Device-model work usually starts in `hw/`, then crosses into `memory/` and
  `system/`.
- Monitor and QMP-like behavior belongs under `monitor/`.
- QEMU comparison tooling and probes live under `tools/` and `tests/`.

## Rules

- Prefer public APIs already exposed for `machina-tests`.
- Do not add crate-local `#[cfg(test)]` test modules; use `tests/`.
- Check both English and Chinese docs when changing documented behavior.
