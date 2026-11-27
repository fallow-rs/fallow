### Performance

- **A run without a reader skips the feature flag walk and the component
  contract scans.** The parse walked each file for feature flags and
  collected the component contracts of each file, also when no rule or
  report of the run read them. Now only `fallow flags`, the HTML output of
  `fallow viz` and the `absent-component-prop` rule turn these scans on. The
  parse cache records which entries hold the facts, so a later run that reads
  them parses only the files whose entry lacks them. The output does not
  change.
