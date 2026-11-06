### Fixed

- **Audit review comments state the attribution of dead-code, duplication and styling findings.** `review-github` and `review-gitlab` put `introduced` or `inherited` after the severity label of these findings, as they do for complexity findings. Under `--gate new-only` an inherited finding now shows `` **error** · inherited · not gated (`--gate new-only`) ``, because it does not gate the audit. The live audit run and `fallow report --from` on a saved audit give the same text. The CodeClimate output and the fingerprints do not change. A stale suppression comment keeps the plain label.
  (Closes [#4195](https://github.com/fallow-rs/fallow/issues/4195))
