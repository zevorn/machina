# Machina Agent Guide

This file is the shared source of project instructions for AI agents and
human contributors working in this repository. Tool-specific files such as
`CLAUDE.md`, `GEMINI.md`, and `.github/copilot-instructions.md` should stay
thin and point back here.

Detailed coding guidelines live under `docs/en/` (English) and `docs/zh/`
(Chinese). When in doubt, this file takes precedence.

## Repo Layout

- **Rust workspace**: Machina is a multi-crate Rust workspace.
- **Documentation**: User and developer docs live in `docs/en/` and
  `docs/zh/`.
- **Tests**: All tests live in the `tests/` crate (`machina-tests`).
- **Tools**: Developer tools live under `tools/` and `scripts/`.
- **Agent skills**: Reusable agent workflows live under `.agents/skills/`.
- **Build artifacts**: Cargo output lives under `target/` and must not be
  committed.

## Agent Skills

Skills live under `.agents/skills/<skill-name>/SKILL.md`. Agents that support
skills should load the matching skill before starting the task. Agents without
native skill support should read the corresponding `SKILL.md` file directly
and follow it as task-specific guidance.

Use these specialized skills for common tasks:

- `machina-code-explorer`: Find definitions, call sites, crate boundaries,
  device models, target code, or QEMU-inspired subsystem mappings.
- `machina-build`: Build Machina, run `cargo check`, or debug Rust build
  failures.
- `machina-testing`: Choose, add, list, or run Machina tests, including
  targeted `machina-tests` filters.
- `machina-code-reviewer`: Review Machina PRs, local diffs, commits, or
  mailing-list patch series.
- `machina-issue-helper`: Summarize or prepare to debug GitHub issues, bug
  reports, feature requests, or reproduction notes.
- `machina-qemu-oracle`: Compare Machina behavior with QEMU or use oracle and
  difftest tooling.

Validate skill metadata with:

```bash
make check-agent-skills
```

## Source Code Layout

- **`accel/`**: Execution loop, DBT control, CPU run state.
- **`core/`**: Core traits and shared emulator interfaces.
- **`decode/`**: Guest instruction decoding.
- **`disas/`**: Disassembly helpers.
- **`gdbstub/`**: GDB remote protocol support.
- **`guest/`**: Guest architecture support code.
- **`hw/`**: Device models and machine-specific hardware.
- **`memory/`**: Address spaces, regions, and memory transactions.
- **`monitor/`**: QMP/HMP monitor handling.
- **`softfloat/`**: Floating-point helpers.
- **`system/`**: Machine assembly and CPU orchestration.
- **`tests/`**: Centralized `machina-tests` crate.
- **`tools/`**: Developer and oracle tooling.
- **`util/`**: Shared low-level utilities.

## Quick Start

```bash
make build          # build all crates (debug)
make release        # build all crates (release)
make test           # run all tests
make clippy         # lint with -D warnings
make fmt            # auto-format
make fmt-check      # check formatting
make docs           # generate rustdocs
make clean          # clean build artifacts
```

## Boundary Constraints

### Unsafe Rust

`unsafe` is only permitted in these scenarios:

- JIT code buffer allocation and execution (`mmap` plus `mprotect` RWX).
- Calling generated host code.
- Raw pointer access for guest memory emulation.
- Inline assembly in the backend code emitter.
- FFI interfaces to external libraries.

Everything else must be safe Rust. Every `unsafe` block requires a short
justification comment and must remain narrowly scoped.

### Test Centralization

All tests must live in the `tests/` crate. Do not add tests in individual
crate `#[cfg(test)] mod tests` blocks or per-crate `tests/` directories.
Individual crates expose `pub` interfaces for the test crate to call.

## Development Workflows

### Device Models

- Device-model work usually starts in `hw/`, then crosses into `memory/` and
  `system/`.
- Preserve reset behavior, MMIO access width, IRQ delivery, and CPU affinity.
- Use `machina-qemu-oracle` when behavior should match QEMU.

### QEMU Parity

- Keep QEMU observations explicit in test names, comments, or failure output.
- Do not weaken oracle assertions to match broken Machina behavior.
- Prefer narrow regression tests in `tests/src/` before changing behavior.

### Monitor and Tooling

- Monitor and QMP-like behavior belongs under `monitor/`.
- Developer utilities belong under `tools/` or `scripts/`.
- Generated or local-only artifacts must stay out of commits.

## Code Style

- 80-column line width for all code and code comments.
- Markdown documentation is exempt from the 80-column limit.
- 4-space indentation, no tabs.
- Run `cargo fmt` before committing.
- `cargo clippy -- -D warnings` must pass with zero warnings.
- Comments must be English and only at key logic points.

Full style guide: `docs/en/contributing.md` / `docs/zh/contributing.md`.

## Commit Style

- English commit messages.
- Format: `module: subject` in imperative mood, subject <= 72 characters.
- Body: describe what changed and why, wrapped at <= 80 columns.
- Add `Signed-off-by: Name <email>` for commits in this repository.
- Do not add AI-related sign-off lines such as `Co-Authored-By: Claude`.
- Keep commits small and bisectable.

Full git guidelines:
`docs/en/contributing.md#part-4-git-guidelines` /
`docs/zh/contributing.md#part-4-git-指南`.

## Testing Rules

- Every bug fix must add or update a regression test.
- Every new feature must include tests for expected path, edge cases, and
  failure cases.
- A change is not done if tests fail, are silently skipped, or do not check
  the claimed behavior.
- Run the narrowest useful tests while iterating, then full validation before
  finishing.

Full testing guide:
`docs/en/contributing.md#part-5-testing-guide` /
`docs/zh/contributing.md#part-5-测试指南`.

## Quality Gates

Before submitting any change:

1. `make fmt-check` passes.
2. `make clippy` passes.
3. `make test` passes.
4. `make check-agent-skills` passes when `.agents/` changes.
5. Relevant documentation is updated.
