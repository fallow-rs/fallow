### Fixed

- **The GitLab CI template keeps the mutation results when the job fails.** GitLab saves a job cache only after a successful job by default. A survived mutant or another error finding fails the job, so the job lost `.fallow/mutation/results.json`, and the next pipeline started the mutation run again from the start. A run that stopped at its time budget could then never continue. The template now saves the `.fallow` cache also when the job fails, the same as the GitHub Action saves its mutation cache. The mutation step also writes its cache stamp before the run, so the next run keeps the results of a job that stopped during the run.
  (Refs [#4383](https://github.com/fallow-rs/fallow/issues/4383))
