# CI gate

Add one gate to the CI provider that the project already uses. Exit code 0 means that the gate passed. Exit code 1 means that the gate found error-severity findings or that the audit result is `fail`. Exit code 2 means invalid input or an execution error. Do not force a successful exit status, because that hides the gate result.

## Select the gate

| Gate | Command | Fails on | Baseline |
|---|---|---|---|
| Changed-code gate | `fallow audit` | Findings that the change introduces in the changed files | Not necessary |
| Whole-project gate | `fallow` | Every error-severity finding in the project | Necessary when the first run has many findings |

Start with the changed-code gate. `fallow audit` uses `--gate new-only` by default, so findings that existed before the change do not fail it. The audit needs the git history of the base ref. In CI, fetch the full history.

## GitHub Actions

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- uses: fallow-rs/fallow@v3
  with:
    command: audit
```

The Action installs the Fallow version that `package.json` names. To start in report-only mode, add `fail-on-issues: false`. Remove it when the team accepts the gate.

## GitLab CI

```yaml
include:
  - remote: 'https://raw.githubusercontent.com/fallow-rs/fallow/v<version>/ci/gitlab-ci.yml'

fallow:
  extends: .fallow
  variables:
    FALLOW_COMMAND: "audit"
```

Replace `<version>` with the installed Fallow version. When the runners cannot reach `raw.githubusercontent.com`, run `fallow ci-template gitlab --vendor` and include the vendored files locally.

## Other CI providers

Run the CLI directly after the dependencies are installed:

```bash
npx fallow audit --base origin/main --format json --quiet
```

Use the runner of the detected package manager, for example `pnpm exec fallow`.

## Baselines

A whole-project gate on an existing project often reports a large backlog on the first run. Save a baseline of that backlog, so that the gate fails only on new findings:

```bash
fallow dead-code --format json --quiet --save-baseline fallow-baselines/dead-code.json
fallow health --format json --quiet --save-baseline fallow-baselines/health.json
fallow dupes --format json --quiet --save-baseline fallow-baselines/dupes.json
```

Commit the baseline files. Pass each file to the matching command with `--baseline <path>`. `fallow audit` uses `--dead-code-baseline`, `--health-baseline`, and `--dupes-baseline` instead. `--fail-on-baseline-growth` makes a committed baseline shrink-only. Tell the user how many findings the baseline contains, and suggest that the team removes them by category.
