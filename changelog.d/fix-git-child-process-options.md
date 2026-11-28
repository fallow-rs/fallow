### Changed

- **`--changed-since` and `--base` refuse a ref that searches commit messages or names a range.** A ref such as `main^{/fix}` or `:/fix` makes git walk the whole history, and it names no fixed commit. A range such as `main...HEAD` always failed, because fallow compares the ref with `HEAD` itself. Fallow now refuses these refs, and also a ref with nested braces, before it starts git. Use a branch, a tag, a commit SHA, or a form such as `HEAD~3` or `main@{upstream}`.

### Fixed

- **Fallow runs no git hook of the repository.** A git command of fallow, for example the `git worktree add` of `fallow audit --base`, could start a hook from the hooks folder of the repository or a hook that the git config names. Each git command of fallow now points `core.hooksPath` at a folder that holds no hook, and turns off the hooks of each event with `hook.<event>.enabled=false` (git 2.54 and later). The install of the fallow `pre-commit` hook still writes to the hooks folder of the repository.
- **A tracked file with the name of a revision no longer stops `--changed-since`.** A file named `HEAD`, or with the name of the base ref, made git stop with "ambiguous argument". Each git diff of fallow now ends its revisions with `--`.
- **A `diff.external` program or a textconv filter no longer changes the changed lines or the flag ages.** The changed diff of `--changed-since` and `fallow mutation` used the external diff program and the textconv filters of the user config. An external program that printed nothing made fallow see no changed lines. The diff now uses `--no-ext-diff --no-textconv`, and the blame and history search of the flag ages use `--no-textconv`.
