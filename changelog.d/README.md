# Changelog fragments

A pull request does not edit `CHANGELOG.md`. It adds its entries in a new file
in this directory. Two pull requests then never change the same lines, so the
merge of one does not make the other conflict.

## Write a fragment

1. Make one file per pull request, for example `changelog.d/<branch-name>.md`.
   Use a name that no other open pull request uses.
2. Start each group of entries with a `###` heading. Use only these headings:
   `Breaking`, `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`,
   `Performance`, `Security`. Use each heading once in a file.
3. Write each entry in the format of `CHANGELOG.md`: a bold headline sentence,
   then the detail. End with `(Closes [#N](url))` when the change closes an
   issue.

```markdown
### Fixed

- **`fallow dead-code` no longer reports X.** Detail of the fix.
  (Closes [#123](https://github.com/fallow-rs/fallow/issues/123))
```

To change an entry later, edit the fragment file. Do not edit the
`[Unreleased]` section of `CHANGELOG.md`: the `Changelog fragments` CI step
fails a pull request that changes it.

## Check and release

- `node scripts/changelog-fragments.mjs check` validates every fragment.
- `node scripts/changelog-fragments.mjs collect --dry-run` prints the
  `[Unreleased]` section that the fragments make.
- The release runs `node scripts/changelog-fragments.mjs collect`. The command
  moves every fragment into `[Unreleased]` in merge order, merges each heading
  into one, and deletes the fragment files.
