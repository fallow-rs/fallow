# Styling PR Smoke

Use this harness when a release review needs evidence from recent public frontend
pull requests in repositories that already use fallow.

```bash
FALLOW_BIN=target/debug/fallow npm run smoke:styling-prs -- --out-dir target/styling-pr-smoke
```

The harness:

- selects recent PRs whose changed files include frontend and styling surfaces;
- clones or updates the public repositories under `target/styling-pr-smoke/repos`
  by default;
- runs `fallow audit --base origin/<base>` against each selected PR checkout;
- writes `selected-prs.json`, `pr-smoke-results.json`, and `pr-smoke-report.md`.

Useful modes:

```bash
npm run smoke:styling-prs -- --select-only
npm run smoke:styling-prs -- --run-only --out-dir target/styling-pr-smoke
npm run smoke:styling-prs -- --max-prs 8 --prs-per-repo 1
```

`--select-only` requires `gh` network access and refreshes the public PR list.
`--run-only` reuses an existing `selected-prs.json`, which is useful after
rebuilding `target/debug/fallow`.

Read the report with a false-positive lens:

- high-confidence structural groups should point at changed files and concrete
  cleanup opportunities;
- low-confidence raw values, dead-surface findings, and semantic token drift are
  review-first signals;
- PRs with no styling findings are still valuable because they prove default
  audit did not add noise for that frontend change.

This complements `npm run smoke:styling-corpus`. The corpus smoke covers named
styling stacks and framework breadth; the PR smoke checks current agent-gate
impact on real pull requests.
