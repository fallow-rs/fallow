### Added

- **The npm package now ships `rule-pack-schema.json`.** A rule pack can point `$schema` at `./node_modules/fallow/rule-pack-schema.json` and get a schema that matches the installed Fallow version. The package also exports the file as `fallow/rule-pack-schema.json`. Thanks [@hckhanh](https://github.com/hckhanh) for the report and [@tahodev](https://github.com/tahodev) for the change. (Closes [#3851](https://github.com/fallow-rs/fallow/issues/3851))
