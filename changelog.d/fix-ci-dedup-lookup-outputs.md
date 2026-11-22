### Fixed

- **The `dedup-lookup-failed` output of the GitHub Action and the `fallow-dedup-lookup-failed.txt` marker of the GitLab template report a failed lookup again.**
  `fallow ci post-pr-comment` and `fallow ci post-review` stop without a post when they cannot read the existing comments. The scripts then still wrote `false`, so a gate on these outputs never fired. The commands now give the error code `FALLOW_CI_DEDUP_LOOKUP_FAILED` in their JSON error document, and the scripts set `dedup-lookup-failed` to `true`. The review step also sets `post-skipped-reason` to `pagination_failure`. A failed lookup never posts a duplicate comment. A pinned fallow version without this code keeps the outputs at `false`. The warning of a failed post now names the reason.
