### Fixed

- **`fallow mutation` stops a run when the mutant run path of the runner executes no tests.**
  The dry run selects whole test files, and a mutant run selects tests by id
  through another path of the runner API. When that path failed before a
  test started, each mutant looked killed, and the score was a false 100%.
  Before the first mutant job, a self-check now runs the tests of one
  covered mutant with no active mutant, through the same path as a mutant
  run. When that control run executes no tests, the run exits with code 2
  and `FALLOW_MUTATION_RUNNER_FAILED`. The message names the runner and its
  version. When the control run times out, the message says so and names
  `--setup-timeout`. The check costs one run.
- **A `fallow mutation` test that fails without a mutant in a mutant run is a baseline failure.**
  A test can pass in the dry run of its whole file, but fail when a mutant
  run selects it alone, for example when it depends on another test of its
  file. When the self-check or a later control run finds such a test, the
  test goes into `baseline_failures` and covers no mutant. The run
  continues, so the test gives no false kill.
- **A `fallow mutation` kill with no executed test needs a passing control run.**
  A mutant run that executes no test and reports an error is `killed` only
  when the same tests pass with no active mutant. A mutant that throws when
  its module loads stays `killed`. Otherwise the mutant is `runtime_error`.
  Each distinct test set gets at most one control run in a run, and the
  result cache never stores a control result.
