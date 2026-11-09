### Fixed

- Mutation-generated changed scopes detect renames and copies, so unchanged copied lines no longer receive mutants when Git recognizes their source. Edited copied lines and new files remain in scope. Supplied diff hunks retain their literal meaning. Fixes #4364.
