# Machina Agent Skills

This directory contains agent-agnostic skills for working in Machina. The root
`AGENTS.md` is the shared project guide and lists when to use each skill.

Available skills:

- `machina-code-explorer`: navigate the Rust workspace and map QEMU-style
  concepts to Machina modules.
- `machina-build`: build or check the workspace with the repository `make`
  targets.
- `machina-testing`: choose and run the right tests from the centralized
  `machina-tests` crate.
- `machina-code-reviewer`: inspect GitHub PRs, local diffs, and mailing-list
  patch series.
- `machina-issue-helper`: summarize GitHub issue context before debugging.
- `machina-qemu-oracle`: compare Machina behavior against QEMU and the
  oracle tooling.

Validate skill metadata with `make check-agent-skills`.
