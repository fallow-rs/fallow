# `hooks`: Managed Hook Status And Installation

```bash
fallow hooks status --format json
fallow hooks install --target git
fallow hooks install --target agent
fallow hooks uninstall --target git
fallow hooks uninstall --target agent
```

The Git pre-commit hook runs `fallow audit --staged`. It audits the staged changes against HEAD and reads each file from the index, so unstaged edits and untracked files do not change the verdict. Before the first commit of a repository, the hook skips the audit and prints one line on stderr. The hook runs the project's own fallow from `node_modules/.bin` first, then a `fallow` on `PATH`, Yarn Plug'n'Play and `npx --no-install`. It needs fallow 3.43.0 or later.

`hooks status` is read-only and reports `git`, `claude`, `codex` (the `AGENTS.md` routing block), and `codex_gate` (the `.codex/hooks.json` gate) surfaces. Each surface includes `installed`, `managed_block_present`, `user_edited`, and `path`; generated agent scripts also include `script_version` and `min_version_floor`. Use it before mutating setup so agents can distinguish fallow-managed artifacts from user-owned hooks or partial managed blocks.
