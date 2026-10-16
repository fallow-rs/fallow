### Security

- **Jest and Vitest `projects` entries no longer read files outside the
  project root.** A Jest `projects` entry or a Vitest `test.projects` entry
  can name an absolute path, a path with `..` segments, or a glob. Fallow
  followed such an entry to a config file outside the project root and read
  that file. A symlink in the project root to a directory above it had the
  same effect. Fallow now resolves each match and reads it only when it is
  inside the project root. A glob whose literal base is outside the project
  root does not run. An in-root entry such as `<rootDir>/../packages/*` still
  works, also from the config of a workspace package. A script `--config` path
  that is a symlink to a file outside the project root is also not read.
