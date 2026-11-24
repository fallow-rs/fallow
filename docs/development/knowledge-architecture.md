# Knowledge architecture

Fallow separates durable knowledge, repeatable workflows, runtime adapters, and
published user documentation. Each fact has one authored source.

## Ownership

| Surface | Canonical source | Consumers |
|---|---|---|
| Open-source maintainer knowledge | `fallow/docs/` | Humans, Codex, Claude |
| Maintainer workflows | `fallow/.agents/skills/` | Codex directly, generated Claude adapters |
| Shared reviewer constraints | `fallow/.agents/rules/` | Maintainer skills, with durable facts routed to `docs/` |
| Claude runtime constraints | `fallow/.claude/rules/` | Claude, with durable facts routed to `docs/` |
| Public CLI, config, MCP, and output contracts | Public Fallow source and generated schemas | Public docs, skills, integrations, private consumers |
| Released Fallow skill contract | `fallow/npm/fallow/skills/<name>/` (`fallow`, `fallow-setup`) | npm package, the binary embed for `fallow agent install`, portable skill packaging, and the generated maintainer copies in `.agents/skills/<name>/` |
| Public user documentation | `fallow-docs` | Documentation site and private site builds |
| Portable plugin packaging and additional end-user skills | `fallow-skills` | Agent hosts and plugin marketplaces |
| Cloud-facing runtime protocol | `fallow-cov-protocol` | Public producers and private consumers |
| Private architecture, operations, roadmap, and security knowledge | Private repository only | Authorized private workflows |

The machine-readable version of this contract is
[`scripts/knowledge-surfaces.json`](../../scripts/knowledge-surfaces.json).

## Layering

1. Root routers tell an agent where to start.
2. The [task context map](task-context-map.md) selects a bounded set of durable
   references.
3. A canonical skill defines a repeatable workflow.
4. Generated host adapters expose that workflow without copying its authored
   content by hand.
5. Tests and CI reject missing routes, unclassified root or maintainer
   documents, stale implementation paths, machine-local references, adapter
   drift, and discoverability regressions.

Auto-loaded routers and rules must contain only high-value constraints and
routing. Stable explanations belong here so they are read on demand and shared
by every host.

## Trust boundary

Public repositories may provide versioned artifacts and machine-readable
contracts to private consumers. Automated synchronization across that boundary
is one-way, from a public allowlisted source into a private build.

No automated path may copy private prose, repository files, or generated
private context into a public repository or artifact. When private work reveals
something useful to public users, a maintainer writes a new sanitized public
change and reviews it as public content. This is promotion, not synchronization.

Public examples must be generic or come from an explicitly public fixture.

## Synchronization rules

- `fallow-docs` is the only authored source for public user documentation.
- A private site consumes an exact public docs revision or deterministic
  artifact and records its provenance.
- Public contract changes originate in the public code or protocol repository.
- The portable Fallow skill consumes the exact released skill contract, with
  only declared host compatibility transforms.
- Private consumers test compatibility with the supported public contract
  versions.
- Generated adapters and artifacts are reproducible and checked with a
  non-mutating `--check` command.
- Cross-repository gates fail closed in CI. They must not report success merely
  because a sibling checkout is absent.
- A source manifest identifies the repository, revision, allowed root, and
  processor for each imported public artifact. Its check command is stored as
  an executable argument array, not shell prose.

