### Fixed

- **`fallow ci` posting steps check who wrote a comment and where its marker is.**
  - `ci post-pr-comment` updates only a comment that the bot account wrote and that has the `fallow-id` marker as a full line. Before, it updated the first comment with the marker, whoever wrote it. On GitHub without `FALLOW_BOT_LOGIN`, the bot is an account of type `Bot`. On GitLab without `FALLOW_BOT_LOGIN`, it is a bot account or the user of `GITLAB_TOKEN`. A personal access token on GitHub needs `FALLOW_BOT_LOGIN`, as for review threads.
  - `ci post-review` and `ci reconcile-review` read the fingerprint and resolution markers only from the last marker line of a comment. Text that a finding comment quotes, such as an import path or a suggestion block, no longer changes the identity of a thread, resolves it, or stops the run.
  - `ci post-review` always posts a GitHub review with the `COMMENT` event. The `event` field of the envelope is ignored.
  - A GitLab note for a finding without a diff position escapes control characters in the file path.
