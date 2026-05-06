---
name: machina-build
description: Use when building Machina, running cargo check, debugging Rust build failures, or deciding which repository build command to run.
license: MIT
---

# Machina Build

Use the repository `Makefile` before inventing ad hoc commands.

## Quick Commands

| Task | Command |
|------|---------|
| Debug build | `make build` |
| Release build | `make release` |
| Fast type check | `cargo check --workspace` |
| Format check | `make fmt-check` |
| Lint | `make clippy` |
| Docs | `make docs` |

## Workflow

1. Confirm the working tree and branch with `git status -sb`.
2. Prefer `cargo check --workspace` while iterating on Rust changes.
3. Use `make build` when generated binaries or integration behavior matter.
4. Run `make fmt-check` before committing any Rust change.
5. Run `make clippy` before PR submission when Rust files changed.

## Failure Handling

- Preserve the first compiler error; later errors may be follow-on noise.
- Do not edit unrelated crates just because the workspace build visits them.
- If `Cargo.lock` changes locally, verify whether it is tracked in the target
  branch before committing it.
- Keep build artifacts out of commits; `target/` is ignored.
