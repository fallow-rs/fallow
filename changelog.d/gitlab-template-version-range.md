### Fixed

- **The GitLab template now works with a fallow version range or `latest` when `FALLOW_COMMENT`, `FALLOW_REVIEW` or `FALLOW_MUTATION` is on.** `npm install -D fallow` writes a `^` range to `package.json`, and the job stopped with exit code 2. The template now reads the exact version of the installed fallow and downloads the merge request scripts of that release. When the template cannot read the installed version, the error names the three fixes: an exact version in `package.json`, an exact `FALLOW_VERSION`, or a vendored template.
  (Closes [#4841](https://github.com/fallow-rs/fallow/issues/4841))
