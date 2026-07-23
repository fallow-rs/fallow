# trigger-tree integration

Fallow uses [trigger-tree](https://github.com/Hedde/trigger_tree) to measure
which maintainer documentation Codex and Claude Code discover. Telemetry stays
in this directory and is ignored by Git, except for this documentation and the
shared project configuration.

## Pinned source

Both clients use trigger-tree v1.23.1 from tag commit
`b4d839f681d8c038d46d6d1f0f1914838ada6162`.

Version 1.23.1 supports Python 3.10 through 3.14. The project status line
prefers `python3.13`, then falls back to `python3` and `python`.

Claude Code declares the tagged marketplace and enabled plugin in
`.claude/settings.json`.

Codex installs the official plugin source for the user from a local v1.23.1
tag snapshot at
`~/.codex/local-marketplaces/trigger-tree-v1.23.1-pinned`. Only the marketplace
entry is changed to resolve that local snapshot because Codex CLI 0.144.6
otherwise follows its floating `main` plugin source even when the marketplace
checkout is pinned. The user-wide `~/.trigger-tree/config.sh` sets
`TT_LOG_PROMPTS='off'`, so repositories without their own configuration store
prompt markers only. Configuration precedence is bundled default, user
default, then project override.

Codex reviews plugin hooks by hash. After installing or updating trigger-tree,
start Codex interactively and choose `Trust all and continue` for its four
hooks. Non-interactive sessions skip untrusted hooks without recording events.
Version 1.23.1 reports persisted Codex hook trust in `tt doctor`; an upgrade
that changes a hook still requires another interactive review.

Fallow overrides the user default with `TT_LOG_PROMPTS='hash'`. A hash still
allows prompts to be correlated and may be vulnerable to guessing when the
input space is small. Use `off` when that linkability is undesirable. Version
1.23.1 makes `tt doctor` report the effective mode and which configuration
layer selected it.

## Local data

Runtime files such as `history.jsonl`, rotated histories, session state,
reports, and badges are ignored. They are never required for a clean checkout
or CI.

Version 1.23.1 scans the Git-visible documentation set, includes `.agents/`
and `.codex/`, and records the originating client on new events. Existing
v1.19.1 events remain valid and appear with client `unknown`.

The static gate can also write a deterministic GitLab Code Quality artifact
with `tt gate --code-quality <path>`.

## Updating

Before upgrading:

1. Verify the new tag and resolved commit.
2. Review the upstream prompt default and privacy policy.
3. Review both hook manifests for new events or tool access.
4. Verify the local Codex marketplace still resolves the exact tag and the
   user-wide `off` default still applies before project overrides.
5. Review and trust the four updated Codex hooks interactively.
6. Run one real Codex session and one real Claude Code session in this
   checkout.
7. Confirm prompt probes remain absent from current and rotated histories.
8. Run trigger-tree doctor and the static gate.

Do not update either marketplace to a floating branch.

## Removal

Remove the client integrations:

```sh
codex plugin remove trigger-tree@trigger-tree-pinned
codex plugin marketplace remove trigger-tree-pinned
claude plugin uninstall trigger-tree@trigger-tree --scope project
claude plugin marketplace remove trigger-tree --scope project
```

The upstream trigger-tree uninstall command removes its Claude status line and
copied script, but intentionally preserves telemetry, project configuration,
and ignore rules. Delete `.trigger-tree/` separately only when its local
history is no longer wanted.
