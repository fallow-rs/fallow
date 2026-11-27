### Fixed

- **The `pr-comment-*` sidecar files use the same path check as `--output-file`.** The files in `FALLOW_PR_COMMENT_ENVELOPE_FILE`, `FALLOW_PR_DECISION_FILE` and `FALLOW_PR_DETAILS_FILE` are no longer written through a symlink that resolves outside the project root, the Git work tree, the CI workspace or the temp directory. Fallow prints a warning for each refused file and still renders the comment.
