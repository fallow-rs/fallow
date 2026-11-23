### Fixed

- **Doctor includes the selected config in its cache repair command.** The command rebuilds the parse and graph caches that doctor inspected.
- **Config fix reports list only the ignore rules that Fallow adds.** The preview and the applied report both skip files that already have an `ignoreExports` rule.
