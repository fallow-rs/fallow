### Fixed

- **Repeated or very large config input no longer makes the config load slow or the memory grow.** Fallow now does each of these steps once and keeps it within a limit:
  - A config file that names the same `extends` source more than once merges it one time, at its last position, which gives the same result. A config file can list at most 64 distinct `extends` entries.
  - A `rulePacks` entry that names a pack file again is a config error, and the pack file is read once. A rule that names an unknown zone gets one error for each distinct name and at most 10 such errors, and the list of configured zones in the message holds at most 512 bytes.
  - External plugin discovery reads each real plugin file and lists each plugin folder once, and it reads at most 256 distinct plugin files.
  - Boundary `autoDiscover` folders such as `src/./d` and `src//d` give the same child zones as `src/d`. A zone that is declared more than once adds its child zones to the rules once.
  - Regexes in `unusedComponentProps.ignorePattern`, `health.tailwindIgnorePatterns` and plugin exclude regexes compile with a 1 MiB program limit. A larger regex is a config error. `health.tailwindIgnorePatterns` holds at most 128 patterns.
  - A `manifestEntries` path that is longer than 512 bytes after interpolation stops the expansion with the entry expansion warning.
  - A `fileExists` or tooling glob with `**` segments reads each folder once for each pattern segment, and a folder link loop no longer makes the walk run without end.
  - Entry rules with the same exclude list, such as the spec files of a Jasmine config, compile the list one time.
