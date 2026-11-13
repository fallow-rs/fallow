### Added

- **`fallow audit --staged` audits the staged index, not the working tree.** The changed files are the staged changes against HEAD, and the analysis reads each file from the index. Unstaged edits and untracked files do not change the verdict. A file that you stage and then delete from the working tree is still audited. When Git gives the hook another index file (`git commit -a`, `git commit <path>`), the audit reads that index. `--staged` does not take `--base` or `--changed-since`.

### Changed

- **The generated pre-commit hook runs `fallow audit --staged`.** `fallow hooks install --target git`, `fallow init --hooks`, the existing-hook hint and the Lefthook hint now audit the commit that Git is about to make. Before, an untracked file in the working tree could block a clean commit, and a staged file that was deleted from the working tree passed without an audit. Before the first commit of a repository, the hook skips the audit and prints one line on stderr. `--branch` has no effect now and prints a note. Run `fallow hooks install --target git --force` to update an installed hook.
