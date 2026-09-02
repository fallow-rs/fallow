# Fallow for Zed

Zed extension for [`fallow-lsp`](https://github.com/fallow-rs/fallow), the language server behind Fallow's editor diagnostics.

## What works

- diagnostics for unused files, exports, types, dependencies, enum/class members, unresolved imports, unlisted deps, duplicate exports, circular dependencies, and duplication
- hover information
- quick-fix code actions
- code lens where Zed surfaces them

This extension is intentionally thin. It launches the existing `fallow-lsp` binary instead of re-implementing analysis logic inside the editor.

## Binary resolution

The extension looks for `fallow-lsp` in this order:

1. `lsp.fallow.binary.path`
2. local `node_modules/.bin/fallow-lsp` in the current worktree
3. `fallow-lsp` on `PATH`
4. a managed binary downloaded from the latest GitHub release and verified against Fallow's Ed25519 signing key

If you already install Fallow through npm or a package manager, you usually do not need to configure anything.

## Settings

If you customize `language_servers` for a language, keep `fallow` or `...` in the list so the extension still runs:

```json
{
  "languages": {
    "TypeScript": {
      "language_servers": ["fallow", "..."]
    },
    "JavaScript": {
      "language_servers": ["fallow", "..."]
    }
  }
}
```

To point Zed at a specific binary:

```json
{
  "lsp": {
    "fallow": {
      "binary": {
        "path": "/absolute/path/to/fallow-lsp",
        "arguments": []
      }
    }
  }
}
```

Fallow currently reads issue toggles from LSP initialization options:

```json
{
  "lsp": {
    "fallow": {
      "initialization_options": {
        "issueTypes": {
          "unused-files": true,
          "unused-exports": true,
          "unused-types": true,
          "unused-dependencies": true,
          "unused-dev-dependencies": true,
          "unused-optional-dependencies": true,
          "unused-enum-members": true,
          "unused-class-members": true,
          "unresolved-imports": true,
          "unlisted-dependencies": true,
          "duplicate-exports": true,
          "type-only-dependencies": true,
          "circular-dependencies": true,
          "stale-suppressions": true
        }
      }
    }
  }
}
```

`issueTypes` uses Fallow config keys, which are plural for many rules. To give
the whole team the same quieter editor baseline, commit exact LSP diagnostic
codes in `.zed/settings.json` instead:

```json
{
  "lsp": {
    "fallow": {
      "initialization_options": {
        "mutedCategories": [
          "unused-file",
          "unused-export",
          "code-duplication",
          "stale-suppression"
        ]
      }
    }
  }
}
```

Diagnostic codes are singular where shown above. The security codes are
exactly `security-sink` and `security-client-server-leak`. A mute affects only
editor diagnostics; CLI, audit, and CI output remain unchanged. After changing
an initialization option, run `editor: restart language server` from Zed's
command palette.

### Inline complexity

Inline complexity is off by default in the editor-agnostic LSP. Enable its Code
Lens in the same initialization options:

```json
{
  "code_lens": "on",
  "lsp": {
    "fallow": {
      "initialization_options": {
        "health": {
          "inlineComplexity": true
        }
      }
    }
  }
}
```

This adds a compact complexity lens above functions that exceed Fallow's
thresholds. Use `"code_lens": "menu"` instead if you prefer to reveal lenses
from the editor menu. It does not run or render the complete Health report.

### Security candidate diagnostics

Security candidates are deliberately opt-in. Enable either or both project
rules in `.fallowrc.json`:

```json
{
  "rules": {
    "security-sink": "warn",
    "security-client-server-leak": "warn"
  }
}
```

The LSP reuses this project configuration and publishes matching candidates as
information diagnostics. Setting the same keys to `true` under `issueTypes`
does not enable rules that remain `off` in the Fallow config. Candidates are
items to verify, not confirmed vulnerabilities.

## Full Health and Security reports

The Zed extension provides the shared LSP surface. For project-wide Health and
Security reports, install the `fallow` CLI separately and run it from the
project root, for example in Zed's terminal:

```bash
npm install --save-dev fallow
npx fallow health
npx fallow security --fail-on-issues
```

For these commands, exit code `0` is a successful clean run, `1` is a
successful run with findings, and `2` is a configuration, input, or execution
error. Without `--fail-on-issues`, `fallow security` remains advisory and can
exit `0` with candidates.

The current Zed extension API does not provide contribution points for a
Fallow-owned sidebar tree or status-bar item. Use Zed's built-in Problems and
LSP surfaces for diagnostics, and the CLI for the complete reports. This is the
best available parity with the current API, not full VS Code UI parity.

## Development

1. Open Zed.
2. Run `zed: install dev extension`.
3. Select `editors/zed`.
4. Open a TypeScript or JavaScript project and confirm `fallow` is running in the language server UI.

If Zed opens the project in Restricted Mode, trust the worktree first. Restricted Mode blocks language servers entirely.

To preflight the actual packaged extension artifact locally, install the target once with `rustup target add wasm32-wasip2` and run `cargo build --target wasm32-wasip2 --manifest-path editors/zed/Cargo.toml`.

## Linux note

Zed's extension API exposes OS and CPU architecture, but not glibc vs musl. The managed download therefore uses the GNU Linux release asset. On musl/Nix-style setups, prefer `PATH` or `lsp.fallow.binary.path`.
