# Rule precision

`benchmarks/rule-precision.json` records the measured precision of each rule.
The [summary](../benchmarks/rule-precision-summary.md) shows the same data as a
table. The
script `scripts/rule-precision.mjs` writes the summary from the JSON file. Do
not edit the summary by hand.

Precision is the share of real findings among the real and noise verdicts of
a seeded random sample of the findings of a rule. The sample takes the same
number of findings from each project, so a project with few findings gets a
larger share of its findings sampled. The estimate therefore weights the
precision of each project by its count of findings. The range weights the
exact (Clopper-Pearson) interval of each project at the Bonferroni level
`0.05 / projects`, so it holds with at least 95 percent probability. The range
describes the rule on code in general, so even a rule whose findings were all
reviewed keeps a range. When a project has findings but no real or noise
verdict, the summary shows `n/a` and names the project. A high precision means that a user can act on a finding
without a second look. The ledger does not measure recall: a rule that reports
nothing has no row.

## Corpus

The `corpus` object of the ledger pins each project to a full commit SHA. The
projects are public, and no earlier false-positive fix used them, so no rule
was tuned on them. Do not tune a rule on a corpus project and then record its
row from the same project. When a fix needs a project for tuning, record the
row on the other projects, or add a new held-out project.

## Method

1. Build fallow from the commit that you measure.
2. Take the sample:

   ```bash
   node scripts/rule-precision.mjs sample \
     --fallow-bin target/debug/fallow \
     --corpus-dir ~/Library/Caches/fallow-precision-corpus \
     --out <work dir> \
     --command dead-code
   ```

   The script checks out each project at its pinned commit. When the checkout
   has no `node_modules`, the script installs the dependencies from the
   lockfile, with install scripts off. Then it runs
   `fallow <command> --format json --quiet --no-cache` in the project root. Each
   top-level array of findings in the report is one rule. For each rule and
   project, the script takes up to 30 findings (`--sample-size`) in a seeded
   random order. The seed is the rule, the project and the commit, so the same
   binary gives the same sample. `--rules a,b` keeps only some rules.

   For each rule, the script writes these files to `<work dir>/<rule>/`:

   - `sample.json`: the sampled finding IDs and the finding counts;
   - `review.json`: each sampled finding with its file, line and a code excerpt;
   - `contract.txt`: the `fallow explain <rule>` text, which states the claim
     of the rule;
   - `all-findings.json`: the file and line of every finding of the rule, not
     only the sample.
3. Give these files to a reviewer who did not write or tune the rule.
   `scripts/rule-precision/reviewer-brief.md` is the brief: fill in its
   `{PLACEHOLDERS}` for each rule. The
   reviewer opens the code of each finding and judges it against the claim in
   `contract.txt`. A fix that also needs edits at other sites, for example the
   call sites of an unused prop, does not make a finding noise. A file whose
   only importers are also in `all-findings.json` is still unreachable, so the
   reviewer follows the chain to its root. The reviewer does not see the counts
   of earlier rows.
4. Record the verdicts:

   ```bash
   node scripts/rule-precision.mjs record \
     --in <work dir>/<rule> \
     --verdicts <verdicts file>
   ```

   The command refuses a verdict file that misses a sampled finding, has two
   verdicts for one finding, or has a finding that is not in the sample. It
   replaces the earlier row of the same rule and command, and writes the
   summary again.

   For a rule where one review is not stable, give `--verdicts` once for each
   independent review. The first two reviews cover the whole sample. A third
   review covers the findings where the first two disagree, and the majority
   verdict wins. With no majority, the verdict is `unsure`. The row records the
   count of reviews and the share of findings where the first two agree.
5. Run `node scripts/rule-precision.mjs check`. `npm run test:scripts` runs the
   same check.

## Verdict file

```json
{
  "reviewed_on": "2026-11-25",
  "verdicts": [
    { "project": "plane", "id": "<finding_id>", "verdict": "real" },
    {
      "project": "plane",
      "id": "<finding_id>",
      "verdict": "noise",
      "class": "framework convention export",
      "reason": "Next.js reads generateMetadata from the page file."
    }
  ]
}
```

## Verdicts

Judge the finding against the contract of the command that reported it. For
`dead-code` in the default mode, test files count as users.

- `real`: the claim of the rule is true for the finding. For example, the
  export has no user, so the removal of the `export` keyword changes no
  behavior.
- `noise`: the claim is false, or the fix changes behavior. Give a
  `class` that names the cause, for example "framework convention export",
  "dynamic import by string", or "used by a script outside the source tree".
  Use the same class name for the same cause across rules.
- `unsure`: the code does not decide it, for example a use that only a
  deployment config outside the repository can show. Keep this verdict rare.
  The ratio leaves it out, and the summary shows its count.

## Freshness

Each row records the fallow version and the date of its review. A rule change
that can change the precision needs a new row. The check does not fail on old
rows, because the corpus run costs review time.
