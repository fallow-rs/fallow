### Added

- **`fallow explain` shows a tested code example for each rule.** Next to the sentence that describes the example, `fallow explain <id>` now prints the files of a small project that makes the rule report a finding. A rule of `fallow audit` shows the files before the change and after it. `--format json` has the files in two new arrays, `example_files` and `example_base_files`, each with a `path` and the `code` of the file; `fallow schema --cli-reference` has the same arrays for each issue type. A test runs fallow on each example and checks that the rule reports it. A rule has no example when it needs data that a small project does not have, such as production runtime data or a test run.

### Fixed

- **Four `fallow explain` examples describe code that fallow reports.** The example of `duplicate-prop-shape` had too few significant props to give a finding. The example of `nextjs-open-redirect` called `.get` on the `searchParams` page prop, which is a plain object. The example of `unsafe-buffer-alloc` showed a call that fallow reports as `resource-amplification`. The example of `circular-dependency` now uses the file names of its code example.
