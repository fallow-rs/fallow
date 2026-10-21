### Fixed

- **A peer of a package that a script runs or that a plugin credits is no
  longer reported as an unused dependency.** Before this fix, only the
  packages that a file imports started the peer-dependency closure. For
  example, a `napi build` script uses `@napi-rs/cli`, but its peer
  `@emnapi/runtime` was reported as an unused devDependency. The closure now
  also starts from the packages that a script runs, that a plugin config
  names, and the plugin tooling with evidence of use. `fallow dead-code
  --trace-dependency` names the same host in `peer_of`.
- **A module that imports `X` under an alias and exports its own `X` is no
  longer a duplicate export of the source module.** Such a module is an
  adapter, for example `import { configureAuth as core } from "../ci/auth"`
  followed by `export function configureAuth()`. It derives its export from
  the source, as a re-export does. When a third module exports the same name
  independently, the finding stays between the source and that module. Two
  modules that import `X` from each other under aliases are still reported.
