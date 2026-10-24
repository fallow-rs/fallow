### Fixed

- **A CODEOWNERS file with thousands of wildcards no longer stalls ownership lookups.**
  All CODEOWNERS rules match in one automaton, and its lookup time grows with
  the number of wildcards (`*`, `?`, `[`, `{`). A 4 KB file with one
  `*a*a*a…` pattern took seconds for each lookup. Fallow now rejects a
  pattern with more than 32 wildcards, or a file with more than 4096, with an
  error that names the limit. Large public CODEOWNERS files, such as those of
  Kibana, GitLab and Grafana, use fewer than 400. Found by OSS-Fuzz.
