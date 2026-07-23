# trigger-tree integration

Fallow uses [trigger-tree](https://github.com/Hedde/trigger_tree) to measure
which maintainer documentation Codex and Claude Code discover. Telemetry stays
in this directory and is ignored by Git, except for this documentation and the
shared project configuration.

## Pinned source

Both clients use trigger-tree v1.19.1 from tag commit
`f860090defde3d11a743bf559d304947f1f825af`.

The locally installed hook manifests on this Mac prefer `python3.13`, the
newest interpreter supported by v1.19.1. The project status line falls back to
`python3` and then `python` for other environments.

Claude Code declares the tagged marketplace and enabled plugin in
`.claude/settings.json`.

Codex currently installs plugins for the user rather than one project. Its
local marketplace lives at
`~/.codex/local-marketplaces/trigger-tree-v1.19.1-off`. That snapshot differs
from upstream only to keep installation deterministic, privacy-safe, and
compatible with this Mac:

- Both fallback `TT_LOG_PROMPTS` values are `off`.
- The marketplace resolves the plugin from the local tagged snapshot instead
  of the floating upstream `main` branch.
- The hook launcher prefers `python3.13` before the newer system `python3`.

Repositories without a project configuration therefore store prompt markers
only. Fallow overrides that fallback with `TT_LOG_PROMPTS='hash'`. A hash still
allows prompts to be correlated and may be vulnerable to guessing when the
input space is small. Use `off` when that linkability is undesirable.

## Local data

Runtime files such as `history.jsonl`, rotated histories, session state,
reports, and badges are ignored. They are never required for a clean checkout
or CI.

The long-lived maintainer checkout contains large ignored scratch trees.
trigger-tree v1.19.1 caps its filesystem inventory before applying watch
patterns, so doctor can report an optional low-coverage warning here even when
the configured tracked paths match. The integration verifies the watch, scan,
and always-loaded expressions with explicit positive and negative path cases.

## Updating

Before upgrading:

1. Verify the new tag and resolved commit.
2. Review the upstream prompt default and privacy policy.
3. Review both hook manifests for new events or tool access.
4. Reapply the Codex `off` fallback to a fresh tagged local marketplace.
5. If the system `python3` remains unsupported, reapply the `python3.13`
   preference to both locally installed hook manifests.
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
