### Fixed

- **A very long CODEOWNERS pattern or config glob no longer stalls fallow.**
  The glob matcher builds an automaton for each literal prefix, and its build
  time grows with the square of the pattern length. A 96 KB CODEOWNERS
  pattern took minutes. Fallow now rejects a CODEOWNERS pattern or a config
  glob that is longer than 512 bytes, with an error that names the limit.
  Found by OSS-Fuzz.
