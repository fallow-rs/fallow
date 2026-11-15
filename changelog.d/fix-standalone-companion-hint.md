### Fixed

- **A Homebrew or `cargo install` binary now names a companion install that works for it.** Such a binary has no npm launcher, so it does not find a global npm companion. When `fallow-type-aware` is missing, the message now names `FALLOW_TYPE_AWARE_BIN` with the global npm path on macOS and Linux. On Windows, it names the npm install of `fallow`. When `fallow-similar-code` is missing, the message names the release archive and the directory of the binary. The npm install keeps its messages and its companion lookup.
  (Closes [#4609](https://github.com/fallow-rs/fallow/issues/4609))
