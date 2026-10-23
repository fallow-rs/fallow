### Added

- **`health.cyclomaticPolicy: "literal-membership"` counts a literal
  membership chain as one predicate.** A chain such as
  `kind === "a" || kind === "b" || kind === "c"` then adds no cyclomatic
  increments for its `||` operators. Only strict equality checks of one
  identifier against string, number, bigint, boolean, or `null` literals
  qualify. The policy applies to the `maxCyclomatic` gate and to every CRAP
  score. Findings report the policy value as `cyclomatic` and the standard
  value as `standard_cyclomatic`, the summary names the policy, and
  `--complexity-breakdown` marks the chain operators with
  `literal_membership`. The default stays `standard`. The health score,
  complexity density, and hotspots keep the standard count. The parse cache
  version changes because the cached breakdown has the new flag.
  (Closes [#3781](https://github.com/fallow-rs/fallow/issues/3781))
