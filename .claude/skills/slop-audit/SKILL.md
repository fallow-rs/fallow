---
name: slop-audit
description: Audit Fallow for unnecessary code, misleading tests, redundant abstractions, avoidable copying, and stale maintenance guidance. Use for a requested slop audit or deep cleanup pass.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Slop audit

Use the native `implement` lifecycle for accepted changes, including its ready
PR requirement. Finish with `review`, `ship` when authorized, and `sweep`.
Read `docs/development/repo-map.md` to choose concrete ownership boundaries.

## Evidence before cleanup

Maintain a temporary ledger outside tracked files. For a repository-wide pass,
cover the analyzer pipeline, engine/API/output/CLI, protocol hosts, editor and
web consumers, packaging/sidecars, and maintenance tooling. Inspect these axes
within each relevant domain:

- tests: tautologies, shadow implementations, unreachable assertions, weak
  failure injection, and duplicate behavior coverage;
- abstractions: forwarding wrappers, test-only seams, speculative options,
  duplicated normalization or derived state;
- runtime cost: avoidable owned copies, intermediate collections, repeated work;
- failure behavior: swallowed errors, stale fallbacks, lifecycle and cleanup;
- ownership: public contracts, dynamic entrypoints, security/release boundaries;
- maintenance: obsolete comments, copied catalogues, dead routes and dependencies.

Record exact paths, real callers, the proposed simplification, retained
behavioral coverage, and validation evidence. Distinguish inspected/no-change
from unexamined or blocked areas. A search hit is a hypothesis, not a finding.
Use disjoint subagents for independent domains when available, then have a
reviewer challenge both accepted changes and plausible no-change decisions.

Before removing a test, establish which executed assertion implies its useful
contract. For misleading tests, inject a realistic production fault and show
that the old test accepts it while the replacement rejects it. Keep the
replacement on the runtime path, not a second implementation in test code.
Similar bodies, assertion counts, or deletion targets do not prove redundancy.

Trace private helpers through all callers before inlining. Preserve public
facades, dynamic consumers, compatibility fallbacks, distinct regressions, and
integrity guards. An always-true private option may be removed only after all
callers are verified; retain its required behavior unconditionally.

For ownership changes, prove lifetimes and error/ordering parity. Measure
allocations or runtime before claiming a performance gain. Avoid generalizing
an abstraction merely to merge two similar implementations.

## Completion

Run focused checks, realistic public-project validation, and the relevant full
suites from `docs/development/quality-gates.md`. Compare nonempty outputs and
negative controls; include cold/warm/no-cache modes when analysis changes.
Bug fixes require the normal failing reproduction and real-world validation.

In a second pass, revisit previous assumptions with new callers, counterexamples,
or runtime probes. Repeating searches or passing the same suite is not deeper
coverage. Stop when accepted findings are resolved, independent review has no
blockers, and remaining hypotheses have explicit evidence or stated limits.
Report concrete changes and verification, without claiming the repository is
free of defects. An authorized merge is complete only after merged-commit CI.
