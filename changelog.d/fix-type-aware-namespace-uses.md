### Fixed

- **`fallow dead-code --type-aware` no longer confirms an export as unused when
  a namespace object can read it.** The type-aware check now credits an export
  that a static read of a namespace object reads, for example
  `({ value } = ns)`, `const { "value": local } = ns`,
  `function run({ value } = ns)`, `(ns as typeof ns).value`,
  `outer.inner.value` after `export * as inner`, and `m.value` after
  `import m = require("./values")`. When the check cannot prove which exports
  a namespace use reads, for example `console.log(ns)`, `const copy = ns`,
  `export { ns }`, `const { ...rest } = ns`, `ns[key]` or
  `module.exports = require("./values")`, it now abstains with
  `dynamic-behavior` for every export of that module, also the exports that a
  barrel re-exports. A destructure that can read a nested namespace, for
  example `const { inner } = outer`, abstains for the exports of the nested
  module. A dynamic import, for example `import("./barrel")`, now also
  abstains for the exports that the barrel re-exports. Before this fix, such an
  export could get a `confirmed-no-static-references` decision.
- **`fallow dead-code --type-aware` abstains for a class member that a computed
  destructure key can read.** `const { [key]: run } = service` and
  `({ [key]: run } = service)` now give `dynamic-member-access`, as
  `service[key]` already did. A string key, for example
  `const { "run": local } = service`, now credits the member.
