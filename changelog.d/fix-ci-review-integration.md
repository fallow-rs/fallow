### Fixed

- **GitLab retries temporary errors when it reads merge request metadata.** The CI job uses the HTTP status to decide whether to retry.
- **GitHub Enterprise Server reviews use the runner's API endpoints.** Sticky comments and inline reviews use the runner's REST URL. Thread lookups and resolutions use its GraphQL endpoint.
