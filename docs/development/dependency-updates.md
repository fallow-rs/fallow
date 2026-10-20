# Dependency updates

Dependabot opens the dependency pull requests for this repository. Two
workflows take them to `main` without a maintainer in most cases.

## Flow

1. Dependabot opens a pull request. The cooldown in `.github/dependabot.yml`
   waits 7 days after each upstream release.
2. `dependabot-auto-merge.yml` arms auto-merge for every update that is not a
   major version. When the checks already passed, it merges at once.
3. When CI or CodeQL fails, `dependabot-repair.yml` triages the run with
   `.github/scripts/dependabot-triage.mjs`:
   - `rerun`: only cancelled jobs, or the flaky `VS Code Extension` job on the
     first attempt, failed.
   - `repair`: a job failed. Claude Sonnet 5.5 gets
     `.github/dependabot-repair.md` and an excerpt of the failed job logs. It
     makes one local commit.
   - `give-up`: two repair commits did not make the run pass. The pull request
     gets the `needs-human` label and a comment.
   - `skip`: the run is out of date, a person pushed to the branch, or the
     pull request has the `needs-human` label.
4. The publish job checks the commit with
   `.github/scripts/dependabot-repair-guard.mjs` and pushes it with the
   Dependabot GitHub App token. The push starts CI.
   - When the commit changes only lockfiles, generated artifacts, snapshots
     or `scripts/*.test.mjs`, a minor or patch update keeps auto-merge. It
     merges when the required checks pass.
   - When the commit changes any other file, auto-merge is turned off and the
     pull request gets the `needs-review` label. That code can change what a
     build or a release runs, so a maintainer reviews it and merges.
5. When the repair makes no commit, or the guard rejects it, the pull request
   gets the `needs-human` label and a comment with the diagnosis.

Major updates do not auto-merge. The repair still runs on them, so a
maintainer can review a green pull request.

## Why updates fail

These causes made a person act on a Dependabot pull request in the past. The
repair instructions hold the fix for each one.

- A test in `scripts/*.test.mjs` pins the SHA or version of an action or a
  package.
- Two manifests must pin the same version, for example `@tanstack/intent` in
  the root and in `npm/fallow`.
- A bundler update changes the committed viz bundle, or a producer update
  changes the digest in the coverage producer corpus.
- A semver-compatible update changes an API that a crate calls.
- `github/codeql-action` sub-actions on different versions. The
  `codeql-action` group in `.github/dependabot.yml` now prevents this.
- Auto-merge armed with `GITHUB_TOKEN` on an update of a workflow action. The
  Dependabot GitHub App removes this cause.

A repair does not fix a change under `.github/`, an advisory that also affects
`main`, or an update that needs a review of changed behavior. A failure in
the `Commit messages` check is outside the repair, because the repair watches
only the CI and CodeQL workflows.

## Setup

| Setting | Type | Use |
| --- | --- | --- |
| `ANTHROPIC_API_KEY` | Actions secret | The repair uses the Claude API. |
| `CLAUDE_CODE_OAUTH_TOKEN` | Actions secret | The repair uses a Claude subscription. Create the token with `claude setup-token`. |
| `DEPENDABOT_APP_CLIENT_ID` | Actions variable | The client ID of the Dependabot GitHub App. |
| `DEPENDABOT_APP_PRIVATE_KEY` | Actions secret and Dependabot secret | The private key of that App. `dependabot-auto-merge.yml` runs for Dependabot, so it can need the Dependabot copy. |

Set one of the two Claude secrets. Without either, the repair job logs a
notice and stops.

The App needs the `contents`, `pull_requests` and `workflows` write
permissions on this repository. Without the App:

- Auto-merge uses `GITHUB_TOKEN`. GitHub disables auto-merge on a pull
  request that changes a workflow file, and a merge by `GITHUB_TOKEN` starts
  no workflow, so CI does not run on `main` for that merge commit.
- The publish job does not push the repair. The patch stays in the run
  artifacts, and the pull request gets the `needs-human` label.

## Security

- The repair runs only for `dependabot/**` branches in this repository.
- The triage script, the guard and the repair instructions come from `main`,
  not from the pull request branch.
- The repair job installs and builds the updated dependency, which is
  third-party code. So that job holds only a read-only `GITHUB_TOKEN`.
  `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` removes the Claude secret from each
  command, and bubblewrap gives each command its own PID namespace. The
  install commands use `--ignore-scripts`, as CI does.
- The publish job runs on a new runner and runs no project code. The guard
  accepts one commit only. It rejects changes under `.github/`, to the
  changelog and to git metadata files, binary files, symbolic links,
  submodules and paths with control characters. It limits the hand-written
  change to 300 lines.
- The push token can write contents only. A second token with `workflows`
  write sets or clears auto-merge after the push.
- Only a repair of lockfiles, generated artifacts, snapshots or script tests
  merges without review. A generated artifact, such as the viz bundle, is not
  reviewable by a person, and a person rebuilt it the same way before this
  workflow existed.
- The comment shows the repair summary in a code block, because the repair
  runner wrote it.
- Claude gets the failed job logs, not the pull request body, which holds
  upstream release notes. The prompt marks the logs as untrusted data.

## Cost

The repair runs only on a failed run that the triage does not rerun or skip.
In the 150 Dependabot pull requests before this workflow, about 10 needed a
companion commit, in roughly one month. A repair run is capped at 80 turns.