The public docs archive records a complete content digest and source commit.
Private consumers pin both values. The portable skill repository records the
exact Fallow source commit, source root, target root, and declared transform.
The vendor step also pins each download-and-run form of the fallow package,
such as `npx fallow`, to the version in `npm/fallow/package.json` of the source
commit. The Claude plugin directory refuses skill text that runs an unpinned
package, so the source text stays unpinned and only the published copy carries
a version. Both checks ignore the pinned version and refuse an unpinned
launcher, so a release does not make the published copy drift.
After a skill change lands on `main`, `node scripts/sync-skills-companion.mjs`
prepares the companion commit. The companion `main` branch is protected, so
the commit lands through a fallow-skills pull request. The
`skills-companion-sync.yml` workflow opens or updates that pull request (see
[Skills companion sync](#skills-companion-sync)). Without its credential, the
maintainer runs the script by hand. The script then signs the commit and prints
the publish commands.
Pull requests compare the published companion with their exact public base
commit, while required generation checks validate the candidate contracts and
adapters. Pushes compare the current source and keep failing on landed drift
until the companion is synchronized.
Protocol consumers pin the public crate in their lockfile and verify published
sidecar parity.

## Skills companion sync

`.github/workflows/skills-companion-sync.yml` runs on each push to `main` that
changes `npm/fallow/skills/**` or the sync scripts. A maintainer can also start
it with `workflow_dispatch` on `main`. It has two jobs:

1. The `prepare` job runs `node scripts/sync-skills-companion.mjs --pin
   "$GITHUB_SHA" --no-gpg-sign` against a fresh checkout of the companion
   `main` branch. The script runs the companion checks before it commits. The
   job uploads the commit as a git bundle.
2. The `publish` job checks the bundle against the companion `main` branch.
   It force-pushes the commit to the `sync-fallow-skills` branch of
   `fallow-rs/fallow-skills`. Then it opens a pull request from that branch,
   or updates the title and body of the open one.
3. When the companion already matches the pin, the `publish` job closes an
   open sync pull request, because that pull request holds an older pin.

When the companion `main` branch moves during the run, the job pushes nothing
and gives a warning. Start the workflow again on `main` with
`workflow_dispatch`. A queued run that a newer run replaces ends as
`cancelled`. The release CI gate counts that as a failure, so do not merge a
skill change while release validation runs.

The pull request still needs its companion checks and a merge. After the
merge, run the failed `Public skills contract` job on `main` again. Do not push
your own commits to `sync-fallow-skills`, because the next run replaces
the branch.

The workflow is not a gate. The `Public skills contract` job stays the gate and
fails closed. So the workflow passes with a notice when no credential is set,
and `main` and the release gate do not turn red for a missing secret.

### Credential

`GITHUB_TOKEN` cannot write to another repository. The `publish` job uses the
first credential that is set:

| Name | Kind | Value |
|---|---|---|
| `SKILLS_SYNC_APP_CLIENT_ID` | Repository variable | Client ID of the GitHub App |
| `SKILLS_SYNC_APP_PRIVATE_KEY` | Repository Actions secret | Private key of that App |
| `SKILLS_SYNC_TOKEN` | Repository Actions secret | Fallback: a fine-grained personal access token |

The App needs these repository permissions, and an installation on
`fallow-rs/fallow-skills` only:

- Contents: read and write
- Pull requests: read and write
- Metadata: read (GitHub adds it)

The `publish` job asks `actions/create-github-app-token` for a token that is limited to
`fallow-skills` with these two permissions. The fine-grained token needs the
same scope: resource owner `fallow-rs`, the `fallow-skills` repository only,
and the same two permissions. Give it an expiry date and rotate it.

Credential boundaries:

- Both jobs have `contents: read` on this repository and no `id-token`.
- Every checkout uses `persist-credentials: false`.
- The companion checks run code of the companion repository. That code can
  change the runner, for example with `GITHUB_ENV` or git hooks. So the
  `prepare` job gets no token. It only tests whether a secret is set.
- The `publish` job runs on a fresh runner and runs no repository code. It
  calls only git, with hooks and fsmonitor turned off, and `gh`.
- The push sends the token in an HTTP header, not in the remote URL.
- Without signing keys on the runner, the commit is unsigned. The companion
  does not require signed commits. The commit carries the DCO sign-off of the
  App bot account, or of the token owner.

## Adding or moving knowledge

Before adding a document:

1. Choose the canonical owner from the table above.
2. Add the document to its local router and ownership manifest.
3. Link it from the smallest relevant task route.
4. Add or update an executable drift check when another surface consumes it.
5. Run the knowledge architecture and Trigger Tree gates.

Do not create a second authored copy to make another agent host convenient.
Generate a thin adapter or link the shared durable reference instead.
