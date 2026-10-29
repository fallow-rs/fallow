### Fixed

- **An `unused-class-member` or `unused-store-member` suppression that only
  `--production` needs is no longer reported stale in a run without
  `--production`.** Production mode excludes test files. A class member or a
  Pinia store member that only a test file reads is unused there, so its
  `fallow-ignore-next-line` comment is necessary. A run without `--production`
  now scans these members again without the files that production mode
  excludes. That scan uses the `usedClassMembers`, `ignoreDecorators`, plugin
  and Lit inputs of the run. The comment stays stale when a production file
  reads the member. A member comment in a file that only a test reaches now
  stays stale for all three member kinds, because production mode reports that
  file as unused. This extends the fix for
  [#3962](https://github.com/fallow-rs/fallow/issues/3962) to the other member
  kinds.
