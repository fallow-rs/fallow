### Fixed

- **`fallow health --production` no longer marks tested functions as untested in the CRAP estimate.**
  Production mode keeps test files out of the analysis. The static coverage
  estimate used the same file set, so it found no tests. Each function with a
  cyclomatic complexity of 6 or more then failed the default `maxCrap` of 30.
  Coverage gaps also reported tested files and exports as untested. Fallow now
  reads test reachability from a graph of the full project, and findings still
  come only from production files. A production run now gives the same CRAP
  scores and coverage gaps as a full run for the files that it analyzes. A
  production health run builds this second graph, so it takes more time.
  Thanks [@Smrtnyk](https://github.com/Smrtnyk) for the report and the
  reproduction in #3822.
