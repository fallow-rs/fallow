### Security

- **A glob segment such as `.*` no longer matches the parent directory.** The
  `glob` crate tests the `..` entry against a path segment that starts with a
  literal `.`. Thus a plugin `fileExists` pattern such as `.*/marker.json`
  passed glob validation, but it also found `marker.json` in the parent of the
  project root. The same applied to the seeded entry check of
  `fallow plugin-check`, to tooling config detection, and to `package.json`
  and `pnpm-workspace.yaml` workspace globs. Fallow now rewrites a leading `.`
  in such a segment to `[.]` before it expands the glob on disk. The pattern
  still matches hidden files and directories inside the project root.
