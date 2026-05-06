---
name: machina-testing
description: Use when choosing, adding, listing, or running Machina tests, including targeted Rust tests and workspace validation.
license: MIT
---

# Machina Testing

All tests live in the standalone `tests/` crate named `machina-tests`.

## Commands

| Scope | Command |
|-------|---------|
| Full test suite | `make test` |
| Backend tests | `make test-backend` |
| Frontend tests | `make test-frontend` |
| Integration tests | `make test-integration` |
| One Rust test | `cargo test -p machina-tests <filter>` |
| Format check | `make fmt-check` |
| Lint | `make clippy` |

## Adding Tests

1. Put new tests under `tests/src/`.
2. Use existing helpers before adding new harness code.
3. Name tests after observable behavior, not implementation details.
4. Cover the expected path, edge cases, and failure cases for new behavior.
5. For bug fixes, first create or update a regression test that fails on the
   old behavior.

## Running Tests

- Start with the narrowest filter that exercises the changed behavior.
- Run `make fmt-check` after code edits.
- Run `make test` before PR submission when behavior changed.
- If a test is skipped, verify the skip is part of the intended harness
  contract and mention it in the result.

## Rules

- Do not add `#[cfg(test)] mod tests` inside individual crates.
- Do not weaken assertions to match current broken behavior.
- Keep expensive QEMU-backed checks behind existing oracle skip behavior.
