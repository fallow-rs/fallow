window.BENCHMARK_DATA = {
  "lastUpdate": 1788179469698,
  "repoUrl": "https://github.com/fallow-rs/fallow",
  "entries": {
    "Fallow Conformance": [
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "f92318a75a2aee0732382d8069d8366931f01572",
          "message": "fix(tanstack): suppress Start virtual modules\n\nTanStack Start imports manifest and injected-head script modules through framework-provided virtual specifiers with a :v suffix. Those specifiers are not npm packages, but the TanStack plugin did not register them as virtual modules, so dead-code analysis reported them as unlisted dependencies.\n\nRegister the colon-suffixed TanStack Start virtual module prefixes through the existing plugin virtual-prefix hook. Add plugin-gated positive and negative coverage, including unresolved-import suppression and end-to-end analysis fixtures for static and dynamic imports.\n\nFixes #636.",
          "timestamp": "2026-05-23T07:33:06Z",
          "url": "https://github.com/fallow-rs/fallow/commit/f92318a75a2aee0732382d8069d8366931f01572"
        },
        "date": 1779524115672,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 670,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 32013,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 3.6,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.3,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.7,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "e32cc0a415dcfddc776e9ec032eed6de218e0135",
          "message": "chore(napi): sync package.json / package-lock / index.js to v2.80.0",
          "timestamp": "2026-05-24T08:10:02Z",
          "url": "https://github.com/fallow-rs/fallow/commit/e32cc0a415dcfddc776e9ec032eed6de218e0135"
        },
        "date": 1779611173764,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 670,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 32010,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 3.6,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.3,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.7,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "e32cc0a415dcfddc776e9ec032eed6de218e0135",
          "message": "chore(napi): sync package.json / package-lock / index.js to v2.80.0",
          "timestamp": "2026-05-24T08:10:02Z",
          "url": "https://github.com/fallow-rs/fallow/commit/e32cc0a415dcfddc776e9ec032eed6de218e0135"
        },
        "date": 1779703115223,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 670,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 32010,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 3.6,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.3,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.7,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "4f150680c3140e2ded8314447cfaefdcee310149",
          "message": "fix: handle Deno jsr/npm/url imports and Supabase Edge Function roots (#690)\n\nTreat jsr: and URL specifiers as external runtime imports and normalize\nnpm:<pkg>@version to its npm package so Deno/Supabase Edge Function imports\nno longer report as unresolved-import or bogus unlisted dependencies. A\npackage imported only via npm: is self-declaring and not reported as\nunlisted (mirrors the per-file bun: carve-out). Add a built-in supabase\nplugin that marks supabase/functions/*/index.* as runtime entry roots and\ncredits the supabase CLI as tooling; _shared code stays reachable via\nrelative imports.\n\nCloses #624",
          "timestamp": "2026-05-26T09:12:40Z",
          "url": "https://github.com/fallow-rs/fallow/commit/4f150680c3140e2ded8314447cfaefdcee310149"
        },
        "date": 1779788764618,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 670,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 32003,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 3.6,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.6,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.6,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "8a1a699fa7482dc2d15427e006ff74cc19417bd5",
          "message": "feat: ignore unresolved imports by specifier\n\nCloses #726",
          "timestamp": "2026-05-27T09:21:33Z",
          "url": "https://github.com/fallow-rs/fallow/commit/8a1a699fa7482dc2d15427e006ff74cc19417bd5"
        },
        "date": 1779874749647,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2.1,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 699,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 32024,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 3.6,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 7.1,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.2,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "bcd212c555565601468535fb59a364a4f5bec638",
          "message": "chore(napi): sync package.json / package-lock / index.js to v2.83.0",
          "timestamp": "2026-05-27T15:00:27Z",
          "url": "https://github.com/fallow-rs/fallow/commit/bcd212c555565601468535fb59a364a4f5bec638"
        },
        "date": 1779962172778,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2.1,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 699,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 32024,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 3.6,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 7.1,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.2,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "49b820fde252762f4fb4b74a2acee76f9cdea575",
          "message": "refactor(core,lsp): unify plugin-result merging via field-exhaustive merge_into (#776)\n\nReplace the two hand-maintained field-by-field merge sites with merge_into\nmethods that exhaustively destructure their own struct, so adding a field\nbecomes a compile error in the merge logic instead of a silent divergence\nbetween the CLI and the LSP.\n\n- AggregatedPluginResult::merge_into + apply_workspace_prefix (core): the\n  workspace merge loop in run_plugins now prefixes each workspace result then\n  folds it in via the single union method. Pre-refactor behavior preserved\n  exactly: workspace config_patterns / used_class_members / scss_include_paths\n  (populated by run_workspace_fast but never merged) stay dropped, and\n  script_used_packages (never populated there) is cleared too so a future\n  change cannot silently alter root script-credit. Whether the populated-field\n  drops are latent bugs is tracked in #772.\n- AnalysisResults::merge_into (types): the LSP merge_results becomes a thin\n  wrapper delegating to it.\n- merge_test_source_with_all_fields drops ..Default::default() so the test\n  fixture is also a compile-time field-coverage gate.\n- Re-export FeatureFlag / FlagKind / FlagConfidence from fallow_core::results\n  so the feature_flags element type is nameable by consumers.\n\nPure refactor: no change to merged outputs (all benchmark fixtures\nbyte-identical OLD vs NEW).\n\nCloses #444.",
          "timestamp": "2026-05-29T09:14:41Z",
          "url": "https://github.com/fallow-rs/fallow/commit/49b820fde252762f4fb4b74a2acee76f9cdea575"
        },
        "date": 1780047958846,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 667,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 31757,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 3.6,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.2,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "005462b33c2696e8a810721f3fdd4a92495498d0",
          "message": "fix(plugins): credit oxlint CLI tooling packages (oxlint-tsgolint) as used (#802)\n\n* fix(plugins): credit oxlint CLI tooling packages (oxlint-tsgolint) as used\n\noxlint-tsgolint is the type-aware companion package the oxlint binary loads\nat runtime (via --type-aware / options.typeAware). It is never imported in\nsource nor listed in an .oxlintrc.json jsPlugins array, so the #607 jsPlugins\ncredit does not cover it. When declared in prod dependencies (where the general\ntooling-prefix credit does not apply, that only covers devDependencies), it was\nfalsely reported as unused.\n\nAdd oxlint-tsgolint to the oxlint plugin's tooling_dependencies, which is honored\nfor both prod and dev categories and is gated on the oxlint plugin being active.\nExact-name credit, not an oxlint- prefix, so an unrelated oxlint-prefixed prod\ndependency still reports.\n\nFixes #753\n\n* docs(plugins): document oxlint CLI tooling credit (oxlint-tsgolint)\n\ndetection.md + plugins.md note the exact-name oxlint-tsgolint tooling credit,\nCHANGELOG [Unreleased] gets the user-facing entry, and the agent-file baseline\nis re-blessed for the two edited rule files.\n\nRefs #753",
          "timestamp": "2026-05-30T05:37:34Z",
          "url": "https://github.com/fallow-rs/fallow/commit/005462b33c2696e8a810721f3fdd4a92495498d0"
        },
        "date": 1780129464783,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 668,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 31758,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 3.6,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.2,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "cab4ab3dacdefa41a43f2172801f189ea42b5df4",
          "message": "chore(napi): sync package.json / package-lock / index.js to v2.85.0 (#816)\n\n* chore(napi): sync package.json / package-lock / index.js to v2.85.0\n\n* docs(telemetry): bump payload example version to 2.85.0",
          "timestamp": "2026-05-30T22:04:25Z",
          "url": "https://github.com/fallow-rs/fallow/commit/cab4ab3dacdefa41a43f2172801f189ea42b5df4"
        },
        "date": 1780217016857,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 668,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 31758,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 3.6,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.2,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "b54c3be7ea7af3c6dd49932de28d8b35941e744d",
          "message": "test(schema): allow schema drift expects\n\nThe schema-emit CI path runs clippy against the test build of fallow-schema-emit with the schema-emit feature enabled. Its drift tests intentionally use expect calls for invariant checks over the committed schema.\n\nAdd the same test-only unwrap and expect allowance used by other test entry points so production schema generation remains covered while CI can compile the drift checks under the workspace lint ratchet.",
          "timestamp": "2026-06-01T11:04:43Z",
          "url": "https://github.com/fallow-rs/fallow/commit/b54c3be7ea7af3c6dd49932de28d8b35941e744d"
        },
        "date": 1780312718235,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 668,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 31758,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 3.6,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.2,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "2df7aaebcb8dba125545bcd933c793bba37c40fe",
          "message": "feat(security): data-driven tainted-sink candidate catalogue\n\nAdd a deterministic, opt-in local security-candidate layer surfaced only by\n`fallow security`. Findings are CANDIDATES for downstream agent verification\n(the DeepSec / Warden model), never under bare `fallow` or the audit gate.\n\nA shape-agnostic ModuleInfo.security_sinks extract capture feeds a data-driven\nmatcher catalogue (crates/core/data/security_matchers.toml + catalogue.rs), so\nadding a CWE category is a TOML row with no Rust enum churn. One generic\nSecurityFindingKind::TaintedSink carries category + cwe; IssueKind::SecuritySink\nand a default-off security_sink rule gate it. Nine seed categories ship, each\nwith positive and literal-negative integration fixtures: dangerous-html (CWE-79),\ncommand-injection (78, provenance-gated), code-injection (94, eval + vm),\nsql-injection (89), ssrf (918), path-traversal (22), open-redirect (601),\nweak-crypto (327), unsafe-deserialization (502). The bespoke graph-structural\nclient-server-leak class is unchanged.\n\nConservative non-literal-argument trigger (literal args never fire); blind spots\ncounted in-band via unresolved_callee_sites. Human / JSON / SARIF output carry\ncategory + cwe. ADR-021 non-goals (SCA, CVE/advisory feeds, auth-logic finding)\nstay out of scope.",
          "timestamp": "2026-06-01T21:20:14Z",
          "url": "https://github.com/fallow-rs/fallow/commit/2df7aaebcb8dba125545bcd933c793bba37c40fe"
        },
        "date": 1780395325192,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 630,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30501,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 3.7,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.9,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.3,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "5a6884f0ae6526884aff0b17024a3786b44fe399",
          "message": "fix(extract): credit structural class member calls\n\nTrack local typed function calls that receive concrete class instances and credit only the class members read through the matching typed parameter. The extractor keeps this scoped to local callees, exact argument positions, direct constructor arguments, and constructor-bound identifiers.\n\nRespect block-scoped shadows of the typed parameter name so unrelated local objects do not credit the concrete class argument. The change adds extractor and core regressions, updates detection notes, bumps the extraction cache for the new member-access semantics, and aligns the CODEOWNERS smoke test with the current scoped owner file.\n\nFixes #910.",
          "timestamp": "2026-06-03T09:25:35Z",
          "url": "https://github.com/fallow-rs/fallow/commit/5a6884f0ae6526884aff0b17024a3786b44fe399"
        },
        "date": 1780484045010,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 630,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30499,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 3.7,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.9,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.3,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "e0c6e2171bba529b632a2e7f54e52513a951a796",
          "message": "chore(napi): sync package.json / package-lock / index.js to v2.88.3",
          "timestamp": "2026-06-04T09:46:20Z",
          "url": "https://github.com/fallow-rs/fallow/commit/e0c6e2171bba529b632a2e7f54e52513a951a796"
        },
        "date": 1780566759486,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 630,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30402,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.9,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.3,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "76ca098b6ec76019f7a1039d2255f82a026a1c39",
          "message": "fix(tanstack): skip route contract duplicate exports\n\nTanStack Router route modules are allowed to repeat framework contract exports such as Route. The duplicate-export detector now applies TanStack route used-export rules to duplicate grouping, and also recognizes Route exports imported by generated routeTree.gen files from nested route directories.\n\nOrdinary duplicate Route exports outside TanStack route files still report. Regression coverage includes the OpenWaggle-shaped generated route tree plus configured route directories, lazy routes, ignore prefixes, and virtual route config.\n\nFixes #947.",
          "timestamp": "2026-06-05T09:39:40Z",
          "url": "https://github.com/fallow-rs/fallow/commit/76ca098b6ec76019f7a1039d2255f82a026a1c39"
        },
        "date": 1780652524759,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 2,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 630,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30402,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.9,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.3,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "65e53f7f3a21226a1a36f2289d3c2be080b5037f",
          "message": "chore(napi): sync package.json / package-lock / index.js to v2.89.0",
          "timestamp": "2026-06-05T17:03:16Z",
          "url": "https://github.com/fallow-rs/fallow/commit/65e53f7f3a21226a1a36f2289d3c2be080b5037f"
        },
        "date": 1780734812508,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30362,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.9,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.3,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "08912ff4b864e56e5e2f1439292338c12feb3207",
          "message": "fix(security): flag source-backed redos regex sinks\n\nCapture risky literal regex applications as source-backed security sink candidates. The extractor records the risky regex fragment for literal regexes and constant RegExp strings, then the existing catalogue emits redos-regex CWE-1333 findings only when the input traces to an untrusted source.\n\nSafe literal patterns, mutable regex bindings, and source-free inputs stay quiet. The extraction cache version is bumped because security_sinks now carries the optional regex fragment metadata.\n\nFixes #928.",
          "timestamp": "2026-06-07T08:38:38Z",
          "url": "https://github.com/fallow-rs/fallow/commit/08912ff4b864e56e5e2f1439292338c12feb3207"
        },
        "date": 1780822470344,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30363,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.9,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.3,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "b6abad014ce375ed88a80cb57b0479dea588fa41",
          "message": "fix(security): keep source reachability diff matches\n\nShared diff filtering already retained security candidates when the sink anchor or detector trace matched the changed lines. Source reachability added a second trace under reachability, but that trace was not included in the diff predicate, so diff-scoped runs could hide a candidate introduced by an untrusted-source path change.\n\nInclude reachability.untrusted_source_trace in the shared diff filter, add regression coverage for that path, and update nearby comments so the ranking and SARIF docs describe the current source-reachability contract.\n\nFollow-up to #1050.",
          "timestamp": "2026-06-08T10:03:23Z",
          "url": "https://github.com/fallow-rs/fallow/commit/b6abad014ce375ed88a80cb57b0479dea588fa41"
        },
        "date": 1780914470780,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30363,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.9,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.3,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "b5868dd2d6ef5cd8f3c7f025bcc125f203e303d4",
          "message": "fix(vscode): open dynamic route paths\n\nRoute VS Code sidebar tree opens through an internal `fallow.openFile` command so bracketed filesystem paths remain decoded until the extension host calls `Uri.file(...)`.\n\nApply the same open-path handling across unused-code, duplicates, health, security, and coverage tree items, with unit coverage for decoded Next.js dynamic route paths and the command handler.\n\nFixes #1071.",
          "timestamp": "2026-06-09T09:04:17Z",
          "url": "https://github.com/fallow-rs/fallow/commit/b5868dd2d6ef5cd8f3c7f025bcc125f203e303d4"
        },
        "date": 1780997670323,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30363,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.9,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.3,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "c22eb0680279b8c96a9f15189d46ca1929600c40",
          "message": "fix: apply boundary coverage rules consistently\n\nApply per-file `boundary-violation` overrides to both import boundary findings and boundary coverage findings.\n\nRender boundary coverage in human output when it is the only structure finding, and keep embedded config-action paths relative in JSON output.",
          "timestamp": "2026-06-10T09:13:32Z",
          "url": "https://github.com/fallow-rs/fallow/commit/c22eb0680279b8c96a9f15189d46ca1929600c40"
        },
        "date": 1781085046461,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30363,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.9,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.3,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "8d811649ba1750b819d43474a374fe81cb8a447e",
          "message": "chore(napi): sync package.json / package-lock / index.js to v2.94.0",
          "timestamp": "2026-06-12T00:48:45Z",
          "url": "https://github.com/fallow-rs/fallow/commit/8d811649ba1750b819d43474a374fe81cb8a447e"
        },
        "date": 1781258808404,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30361,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.9,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.3,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "4bbacc94af59a4e5b20523d7422bf21cd5f4696b",
          "message": "feat(nextjs): flag server-only exports in \"use client\" files (#1229)\n\nFirst stage of the Next.js RSC differentiated-detection program (panel-approved).\n\n**C.1 `invalid-client-export` (new issue type, default warn):** a file carrying the `\"use client\"` directive that also exports a Next.js server-only or route-segment-config name (`metadata`, `generateMetadata`, `generateStaticParams`, `getServerSideProps`, route HTTP methods, `revalidate`, `dynamic`, ...). Next.js throws this at build time; fallow catches it statically in the same pass as the rest of dead-code analysis. The client component's `default` export is never flagged, and the rule only runs when `next` is a declared dependency (FP gate). Reported across human, JSON, SARIF, CodeClimate, compact, and markdown plus the LSP; suppressible via `// fallow-ignore-next-line invalid-client-export`; participates in audit introduction attribution and baselines.\n\n**E (capability headline):** integration coverage proving fallow reports route-internal unused exports (a stray helper export or a typo'd `metadata`) inside `app/page.tsx` where knip cannot, because fallow credits a precise per-route-file export allowlist rather than treating the whole route file as an opaque entry point.\n\nTeam review: rust, cli-output, json-output, ci-formats, lsp, github-action reviewers; one BLOCK (audit-attribution annotation) and two CONCERNs (human footer/suppress hint, jq tests) all resolved with regression tests. Full workspace test, clippy, fmt, doc, VS Code codegen, and jq suites green.",
          "timestamp": "2026-06-13T08:39:15Z",
          "url": "https://github.com/fallow-rs/fallow/commit/4bbacc94af59a4e5b20523d7422bf21cd5f4696b"
        },
        "date": 1781341002705,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30361,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.9,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.3,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "f9b6926884113b997f08366df1474c236d81b46d",
          "message": "fix(extract): credit Vue components rendered after a nested template slot (#1247)\n\nThe Vue SFC template-usage scanner matched the root template against the first </template> (non-greedy capture), truncating the body at a nested <template #slot> close and dropping every component rendered after it, causing false unused-export findings.\n\nThe scanner now locates the root close with nesting depth tracking, byte-safe (CJK), with an unclosed-comment fall-through. Verified on a real corpus: vue-vben-admin layout-ui went from 4 false unused-exports to 0, no new FPs. CACHE_VERSION 156 to 157.",
          "timestamp": "2026-06-14T08:16:14Z",
          "url": "https://github.com/fallow-rs/fallow/commit/f9b6926884113b997f08366df1474c236d81b46d"
        },
        "date": 1781429370580,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30362,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.9,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.3,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "8593f955ae31647444ec6f6f679571339cefafec",
          "message": "fix(dupes): ignore module wiring in imports filter\n\nExtend the existing ignoreImports duplicate filter beyond ES imports so re-export barrels and top-level static CommonJS require binding declarations no longer create clone groups by default.\n\nThe tokenizer now skips source-backed re-exports and whole top-level require-binding declarations while preserving runtime code, local exports, side-effect require calls, nested require calls, dynamic require arguments, and mixed declarations. The duplicate token cache version is bumped so warm caches do not reuse the old token stream.\n\nConfig schema, CLI help, changelog, agent rules, and shipped skill reference wording now describe the broader module-wiring scope.\n\nFixes #1225.",
          "timestamp": "2026-06-15T11:06:02Z",
          "url": "https://github.com/fallow-rs/fallow/commit/8593f955ae31647444ec6f6f679571339cefafec"
        },
        "date": 1781524571634,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30208,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2028,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.9,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 24.3,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "c541c92cf270988b4b6dd7b75d03c2da812ac335",
          "message": "refactor: reduce sig unit-size hotspots\n\nSplit large Rust units across CLI audit, health, reporting, LSP, MCP, config, extract, and core analysis paths into focused helpers.\n\nThis is a maintainability-only refactor. Output contracts, schemas, and user-facing behavior stay stable while the SIG unit-size pressure drops across the branch.",
          "timestamp": "2026-06-16T10:40:25Z",
          "url": "https://github.com/fallow-rs/fallow/commit/c541c92cf270988b4b6dd7b75d03c2da812ac335"
        },
        "date": 1781607970913,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30233,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2035,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "9fb44aac4684f23967b73dcaaa30ca8598e2a4f1",
          "message": "chore(napi): sync package.json / package-lock / index.js to v2.98.0",
          "timestamp": "2026-06-17T10:30:55Z",
          "url": "https://github.com/fallow-rs/fallow/commit/9fb44aac4684f23967b73dcaaa30ca8598e2a4f1"
        },
        "date": 1781692686089,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30232,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2035,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "906d0beef87ce240c6558844d71070cad03d938a",
          "message": "perf(cli): stream SARIF file output\n\nWrite SARIF files through a buffered JSON writer instead of first building a pretty JSON string in memory. This keeps the existing parent directory handling and warning behavior while avoiding the extra allocation for file output.\n\nFlush the writer explicitly so late IO errors are still reported before the success message.",
          "timestamp": "2026-06-18T09:44:58Z",
          "url": "https://github.com/fallow-rs/fallow/commit/906d0beef87ce240c6558844d71070cad03d938a"
        },
        "date": 1781777896630,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 586,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30223,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2044,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.6,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "1779391d89029381f0ca8130c64fc37b2b6009ab",
          "message": "test(audit): pin LF in shifted-duplicate test for Windows",
          "timestamp": "2026-06-19T10:21:05Z",
          "url": "https://github.com/fallow-rs/fallow/commit/1779391d89029381f0ca8130c64fc37b2b6009ab"
        },
        "date": 1781864577752,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30232,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2044,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "ba042b08ddcc69dedc3c4ad80b973c9fd58e14bb",
          "message": "refactor: split thin wrapper classification",
          "timestamp": "2026-06-20T08:51:05Z",
          "url": "https://github.com/fallow-rs/fallow/commit/ba042b08ddcc69dedc3c4ad80b973c9fd58e14bb"
        },
        "date": 1781945576493,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30232,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2044,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "16ef25b141f81fc45db5809eee3a6abd725a9a16",
          "message": "chore(napi): sync package.json / package-lock / index.js to v2.101.0",
          "timestamp": "2026-06-20T22:56:22Z",
          "url": "https://github.com/fallow-rs/fallow/commit/16ef25b141f81fc45db5809eee3a6abd725a9a16"
        },
        "date": 1782035292315,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30232,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2044,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "d65589eac55a4823d0f9cbf98990200f7c200e30",
          "message": "fix(audit): make non-reusable base-worktree paths unique per call\n\nBaseWorktree::create built the non-reusable worktree path from pid plus a\nwall-clock nanos read. nanos is not monotonic and repeats across threads, so two\naudit runs in one process (parallel unit tests, or a future in-process batch)\ncould mint the same temp path and race on `git worktree add`; the loser failed\nand the audit aborted with a generic exit 2. It surfaced as a flaky\naudit::tests::audit_dupes_falls_back_to_own_discovery_when_health_off (and audit\nsiblings) under parallel test runs, exposed once the Windows schema stack\noverflow stopped aborting the suite before those tests ran.\n\nAppend a process-global monotonic counter so every path is distinct regardless\nof clock resolution; the pid stays the first segment so orphan-sweep parsing is\nunchanged. Adds deterministic uniqueness and pid-parse regression tests.",
          "timestamp": "2026-06-22T10:52:17Z",
          "url": "https://github.com/fallow-rs/fallow/commit/d65589eac55a4823d0f9cbf98990200f7c200e30"
        },
        "date": 1782128868130,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30232,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2044,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "b7691eeaa076d322f01e5a30cafc87b6b9b3b2b5",
          "message": "chore(typos): allowlist the review app's fre prefix\n\nThe review app namespaces its persisted UI state and QA env under a `fre` (fallow review) prefix. typos read these as a misspelling of \"free\" and failed tree-wide, reddening the Typos CI job. Allowlist `fre`/`FRE` as intentional.",
          "timestamp": "2026-06-23T09:03:36Z",
          "url": "https://github.com/fallow-rs/fallow/commit/b7691eeaa076d322f01e5a30cafc87b6b9b3b2b5"
        },
        "date": 1782205647239,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30232,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2036,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "dependabot[bot]",
            "username": "dependabot[bot]",
            "email": "49699333+dependabot[bot]@users.noreply.github.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "27d8a5402aededded27235521e4ad04400b59585",
          "message": "chore(deps): bump pnpm/action-setup from 6.0.8 to 6.0.9 (#1379)\n\nBumps [pnpm/action-setup](https://github.com/pnpm/action-setup) from 6.0.8 to 6.0.9.\n- [Release notes](https://github.com/pnpm/action-setup/releases)\n- [Commits](https://github.com/pnpm/action-setup/compare/0e279bb959325dab635dd2c09392533439d90093...0ebf47130e4866e96fce0953f49152a61190b271)\n\n---\nupdated-dependencies:\n- dependency-name: pnpm/action-setup\n  dependency-version: 6.0.9\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>\nCo-authored-by: dependabot[bot] <49699333+dependabot[bot]@users.noreply.github.com>",
          "timestamp": "2026-06-24T07:59:10Z",
          "url": "https://github.com/fallow-rs/fallow/commit/27d8a5402aededded27235521e4ad04400b59585"
        },
        "date": 1782291912076,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30232,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2036,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "7d80978332b8dba841a4ace0abc8a375b4b19df3",
          "message": "feat(coverage): hint source-map upload command when cloud coverage is unresolved (#1431)\n\nWhen `fallow coverage analyze --cloud` returns a coverage_unresolved warning\n(the cloud could not map runtime positions to source, almost always because no\nsource maps were uploaded for the commit) and the project has built source maps\non disk, print a copy-paste hint naming the exact upload command and build dir:\n\n  Hint: found source maps under .next/ that may not be uploaded for this commit.\n    Run `fallow coverage upload-source-maps --dir .next` so runtime coverage\n    attributes to your source files.\n\nRe-running the upload fixes both the never-uploaded and the stale-SHA cases, so\none hint covers both. Human output only: JSON consumers already get the\nstructured coverage_unresolved warning in report.warnings. The hint is gated on\nthe cloud warning code so it never fires when resolution is healthy. Scanned\nbuild dirs (dist, .next, out, build) cover the common bundlers; the scan skips\nnode_modules and stops at the first .map.",
          "timestamp": "2026-06-24T21:53:51Z",
          "url": "https://github.com/fallow-rs/fallow/commit/7d80978332b8dba841a4ace0abc8a375b4b19df3"
        },
        "date": 1782377972023,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30232,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2036,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "7d80978332b8dba841a4ace0abc8a375b4b19df3",
          "message": "feat(coverage): hint source-map upload command when cloud coverage is unresolved (#1431)\n\nWhen `fallow coverage analyze --cloud` returns a coverage_unresolved warning\n(the cloud could not map runtime positions to source, almost always because no\nsource maps were uploaded for the commit) and the project has built source maps\non disk, print a copy-paste hint naming the exact upload command and build dir:\n\n  Hint: found source maps under .next/ that may not be uploaded for this commit.\n    Run `fallow coverage upload-source-maps --dir .next` so runtime coverage\n    attributes to your source files.\n\nRe-running the upload fixes both the never-uploaded and the stale-SHA cases, so\none hint covers both. Human output only: JSON consumers already get the\nstructured coverage_unresolved warning in report.warnings. The hint is gated on\nthe cloud warning code so it never fires when resolution is healthy. Scanned\nbuild dirs (dist, .next, out, build) cover the common bundlers; the scan skips\nnode_modules and stops at the first .map.",
          "timestamp": "2026-06-24T21:53:51Z",
          "url": "https://github.com/fallow-rs/fallow/commit/7d80978332b8dba841a4ace0abc8a375b4b19df3"
        },
        "date": 1782464529058,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30232,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2036,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "2a348354c0102d245827e063e16b07ac43e38ba4",
          "message": "docs(changelog): record the #1634 cluster FP fixes\n\nBroaden the store-member [Unreleased] entry to cover inline useFooStore().member (#1489 Case 1) and add the #1439 component-props entry that #1634 omitted. Docs-only.",
          "timestamp": "2026-06-26T13:51:11Z",
          "url": "https://github.com/fallow-rs/fallow/commit/2a348354c0102d245827e063e16b07ac43e38ba4"
        },
        "date": 1782549491223,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30230,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2036,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "e28929f58d96b190038826bce07c38808cff4166",
          "message": "chore(napi): sync package.json / package-lock / index.js to v2.103.0",
          "timestamp": "2026-06-28T07:38:13Z",
          "url": "https://github.com/fallow-rs/fallow/commit/e28929f58d96b190038826bce07c38808cff4166"
        },
        "date": 1782636723440,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30230,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2036,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "a64ff318455f77583bbe9d4805792fc5da7e2962",
          "message": "fix(telemetry): note find-state for security survivors and blind-spots\n\nThe security survivors and blind-spots subcommands emit a security workflow\ntelemetry event but never noted their find-state, so the process-global\nfindings-present accumulator stayed unset and findings_present serialized as\nnull. findings_present is the field that distinguishes found-candidates from\nerrored (security exits non-zero only on findings when the rule is raised to\nerror), so a null value lost that signal for these modes.\n\nrun_survivors now notes its retained (non-dismissed) candidate count and\nrun_blind_spots notes its unresolved-callee-site count before exit, matching the\ndefault, --file, and --gate paths. No change to the telemetry payload shape.\nFour neuter-verified regression tests assert findings_present is non-null per\nsubcommand.\n\nFixes #1650.",
          "timestamp": "2026-06-29T09:55:32Z",
          "url": "https://github.com/fallow-rs/fallow/commit/a64ff318455f77583bbe9d4805792fc5da7e2962"
        },
        "date": 1782729308505,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30230,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2036,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "f47519d5a421444f6515200b944b0efb3a4be4af",
          "message": "docs(changelog): record fuzzy CSS clones (Phase 4, PR #1669)",
          "timestamp": "2026-06-30T06:25:01Z",
          "url": "https://github.com/fallow-rs/fallow/commit/f47519d5a421444f6515200b944b0efb3a4be4af"
        },
        "date": 1782810494600,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30230,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2036,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.2,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "dependabot[bot]",
            "username": "dependabot[bot]",
            "email": "49699333+dependabot[bot]@users.noreply.github.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "16d7934f39e7cb96d0e29f906b251fb621af3d18",
          "message": "chore(deps): bump syn from 2.0.117 to 2.0.118 (#1695)\n\nBumps [syn](https://github.com/dtolnay/syn) from 2.0.117 to 2.0.118.\n- [Release notes](https://github.com/dtolnay/syn/releases)\n- [Commits](https://github.com/dtolnay/syn/compare/2.0.117...2.0.118)\n\n---\nupdated-dependencies:\n- dependency-name: syn\n  dependency-version: 2.0.118\n  dependency-type: direct:production\n  update-type: version-update:semver-patch\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>\nCo-authored-by: dependabot[bot] <49699333+dependabot[bot]@users.noreply.github.com>",
          "timestamp": "2026-07-01T09:28:01Z",
          "url": "https://github.com/fallow-rs/fallow/commit/16d7934f39e7cb96d0e29f906b251fb621af3d18"
        },
        "date": 1782898943417,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 593,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30216,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2036,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "7d7ca6d3c8d7737501beba662a7b50146ff8a4be",
          "message": "chore(napi): sync package.json / package-lock / index.js to v2.104.0",
          "timestamp": "2026-07-01T21:16:05Z",
          "url": "https://github.com/fallow-rs/fallow/commit/7d7ca6d3c8d7737501beba662a7b50146ff8a4be"
        },
        "date": 1782982457757,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 591,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30216,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Revaz Zakalashvili",
            "username": "revazi",
            "email": "revaz.zakalashvili@gmail.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "88c3f434bd53d2536914607ee9a6d193a7bacb82",
          "message": "docs: add repo-scoped agent skills\n\nAdd portable Agent Skills adapters for the CI formats, CLI output, and JSON output reviewers.\n\nKeep the Agent Skills bodies aligned with their Claude reviewer counterparts where possible, while removing dependencies on local internal files or private maintainer context.",
          "timestamp": "2026-07-03T07:55:37Z",
          "url": "https://github.com/fallow-rs/fallow/commit/88c3f434bd53d2536914607ee9a6d193a7bacb82"
        },
        "date": 1783069292669,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 591,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30216,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "348caa5d35f0c8d73f79c4950f9ab55db056b150",
          "message": "feat(core): add rule-pack v2 matchers\n\nAdds zone-scoped rule-pack policies, banned-export rules, and deep-import matching while keeping policy violations on the existing typed output contract.",
          "timestamp": "2026-07-04T08:15:19Z",
          "url": "https://github.com/fallow-rs/fallow/commit/348caa5d35f0c8d73f79c4950f9ab55db056b150"
        },
        "date": 1783154220182,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 591,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30216,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "b1a2c07b8a247658f61eb133c0ce35c78e017606",
          "message": "fix(extract): credit factory members via return-type annotation (#1744)\n\nCross-module factory-return member crediting was body-only: it fired for\n`return new Class()` or a returned identifier whose type is a proven local,\nbut ignored the factory's own return-TYPE annotation. So a hook/factory\nwhose body has no value proof (`function useController(): ReadyAppController {\nreturn registry.get() as ReadyAppController }`) recorded no class binding,\nand every public method read on `const c = useController()` false-flagged as\nunused-class-member even though the class export itself was reachable.\n\nThread the function's declared return-type annotation into the factory-return\nrecording. When neither body value-proof fires, a sync (non-async,\nnon-generator) factory whose return type names a class records a strict\nfactory-return entry, so the cross-module `exported_factory_returns` metadata\ncredits `c.method()` across the module boundary (both fn-decl and arrow\nforms). This deliberately widens the #1441 value-vs-type doctrine: unlike a\nreturned-identifier's contradictable variable annotation, a function\nreturn-type annotation is the author's compiler-checked contract. It stays\nover-credit-safe: the analyze layer credits only when the name resolves to a\nreal class-with-members export, so a wrong annotation is a false negative at\nworst, never a false positive. A genuinely-unused method on the returned\nclass still reports.\n\nAdds extract unit tests (records the strict entry, arrow variant, async\nabstain) and a cross-file integration fixture + test. Bumps extract\nCACHE_VERSION 220 to 221 (the exported_factory_returns output changes).",
          "timestamp": "2026-07-05T08:05:26Z",
          "url": "https://github.com/fallow-rs/fallow/commit/b1a2c07b8a247658f61eb133c0ce35c78e017606"
        },
        "date": 1783241549139,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 591,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30216,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "ba423ccec8b0e7edc3e5cdb67ba97ea1c9b2c25d",
          "message": "docs(extract): clarify parse scheduling\n\nClarifies that extraction uses a sequential fast path for small file sets and parallel parsing for larger file sets.\n\nAlso documents why the small-input threshold exists: avoiding Rayon scheduling overhead on cache-hot inputs.",
          "timestamp": "2026-07-06T10:02:31Z",
          "url": "https://github.com/fallow-rs/fallow/commit/ba423ccec8b0e7edc3e5cdb67ba97ea1c9b2c25d"
        },
        "date": 1783332548305,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 591,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30220,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "e7878f27ede3575df0ca7142e03db38ad56acb00",
          "message": "refactor: remove architecture debt\n\nMove SARIF-family assembly and shared formatter facts behind fallow-output while keeping fallow-api as a compatibility facade.\n\nReclassify fallow-core as an internal detector backend behind fallow-engine, tighten architecture guards for direct core calls, IO/cache ownership, analyzer placement, manifest drift, and protocol prose.\n\nRemove stale exception language from contributor docs and add pre-ship guard coverage so the architecture debt cannot silently return.",
          "timestamp": "2026-07-07T05:51:22Z",
          "url": "https://github.com/fallow-rs/fallow/commit/e7878f27ede3575df0ca7142e03db38ad56acb00"
        },
        "date": 1783417004975,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 591,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30220,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2031,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "7a827074ceedafb3199ae6ea1f729910ec1354b1",
          "message": "feat(plugins): add fallow plugin-check for agent plugin-authoring\n\nRead-only fallow plugin-check [--format json] dry-run so an agent can verify an external plugin (fallow-plugin-*.jsonc, especially manifestEntries) without a full analysis. Reports per plugin whether it activated (with the unmet detection/enabler requirement when inactive), and for active manifestEntries plugins the per-rule matched manifests, when-gate result, seeded entries (with path_exists), and typed warnings. The report is a shared RuleReport that production seeding also consumes, so the two cannot drift. Deterministic output; always exits 0 (advisory).\n\nA dead-code --format json run with active external plugins + unused files surfaces a verify-plugins next step, and fallow schema related_schemas gained plugin_schema_command / plugin_check_command pointers. Fixes the dead fallow.dev/plugin-schema.json URL to raw.githubusercontent. Refs #1774.",
          "timestamp": "2026-07-08T07:28:12Z",
          "url": "https://github.com/fallow-rs/fallow/commit/7a827074ceedafb3199ae6ea1f729910ec1354b1"
        },
        "date": 1783498864560,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 591,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30220,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2031,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "48f415e1191095a8b4a7a170e0ceee8eaccb402e",
          "message": "refactor(core): remove engine-owned copy modules\n\nRemove the stale fallow-core copies for churn, trace, trace-chain, and cross-reference now that fallow-engine owns those surfaces. This keeps core focused on internal orchestration instead of publishing duplicate adapters that can drift.\n\nMove the trace and trace-chain regression coverage onto the engine path, add an architecture guardrail that rejects reintroducing the core modules, and drop the unused core bitcode dependency.",
          "timestamp": "2026-07-09T08:47:28Z",
          "url": "https://github.com/fallow-rs/fallow/commit/48f415e1191095a8b4a7a170e0ceee8eaccb402e"
        },
        "date": 1783589619125,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 591,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30220,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2031,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "4d019eeb4785ea71412d8e7f56dc8a208d03e94f",
          "message": "feat(mcp): add list_suppressions tool for the suppression inventory\n\nThe fallow suppressions inventory was CLI-only, so agents on the MCP surface could not see what a clean verdict was hiding without shelling out. The new read-only list_suppressions tool wraps `fallow suppressions --format json` as a subprocess (the security_candidates shape, so timeout handling, telemetry tagging, and process-lifecycle hardening come from the shared run_tool path) and returns the suppression-inventory envelope verbatim, introducing no new wire contract.\n\nThe tool forwards workspace, changed_since, and repeated file scoping plus production, root, config, no_cache, threads, and the per-request allow_remote_extends config-trust opt-in; empty file entries are rejected with a structured validation error. changed-workspaces is deliberately not forwarded in v1. Includes the capability-manifest row, regenerated capabilities.json and MCP tools table, tests, and a corrected feature_flags doc line that advertised never-forwarded params.",
          "timestamp": "2026-07-10T09:19:18Z",
          "url": "https://github.com/fallow-rs/fallow/commit/4d019eeb4785ea71412d8e7f56dc8a208d03e94f"
        },
        "date": 1783675948582,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 591,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30220,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2031,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "c05420909051aec49c0362c8e1de28132d82a86b",
          "message": "ci: move telemetry fail-rate gate to cloud",
          "timestamp": "2026-07-10T21:56:39Z",
          "url": "https://github.com/fallow-rs/fallow/commit/c05420909051aec49c0362c8e1de28132d82a86b"
        },
        "date": 1783756379867,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 592,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30220,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "dfcc69f429c943a91751a587990a0e7f078b325e",
          "message": "ci: move cross-platform checks to release\n\nKeep regular pull request and main CI on Ubuntu for fast feedback. Move Windows correctness, lifecycle, NAPI, ARM64, and macOS or Windows Zed coverage into token-free release verification.\n\nGate crates.io, npm, GitHub release, and editor publication paths behind the aggregate release verification job.",
          "timestamp": "2026-07-11T20:36:08Z",
          "url": "https://github.com/fallow-rs/fallow/commit/dfcc69f429c943a91751a587990a0e7f078b325e"
        },
        "date": 1783844000674,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 592,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30220,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "ab61b4073f08af78335aef83d5fb997836c57c85",
          "message": "fix(cli): emit parseable suppress tokens in every human footer section (#1835)\n\nsection_suppress_rule in the human report emitted hardcoded token strings that\nfallow_types::suppress::parse_suppression_target did not recognize for eight\nsections (unused-exports, unused-types, unused-dependencies, unused-enum-members,\nunused-class-members, unresolved-imports, unlisted-dependencies, duplicate-exports):\nthe strings existed only as config keys or MCP names, never as IssueKind codes or\naliases. Following the printed hint produced a comment that suppressed nothing and\nthen surfaced a stale-suppression finding.\n\nDerive each section's token from the issue registry instead. A section-title to\nIssueKind map feeds issue_kind_to_kebab (the kind's suppress_token, falling back\nto its code), so the emitted token always parses back to the same kind.\nis_file_level_only is likewise derived from the registry's suppress_file_level\nflag, fixing two drifts: duplicate-export (file-level-only per its detector) now\nprints the file-level form, and circular-dependency / boundary-violation (which\nhonor next-line suppression) now print the next-line form. Dependency sections\nwhose findings live in package.json emit no hint rather than a token pointing at\nan impossible inline comment.\n\nA roundtrip guard test asserts every token section_suppress_rule can emit parses\nvia parse_suppression_target and uses the file-level form exactly when the\nregistry marks the kind file-level-only, so this surface stays locked down.\n\nFixes #1828",
          "timestamp": "2026-07-13T08:51:20Z",
          "url": "https://github.com/fallow-rs/fallow/commit/ab61b4073f08af78335aef83d5fb997836c57c85"
        },
        "date": 1783933350860,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 592,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30219,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "bc7cd251ba3f813b7d7a35712fa140baaf058647",
          "message": "chore(napi): sync package.json / package-lock / index.js to v3.5.0",
          "timestamp": "2026-07-14T06:58:48Z",
          "url": "https://github.com/fallow-rs/fallow/commit/bc7cd251ba3f813b7d7a35712fa140baaf058647"
        },
        "date": 1784015939143,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 592,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30219,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "d86455c69123a2f8ff5d4aeb6e3fc0786609c534",
          "message": "feat(cli): compact JSON output by default\n\nEmit compact machine-readable JSON across CLI, error, watch, and MCP paths while preserving the parsed schema and fixed CI formats.\n\nAdd `--pretty` for explicit indented output and validate it against each command's actual payload. This addresses the efficiency goal without adding TOON or another interchange format.\n\nFixes #1861.",
          "timestamp": "2026-07-15T07:16:39Z",
          "url": "https://github.com/fallow-rs/fallow/commit/d86455c69123a2f8ff5d4aeb6e3fc0786609c534"
        },
        "date": 1784102540043,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 592,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30218,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "3d30c38a4a49ccb912174e22d674e19377ebf910",
          "message": "chore(napi): sync package.json / package-lock / index.js to v3.6.0",
          "timestamp": "2026-07-15T20:36:00Z",
          "url": "https://github.com/fallow-rs/fallow/commit/3d30c38a4a49ccb912174e22d674e19377ebf910"
        },
        "date": 1784189309716,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 590,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30203,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2031,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "fc632a1c2f2b57580bc0af8222038fe3b3cd1e53",
          "message": "fix: harden external input boundaries\n\nHarden external input boundaries before analysis: bound churn and diff reads, reject unsafe imported paths and aggregate overflow, and keep source and manifest symlink targets inside the configured project root. Oversized diffs continue with unfiltered reporting instead of truncated parsing.\n\nAdd focused regression coverage, real-repository probes, and Windows CI coverage for the platform-gated core symlink paths and all-target Clippy.",
          "timestamp": "2026-07-16T13:52:16Z",
          "url": "https://github.com/fallow-rs/fallow/commit/fc632a1c2f2b57580bc0af8222038fe3b3cd1e53"
        },
        "date": 1784275447760,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 590,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30203,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2031,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "fc632a1c2f2b57580bc0af8222038fe3b3cd1e53",
          "message": "fix: harden external input boundaries\n\nHarden external input boundaries before analysis: bound churn and diff reads, reject unsafe imported paths and aggregate overflow, and keep source and manifest symlink targets inside the configured project root. Oversized diffs continue with unfiltered reporting instead of truncated parsing.\n\nAdd focused regression coverage, real-repository probes, and Windows CI coverage for the platform-gated core symlink paths and all-target Clippy.",
          "timestamp": "2026-07-16T13:52:16Z",
          "url": "https://github.com/fallow-rs/fallow/commit/fc632a1c2f2b57580bc0af8222038fe3b3cd1e53"
        },
        "date": 1784360288807,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 590,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30203,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2031,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "fc632a1c2f2b57580bc0af8222038fe3b3cd1e53",
          "message": "fix: harden external input boundaries\n\nHarden external input boundaries before analysis: bound churn and diff reads, reject unsafe imported paths and aggregate overflow, and keep source and manifest symlink targets inside the configured project root. Oversized diffs continue with unfiltered reporting instead of truncated parsing.\n\nAdd focused regression coverage, real-repository probes, and Windows CI coverage for the platform-gated core symlink paths and all-target Clippy.",
          "timestamp": "2026-07-16T13:52:16Z",
          "url": "https://github.com/fallow-rs/fallow/commit/fc632a1c2f2b57580bc0af8222038fe3b3cd1e53"
        },
        "date": 1784448686095,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 590,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30203,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2031,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "124ad5cc84484f890cfd728240d6688d3b68df4d",
          "message": "fix(napi): bump lockfile fallow-node entries to v3.7.0",
          "timestamp": "2026-07-20T08:39:44Z",
          "url": "https://github.com/fallow-rs/fallow/commit/124ad5cc84484f890cfd728240d6688d3b68df4d"
        },
        "date": 1784537385972,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 590,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30203,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2031,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "bfe588034642dfa5d812e9e06d8c79d00d3cb0ca",
          "message": "ci: replace docker-lockstep PR job with maintainer-flow Dockerfile pin\n\nThe release.yml docker-lockstep job opened a docker-lockstep/vX.Y.Z PR each\nrelease that then needed an admin merge. Fold the Dockerfile pin into the\nmaintainer release flow (fallow-release skill step 13) as a direct commit to\nmain, mirroring the crates/napi lockfile catch-up (step 12): download the\njust-published musl assets, re-hash them, run update-dockerfile-pins.mjs, and\npush. The ci.yml Docker job re-verifies the pin end-to-end on that commit.\n\nThe shared rewrite helper and its node --test suite stay. Refs #1817.",
          "timestamp": "2026-07-20T12:11:24Z",
          "url": "https://github.com/fallow-rs/fallow/commit/bfe588034642dfa5d812e9e06d8c79d00d3cb0ca"
        },
        "date": 1784621987406,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 590,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30199,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2031,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "5184b9d74d2ede605538a96f1bae9ec0edaf73e3",
          "message": "fix: resolve audit and analysis improvement findings\n\nImprove inherited member and alias resolution, audit comparison context, styling attribution, and annotation safety across CLI, API, MCP, CI, and editor surfaces.\n\nReuse shared repository context for audit attribution and base snapshots, and reduce clone-family and warm CSS analysis overhead without changing stable output contracts. Preserve the existing CSS benchmark workload and track the heavier many-file workload separately.\n\nRefresh dependency coverage and invalidate affected extraction and audit caches.",
          "timestamp": "2026-07-22T08:13:36Z",
          "url": "https://github.com/fallow-rs/fallow/commit/5184b9d74d2ede605538a96f1bae9ec0edaf73e3"
        },
        "date": 1784708417787,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 590,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30199,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2031,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.7,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "acd28051c4c2c217f9fac612f288e9546c86b6c3",
          "message": "chore(docker): pin FALLOW_VERSION 3.8.0 with refreshed checksums",
          "timestamp": "2026-07-22T16:22:02Z",
          "url": "https://github.com/fallow-rs/fallow/commit/acd28051c4c2c217f9fac612f288e9546c86b6c3"
        },
        "date": 1784795028629,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 589,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30199,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.6,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "45f1642a21b049771ecf54fb92133dda4ce0c1fe",
          "message": "chore(docker): pin FALLOW_VERSION 3.9.1 with refreshed checksums",
          "timestamp": "2026-07-23T23:22:42Z",
          "url": "https://github.com/fallow-rs/fallow/commit/45f1642a21b049771ecf54fb92133dda4ce0c1fe"
        },
        "date": 1784881182767,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 589,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30199,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.6,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "45f1642a21b049771ecf54fb92133dda4ce0c1fe",
          "message": "chore(docker): pin FALLOW_VERSION 3.9.1 with refreshed checksums",
          "timestamp": "2026-07-23T23:22:42Z",
          "url": "https://github.com/fallow-rs/fallow/commit/45f1642a21b049771ecf54fb92133dda4ce0c1fe"
        },
        "date": 1784966552936,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 589,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30199,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.6,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "45f1642a21b049771ecf54fb92133dda4ce0c1fe",
          "message": "chore(docker): pin FALLOW_VERSION 3.9.1 with refreshed checksums",
          "timestamp": "2026-07-23T23:22:42Z",
          "url": "https://github.com/fallow-rs/fallow/commit/45f1642a21b049771ecf54fb92133dda4ce0c1fe"
        },
        "date": 1785054032878,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 589,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30199,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.6,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "45f1642a21b049771ecf54fb92133dda4ce0c1fe",
          "message": "chore(docker): pin FALLOW_VERSION 3.9.1 with refreshed checksums",
          "timestamp": "2026-07-23T23:22:42Z",
          "url": "https://github.com/fallow-rs/fallow/commit/45f1642a21b049771ecf54fb92133dda4ce0c1fe"
        },
        "date": 1785145443518,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.9,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 589,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30199,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.6,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "55944e88c96fe0cc60679470781808977fd1e0fc",
          "message": "fix(dupes): apply the duplication threshold gate to standalone runs\n\nStandalone `fallow dupes` rendered through `print_dupes_result_with_grouping`,\nwhich returned the renderer's exit code without ever consulting\n`exceeds_threshold`. The gate lived only in `print_dupes_result`, which\nstandalone runs no longer call after the grouping refactor, so\n`fallow dupes --threshold 1` exited 0 at 100% duplication and printed no\ndiagnostic. Both the `--threshold` flag and a `duplicates.threshold` config\nvalue were affected, in every output format. Combined mode (bare `fallow`)\nrendered through the second, near-identical function that did gate, so the two\nentry points disagreed.\n\nThe two renderers differed only in `group_by`, which `print_dupes_result`\nalready passed as `None`. That duplication is what let the gate drift out of\none copy, so they are collapsed into one: `print_dupes_result` delegates and\nthe gate moves onto the single shared renderer. The source diff is\nnet-negative. New coverage in `crates/cli/tests/dupes_tests.rs` exercises the\nflag path, the config path, and the output formats, each with a\nbelow-threshold control so it cannot pass vacuously. The existing\n`exit_code_tests` case asserted `code == 0 || code == 1`, true of any\nnon-crashing run, and was named for `--fail-on-issues`, which `fallow dupes`\ndoes not wire. It is renamed to `dupes_threshold_exits_1_with_clones`, asserts\nthe exit code exactly, and carries a comment recording why the inert flag is\nabsent. Wiring `--fail-on-issues` for dupes is a separate behaviour change and\nis out of scope here.\n\nProjects that set a duplication threshold and were silently passing will start\nfailing as documented. Runs that set no threshold are unaffected, since the\ndefault (`0`) still means no limit.\n\nFixes #2009.",
          "timestamp": "2026-07-28T00:37:53Z",
          "url": "https://github.com/fallow-rs/fallow/commit/55944e88c96fe0cc60679470781808977fd1e0fc"
        },
        "date": 1785227250966,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 576,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30173,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "57adb47d2ddc8720f1870dcaabca5096529d0c05",
          "message": "feat(health): add an identity-preserving baseline mode (#2064)\n\nThe default count baseline matches per file and category, so a new hotspot replacing an old one in the same file consumed the existing allowance and the gate stayed green. --baseline-mode identity matches per function identity instead: a replacement hotspot is reported, line shifts and severity improvements stay suppressed, and resolved findings disappear without a refresh.\n\nThe default stays count. Identity baselines keep their count buckets so both modes read them, and comparing in identity mode against a count-only baseline is an input error rather than a silent fallback. A finding identity is file path plus function name, so renaming or moving a function that is still in the baseline reports it as new; the flag documentation states that limit and the re-save rule.\n\nRefs #2010",
          "timestamp": "2026-07-28T20:48:23Z",
          "url": "https://github.com/fallow-rs/fallow/commit/57adb47d2ddc8720f1870dcaabca5096529d0c05"
        },
        "date": 1785314022531,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 576,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30173,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "92623a5f54a52e1834318256218ba989efadeefc",
          "message": "chore(agents): finish the reviewer-skill migration (#2072)\n\nSeven review subjects already follow one shape: a thin <x>-review skill that reads its checklist from .agents/agents/<x>-reviewer.md and its constraints from .agents/rules. Three still carried the older fat <x>-reviewer skill alongside it, so both generations loaded and competed for the same triggers.\n\nThe fat skills were not pure duplicates, so their unique content moved into the agent definitions first: the four CI-format audits, the human-format audit, the pluralization rule for counted nouns, and the note that the real-world corpus must be downloaded before any audit command works, which was missing from every agent definition.\n\nReviewer names in team-assembly refer to agents to spawn rather than skills, so they keep resolving.",
          "timestamp": "2026-07-29T22:06:22Z",
          "url": "https://github.com/fallow-rs/fallow/commit/92623a5f54a52e1834318256218ba989efadeefc"
        },
        "date": 1785399652025,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 576,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30173,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "92623a5f54a52e1834318256218ba989efadeefc",
          "message": "chore(agents): finish the reviewer-skill migration (#2072)\n\nSeven review subjects already follow one shape: a thin <x>-review skill that reads its checklist from .agents/agents/<x>-reviewer.md and its constraints from .agents/rules. Three still carried the older fat <x>-reviewer skill alongside it, so both generations loaded and competed for the same triggers.\n\nThe fat skills were not pure duplicates, so their unique content moved into the agent definitions first: the four CI-format audits, the human-format audit, the pluralization rule for counted nouns, and the note that the real-world corpus must be downloaded before any audit command works, which was missing from every agent definition.\n\nReviewer names in team-assembly refer to agents to spawn rather than skills, so they keep resolving.",
          "timestamp": "2026-07-29T22:06:22Z",
          "url": "https://github.com/fallow-rs/fallow/commit/92623a5f54a52e1834318256218ba989efadeefc"
        },
        "date": 1785487581195,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 576,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30173,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "92623a5f54a52e1834318256218ba989efadeefc",
          "message": "chore(agents): finish the reviewer-skill migration (#2072)\n\nSeven review subjects already follow one shape: a thin <x>-review skill that reads its checklist from .agents/agents/<x>-reviewer.md and its constraints from .agents/rules. Three still carried the older fat <x>-reviewer skill alongside it, so both generations loaded and competed for the same triggers.\n\nThe fat skills were not pure duplicates, so their unique content moved into the agent definitions first: the four CI-format audits, the human-format audit, the pluralization rule for counted nouns, and the note that the real-world corpus must be downloaded before any audit command works, which was missing from every agent definition.\n\nReviewer names in team-assembly refer to agents to spawn rather than skills, so they keep resolving.",
          "timestamp": "2026-07-29T22:06:22Z",
          "url": "https://github.com/fallow-rs/fallow/commit/92623a5f54a52e1834318256218ba989efadeefc"
        },
        "date": 1785572106028,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 576,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30173,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "763066883ef36e51c0c99c9971b676beca9a8e55",
          "message": "docs: use dead-code instead of the deprecated check alias in the migration doc",
          "timestamp": "2026-08-02T06:28:57Z",
          "url": "https://github.com/fallow-rs/fallow/commit/763066883ef36e51c0c99c9971b676beca9a8e55"
        },
        "date": 1785658689866,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 576,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30176,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "4e83c23c025e17874f34f0c674166c86cfba6bf1",
          "message": "chore(docker): pin FALLOW_VERSION 3.12.0 with refreshed checksums",
          "timestamp": "2026-08-03T08:59:33Z",
          "url": "https://github.com/fallow-rs/fallow/commit/4e83c23c025e17874f34f0c674166c86cfba6bf1"
        },
        "date": 1785749671612,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 575,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30177,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "3cf8074a0e2e91c895c0a4224ba1c3bec4630d65",
          "message": "chore: release v3.14.0",
          "timestamp": "2026-08-04T07:26:26Z",
          "url": "https://github.com/fallow-rs/fallow/commit/3cf8074a0e2e91c895c0a4224ba1c3bec4630d65"
        },
        "date": 1785832451018,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 575,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30177,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "589885290490eeeb02fcc274defc55a3c11789dc",
          "message": "fix(viz): add the wasm-runtime peer entry for @emnapi/runtime 1.11.3\n\nThe @napi-rs/wasm-runtime peer dependency needs a top-level runtime entry\nthat macOS resolution never materializes; clean npm ci now passes with and\nwithout --omit=optional and the build and tests are green from scratch.",
          "timestamp": "2026-08-04T16:31:25Z",
          "url": "https://github.com/fallow-rs/fallow/commit/589885290490eeeb02fcc274defc55a3c11789dc"
        },
        "date": 1785918673648,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 575,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30177,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2030,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 6.1,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 2.8,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "24e84bc0ca81bd48d3b0a520cbcdcd2ab090ef75",
          "message": "feat(cli): progressive root help, specifier-anchored imports, code-span table cells (#2149)\n\nfallow -h leads with the task cheat sheet, shows the Analysis and Workflow groups plus everyday options, and points to --help for the complete list (197 to 49 lines; the machine schema is unchanged). Unresolved-import findings anchor on the source specifier via new statement and source spans threaded through extract, the caches, and the graph, so one suppression above a multi-line re-export covers the deduped finding and the stale-suppression contradiction is gone. Identifier and path cells in github-summary tables render as code spans with pipe escaping, mirrored in the action/ and ci/ jq fallback renderers, and the shared helper collapses CR/LF so identifiers cannot split a table row.",
          "timestamp": "2026-08-05T21:34:55Z",
          "url": "https://github.com/fallow-rs/fallow/commit/24e84bc0ca81bd48d3b0a520cbcdcd2ab090ef75"
        },
        "date": 1786004906143,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 571,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30155,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "acab6e72f14ee8c7f5e1c3fe239c2cb456551281",
          "message": "refactor: share resolve payloads, consolidate the discovery walk, document four crates (#2153)\n\nThe seven heavy read-only extraction fields on ModuleInfo/ResolvedModule are Arc slices, so per-file resolution and graph-cache restore bump refcounts instead of deep-copying (measured 2-4% CPU reduction and lower peak RSS on real projects; cached wire shapes untouched). The engine's near-verbatim fork of the core discovery walk is deleted (net -1974 lines) along with its hardcoded config-candidate list, the no-drift gate that guarded it, and the orphaned ignore dependency; engine routes through a core_backend adapter and config candidates derive from the plugin registry, with JSON output verified byte-identical on real projects. missing_docs is burned to zero in fallow-output, fallow-api, fallow-engine, and fallow-config, each now enforcing the lint; generated contract surfaces are regenerated and the inspect identity verdict fields carry typed boolean/string schemas matching runtime output.",
          "timestamp": "2026-08-07T05:50:26Z",
          "url": "https://github.com/fallow-rs/fallow/commit/acab6e72f14ee8c7f5e1c3fe239c2cb456551281"
        },
        "date": 1786086835207,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 571,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30155,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "49c0bb438c8ddf4b207ad35446f6525147ecb2ae",
          "message": "refactor: keep one definition of each discovery constant (#2159)\n\nSOURCE_EXTENSIONS, PRODUCTION_EXCLUDE_PATTERNS, and ALLOWED_HIDDEN_DIRS were byte-identical copies in the engine and the core walk that consumes them; the engine re-exports the core definitions through the backend adapter, leaving its public paths unchanged. OUTPUT_DIRS had a third copy and moves to fallow-graph, the lowest crate both sides already depend on, so no adapter hop or boundary exception is needed. Verified byte-identical check and list JSON on two real projects against a binary built from a pristine worktree at the parent commit.",
          "timestamp": "2026-08-07T22:15:56Z",
          "url": "https://github.com/fallow-rs/fallow/commit/49c0bb438c8ddf4b207ad35446f6525147ecb2ae"
        },
        "date": 1786171645860,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 571,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30155,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "d4c08465b2c66afbaf3ea1e25b6259a02da30327",
          "message": "fix: close duplication sweep gaps\n\n* chore: start duplication filter follow-up\n\n* fix: close duplication sweep gaps",
          "timestamp": "2026-08-08T20:57:45Z",
          "url": "https://github.com/fallow-rs/fallow/commit/d4c08465b2c66afbaf3ea1e25b6259a02da30327"
        },
        "date": 1786258258603,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 571,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30155,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "b8e9c8c305dd7a5ae9f4f3afa7880738028ad943",
          "message": "fix(health): handle inline Svelte await states",
          "timestamp": "2026-08-09T21:00:28Z",
          "url": "https://github.com/fallow-rs/fallow/commit/b8e9c8c305dd7a5ae9f4f3afa7880738028ad943"
        },
        "date": 1786346666818,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 571,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30155,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "b8e9c8c305dd7a5ae9f4f3afa7880738028ad943",
          "message": "fix(health): handle inline Svelte await states",
          "timestamp": "2026-08-09T21:00:28Z",
          "url": "https://github.com/fallow-rs/fallow/commit/b8e9c8c305dd7a5ae9f4f3afa7880738028ad943"
        },
        "date": 1786431557628,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 571,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30155,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "66ae724873c3002fe4c81d20d4a30ecb78139802",
          "message": "fix(napi): keep the node-binding manifests at the last published version until the release publishes\n\nThe npm-prep job runs npm ci in crates/napi before CI bumps the version\nfrom the dispatched tag, so the committed package.json and lockfile must\nstay at the last version whose platform packages exist on npm. The 3.15.0\nbump left the nested platform entries unresolvable and npm ci rejects the\ntree; the post-release sync brings these files to 3.15.0 once the platform\npackages are published.",
          "timestamp": "2026-08-11T21:05:14Z",
          "url": "https://github.com/fallow-rs/fallow/commit/66ae724873c3002fe4c81d20d4a30ecb78139802"
        },
        "date": 1786519263029,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 571,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30155,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.3,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 5.8,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 22.8,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bartwaardenburg@gmail.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "95728543e804a0e4a4a0ca9dd239f328c6adde4d",
          "message": "Merge pull request #2247 from fallow-rs/feat/manual-rust-walltime\n\nci: add manual Rust walltime benchmarks",
          "timestamp": "2026-08-13T06:56:58Z",
          "url": "https://github.com/fallow-rs/fallow/commit/95728543e804a0e4a4a0ca9dd239f328c6adde4d"
        },
        "date": 1786606083430,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 577,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30103,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "95a7ae9faf9e987616fe2366b74e99626dfd58c6",
          "message": "fix: surface star-export ambiguity instead of blaming the sources (#2268)\n\nWhen two star re-export sources supply the same name, the barrel exports nothing under that name. Unused-export and unused-type findings are now suppressed for the declarations that contribute to such a collision, instead of blaming both source files for a mistake in the barrel. Traces carry an optional star_export_ambiguity block naming the contributing files and namespaces, so an ambiguous name is no longer indistinguishable from a misspelled one. The unrendered-component and unprovided-inject headers now state the guarantee the code actually offers, including the abstain carve-out that remains. The value-derived type fallback lane is seeded lazily, which makes barrel-chain resolution roughly ten percent cheaper.\n\nCloses #2262\nCloses #2263\nCloses #2264",
          "timestamp": "2026-08-14T06:35:39Z",
          "url": "https://github.com/fallow-rs/fallow/commit/95a7ae9faf9e987616fe2366b74e99626dfd58c6"
        },
        "date": 1786692132332,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 577,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30103,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "6ab2c847bad9bc88a85e6fa29139a811db7203a0",
          "message": "fix(type-aware): bound generic scans and identify Svelte host gaps\n\n* chore: start type-aware issue fixes\n\n* fix: harden type-aware generic and Svelte analysis",
          "timestamp": "2026-08-14T09:14:51Z",
          "url": "https://github.com/fallow-rs/fallow/commit/6ab2c847bad9bc88a85e6fa29139a811db7203a0"
        },
        "date": 1786775559777,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 577,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30103,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "8437d52e6688cd1ce823d5da8c6670e7a23f839f",
          "message": "fix: close analysis and audit follow-ups\n\n* chore: start issue follow-up batch\n\n* chore: start issue follow-up batch\n\n* fix: close analysis and audit follow-ups",
          "timestamp": "2026-08-15T23:07:16Z",
          "url": "https://github.com/fallow-rs/fallow/commit/8437d52e6688cd1ce823d5da8c6670e7a23f839f"
        },
        "date": 1786862091605,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 577,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30103,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "fc234ba804a29edc872f2ace40045b6254da5c81",
          "message": "chore(docker): pin v3.17.0 assets",
          "timestamp": "2026-08-17T00:40:35Z",
          "url": "https://github.com/fallow-rs/fallow/commit/fc234ba804a29edc872f2ace40045b6254da5c81"
        },
        "date": 1786949119992,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 577,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30103,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "172d9f1ca0f3eb73b1a8404548b67cbe6dafdee4",
          "message": "ci(benchmarks): add config walltime workload",
          "timestamp": "2026-08-17T19:38:28Z",
          "url": "https://github.com/fallow-rs/fallow/commit/172d9f1ca0f3eb73b1a8404548b67cbe6dafdee4"
        },
        "date": 1787035044371,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 577,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30103,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "f6ce30f8c3663ffa8d042a61e792ed8e03c11b09",
          "message": "perf(benchmarks): track list inventory",
          "timestamp": "2026-08-19T04:29:00Z",
          "url": "https://github.com/fallow-rs/fallow/commit/f6ce30f8c3663ffa8d042a61e792ed8e03c11b09"
        },
        "date": 1787121587124,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 577,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30103,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "fecc8c221bd0c75346720209d8e6ec18d78f8495",
          "message": "perf(benchmarks): cover hotspot ownership churn",
          "timestamp": "2026-08-19T19:24:21Z",
          "url": "https://github.com/fallow-rs/fallow/commit/fecc8c221bd0c75346720209d8e6ec18d78f8495"
        },
        "date": 1787208076072,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 577,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30103,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "69dc2c13221ee32b578617d659352c5218191888",
          "message": "perf(benchmarks): cover Istanbul health CRAP matching\n\nAdd stable CodSpeed coverage for Istanbul ingestion, declaration-alias matching, CRAP scoring, and health report assembly.",
          "timestamp": "2026-08-20T14:38:05Z",
          "url": "https://github.com/fallow-rs/fallow/commit/69dc2c13221ee32b578617d659352c5218191888"
        },
        "date": 1787294516572,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 577,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30103,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "03b7e74802176f653ae5fcf6d92a1883335a8599",
          "message": "chore(contracts): regenerate output contract types for bun.lock override resolution\n\nRegenerates the two TypeScript output-contract surfaces that went stale with the UnusedDependencyOverrideFinding doc update from #2350. No structural changes.",
          "timestamp": "2026-08-21T20:17:00Z",
          "url": "https://github.com/fallow-rs/fallow/commit/03b7e74802176f653ae5fcf6d92a1883335a8599"
        },
        "date": 1787380485445,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 577,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30102,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "dc03fd1484261a623e044e1dbe97378095bae4ac",
          "message": "fix(core): analyze package.json resolutions as a bun dependency-override source\n\n## What was broken\n\nbun honours Yarn-style `resolutions` in `package.json` as an alias of `overrides`, but the dependency-override analyzer only read the top-level `overrides` object, `pnpm.overrides`, and `pnpm-workspace.yaml`. A bun repository that pins transitive versions under `resolutions` was never analyzed: no `unused-dependency-overrides` or `misconfigured-dependency-overrides` findings with any lockfile, and since #2362 no `bun-lockb-override-resolution-skipped` diagnostic next to a `bun.lockb` either, because no override state was gathered. Both repros from the issue (a `resolutions` manifest next to a `bun.lockb`, and a `resolutions` manifest next to a text `bun.lock` that resolves only `ws`) produced nothing: no findings, no diagnostic, no stderr warning.\n\n## Root cause\n\n`gather_pnpm_override_state` builds its state from three parsers (`pnpm-workspace.yaml`, `pnpm.overrides`, and the npm `overrides` object) and returns `None` when all three are empty, so the detectors and the bun.lockb skip path never ran for a `resolutions`-only manifest. The npm parser also hard-codes the `overrides` key and flattens nested objects, and it does not understand yarn's `parent/child` and `**/child` path keys, which bun accepts under `resolutions`.\n\n## The fix\n\n- `fallow_config::parse_bun_package_json_resolutions` parses the top-level `resolutions` object through the shared override entry shape. It reuses the npm line scanner (now parameterised by section key, recording only direct keys for `resolutions` so a nested object bun rejects cannot shift a later key's line) and maps bun's key dialect: bare packages, `@scope/pkg`, `pkg@<2`, the yarn paths `parent/child`, `**/child`, and `parent/**/child` (with `@scope/name` spanning two segments), and the pnpm `parent>child` form, using bun's delimiter rule so `pkg@>=1` keeps its selector. Shapes bun warns about and skips (more than one parent level, a bare scope, a trailing `**`, a non-string value) stay entries without a parsed key or value so the misconfigured detector reports them. `//` comment keys are skipped.\n- The core analyzer gathers `resolutions` only for bun repositories: the root `packageManager` names bun, or no recognised `packageManager` is declared and a `bun.lock` or `bun.lockb` sits at the root (a manifest naming npm, pnpm, or yarn is never a bun repository, even next to a leftover bun lockfile, mirroring the packageManager-first rule the transitive hint already uses).\n- bun precedence, cited in a code comment: `OverrideMap::parse_append` in bun's `src/install/lockfile/OverrideMap.rs` takes the `overrides` property when it exists, whatever its value, and falls through to `resolutions` only when `overrides` is absent. The analyzer therefore ignores `resolutions` whenever the manifest has an `overrides` key, including an empty one.\n- `resolutions` entries run through both detectors with `source: \"package.json\"` (the declaring-file label the field is documented as), the entry's line, and a bun hint that names `resolutions`: `declared under `resolutions`, which bun applies as an alias of `overrides`; may target a transitive dependency; bun install --frozen-lockfile is the ground truth`. A `resolutions`-only manifest next to a `bun.lockb` without a parseable text lockfile now records the existing skip diagnostic.\n- yarn repositories keep the current stance, documented in the module docs: `resolutions` is not parsed, and the inert-`overrides` hint is unchanged.\n- The root manifest is parsed once in `gather_pnpm_override_state` and the declared package manager is passed into `collect_lockfile_packages` instead of being re-derived from the source string.\n\n## Behavior change\n\nAdditive findings only, for bun repositories that declare `resolutions` without `overrides`: new `unused-dependency-overrides` and `misconfigured-dependency-overrides` entries, and the `bun-lockb-override-resolution-skipped` diagnostic next to a `bun.lockb`. npm, pnpm, and yarn repositories, and bun repositories with an `overrides` key, are unchanged. No contract change: `DependencyOverrideSource` keeps its two values (the policy in `docs/backwards-compatibility.md` bumps the envelope for a value added to an existing enum-valued required field, and `source` is documented as the declaring-file label, which `package.json` already is), so only the rustdoc descriptions of `DependencyOverrideSource::PnpmPackageJson` and `UnusedDependencyOverride` changed and `docs/output-schema.json` was regenerated from them. Suppression works with the existing `ignoreDependencyOverrides: [{ \"package\": \"...\", \"source\": \"package.json\" }]` rule.\n\n## Cache invalidation\n\nNone. Override analysis reads the manifest and lockfiles on every run; nothing about it is persisted in the extract or graph caches. Warm-cache proof on the fixture: baseline binary cold run writes `.fallow` and reports nothing, the fixed binary on that warm cache reports `left-pad` and `**/trim-newlines`, and a second warm run is stable.\n\n## How it was tested\n\n- Config unit tests (`npm_overrides::tests`): flat entries with lines; every yarn path shape and the pnpm delimiter form; shapes bun rejects are unparsable or valueless and a nested object does not shift a later key's line; comment keys are skipped; the `resolutions` parser ignores `overrides` and nested `resolutions`, and the npm parser ignores `resolutions`.\n- Core unit tests (`unused_overrides::tests`, mirroring the #2362 set): resolutions-only next to `bun.lockb` records the skip diagnostic once and stays deduplicated; resolutions resolve against a text `bun.lock` (left-pad flagged, `source`, `path`, line, hint); an `overrides` key (even empty) shadows `resolutions`; yarn, npm (with a leftover `bun.lock`), pnpm, and lockfile-less repositories ignore `resolutions`; a root bun lockfile without a `packageManager` field enables them; yarn path keys credit a declared parent and the shapes bun rejects reach the misconfigured detector.\n- Integration test on fixture `tests/fixtures/issue-2367-bun-resolutions` (bun repo, `resolutions` with `ws`, `left-pad`, and `**/trim-newlines`, text `bun.lock` resolving `ws`): both unresolved pins report at their lines with the `package.json` source and the resolutions hint, no misconfigured findings, no skip diagnostic; the `ignoreDependencyOverrides` rule with `source: \"package.json\"` suppresses an entry.\n- CLI test: `dead-code --format json` on the fixture carries both findings with `source` and `path` `package.json` and the resolutions hint, and no workspace diagnostics.\n- Issue repros through the baseline binary (built from e65a9083e) and the fixed binary: the `bun.lockb` repro reports the skip diagnostic in JSON and on stderr on the fixed binary only; the text `bun.lock` repro reports `left-pad` with the bun hint on the fixed binary only; a manifest with both `overrides` and `resolutions` reports nothing on either.\n- Real-project parity: `dead-code --format json --no-cache` on the in-repo `viz-frontend` (package-lock.json) and `editors/vscode` (pnpm-lock.yaml); outputs identical apart from `analysis_run_id` and `elapsed_ms`, stderr identical apart from timestamps.\n- Mutation matrix: with the non-test hunks of `unused_overrides.rs` reverted, the core unit tests, the integration test, and the CLI test fail; with the non-test hunks of the config parser reverted, the config tests and the core crate fail to compile on the missing parser.\n- Gates: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --lib --bins --tests --examples`, `cargo check --workspace --benches`, `RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps --document-private-items`, `typos`, the hidden-unicode scan, the comment-quality check, and `npm run generate:contracts:check`.\n\nFixes #2367",
          "timestamp": "2026-08-22T23:33:15Z",
          "url": "https://github.com/fallow-rs/fallow/commit/dc03fd1484261a623e044e1dbe97378095bae4ac"
        },
        "date": 1787467001385,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 577,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30096,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "8eb190505e6a310544e897eb841dc398dac73e36",
          "message": "fix(extract): record import X = require as a CommonJS import edge\n\n## What was broken\n\n`import X = require('./x')` produced no import at all. `crates/extract/src/visitor/` had no arm for a `TSImportEqualsDeclaration` with an external module reference: only `visit_impl_structural.rs` saw the node, and only to shadow a function-type alias name. With neither an `ImportInfo` nor a require call, the target was reported as an unused file and nothing reached through the binding was credited.\n\nReproduction from the issue, `package.json` with `\"main\": \"src/index.ts\"`:\n\n```ts\n// src/assigned.ts\nnamespace Assigned {\n  export const viaAssignment = 1;\n}\nexport = Assigned;\n\n// src/index.ts (entry)\nimport Assigned = require('./assigned');\nconsole.log(Assigned.viaAssignment);\n```\n\n`fallow dead-code --format json --quiet --no-cache` reported `src/assigned.ts` as an unused file. Replacing the consumer line with `import Assigned from './assigned'` made it reachable, so the gap was specific to the import-equals form.\n\n## The fix\n\nA `visit_ts_import_equals_declaration` arm on the `Visit` impl hands the declaration to `handle_import_equals_declaration`, which pushes one non-destructured `RequireCallInfo` (the specifier, the `require('./x')` reference span, the specifier string span, no destructured names, the local binding) plus the local name onto `namespace_binding_names`. That is the same shape `handle_require_declaration` records for `const X = require('./x')`; the two paths run in parallel rather than one calling the other, because a `TSExternalModuleReference` carries a `StringLiteral` where the variable form carries a `CallExpression`.\n\nRecording it as a require call rather than as a hand-rolled `ImportInfo` is the point. `resolve_single_require` already turns a non-destructured require into a `Namespace` import against a target carried through `into_commonjs_require()`, so reusing it gives the CommonJS mechanism, `narrow_namespace_references`, the whole-object namespace seeds from #2372, and the specifier-anchored `source_span`, with no second spelling of the same edge to keep in sync.\n\nThe arm sits on the visitor, so it fires wherever the declaration appears: at file scope, and inside a `declare module '...'` body.\n\n### Both semantic namespaces\n\nLiving outside `ModuleInfo.imports` cost the binding its type/value classification: `compute_semantic_usage_with_candidates` only walked `imports`, so `type_referenced_import_bindings` never named a require-derived binding and `desired_import_namespaces` fell through to value only. A type reached through the binding was therefore left uncredited, and the target's interface surfaced as an `unused-type` row the `import * as` twin never produces.\n\nThe extractor now carries an `import_equals_bindings` vector and feeds it into the same semantic classification an ESM namespace binding gets, so `T.SomeType` in an annotation credits type space and `T.value` credits value space independently. Classification reads the root scope only, exactly the restriction the `imports` loop beside it has: a binding declared inside a `declare module '...'` body is not classified and stays value-only, the same way an `import * as X` binding in that position does.\n\n### An unreferenced binding credits nothing\n\nA binding with no resolved reference anywhere in the file is now reported as an unused import binding, which is the verdict the `imports` loop reaches for an unreferenced `import * as X from './x'`. TypeScript elides both declarations completely, so neither may buy the target a whole-object credit.\n\nCrediting it was strictly worse than the missing edge this PR started from: `import Utils = require('./utils')` in a file that never mentions `Utils` again deleted every `unused-export` and `unused-type` row on `utils.ts`, rows `main` reports correctly, and `import type Shapes = require('./shapes')` deleted both lanes at once. The edge itself is untouched, so the target is still a reachable file, exactly as it is behind an unreferenced `import * as X`.\n\n`export import X = require('./x')` is exempt: it has no local reference by construction and the binding is the file's public API.\n\n### The erased spelling\n\n`import type X = require('pkg')` is the one require spelling TypeScript erases completely: the emitted JavaScript holds no `require` call at all, so the package is a type-space reference and never a runtime import. `RequireCallInfo` now carries `is_type_only`, set from `decl.import_kind.is_type()` and resolved through to the `ImportInfo`, so dependency classification treats the declaration the way it treats `import type * as X from 'pkg'`.\n\nWithout that, a type-only devDependency reported `dev-dependency-in-production` with the note \"production code imports this at runtime\" and an auto-action telling the user to move the package into `dependencies`, where the byte-equivalent ESM twin reported nothing. The unerased `import X = require('pkg')` still reports it, and still matches its own twin.\n\n### The exported form\n\n`export import X = require('./x')` recorded no export either. `narrow_namespace_references` derives `is_re_exported` from an export whose `local_name` matches the binding, so it stayed false, `is_entry_with_no_access` fired on an entry point, and every export of the target became a false `unused-export` row. That is a direct coherence break with #2373 on the shape the declaration exists for.\n\nThe exported form now earns a whole-object use of the binding. That is the credit `import * as X from './x'; export { X }` already produces: the module object reaches consumers the graph cannot enumerate, so every name on it is observed, including the names the target only exposes through its own `export *` chain. No export row is invented for the binding itself, so the change adds no new finding class.\n\n`whole_object_uses` is keyed by bare name, so the credit is only safe while the name resolves to this one binding, and the visitor cannot know that: scope information first exists in the semantic pass. The credit is therefore **granted** there, only for a name the file binds once, rather than pushed at walk time and withdrawn afterwards. Withdrawing by name deleted a whole-object use the file genuinely wrote:\n\n```ts\nexport import Config = require('./config');\nexport const readConfig = (): number => Object.values(Config).length;\nexport const parseConfig = (Config: { n: number }): number => Config.n;\n```\n\nThe unrelated parameter made the name shadowed, the withdrawal removed the entry `Object.values(Config)` had recorded, and both exports of `src/config.ts` turned into false `unused-export` rows with an auto-fixable remove-export action. Renaming only the parameter made them disappear, and `tsc` is clean on the file. Granting instead of withdrawing also composes with #2377, which deduplicates `whole_object_uses` at the recording site: with one entry per name, removing \"the provisional one\" would have removed the only one.\n\n### A bare handover credits the whole object\n\nThe binding is registered as a namespace-import local, so a reference the visitor cannot resolve to one member (a call argument, an alias, a return value) records a whole-object use, exactly as #2377 does for `import * as X`. Without it, a consumer that writes one dotted access plus one handover narrowed to that member alone and reported every sibling the receiver still reaches:\n\n```ts\nimport Icons = require('./icons');\nconst register = (value: unknown): number => Object.keys(value as object).length;\nexport const handedOver = (): number => Icons.star + register(Icons);\n```\n\n`src/icons.ts: moon` reported, where the `import * as Icons` twin reports nothing. That false positive is created by this PR (before it, the target was simply unreachable), so it is closed here rather than left to a follow-up.\n\n`import X = Some.Namespace` keeps today's behaviour. An entity-name reference aliases a binding declared in the same file, not a module, so the require-call guard, the whole-object guard and the namespace-local registration all return before recording anything, and the qualified-name walk that credits `Some.Namespace` is untouched.\n\n## Design decisions\n\n1. **Reuse the require path, do not invent an `ImportInfo`.** The require path already carries the CommonJS mechanism, namespace narrowing, the whole-object seeds and the specifier span. A second spelling of the same edge would have to be kept in sync with all of it.\n2. **The whole-object credit, not an `ExportInfo` row, for `export import`.** The credit reaches the outcome the fix needs (every export of the target credited on an entry point that only re-exports the binding) without adding a finding class to a fix whose purpose is to remove false rows. Recording an export row for the binding would also start reporting a re-export nothing consumes as an unused export, which is a behaviour change of its own; it is left out rather than taken here.\n3. **Grant the whole-object credit in the semantic pass, never withdraw it.** Withdrawal is name-keyed and cannot tell a provisional mark from a genuine `Object.values(X)`, and #2377's deduplication leaves exactly one entry to remove. Granting is the same outcome for every unshadowed case and strictly safer for the shadowed one.\n4. **The unreferenced binding follows the ESM twin, not `const X = require()`.** The declaration is spelled `import` and TypeScript elides it like an import. Following the `const` model instead deleted rows `main` reports, which is a false negative introduced by this PR rather than a pre-existing gap it inherits.\n5. **The namespace-body call site stays, as leniency only.** `namespace N { export import X = require('./x') }` is TS1147, so no compiling project reaches it. fallow parses leniently and still has to behave, so the arm is kept and pinned by one clearly labelled extract test. The fixture covers the two spellings TypeScript accepts instead: file level, and inside a `declare module '...'` body.\n6. **Type-only is a flag on the require call, not a separate lane.** One extra bool keeps the erased spelling on the same edge, narrowing and reachability path as the unerased one, and only changes what dependency classification concludes.\n\n## Parity with the `import * as X` twin\n\nEvery shape this PR touches now produces the same findings as its ESM twin. Measured on one scratch project holding each import-equals shape next to a byte-equivalent `import * as` shape against its own target, `dead-code --no-cache --format json`, patched binary:\n\n| Shape | import-equals target | `import * as` twin target |\n|---|---|---|\n| unreferenced binding | `utils.ts`: `a1`-`a5` + `UtilsShape` | `utils-esm.ts`: `b1`-`b5` + `UtilsEsmShape` |\n| unreferenced `import type` binding | `shapes.ts`: `shapeValue` + `ShapesShape` | `shapes-esm.ts`: `shapeEsmValue` + `ShapesEsmShape` |\n| exported form, genuine whole-object use, shadowed name | `config.ts`: nothing | `config-esm.ts`: nothing |\n| dotted access plus bare handover | `icons.ts`: nothing | `icons-esm.ts`: nothing |\n| entry-point re-export with a type export | `re.ts`: `ReShape` | `re-esm.ts`: `ReEsmShape` |\n\nThe same project on the pre-review revision of this branch, rebased onto the same `main`, reported `config.ts: alpha`, `config.ts: beta` and `icons.ts: moon` as false positives and was missing `utils.ts: a1`-`a5`, `shapes.ts: shapeValue` and `shapes.ts: ShapesShape`.\n\nOne asymmetry remains, listed below.\n\n## Behavior change\n\nUnused-file and unused-export findings decrease for repositories using the form. A file that becomes reachable for the first time has its exports narrowed against the members the consumer writes, so a sibling nothing accesses surfaces as an unused export where the unused-file row used to stand in its place.\n\nA binding re-exported through `export import` credits its whole target instead of narrowing to the members the declaring file happens to read. Narrowing there was unsound: a consumer holding the re-exported module object can reach any member of the target. This matches what `export { X }` already credits for a namespace import, and it is what removes the entry-point false positives above.\n\nA type-only devDependency reached only through `import type X = require('pkg')` is no longer reported as production usage.\n\nTotal finding count is not guaranteed to decrease. The edge is now visible to every check that reads imports, so an import-equals whose specifier does not resolve reports `unresolved-import`, and a bare specifier no manifest lists reports `unlisted-dependency`, exactly as the equivalent `import X from './x'` already did.\n\n## Known deviation\n\n- **No export row for the re-exported binding.** `export import X = require('./x')` records no export for `X` itself, so a re-export nothing consumes is not reported, where the `import * as X; export { X }` twin does report `X` as an unused export. Verified on the patched binary with a non-entry consumer: the twin reports `src/mid-esm.ts: ReEsm`, the import-equals form reports nothing. A miss, never a false positive, and it matches `main`. Worth a follow-up, not taken here.\n\nThe `export = <binding>` deviation listed on the previous revision no longer reproduces: with #2377 on `main`, `import T = require('./t'); export = T;` and its `import * as T` twin both credit the target in full, and both report nothing.\n\n## Cache invalidation\n\n- extract `CACHE_VERSION` 278 to 279: extraction records a require call, its type-only flag, a type/value binding classification, an unused-binding verdict and a whole-object use it did not record before, so a warm 278 module replays without them. `CachedRequireCall` grows by the flag, and its size assertion moves from 88 to 96.\n- `GRAPH_CACHE_VERSION` 42 to 43: the edge and the references it credits are baked into the persisted graph.\n\nBoth are exactly one above `main` at 969689e6d. `DUPES_CACHE_VERSION` is untouched.\n\nWarm-cache proof on the scratch parity project, against a binary built from that same `main` commit:\n\n1. `main` binary (extract 278, graph 42) cold run writes `.fallow`: `src/config.ts`, `src/icons.ts`, `src/shapes.ts` and `src/utils.ts` reported as unused files, plus every ESM twin's rows.\n2. Patched binary on that warm cache: the fixed verdict, byte-identical to its own cold `--no-cache` run.\n3. Second patched run on the now 279/43 cache: identical to step 2.\n\n## How it was tested\n\n- Extract unit tests: the require call and its local binding, the require and specifier spans, member accesses through the binding, the type and value classification of the binding (both together and type-only), the unused-import-binding verdict on an unreferenced binding and on the erased `import type` spelling (each against its own `import * as` twin, with a referenced binding as the positive control), the exemption of the exported form (against its `import * as X; export { X }` twin), the type-only flag on the erased spelling against the unerased one, the require call from inside a `declare module` body, the whole-object credit for the file-level exported form, its withholding when the name is shadowed (with an unshadowed positive control), the survival of a genuine `Object.values(X)` under a shadowed name plus a one-name-one-entry assertion, the bare-handover whole-object use against its twin with a dotted-only negative control, and negative controls pinning that an unexported binding, an ambient-module member and an entity-name import-equals record no whole-object use. One test is labelled as a deliberate lenient-parse pin for the TS1147 namespace-body spelling.\n- Integration tests on fixture `issue-2365-import-equals`: reachability through the binding, value narrowing and type narrowing each held against an equivalent `import * as` twin declared with the same shape, object destructuring off the binding held against its twin, a whole-object use crediting the target's own `export *` chain, the entry-point `export import` form held against the `import * as X; export { X }` twin, an unreferenced binding crediting neither exports nor types (held against its twin, reachability asserted first so the assertion cannot hold vacuously), a genuine whole-object use surviving a shadowed exported binding (held against its twin), the `declare module` body crediting its package, the type-only devDependency held against its `import type * as` twin with an unerased devDependency as the positive control, and the entity-name negative control.\n- Mutation matrix for this revision's three source changes, applied one at a time to the committed tree and restored afterwards: neutralising the unreferenced-binding report fails `unreferenced_import_equals_reports_an_unused_import_binding` and `an_unreferenced_import_equals_binding_credits_nothing`; restoring the push-then-withdraw shape fails `a_genuine_whole_object_use_survives_a_shadowed_export_import_equals` and `a_shadowed_export_import_equals_keeps_a_genuine_whole_object_use` (which reports `[\"shadowedAlpha\", \"shadowedBeta\"]`, the defect verbatim); dropping the namespace-local registration fails `a_bare_import_equals_reference_is_a_whole_object_use`. `exported_import_equals_is_not_an_unused_import_binding` and the other negative controls pass either way by design, since they pin behaviour the fix must preserve. The full suite is green with every hunk restored.\n- Real projects, `main` at 969689e6d versus patched, `dead-code --no-cache --format json`. `viz-frontend` (0), `editors/vscode` (1), `vitest` (810), `rijkshuisstijl-community` (234), `vue-core` (160) and `next.js` (24272) are identical row for row; the two large ones differ only in a `next_steps` recommendation, which reads local run history rather than the analysis. On the TypeScript compiler repository, which carries the form throughout its test corpus, `unused_files`, `unused_exports`, `unused_types` and every other finding list are identical; only `unresolved_imports` (1453 to 1693) and `unlisted_dependencies` (201 to 300) move. Every new row sits on an `import X = require(...)` line in `tests/baselines/reference/**`, where a TypeScript emit baseline embeds the original source next to the emitted JavaScript and the specifier resolves to nothing, so an `import X from './foo'` on the same line would already report the same way.\n- Rebased onto `main` at 969689e6d, which carries #2377. Conflicts in `CHANGELOG.md`, `crates/extract/src/cache/types.rs`, `crates/graph/src/cache/mod.rs` and `crates/extract/src/visitor/mod.rs` were resolved by hand, keeping both sides and stacking the cache bumps one above `main`. The `assert_cached_type_size!(CachedRequireCall, 96)` assertion still holds after the rebase.\n- Gates on the rebased tree: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --lib --bins --tests --examples`, `cargo check --workspace --benches`, `RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps --document-private-items`, `typos`, `scripts/scan-hidden-unicode.py --mode committed --staged`, `scripts/check-comment-quality.mjs --staged`. No serde or schemars type changed, so no generated contract surface is implicated.\n\nFixes #2365",
          "timestamp": "2026-08-24T05:17:45Z",
          "url": "https://github.com/fallow-rs/fallow/commit/8eb190505e6a310544e897eb841dc398dac73e36"
        },
        "date": 1787554129507,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 573,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30087,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "a2931f099f4f8a8088dd7be40b46c7c0e8aa33e8",
          "message": "chore: prepare v3.18.0 post-release sync",
          "timestamp": "2026-08-25T05:34:32Z",
          "url": "https://github.com/fallow-rs/fallow/commit/a2931f099f4f8a8088dd7be40b46c7c0e8aa33e8"
        },
        "date": 1787640107434,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 574,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30110,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bartwaardenburg@gmail.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "da6a0486f88623d045799b47ad9b13faed04c362",
          "message": "Merge pull request #2434 from fallow-rs/fix/similar-code-review-findings\n\nfix: harden similar-code evidence and companion verification",
          "timestamp": "2026-08-25T22:15:40Z",
          "url": "https://github.com/fallow-rs/fallow/commit/da6a0486f88623d045799b47ad9b13faed04c362"
        },
        "date": 1787726623505,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 574,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30110,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "9406571ba1749fc34c0a516720c9fb167ed7a233",
          "message": "chore(napi): sync transitive similar-code platform pins to v3.19.0",
          "timestamp": "2026-08-27T13:13:00Z",
          "url": "https://github.com/fallow-rs/fallow/commit/9406571ba1749fc34c0a516720c9fb167ed7a233"
        },
        "date": 1787851253490,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 574,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30110,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "ef4231886a47d810896a6d0d1ed5a0f0c0eaa2b5",
          "message": "chore(mcp): sync the MCP Registry card to v3.20.0",
          "timestamp": "2026-08-28T03:33:56Z",
          "url": "https://github.com/fallow-rs/fallow/commit/ef4231886a47d810896a6d0d1ed5a0f0c0eaa2b5"
        },
        "date": 1787940516673,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 574,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30110,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Daniel Morales",
            "username": "PrinceD96",
            "email": "53633741+PrinceD96@users.noreply.github.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "df15924cb3ace9155aa431625f7e151d445617a6",
          "message": "fix(health): attribute Istanbul coverage to the function that owns the position (#2449)\n\nIstanbul coverage now reaches the functions whose extracted position falls\nbetween the producer's declaration and its body: a class member carrying a\ndecorator and a wrapped parameter list, and the innermost arrow of a curried\nchain formatted one per line. The header span identifies those, and it is read\nonly when exactly one anonymous record covers the position and no other\nfunction is declared inside it.\n\nAttribution is tightened at the same time. A member whose parameter list holds\na function no longer reports that function's coverage, a private class member\ntakes the static estimate rather than the coverage of whatever encloses it, and\na named function expression is resolved against the real source rather than a\nguess at the keyword's width. Coverage maps with project-relative keys join\nfrom any working directory, and the fallbacks are bounded by line indexes so a\nmap that does not join no longer costs a full scan per function.\n\nCloses #2448\n\nThanks to @PrinceD96 for the report and the implementation.",
          "timestamp": "2026-08-29T06:01:05Z",
          "url": "https://github.com/fallow-rs/fallow/commit/df15924cb3ace9155aa431625f7e151d445617a6"
        },
        "date": 1788005528591,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 574,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30110,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "900d29a76e7cb1488930b2fa71f7053ead69b187",
          "message": "docs(quality-gates): list the install the NAPI gate step needs (#2462)\n\n`verify:full` ends with a local NAPI build whose `napi` binary comes from\n`crates/napi`'s own devDependencies. The documented setup covered the root, the\ntype-aware sidecar, and the VS Code extension, so a checkout that followed it\npassed every earlier gate and stopped at the last step with\n`napi: command not found`.",
          "timestamp": "2026-08-30T09:54:02Z",
          "url": "https://github.com/fallow-rs/fallow/commit/900d29a76e7cb1488930b2fa71f7053ead69b187"
        },
        "date": 1788088005631,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 574,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30110,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "committer": {
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg",
            "email": "bart@waardenburg.dev"
          },
          "id": "d5a422c6206d00c813fe7c77d5038855d1051780",
          "message": "fix(telemetry): stop asserting a lock release is instantly visible\n\n`spool_lock_excludes_concurrent_acquire` demanded that the post-drop reacquire\nsucceed on its first attempt. Dropping the holder closes the descriptor, but\nthe kernel does not promise the release is visible to the next `flock` right\naway, and under a loaded parallel workspace run it measurably is not. Refs\n#2460.\n\nThe diagnostic added in #2459 is what pinned this down. The failure arrives as\n`Contended`, not `Unusable`, so the lock file opened fine and the lock was\nsimply still held a moment after its holder was gone. That rules out the\nenvironment explanations (a missing directory, a permissions denial, a\ndescriptor limit) and leaves release visibility, which is a property of the\nplatform rather than of this code.\n\nRetrying is not a mask, because production never needed the guarantee the test\nwas asserting. Both callers of `try_acquire`, the over-cap trim and the drain,\ntreat contention as \"skip, the next run picks it up\". A release that becomes\nvisible a few milliseconds later costs nothing there. What still matters is\nthat the lock does come free once its holder is gone, and that is still\nasserted: the test fails if it never reacquires across the full window.\n\nThe sibling assertion, that a second acquire contends while the first is held,\nis unchanged and still immediate, since that direction has no visibility delay\nto absorb.",
          "timestamp": "2026-08-31T12:28:52Z",
          "url": "https://github.com/fallow-rs/fallow/commit/d5a422c6206d00c813fe7c77d5038855d1051780"
        },
        "date": 1788179466023,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Agreement Rate",
            "value": 1.8,
            "unit": "%"
          },
          {
            "name": "Agreed Issues",
            "value": 574,
            "unit": "issues"
          },
          {
            "name": "Fallow Total",
            "value": 30110,
            "unit": "issues"
          },
          {
            "name": "Knip Total",
            "value": 2003,
            "unit": "issues"
          },
          {
            "name": "fastify Agreement",
            "value": 4.9,
            "unit": "%"
          },
          {
            "name": "next.js Agreement",
            "value": 1.7,
            "unit": "%"
          },
          {
            "name": "preact Agreement",
            "value": 4.4,
            "unit": "%"
          },
          {
            "name": "query Agreement",
            "value": 0,
            "unit": "%"
          },
          {
            "name": "svelte Agreement",
            "value": 0.3,
            "unit": "%"
          },
          {
            "name": "vite Agreement",
            "value": 6.4,
            "unit": "%"
          },
          {
            "name": "vue-core Agreement",
            "value": 23.4,
            "unit": "%"
          },
          {
            "name": "zod Agreement",
            "value": 1.9,
            "unit": "%"
          }
        ]
      }
    ]
  }
}