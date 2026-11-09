### Fixed

- The GitHub Action rejects symlinks and non-regular destinations before writing analysis, comment, review, and scoped-result artifacts. Shared mutation helpers check output paths before cleanup and publication. Regular artifact files remain replaceable on repeated runs. These checks cover a static checkout, without guaranteeing protection against concurrent path replacement.
- Rejected dependency versions in Action installation diagnostics are escaped so embedded line breaks cannot start another workflow command.
  (Related to [#4259](https://github.com/fallow-rs/fallow/issues/4259))
