---
name: machina-code-reviewer
description: Use when reviewing Machina pull requests, local diffs, commits, or mailing-list patch series that may inform Machina changes.
license: MIT
---

# Machina Code Reviewer

Review for behavior, safety, tests, and repository fit before style.

## Inputs

- GitHub PR: `gh pr view <id> --json title,body,files,commits,reviews`
- Local diff: `git diff --stat` then `git diff`
- Commit range: `git log --oneline <base>..HEAD` and `git diff <base>...HEAD`
- Mailing-list series: `b4 am <message-id-or-url>`

## Review Focus

1. User-visible behavior and regressions.
2. Unsafe Rust scope and safety comments.
3. Device-model side effects, reset paths, and memory access width.
4. Test coverage in `tests/`, including edge and failure cases.
5. Documentation updates under `docs/` when behavior or interfaces change.
6. Commit hygiene: small commits, English subject, `Signed-off-by` only.

## Output Format

Lead with findings ordered by severity. Use file and line references when
available. Keep summaries short and put them after findings.

## Patch-Series Handling

Use `b4 am` for lore/public-inbox series, inspect the cover letter first, and
apply only in an isolated branch or worktree. Do not apply external patches on
top of unrelated local changes.
