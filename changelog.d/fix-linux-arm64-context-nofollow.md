### Fixed

- **`fallow audit` no longer follows a symlinked context file on Linux arm64.**
  The reusable audit cache hashes context files such as lockfiles. It opens
  them with a flag that refuses symlinks. On Linux arm64 the code used the
  x86-64 value of that flag, so the open followed a symlink and hashed the
  target, also a target outside the project. The flag values now come from
  the platform headers.
