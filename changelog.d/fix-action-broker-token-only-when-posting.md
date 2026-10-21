### Changed

- **The GitHub Action requests the branded token only when it posts.** The
  `Fallow branded token` step now runs only on a `pull_request` event with
  `comment: true` or `review-comments: true`. Other runs do not request an
  OIDC token. The `branded-token` input now defaults to `auto`. `auto` posts
  as the Fallow app when the workflow grants `id-token: write`, and it falls
  back to `github-token` without a log line when the permission is missing.
  An explicit `branded-token: true` keeps one short fallback line. No line
  asks the workflow for more permissions. `false` behaves as before.
