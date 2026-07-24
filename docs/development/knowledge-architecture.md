# Knowledge architecture

Fallow separates durable knowledge, repeatable workflows, runtime adapters, and
published user documentation. Each fact has one authored source.

## Ownership

| Surface | Canonical source | Consumers |
|---|---|---|
| Open-source maintainer knowledge | `fallow/docs/` | Humans, Codex, Claude |
| Maintainer workflows | `fallow/.agents/skills/` | Codex directly, generated Claude adapters |
| Claude runtime constraints | `fallow/.claude/rules/` | Claude, with durable facts routed to `docs/` |
| Public CLI, config, MCP, and output contracts | Public Fallow source and generated schemas | Public docs, skills, integrations, private consumers |
| Released Fallow skill contract | `fallow/npm/fallow/skills/fallow/` | npm package and portable skill packaging |
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
Protocol consumers pin the public crate in their lockfile and verify published
sidecar parity.

## Adding or moving knowledge

Before adding a document:

1. Choose the canonical owner from the table above.
2. Add the document to its local router and ownership manifest.
3. Link it from the smallest relevant task route.
4. Add or update an executable drift check when another surface consumes it.
5. Run the knowledge architecture and Trigger Tree gates.

Do not create a second authored copy to make another agent host convenient.
Generate a thin adapter or link the shared durable reference instead.
