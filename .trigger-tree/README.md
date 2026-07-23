# trigger-tree integration

Fallow uses [trigger-tree](https://github.com/Hedde/trigger_tree) to measure
which maintainer documentation Codex and Claude Code discover. Telemetry stays
in this directory and is ignored by Git, except for this documentation and the
shared project configuration.

## Pinned source

Both clients use trigger-tree v1.21.0 from tag commit
`84e478d550de752a9a8eabfa9a6323fa4257543c`.

Version 1.21.0 supports Python 3.10 through 3.14. The project status line
prefers `python3.13`, then falls back to `python3` and `python`.

Claude Code declares the tagged marketplace and enabled plugin in
`.claude/settings.json`.

Codex currently installs plugins for the user rather than one project. Its
local marketplace lives at
`~/.codex/local-marketplaces/trigger-tree-v1.21.0-off`. That snapshot differs
from upstream only to keep installation deterministic and privacy-safe:

- Both fallback `TT_LOG_PROMPTS` values are `off`.
- The marketplace resolves the plugin from the local tagged snapshot instead
  of the floating upstream `main` branch.

Codex reviews plugin hooks by hash. After installing or updating trigger-tree,
start Codex interactively and choose `Trust all and continue` for its four
hooks. Non-interactive sessions skip untrusted hooks without recording events.

Repositories without a project configuration therefore store prompt markers
only. Fallow overrides that fallback with `TT_LOG_PROMPTS='hash'`. A hash still
allows prompts to be correlated and may be vulnerable to guessing when the
input space is small. Use `off` when that linkability is undesirable.

## Local data

Runtime files such as `history.jsonl`, rotated histories, session state,
reports, and badges are ignored. They are never required for a clean checkout
or CI.

Version 1.21.0 scans the Git-visible documentation set, includes `.agents/`
and `.codex/`, and records the originating client on new events. Existing
v1.19.1 events remain valid and appear with client `unknown`.

## Updating

Before upgrading:

1. Verify the new tag and resolved commit.
2. Review the upstream prompt default and privacy policy.
3. Review both hook manifests for new events or tool access.
4. Reapply the Codex `off` fallback to a fresh tagged local marketplace.
5. Review and trust the four updated Codex hooks interactively.
6. Run one real Codex session and one real Claude Code session in this
   checkout.
7. Confirm prompt probes remain absent from current and rotated histories.
8. Run trigger-tree doctor and the static gate.

Do not update either marketplace to a floating branch.

## Removal

Remove the client integrations:

```sh
codex plugin remove trigger-tree@trigger-tree-private
codex plugin marketplace remove trigger-tree-private
claude plugin uninstall trigger-tree@trigger-tree --scope project
claude plugin marketplace remove trigger-tree --scope project
```

The upstream trigger-tree uninstall command removes its Claude status line and
copied script, but intentionally preserves telemetry, project configuration,
and ignore rules. Delete `.trigger-tree/` separately only when its local
history is no longer wanted.
