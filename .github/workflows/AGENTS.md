# Workflow Agent Guide

Use this file when editing `.github/workflows/**`.

## Rules

- Keep privileged publish jobs separate from jobs that install dependencies or run untrusted project code.
- Pin third-party actions and publish tools. Do not switch to floating versions.
- Use the smallest permissions block each job needs.
- Keep `persist-credentials: false` unless a job explicitly pushes refs.
- Prefer reusable setup actions already in `.github/actions` before adding new workflow boilerplate.
- For path-filtered jobs, update filters and local guidance together when adding a new surface.
- A job that runs on pull requests must be a required status check on `main`, or must catch a bug class that `main` cannot catch one commit later. Run every other job on push to `main`, on a schedule, or behind the `ci:perf` label. See `docs/development/quality-gates.md`.
- Update `REQUIRED_WORKFLOWS` in `scripts/verify-release-ci.mjs` when you add or move a workflow that runs on push to `main`. The release gate fails when a required run is missing.
- Do not add `merge_group` triggers. The repository does not use a merge queue.

## Release Boundary

- Build or package artifacts in prep jobs with read-only permissions.
- Publish only downloaded artifacts in privileged jobs.
- Do not run `npm install`, `pnpm install`, `cargo` verify-build, or package lifecycle scripts in jobs that hold publish tokens.
- Preserve job names and workflow file names that are tied to trusted publishing configuration.

## Validation

- Run `actionlint` for workflow syntax.
- Run `zizmor` for security-sensitive workflow changes.
- For release workflow edits, also parse the YAML and verify any package-name or artifact-count constants changed together.
