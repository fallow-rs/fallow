# Tooling detection

Inspect the repository before you change it. Record what you find, because the later steps use it.

## Files to inspect

| Tool area | Signals |
|---|---|
| Package manager | `packageManager` in `package.json`; the lockfile: `pnpm-lock.yaml`, `yarn.lock`, `bun.lock` or `bun.lockb`, `package-lock.json` |
| Formatter | `.oxfmtrc.json`, `.prettierrc*`, `prettier.config.*`, `biome.json`, `biome.jsonc`, a `format` script in `package.json` |
| Linter | `.oxlintrc.json`, `eslint.config.*`, `.eslintrc*`, `biome.json`, a `lint` script in `package.json` |
| TypeScript | `tsconfig.json`, `tsconfig.*.json`, a `typecheck` script, `typescript` in `devDependencies` |
| CI | `.github/workflows/*.yml`, `.gitlab-ci.yml`, other CI config files |
| Existing analysis | `knip.json`, `knip.jsonc`, `.knip.json`, `.knip.jsonc`, a `knip` field in `package.json`; `.jscpd.json`; `.dependency-cruiser.*` |
| Existing Fallow | `fallow config --path` prints the config path, or exits 3 when there is no config |
| Agent harnesses | `CLAUDE.md`, `.claude/`, `.codex/`, `.cursor/` (`AGENTS.md` alone does not name a harness) |

`fallow doctor --format json --quiet` checks the project root, the config, the workspaces, and the installed dependencies. It changes nothing. Run it when the project layout is not clear.

## Responsibility split

Give each tool one job. Do not configure two tools for the same job.

| Job | Tool | Command |
|---|---|---|
| Formatting | Oxfmt | `oxfmt` |
| Local lint rules | Oxlint | `oxlint` |
| Type correctness | TypeScript | `tsc --noEmit` |
| Repository and system analysis | Fallow | `fallow`, `fallow audit` |

Repository and system analysis covers unused files, exports, types, and dependencies, duplication, complexity, circular dependencies, architecture boundaries, and changed-code risk.

For a new project, use the tools in this table. For an existing project, keep the current formatter, linter, and type check. Add Fallow for the system analysis.

## Parity check

Do this check before you remove a tool whose job overlaps with Fallow, for example Knip, jscpd, or dependency-cruiser. When a step fails, keep the tool and tell the user why.

1. Ask the user for approval to replace the tool.
2. Migrate the config when Fallow can read it: `fallow migrate --dry-run` for Knip, jscpd, and stylelint. When the project has no Fallow config, review the preview, then run `fallow migrate`. When a Fallow config exists, `fallow migrate` refuses to write. Merge the settings from the preview into that config by hand. Fallow cannot migrate a dependency-cruiser config. Write the rules again as `boundaries` in the Fallow config (`fallow config-schema` gives the format).
3. Run the old tool and Fallow on the same commit. Compare the findings by category.
4. Explain each finding that only one tool reports. A finding that Fallow does not report must have a reason, for example a framework entry point that Fallow detects.
5. Move each CI step and each `package.json` script of the old tool to Fallow.
6. Remove the old tool and its config in a separate commit, so that the user can revert it.
