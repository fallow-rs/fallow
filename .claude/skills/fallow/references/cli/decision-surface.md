# `decision-surface`: Structural Decisions

Surface only the consequential structural decisions a change embeds (the apex of the review brief): a ranked, capped (3 to 5, default 4) set of coupling/boundary, public-API/contract, and dependency decisions, each framed as a judgment question with the routed expert to ask, plus a trade-off clause and the count of in-repo consumers that already depend on the anchor. Runs the same changed-code analysis as `fallow review` but emits only the decisions, separable and cheap. Always exits 0 (advisory, never a gate); every decision is suppressible with `// fallow-ignore`. Use `--base` / `--changed-since` to pick the comparison point, exactly like `fallow audit`.

```bash
fallow decision-surface --base main
fallow decision-surface --base main --format json --quiet
```

<!-- generated:flags:decision-surface:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--max-decisions` | `string` | `4` | Cap on the number of surfaced decisions (the working-memory limit). Default 4; clamped to the 3-5 band (4 plus or minus 1) |

Common global flags for this command: [`--changed-since`](global-flags.md), [`--format`](global-flags.md), [`--quiet`](global-flags.md), [`--workspace`](global-flags.md), [`--root`](global-flags.md), [`--config`](global-flags.md).
<!-- generated:flags:decision-surface:end -->
