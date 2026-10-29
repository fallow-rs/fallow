# Dependency updates

Dependabot opens the dependency pull requests for this repository. Two
workflows take them to `main` without a maintainer in most cases.

## Flow

1. Dependabot opens a pull request. The cooldown in `.github/dependabot.yml`
   waits 7 days after each upstream release.
2. `dependabot-auto-merge.yml` arms GitHub auto-merge for every update that is
   not a major version. It uses the token of the fallow-dependabot GitHub App,
   so a merge of a workflow change works and starts CI on `main`. When the
   checks already passed, it merges at once.
3. `dependabot.yml` calls the org-wide workflow in
   [`fallow-rs/.github`](https://github.com/fallow-rs/.github/blob/main/dependabot/README.md)
   after each CI or CodeQL run on a Dependabot branch, and every three hours:
   - After a failed run, it reruns cancelled jobs and the flaky
     `VS Code Extension` job, or asks Claude for a repair, or labels the pull
     request `needs-human` after two failed repairs. A branch that is behind
     `main` first gets `main` merged in, so the repair builds on the current
     base.
   - A repair of lockfiles, dependency version lines in package manifests,
     tests, snapshots or generated files (the viz bundle, the coverage
     producer manifest) becomes a commit that GitHub signs for the App. It
     merges when the checks pass.
   - Any other repair is not committed. The pull request gets the
     `needs-review` label, auto-merge is turned off, and a comment holds the
     patch, the list of changed files and a command that applies the patch
     after a SHA-256 check.
   - After a successful run, it merges the pull request when every check on
     the head commit passed, except CodSpeed, majors included. A pull request
     with a repair merges only on top of the current `main`. The squash
     subject stays within the 100 character limit of commitlint.

The commit lint and the title lint in `commitlint.yml` skip bot pull requests:
their commit headers and long titles never reach `main`.

A Dependabot pull request that changes a file with a code owner in
`.github/CODEOWNERS` waits for that review, because branch protection requires
it for everyone.

## Repository notes

`.github/dependabot-repair.md` holds the fallow install commands, local checks
and known causes. The repair reads the `main` copy. Update it when a new cause
repeats.

These causes made a person act on a Dependabot pull request in the past:

- A test in `scripts/*.test.mjs` pins the SHA or version of an action or a
  package.
- Two manifests must pin the same version, for example `@tanstack/intent` in
  the root and in `npm/fallow`.
- A bundler update changes the committed viz bundle, or a producer update
  changes the digest in the coverage producer corpus.
- A semver-compatible update reorders the generated contract files or changes
  an API that a crate calls.
- `github/codeql-action` sub-actions on different versions. The
  `codeql-action` group in `.github/dependabot.yml` prevents this.

## Security overrides

The root toolchain overrides `fast-uri` 3 to its patched release. Lockfile
regeneration during a Commitlint update previously restored a vulnerable
version. Remove the override when the parent dependency requires a patched
version, then run the root audit and repository script suites.

The isolated Marketplace and Open VSX publishers override `@azure/msal-node`
to the patched major because Azure Identity still requests the vulnerable
major. The extension applies the same override in `pnpm-workspace.yaml`.
Remove these overrides when Azure Identity's supported range includes the
patched release, and rerun the publisher authentication and extension suites.

The extension also overrides Mocha's `diff` and `serialize-javascript`
dependencies. Keep Mocha compatible with the VS Code engine floor. Remove an
override when Mocha requests a patched version, then run the extension's
integration suites in the oldest supported host.

## Setup

| Setting | Scope | Use |
| --- | --- | --- |
| `DEPENDABOT_APP_CLIENT_ID` | Organization and repository variable | The client ID of the fallow-dependabot GitHub App. |
| `DEPENDABOT_APP_PRIVATE_KEY` | Actions and Dependabot secret | The private key of that App. `dependabot-auto-merge.yml` runs for Dependabot, so it can need the Dependabot copy. |
| `CLAUDE_CODE_OAUTH_TOKEN` | Organization secret | The Claude token for the repair. |

The App needs the `contents`, `pull_requests` and `workflows` write
permissions on this repository.

## Security

The org-wide workflow documents its trust boundary in its README. In short:

- The repair job installs and builds third-party code, so it holds only a
  read-only `GITHUB_TOKEN` and no OIDC permission.
- A separate job on a new runner checks the patch and commits only
  dependency, test and generated changes, as a commit that GitHub signs for
  the App. A commit from the App starts CI with the repository secrets, so
  code changes wait for a maintainer.
- Only signed commits from Dependabot or the App count as automation. Any
  other commit makes the pull request a maintainer's, and the automation
  leaves it alone.
- The scripts and instructions come from the pinned commit of the org-wide
  workflow, and the repository notes from `main`.
