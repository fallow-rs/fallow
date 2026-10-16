### Fixed

- **`fallow audit` no longer blocks a merge of the default branch.** When a
  branch merged the default branch, the new-only gate compared only with a
  base on the branch. Every finding that the merge brought in from the default
  branch was then `introduced`, so the commit gate blocked the merge commit and
  the push after it. With an auto-detected base, audit now also compares with
  the merged default-branch commits: `MERGE_HEAD` of a merge in progress, and
  the merge-base with the default branch after a merge commit. A finding that
  one of these commits has is inherited. Code from a merged branch that the
  default branch does not contain stays introduced. An explicit `--base` or
  `FALLOW_AUDIT_BASE` keeps the old single-base comparison.
  The diff-risk rules still read the diff against the primary base, so with
  their default `warn` severity such a merge gives `warn`, not `fail`.
