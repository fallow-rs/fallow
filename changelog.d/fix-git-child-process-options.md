### Changed

- **`--changed-since` and `--base` refuse a ref that searches commit messages.** A ref such as `main^{/fix}` or `:/fix` makes git walk the whole history, and it names no fixed commit. Fallow now refuses it, and also a ref with nested braces, before it starts git. Use a branch, a tag, a commit SHA, or a form such as `HEAD~3` or `main@{upstream}`.

### Fixed

- **Fallow runs no git hook of the repository.** A git command of fallow, for example the `git worktree add` of `fallow audit --base`, could start a hook from the hooks folder of the repository. Each git command of fallow now sets `core.hooksPath` to the null device. The install of the fallow `pre-commit` hook still writes to the hooks folder of the repository.
- **A tracked file with the name of a revision no longer stops `--changed-since`.** A file named `HEAD`, or with the name of the base ref, made git stop with "ambiguous argument". Each git diff of fallow now ends its revisions with `--`.
