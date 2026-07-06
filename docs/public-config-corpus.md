# Public Fallow Config Corpus

This maintainer workflow turns public Fallow config files into repeatable evidence for recurring false-positive workarounds. It uses only public GitHub repositories that already publish `.fallowrc.json`, `.fallowrc.jsonc`, or `fallow.toml`.

The corpus is not telemetry. It does not read private repositories, upload data, or prove that every comment is a confirmed Fallow bug. Comment matches are candidate evidence for maintainers to inspect.

## How To Run

Prerequisites:

- `gh` authenticated with a normal GitHub token that can use code search.
- Network access to `raw.githubusercontent.com`.
- Python 3 with the standard library.

Run a capped live pass:

```bash
python3 scripts/public-config-corpus.py \
  --limit 40 \
  --search-timeout 30 \
  --timeout 15 \
  --output docs/public-config-corpus.md
```

The default cache lives under `.fallow/public-config-corpus/`, which is intentionally untracked. The manifest is written to `.fallow/public-config-corpus/manifest.json` unless `--manifest` is provided.

For reviewable local testing without GitHub access:

```bash
scripts/test-public-config-corpus.sh
```

The offline fixture mode reads fake `gh search code --json` output and cached config bodies from `scripts/fixtures/public-config-corpus/`.

## Public Smoke Conformance

Use the public smoke lane when analyzer, framework, output-contract, or release
work needs real-project evidence without running the full scheduled
Fallow-vs-Knip conformance suite.

```bash
npm run conformance:public-smoke
```

The default command performs no network access. It writes compact skip/pass/fail
summaries under `target/public-smoke-conformance/`, which is ignored by git. To
run against local clones, pass one or more project paths:

```bash
npm run conformance:public-smoke -- --project next=/path/to/next.js --project vite=/path/to/vite
```

For release-readiness evidence, opt in to network fetches explicitly:

```bash
npm run conformance:public-smoke -- --clone
```

The pinned smoke set lives in `scripts/public-smoke-projects.json` and covers:

| Category | Project |
| --- | --- |
| Next.js shape | `vercel/next.js` |
| Vue/Nuxt shape | `vuejs/core` |
| SvelteKit shape | `sveltejs/svelte` |
| Workspace-heavy shape | `TanStack/query` |
| Catalog or override-heavy shape | `vitejs/vite` |

Artifacts:

- `target/public-smoke-conformance/public-smoke-summary.json`
- `target/public-smoke-conformance/public-smoke-summary.md`

Artifacts intentionally contain compact counts, command names, public repo refs,
and project ids only. They do not store raw Fallow JSON, source snippets, or
absolute project paths.

## Reproducibility

Each manifest entry records:

- repository, config path, blob URL, raw URL, and query
- API-provided blob SHA when available, plus validated URL ref fallback
- local cache path
- detected format and parse status
- byte count and `sha256`
- fetch time, per-filename cap, and `gh` version
- fetch error when a config could not be downloaded

Partial fetches are visible in the report instead of silently shrinking the corpus. Re-run partial reports before filing issues unless the failure itself is useful search-noise evidence.

## Privacy And License Boundaries

Use this workflow only for public repositories. Do not run it with private repository search results or copy private config files into fixtures.

Do not publish private project output in public release notes, README examples,
or GitHub issues. When public-smoke output is useful as release evidence, cite
the public repository id, pinned ref, command, and summary category only.

When turning a finding into an issue:

- link to the public source URL
- quote only short config/comment snippets
- describe comments as candidate evidence until confirmed against a reproduction
- avoid pasting large third-party config files into GitHub issues

## How To Triage The Report

Start with the review queue:

1. Look for repeated use of `entry`, `dynamicallyLoaded`, `ignorePatterns`, `ignoreDependencies`, `ignoreExports`, `usedClassMembers`, and rule downgrades.
2. Inspect candidate workaround comments for phrases such as `false positive`, `workaround`, `fallow misses`, `loaded by`, `framework`, `generated`, `runtime`, `plugin`, `dynamic`, `entrypoint`, `keep`, and `manual`.
3. Check parse failures separately. Some are unsupported syntax, but some are search noise or invalid historical configs.
4. File a focused issue only after confirming the workaround maps to a current Fallow false positive or missing plugin convention.

## Current Seed Report

The 2026-05-22 manual pass over 100 public configs found:

| Signal | Configs |
|---|---:|
| `entry` | 72 |
| `ignorePatterns` | 59 |
| `ignoreDependencies` | 57 |
| `rules` | 48 |
| `audit` or baseline config | 18 |

Known issue family from that pass:

- [#546](https://github.com/fallow-rs/fallow/issues/546): Storybook `staticDirs` assets and manager-runtime imports.
- [#586](https://github.com/fallow-rs/fallow/issues/586): Playwright fixture class-member propagation.
- [#588](https://github.com/fallow-rs/fallow/issues/588), [#589](https://github.com/fallow-rs/fallow/issues/589), [#590](https://github.com/fallow-rs/fallow/issues/590): rwsdk, Wrangler, Node loader, and content-collections convention-loaded files.
- [#600](https://github.com/fallow-rs/fallow/issues/600): Electron-Vite renderer HTML entries.
- [#601](https://github.com/fallow-rs/fallow/issues/601), [#602](https://github.com/fallow-rs/fallow/issues/602): Vitest alias and mock-module consumers.

## Follow-Up Ideas

- Clone a tiny pinned subset and run the local Fallow binary to compare findings against config comments and baselines.
- Add JSON output if the corpus becomes a trendable quality signal.
- Consider a scheduled workflow only after the manual process proves useful and rate-limit behavior is well understood.
