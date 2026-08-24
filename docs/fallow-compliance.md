# Fallow Compliance Happy Path

Fallow compliance defines the maintainability and codebase-quality expectations for a repository.

A compliant project stays within agreed thresholds across the full pillar set: quality score, changed-code risk, complexity hotspots, duplication, architecture boundaries, dependency hygiene, and cleanup opportunities. Cleanup (dead code, unused exports, unused dependencies) is one compliance signal, not the compliance model.

This is the shortest path from "we installed fallow" to "this repo is fallow-compliant."

For most teams, **fallow-compliant** means:

- `fallow health` shows **`Above threshold: 0`** for the thresholds the team chose on purpose
- repo-wide duplication and architecture violations are either fixed or intentionally documented
- repo-wide cleanup opportunities (unused code, unused dependencies, stale suppressions) are either resolved or narrowly documented
- every exclusion is narrow, documented, and tied to a real reason
- stale suppressions are removed instead of carried forward
- if the team uses staged adoption, `fallow audit` passes on the current change set with intentionally chosen per-analysis baselines

## Compliance signals

In priority order:

1. **Quality score** -- `fallow health --score` reaches the team's agreed minimum
2. **Changed-code risk** -- `fallow audit` returns verdict `pass` for the current change set
3. **Complexity and hotspots** -- `fallow health` shows `Above threshold: 0`
4. **Duplication** -- clone families either consolidated or documented
5. **Architecture boundaries** -- no unintended boundary violations or new circular dependencies
6. **Dependency hygiene** -- no unused, unresolved, or unlisted dependencies; package-manager catalogs and overrides clean
7. **Cleanup opportunities** -- unused files, exports, types either deleted or explicitly kept with a documented reason
8. **Suppression hygiene** -- no stale suppression comments, no broad ignore patterns covering real issues

If you want one sentence for agents, it is this:

> Keep fixing repo-wide findings across all compliance signals until quality score meets the team's target, `fallow health` has `Above threshold: 0`, and duplication / architecture / cleanup findings are either resolved or narrowly documented; if adoption is staged, use per-analysis baselines so `fallow audit` only gates new issues.

## What "done" looks like

Use this as the end state:

1. Real issues are fixed in code.
2. Intentional patterns are encoded as explicit exceptions.
3. Exceptions explain *why* they exist.
4. The repo can be re-analyzed later without re-triaging the same findings from scratch.

That usually means:

- code was deleted, simplified, deduplicated, or refactored where the finding was correct
- `.fallowrc.json`, `fallow.toml`, or inline suppressions only capture intentional cases
- broad ignores are avoided when a file-, export-, dependency-, or override-scoped exception would do
- baselines are used only as a temporary migration aid, not as the ideal steady state
- `fallow audit` is treated as the PR or change-review gate, not the repo-wide cleanup verdict

## Decision Rules

When a finding appears, make one of these decisions:

### 1. Fix it now

Choose this when the finding is real and the behavior is safe to preserve by editing code.

Examples:

- delete an unused file or export
- remove an unused dependency
- extract or simplify a function above the complexity threshold
- remove duplicated logic by consolidating it

### 2. Keep it intentionally and encode that explicitly

Choose this when the code is supposed to stay, but fallow cannot infer that from syntax alone.

Common reasons:

- public API consumed externally
- framework-discovered entry point or convention file
- generated code
- optional tooling dependency
- known false positive
- deliberate temporary exception during migration

Prefer the narrowest mechanism that matches the reason:

- inline suppression for one line or one file
- `ignoreExports` for a specific export
- `ignoreDependencies` for a specific dependency
- `overrides` for a specific directory, pattern, or rule adjustment
- `ignorePatterns` for generated or out-of-scope files

Document the reason next to the exception whenever the format allows it.

### 3. Change the policy, not just the finding

Choose this when the issue is not one bad file, but a repo-wide standard that should be different.

Examples:

- the default complexity thresholds are too strict for the codebase's agreed standard
- duplication mode or threshold needs to reflect generated code or template-heavy code
- a framework convention should be modeled once in config, not suppressed repeatedly

Do this deliberately. Do **not** raise thresholds globally just to hide a few bad hotspots.

## Recommended Adoption Loop

1. Run `fallow` or the relevant repo-wide subcommands to see the current state.
2. Fix straightforward real issues first.
3. For each remaining finding, decide whether it is:
   - real and should be fixed
   - intentional and should be encoded as an exception
   - a sign that repo-wide policy needs to be adjusted
4. Re-run fallow after each batch.
5. Once the repo-wide state is intentional, wire `fallow audit` in as the change-set gate.
6. Stop when the repo reaches the done state above.

Useful commands:

```bash
fallow
fallow dead-code
fallow dupes
fallow health
fallow audit
fallow fix --dry-run
fallow --format json
```

If the repo is not ready for a full cleanup in one pass, stage adoption deliberately:

```bash
fallow dead-code --save-baseline fallow-baselines/dead-code.json
fallow health    --save-baseline fallow-baselines/health.json
fallow dupes     --save-baseline fallow-baselines/dupes.json

fallow audit \
  --dead-code-baseline fallow-baselines/dead-code.json \
  --health-baseline    fallow-baselines/health.json \
  --dupes-baseline     fallow-baselines/dupes.json
```

Keep committed baselines outside `.fallow/`; that directory is usually gitignored cache/local state, not a good home for review gates that should travel with the repo. `fallow-baselines/` is the recommended default.

## Notes On Health

For the **health** part of the happy path, yes: the simple target is usually **`Above threshold: 0`**.

That does **not** mean every project must use the same thresholds. It means:

- choose thresholds intentionally
- encode them in config if needed
- then drive the codebase to zero functions above those thresholds

If one function is intentionally complex and should stay that way, prefer a narrow, documented exception over silently normalizing the whole repo upward.

## Copy-Paste Agent Prompt

Use this when you want an agent to make an existing repo fallow-compliant.

```text
Make this repository fallow-compliant.

Goal:
- repo-wide dead code and duplication findings are either fixed or intentionally excluded
- `fallow health` reports `Above threshold: 0` for the thresholds chosen for this repo
- every exclusion is narrow, documented, and kept only when there is a real reason
- if staged adoption is needed, `fallow audit` passes with intentionally chosen per-analysis baselines so only new issues fail

Process:
1. Run fallow and inspect the current findings.
2. Fix real issues directly in code when that is safe.
3. For findings that are intentional, add the narrowest possible exception.
4. Document every exception with what it is for:
   - public API
   - framework convention
   - generated file
   - optional tooling dependency
   - false positive
   - temporary migration debt
5. Prefer specific exceptions over broad ignores:
   - inline suppression over file-wide suppression
   - specific `ignoreExports` / `ignoreDependencies` over broad patterns
   - targeted `overrides` over global rule changes
6. Only change health thresholds if that reflects an intentional repo-wide policy.
7. Remove stale suppressions if you find them.
8. If the repo cannot be fully cleaned up in one pass, save per-analysis baselines and use `fallow audit` as a gate on new issues while cleanup continues.
9. Re-run fallow until the repo reaches the goal state.

Constraints:
- do not hide real issues behind broad ignore patterns
- use baselines only as a temporary migration aid, not as the desired steady state
- if you add an exception, say why it exists
- if you change a threshold or rule, explain the policy decision

At the end, report:
- what was fixed in code
- what exceptions were added or changed
- whether baselines were added or changed
- why each exception exists
- the final fallow result
```
