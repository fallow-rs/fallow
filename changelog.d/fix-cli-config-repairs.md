### Fixed

- **Doctor includes the selected config in its cache repair command.** The command rebuilds the parse and graph caches that doctor inspected. Doctor JSON omits `remediation` and gives manual steps in `message` when the config path contains shell-unsafe characters, the config is outside the project root, or the run overrides the cache directory.
- **Config fix reports list only the ignore rules that Fallow adds.** Dry-run and apply print a note on stderr when an existing `ignoreExports` rule prevents a write. The note names the affected file.
