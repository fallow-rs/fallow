### Fixed

- **A changed file with a non-ASCII character or a space in its name is in the diff scope.** Git writes a quoted path, or a tab after the path, in the header of such a file. Diff-scoped results (`--diff-file`, `--diff-stdin`, `fallow audit`, `fallow security --gate new`, the review comments) did not find the file, so its findings dropped. The change anchors of `fallow audit --brief` for a file with a space in its name get new ids one time.
- **An added line that starts with `++` no longer stops the base-line mapping of a diff.** `fallow audit --gate new-only` then kept clone groups as introduced that the change did not write. The walkthrough anchors now also contain such a line.
- **`fallow audit --brief` compares a moved `package.json` with its old path.** Without `--diff-file`, each dependency of a moved package showed as added, and its review decisions lost their previous id.

### Performance

- **One `fallow audit` run reads its git diff one time for attribution.** The moved dead-code check, the duplication demotion and the styling comparison each started `git diff` again.
