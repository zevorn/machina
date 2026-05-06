#!/usr/bin/env python3
"""Validate repository agent skill metadata."""

from pathlib import Path
import re
import sys


REQUIRED_FIELDS = ("name", "description", "license")
SKILL_NAME_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")


def parse_frontmatter(path):
    lines = path.read_text(encoding="utf-8").splitlines()
    if not lines or lines[0] != "---":
        raise ValueError("missing opening frontmatter marker")

    fields = {}
    for line in lines[1:]:
        if line == "---":
            return fields
        if ":" not in line:
            raise ValueError(f"invalid frontmatter line: {line}")
        key, value = line.split(":", 1)
        fields[key.strip()] = value.strip()

    raise ValueError("missing closing frontmatter marker")


def validate_skill(path):
    fields = parse_frontmatter(path)
    errors = []

    for field in REQUIRED_FIELDS:
        if not fields.get(field):
            errors.append(f"missing `{field}`")

    name = fields.get("name", "")
    if name and not SKILL_NAME_RE.fullmatch(name):
        errors.append("`name` must use lowercase words separated by hyphens")

    description = fields.get("description", "")
    if description and not description.startswith("Use when "):
        errors.append("`description` must start with `Use when `")

    expected_name = path.parent.name
    if name and name != expected_name:
        errors.append(f"`name` must match directory `{expected_name}`")

    return errors


def main():
    root = Path(".agents/skills")
    skill_files = sorted(root.glob("*/SKILL.md"))
    if not skill_files:
        print("no skill files found", file=sys.stderr)
        return 1

    failed = False
    for path in skill_files:
        errors = validate_skill(path)
        if errors:
            failed = True
            for error in errors:
                print(f"{path}: {error}", file=sys.stderr)

    if failed:
        return 1

    print(f"validated {len(skill_files)} skill file(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
