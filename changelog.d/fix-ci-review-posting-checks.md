### Fixed

- **`fallow ci` posting steps check who wrote a comment and where its marker is.**
  - `ci post-pr-comment` updates only a comment that the bot account wrote and that has the `fallow-id` marker as a full line. Before, it updated the first comment with the marker, whoever wrote it. Without `FALLOW_BOT_LOGIN`, the bot is a bot account or the user of the token. On GitHub, fallow reads the login of the token from `/user` only when a comment with the marker is not from an account of type `Bot`, so a personal access token keeps its comment.
  - `ci post-review` and `ci reconcile-review` read the fingerprint, resolution and review-id markers only from the marker lines at the end of a comment. Text that a finding comment quotes, such as an import path or a suggestion block, no longer changes the identity of a thread, resolves it, or stops the run.
  - `ci post-review` always posts a GitHub review with the `COMMENT` event. The `event` field of the envelope is ignored.
  - A GitLab note for a finding without a diff position escapes control characters in the file path.
