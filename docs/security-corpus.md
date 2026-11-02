# Security corpus

`scripts/security-corpus.mjs` measures `fallow security` against public data.
The data comes from the security-rules study of the Ultracite project
(`haydenbleasel/ultracite`, folder `research/corpus/results/security-rules`).
The harness pins one Ultracite commit and the eight project commits that the
study measured.

The harness has two phases:

- **Precision.** The harness checks out each project at the measured commit.
  It runs `fallow security --format json --surface --quiet` and joins each
  candidate to the classified sink sites of the study. Each injection sink has
  a data-flow category: `constant`, `escaped`, `trusted-dev` or
  `user-controlled`. The report also lists the candidates that match no
  classified site.
- **Recall.** For each published advisory with a fix commit, the harness runs
  fallow on the parent (vulnerable) commit. A line hit is a candidate on a line
  that the fix changed, with a small line slack. A file hit is a candidate in a
  changed file. The report groups the results by advisory class. For each line
  hit, the harness also runs fallow on the fix commit. A candidate that is still
  there after the fix marks the sink, not the defect.

## How to run

Prerequisites: `git`, Node.js, network access to GitHub, and a release build.

```bash
cargo build --release -p fallow-cli
npm run security:corpus > /tmp/security-corpus.log 2>&1
```

Pass flags after `--`, for example `npm run security:corpus -- --phase precision`.
Useful flags:

| Flag | Effect |
| --- | --- |
| `--phase precision\|recall\|all` | Selects the phases. The default is `all`. |
| `--cache-dir <dir>` | Sets the clone cache. The default is `<os tmpdir>/fallow-security-corpus`. |
| `--ultracite-dir <dir>` | Reads the study data from an existing checkout and does not clone. |
| `--out-dir <dir>` | Sets the output directory. The default is `.plans/security-corpus`. |
| `--repos zod,hono` | Selects projects (Ultracite names). |
| `--class xss,ssrf` | Selects advisory classes for the recall phase. |
| `--limit <n>` | Keeps the first `n` advisories in the recall phase. |
| `--slack <n>` | Sets the line slack of a recall line hit. The default is 3. |
| `--max-scope-files <n>` | Sets the largest checkout scope per fix commit. The default is 4000 source files. |
| `--fallow-bin <path>` | Uses this fallow binary. The default is the release build of the checkout. |
| `--timeout-secs <n>` | Stops one fallow scan after `n` seconds. The default is 900. |
| `--rerun` | Ignores cached fallow output. |

A full recall run is slow, because each fix commit needs its own
fetch and scan. When a build cache can remove the release binary during that
time, copy the binary to a stable path and pass `--fallow-bin`. The harness
caches each successful scan in `raw/` and does not cache a failed scan, so a
second run continues where the first run stopped.

The harness writes `report.md`, `results.json` and `raw/` (compact fallow
output per scan) to the output directory. The default directory is ignored by
git. Do not commit the numbers of a run into tracked documentation, because
they change with each fallow release.

## Method details

- **Census scope.** Precision uses the include and exclude lists of the study
  (`research/corpus/scripts/repos.json`). It also applies the same test,
  fixture, vendor and license-header rules. A candidate outside that scope is
  counted, but it is not compared.
- **Clones.** Every clone is shallow and blob-less, with a sparse checkout. The
  precision phase checks out the include directories of the study. The recall
  phase checks out the nearest package root of each changed file. When that
  root holds more source files than `--max-scope-files`, the harness narrows the
  scope toward the changed file. Narrow scopes can hide a cross-package flow.
  The report lists the scope of each commit.
- **Diff ranges.** The harness reads the old side of a zero-context diff. A pure
  insertion maps to the old lines around the insertion point. New files and
  test files do not count.
- **Untrusted input.** The harness reads the study data as JSON. It never runs a
  script, a build or a package install from Ultracite or from the projects.

## Privacy

Use only public repositories. The output contains file paths and evidence
strings of the public projects. Cite the advisory id, the public commit and the
fallow category in an issue. Do not paste large source windows.
