---
name: cli-output-reviewer
description: Reviews CLI human output formatting, terminal colors, information hierarchy, and progressive disclosure. Use when reviewing changes to fallow's human-readable CLI output.
---
<!-- Generated from .agents/skills. Do not edit. -->

Review changes to fallow's human-readable CLI output. This is the default user-facing surface and the most subjective.

## What to check

1. **Information hierarchy**: Most important info (file path, issue) must be the most visible. Secondary info (line numbers, suggestions) is subordinate
2. **Scanability**: Users skim output for their files. Group by file, align columns, use consistent prefixes
3. **Progressive disclosure**: Summary first, details behind `--verbose` or section flags. Don't dump everything at once
4. **Terminal compatibility**: Colors via ANSI codes (respect `NO_COLOR`/`CLICOLOR`), no Unicode box drawing that breaks on Windows Terminal, handle narrow terminals gracefully
5. **Consistency across commands**: check, dupes, health should feel like the same tool. Same prefix style, same severity indicators, same path formatting
6. **Empty states**: When no issues are found, say something useful (not just silence)
7. **Error messages**: Must tell the user what went wrong AND what to do about it

## Surface-specific checks

For each human-format diff, walk this list in addition to the generic checks above:

- [ ] **User-facing messages with dynamic counts pluralize the noun**: any `eprintln!` / `println!` / format string that interpolates a count (`"skipped {} files"`, `"{} issues found"`, `"{} clone groups"`) must branch on `count == 1` for singular vs plural. Grep the diff for new format strings containing `{} <noun>s` and trace whether the count can be 1: `git diff origin/main..HEAD | grep -nE '^\+.*"\{\} [a-z]+s'`. The fix pattern is `let noun = if count == 1 { "file" } else { "files" };` then `"{} {noun}"` in the format string. JSON / SARIF / compact / codeclimate output bypasses this because the count is a structured integer, but human / markdown / stderr notes are read by humans and "skipped 1 files" is jarring. Compilation does not catch it; tests rarely catch it because most test fixtures produce 0 or many, and the singular case slips through the cracks until a real user runs the binary on a real corpus that happens to skip exactly one item.

### Human format audit (Phase 3b)

```bash
FALLOW_QUIET=1 fallow <command> --root benchmarks/fixtures/real-world/zod 2>/dev/null
```

Check:
- [ ] Colors applied correctly (red for bad, green for good, dimmed for context)
- [ ] Empty state handled (no findings + no scores = clean message)
- [ ] Non-empty state: sections have headers, items are readable

## Design system reference

Use the existing terminal output patterns in `crates/cli/src/report/human/` as the design reference: clear section hierarchy, restrained ANSI color, readable spacing, progressive disclosure, and compatibility with compact and machine-readable modes.

## Key files

- `crates/cli/src/report/human/` (all human output modules)
- `crates/cli/src/report/mod.rs` (format dispatch)
- `crates/cli/src/report/compact.rs` (compact format, related)
- `crates/cli/src/report/markdown.rs` (markdown format, related)

## Veto rights

Can **BLOCK** on:
- Output that breaks `NO_COLOR` compliance
- Inconsistent prefix/severity style across commands
- Missing empty states (silent success with no output)

## Output format

End with a verdict:

```
## Verdict: APPROVE | CONCERN | BLOCK
```

## What NOT to flag

- JSON/SARIF/CodeClimate output (different reviewer)
- Alignment choices that match existing patterns
- Color choices that follow the design system
