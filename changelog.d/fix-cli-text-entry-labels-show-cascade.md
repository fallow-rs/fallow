### Changed

- **The `source` of a package.json entry point names the field that declares it.**
  `fallow list --entry-points`, its JSON `entry_points[].source` value and the
  entry weight listing used the label `package.json main` for each field of the
  root package and of a workspace package, and `package.json exports` for each
  field of a nested package. A wildcard target such as `"main": "./src/*.ts"`
  used `package.json exports` in each package. The label now names the real
  field: `package.json main`, `package.json module`, `package.json types`,
  `package.json typings`, `package.json source`, `package.json browser`,
  `package.json bin` or `package.json exports`. The four labels `types`,
  `typings`, `source` and `browser` are new values. A script that matches
  `"source": "package.json main"` to find every package.json entry point must
  match the `package.json ` prefix instead.
  (Closes [#4831](https://github.com/fallow-rs/fallow/issues/4831))

### Fixed

- **`fallow dead-code --show-cascade` lists the hidden findings in the human report.**
  The JSON report listed the unused exports and unused types of unused files
  with the flag, but the human report still left them out and printed only a
  count line. The human report and its summary line now list and count them,
  as the help text says. The same applies when the config sets
  `showCascade: true`. Without the flag or the config key, the report hides
  them as before.
  (Closes [#4832](https://github.com/fallow-rs/fallow/issues/4832))
- **CLI texts match the behavior.** The help and the input error of `fallow dupes --trace` name
  both accepted forms, `FILE:LINE` and `dup:<fingerprint>`. The
  `code-duplication` explanation describes each `duplicates.mode` value
  correctly: `weak` ignores only string values, and only `semantic` ignores
  identifiers. The combined report prints no em dash in its target hint and
  in the warning for a project without entry points.
  (Closes [#4830](https://github.com/fallow-rs/fallow/issues/4830))
