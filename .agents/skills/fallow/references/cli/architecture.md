# `architecture`: Cycles, Boundaries and Policy Rules

Reports circular dependencies, re-export cycles, package cycles, boundary violations (with boundary coverage and forbidden calls) and rule-pack policy violations. The command runs the `dead-code` analysis with these issue types selected. The JSON output has `kind: "architecture"` (with `--group-by`: `architecture-grouped`) and its own `schema_version`. The arrays, finding ids, actions, exit codes, gate outcomes and baselines are the same as on `dead-code`. A saved baseline keeps `kind: "dead-code"`. `fallow report --from` renders a saved architecture envelope in every format. The global scope and output flags (`--format`, `--changed-since`, `--workspace`, `--baseline`, `--save-baseline`) work as on `dead-code`.

`fallow dead-code` still reports these findings by default until the next major version. Its `--circular-deps`, `--re-export-cycles`, `--package-cycles`, `--boundary-violations` and `--policy-violations` flags are deprecated aliases. `fallow architecture` reports only part of the issue types, so `--fail-on-stale-baseline` does not gate on it, with any baseline. A baseline that `fallow architecture --save-baseline` writes lists only the architecture findings, and `fallow dead-code` warns when it reads such a baseline.

| Flag | Selects |
|---|---|
| `--cycles` | Circular dependencies, re-export cycles and package cycles (`dead-code --circular-deps --re-export-cycles --package-cycles`) |
| `--boundaries` | Boundary violations, boundary coverage and forbidden calls (`dead-code --boundary-violations`) |
| `--policy` | Rule-pack policy violations (`dead-code --policy-violations`) |
| `--file <PATH>` | Only report findings in these files |
| `--finding-id <ID>` | Only report the findings with these ids |

Without a selection flag, the command reports every architecture issue type.

```json
{
  "kind": "architecture",
  "schema_version": 1,
  "total_issues": 1,
  "circular_dependencies": [
    { "files": ["src/a.ts", "src/b.ts"], "length": 2, "finding_id": "dc1:circular-dependency:...", "actions": [] }
  ],
  "boundary_violations": [],
  "policy_violations": []
}
```

The MCP tool is `check_architecture` and the Node binding is `detectArchitecture`.

With `--group-by`, each group shows its findings under one "Architecture" heading.

Bare `fallow --only architecture` runs the `dead-code` analysis and reports only these findings in the `check` section. Bare `fallow --skip architecture` removes them from the `check` section. The health score, the duplication section and the file that `--save-baseline` writes stay the same as in a run without the value. The regression counts follow the narrowed section.

## Examples

```bash
# Before an edit: the rules that apply to the files
fallow guard src/ui/App.ts --format json --quiet

# After an edit: cycles, boundaries and policy rules
fallow architecture --format json --quiet

# Only import cycles on the changed files
fallow architecture --cycles --changed-since main --format json --quiet
```
