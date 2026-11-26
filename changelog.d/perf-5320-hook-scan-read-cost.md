### Performance

- **The scan of git hook files is fast again.** The scan reads a list of
  candidate files (husky, lefthook, simple-git-hooks, lint-staged and agent
  hook configs) for the project root and for each workspace. The bounded
  read of each candidate resolved the path again, and a missing candidate
  took several path resolutions. The scan now resolves the read scope once
  for each root, and a missing candidate costs one `lstat` call. On a
  checkout of `vuejs/core`, the `script analysis` row of `--performance`
  goes from about 63 ms to about 9 ms. Each bounded read of a missing
  project file, for example a missing `tsconfig.json`, also takes fewer path
  resolutions, and the check of a workspace member `package.json` that is a
  plain file takes two `lstat` calls. A link that leaves the project, a
  dangling link, a file that is not a regular file and a file that is too
  large are still refused.
  (Closes [#5320](https://github.com/fallow-rs/fallow/issues/5320))
