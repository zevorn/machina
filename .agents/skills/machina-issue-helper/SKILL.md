---
name: machina-issue-helper
description: Use when summarizing or preparing to debug Machina GitHub issues, bug reports, feature requests, or reproduction notes.
license: MIT
---

# Machina Issue Helper

Summarize issue context before diagnosis or implementation.

## Fetching Issue Data

Use GitHub CLI from the repository:

```bash
gh issue view <id-or-url> --comments
gh issue list --search "<keywords>"
```

## Summary Format

Provide these sections:

1. Issue source, title, reporter, and linked PRs or commits.
2. Host, target architecture, machine type, and command line.
3. Expected behavior, actual behavior, and reproduction steps.
4. Logs, panic messages, or test output.
5. Existing discussion, proposed fixes, and unresolved questions.
6. Next diagnostic step and the narrowest test that would prove it.

## Rules

- Do not infer missing environment details; mark them as missing.
- Prefer exact commands and logs over prose summaries.
- If the issue touches QEMU parity, use `machina-qemu-oracle` next.
- If critical repro data is absent, ask for the minimum missing data.
