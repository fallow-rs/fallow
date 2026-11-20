### Fixed

- **`fallow flags` no longer reports `boundaries-not-configured` and `rule-packs-not-configured`.** The flag scan runs the dead-code pass to match flags with unused exports, and that pass recorded the two kinds. `fallow flags` runs no boundary or policy check, so its JSON `workspace_diagnostics` now omits them. Other diagnostics, for example `node-modules-missing`, stay.
  (Refs [#4845](https://github.com/fallow-rs/fallow/issues/4845))
- **A report or baseline path in `/tmp` now works on macOS.** On macOS `TMPDIR` names a per-user folder, so `--save-baseline /tmp/x.json` exited 2. Each Unix now allows `/tmp` for baselines, snapshots and report files, as Linux already did.
  (Refs [#4845](https://github.com/fallow-rs/fallow/issues/4845))
- **The human output of `fallow inspect` prints the target and identity as fields.** Each field is on its own `key: value` line, not in raw JSON. A field with no value prints as `unknown`.
  (Refs [#4845](https://github.com/fallow-rs/fallow/issues/4845))
- **`fallow impact status` prints a short human view.** It shows the tracking state, the record count and the store file. Bare `fallow impact` still prints the full report. The JSON and Markdown output do not change.
  (Refs [#4845](https://github.com/fallow-rs/fallow/issues/4845))
- **The GitHub Action names the input that stops the "SARIF upload skipped" warning.** The warning now says `To stop this warning, set sarif: false.` The old advice, `format: json`, did not stop it.
  (Refs [#4845](https://github.com/fallow-rs/fallow/issues/4845))
