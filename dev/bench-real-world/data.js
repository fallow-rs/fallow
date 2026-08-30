window.BENCHMARK_DATA = {
  "lastUpdate": 1788093657461,
  "repoUrl": "https://github.com/fallow-rs/fallow",
  "entries": {
    "Fallow Real-World Benchmarks": [
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
          "id": "d177bab8546290ca50321e3a8ab16d02ca74d456",
          "message": "fix(core): serialise trace output PathBufs with serde_path forward-slash (#585)\n\nAttach #[serde(serialize_with = \"serde_path::serialize\")] to every single-PathBuf field and serialize_vec to every Vec<PathBuf> field in the trace output structs (ExportTrace, ExportReference, ReExportChain, FileTrace, TracedReExport, DependencyTrace, CloneTrace). After PR #584 fixed path_matches so the lookup succeeded on Windows, the output still serialised backslash-separated paths via serde's default, breaking JSON consumers (MCP agents, CI glob filters, downstream pipelines) that expect forward-slash. CloneInstance.file already used this convention; trace structs now match.\n\nTwo cross-platform regression tests build a backslash-shaped PathBuf literal and assert the JSON contains the forward-slash form for every newly-decorated field.\n\nFixes the remaining MCP e2e e2e_trace_export_returns_json and e2e_trace_file_returns_json failures.\n\nRefs #561",
          "timestamp": "2026-05-22T08:43:06Z",
          "url": "https://github.com/fallow-rs/fallow/commit/d177bab8546290ca50321e3a8ab16d02ca74d456"
        },
        "date": 1779445612576,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 136,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 133,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 242,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 239,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 132,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 138,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 450,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 397,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 999,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 911,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 914,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 904,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 756,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 664,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 8107,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 7042,
            "unit": "ms"
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
          "id": "f92318a75a2aee0732382d8069d8366931f01572",
          "message": "fix(tanstack): suppress Start virtual modules\n\nTanStack Start imports manifest and injected-head script modules through framework-provided virtual specifiers with a :v suffix. Those specifiers are not npm packages, but the TanStack plugin did not register them as virtual modules, so dead-code analysis reported them as unlisted dependencies.\n\nRegister the colon-suffixed TanStack Start virtual module prefixes through the existing plugin virtual-prefix hook. Add plugin-gated positive and negative coverage, including unresolved-import suppression and end-to-end analysis fixtures for static and dynamic imports.\n\nFixes #636.",
          "timestamp": "2026-05-23T07:33:06Z",
          "url": "https://github.com/fallow-rs/fallow/commit/f92318a75a2aee0732382d8069d8366931f01572"
        },
        "date": 1779527180055,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 151,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 143,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 260,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 240,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 145,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 130,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 446,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 379,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1194,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1160,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 891,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 874,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 786,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 710,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 7728,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 6880,
            "unit": "ms"
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
        "date": 1779614608573,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 170,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 146,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 262,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 223,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 141,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 127,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 433,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 356,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1090,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 955,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 852,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 893,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 755,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 674,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 7470,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 6742,
            "unit": "ms"
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
          "id": "04fc48077af977a27894861d8db1a7c4243c4640",
          "message": "fix: recognize Danger and Stryker tooling configs\n\n* chore: open issue #618 implementation branch\n\n* fix: recognize Danger and Stryker tooling configs",
          "timestamp": "2026-05-25T11:08:53Z",
          "url": "https://github.com/fallow-rs/fallow/commit/04fc48077af977a27894861d8db1a7c4243c4640"
        },
        "date": 1779707581524,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 133,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 131,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 247,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 229,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 132,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 126,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 436,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 350,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1104,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 997,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 904,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 918,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 759,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 679,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 7389,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 6727,
            "unit": "ms"
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
          "id": "e581961b5f8f1a63708017c4aeeb8beb299e855a",
          "message": "docs(coverage): correct stable_id cross-surface framing\n\nA thorough end-to-end smoke (real published 0.3.0 sidecar) showed stable_id is NOT immune to a function moving lines: function_identity_id hashes start_line, so a moved function gets a new stable_id (verified: coldFn fallow:fn:de5223fd@2 -> fallow:fn:62a6be2a@5, and the finding resurfaced against a saved baseline).\n\nThe shipped #506 docs claimed stable_id 'survives line moves' and that baselines keyed on it 'keep suppressing after a function moves lines'. That is false. Correct the framing in CHANGELOG, the --explain text (explain.rs), the baseline.rs writer/reader comments, and the RuntimeCoverageFinding.stable_id doc to describe the ACTUAL property: stable_id is a cross-surface (one value across findings/hot-paths/blast-radius/importance; the per-finding id uses a per-surface salt) and cross-producer (V8/Istanbul/oxc agree, columns excluded) join key. Like id, it changes when file/name/start_line change.\n\nCode behavior is unchanged; this is a documentation accuracy fix. Schema + VS Code/npm TS contracts regenerated. Refs #506.",
          "timestamp": "2026-05-27T10:24:58Z",
          "url": "https://github.com/fallow-rs/fallow/commit/e581961b5f8f1a63708017c4aeeb8beb299e855a"
        },
        "date": 1779879599520,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 179,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 157,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 254,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 233,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 145,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 133,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 459,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 375,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1302,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1265,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 960,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 925,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 828,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 749,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 8387,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 7524,
            "unit": "ms"
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
        "date": 1779965779722,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 166,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 145,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 260,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 240,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 144,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 128,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 490,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 402,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1355,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1175,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 933,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1024,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 833,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 761,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 8591,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 7401,
            "unit": "ms"
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
          "id": "9af2175229a48f92e08f38e0a03eddbe47792a25",
          "message": "feat(config): warn when multiple config files coexist in one directory (#780)\n\nfind_and_load picks the first of .fallowrc.json > .fallowrc.jsonc >\nfallow.toml > .fallow.toml per directory. Previously a stale lower-precedence\nconfig (left over from a migration or a partial fallow init) was silently\nshadowed, so output looked correct but came from the wrong source.\n\nNow config discovery emits a deduped tracing::warn! (visible on stderr at the\ndefault level) naming the loaded file and the lower-precedence file(s) it\nignored, mirroring the existing warn_on_unknown_rule_keys path (process-wide\nOnceLock dedupe keyed on the canonical directory, thread-local test capture\nwith capture-before-dedupe). It fires once per directory per run; an explicit\n--config <path> performs no discovery and never warns.\n\nDocs and the fallow config help text now state that .fallowrc.json accepts\nJSONC and .fallowrc.jsonc is identical (the extension is only an editor hint),\nand document the first-match-wins precedence ladder.\n\nCloses #458",
          "timestamp": "2026-05-29T10:35:55Z",
          "url": "https://github.com/fallow-rs/fallow/commit/9af2175229a48f92e08f38e0a03eddbe47792a25"
        },
        "date": 1780051741808,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 144,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 146,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 278,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 251,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 151,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 136,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 459,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 405,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1265,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1137,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 947,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1017,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 821,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 757,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 8375,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 7656,
            "unit": "ms"
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
        "date": 1780132763233,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 165,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 154,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 257,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 258,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 144,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 130,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 453,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 384,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1302,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1158,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 929,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 900,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 816,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 741,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 7534,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 6797,
            "unit": "ms"
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
        "date": 1780221156292,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 144,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 134,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 269,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 259,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 147,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 134,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 471,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 389,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1255,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1236,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 924,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 925,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 835,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 752,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 8154,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 7631,
            "unit": "ms"
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
          "id": "d57b9ba37630c6b5f9cf748b6a25ae3bb9a6c6bb",
          "message": "refactor(plugins): type config path parsing\n\nPath-shaped values extracted from JavaScript and TypeScript config files now flow through typed PathBuf helpers instead of plain strings. The parser keeps plugin output in forward-slash string form at the boundary, while webpack, Nuxt, Vite, SvelteKit, TypeScript, Wrangler, and Docusaurus consume filesystem paths internally where appropriate.\n\nThis keeps package-style alias semantics out of scope and preserves the existing PluginResult contract. Regression coverage now exercises mixed separators, project-root-style leading slashes, imported alias spread kind preservation, webpack context entries, and Nuxt srcDir normalization.\n\nFixes #448.",
          "timestamp": "2026-06-01T11:39:26Z",
          "url": "https://github.com/fallow-rs/fallow/commit/d57b9ba37630c6b5f9cf748b6a25ae3bb9a6c6bb"
        },
        "date": 1780317511063,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 140,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 137,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 264,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 238,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 148,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 133,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 426,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 379,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1277,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1058,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 930,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1028,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 802,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 729,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 7875,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 7701,
            "unit": "ms"
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
          "id": "1c8319d890a2c984d3ef6dd67aaeca79fa1a284c",
          "message": "chore: release v2.86.0",
          "timestamp": "2026-06-02T11:00:50Z",
          "url": "https://github.com/fallow-rs/fallow/commit/1c8319d890a2c984d3ef6dd67aaeca79fa1a284c"
        },
        "date": 1780399306174,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 185,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 175,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 326,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 302,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 164,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 141,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 532,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 478,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1045,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1111,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 800,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 774,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 695,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 613,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 7308,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 6836,
            "unit": "ms"
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
          "id": "a45deb010be1b521c33ab78f8e3be4106658c183",
          "message": "fix: credit bare pnpm script binaries\n\nCredit dependency usage for package scripts, workspace scripts, and CI run blocks that invoke a declared package binary through bare `pnpm <binary>`. The parser keeps its syntax-only public path conservative, while the analysis path now receives dependency and script-name context before crediting pnpm implicit execution.\n\nKeep pnpm script delegation and built-in commands out of dependency credit, including local script names that were filtered out of production-mode analysis. This avoids trading the envinfo false positive for new false negatives around `pnpm build`, `pnpm lint`, `pnpm test`, `pnpm start`, `pnpm install`, `pnpm audit`, and `pnpm add`.\n\nFixes #914.",
          "timestamp": "2026-06-03T10:59:58Z",
          "url": "https://github.com/fallow-rs/fallow/commit/a45deb010be1b521c33ab78f8e3be4106658c183"
        },
        "date": 1780488007511,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 144,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 141,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 277,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 234,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 149,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 131,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 437,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 404,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1324,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1084,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 931,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 955,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 861,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 773,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 7611,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 6824,
            "unit": "ms"
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
          "id": "b740cf1a817c8d9bc8a7498f61e0e404f71a4ba4",
          "message": "fix(vscode): align provider and duplication behavior\n\nAdd a VS Code plugin that credits provider interface methods when a class implements the matching VS Code provider interface.\n\nForward VS Code duplication settings into LSP initialization options so live diagnostics match sidebar CLI analysis.\n\nHarden VS Code LSP restart handling for rapid config changes during client startup.\n\nFixes #948.\nFixes #905.",
          "timestamp": "2026-06-04T10:33:57Z",
          "url": "https://github.com/fallow-rs/fallow/commit/b740cf1a817c8d9bc8a7498f61e0e404f71a4ba4"
        },
        "date": 1780569897643,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 151,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 144,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 272,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 154,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 142,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 486,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1366,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1274,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1003,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 955,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 946,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 829,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 8766,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 8067,
            "unit": "ms"
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
          "id": "3fa9bc58bde716adda65d24bd66cdfb4af886d23",
          "message": "fix(mcp): gate unix-only test helpers",
          "timestamp": "2026-06-05T10:24:48Z",
          "url": "https://github.com/fallow-rs/fallow/commit/3fa9bc58bde716adda65d24bd66cdfb4af886d23"
        },
        "date": 1780656636338,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 141,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 132,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 272,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 231,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 148,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 135,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 424,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 368,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1297,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1126,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 914,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 954,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 839,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 773,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 7469,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 7199,
            "unit": "ms"
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
          "id": "e665f88427a70751a7c0b7cecc8f7379d00d3da4",
          "message": "fix(cli): name canonical `fallow dead-code` in user-facing messages (#1011)\n\nSeveral user-facing messages still told users to run the deprecated `fallow check` alias (which already prints a deprecation warning). The fix skip notes, migrate caveat, regression-baseline hint, and fix MCP tool descriptions now reference `fallow dead-code`. Internal doc comments updated to match; the `check` alias keeps working.",
          "timestamp": "2026-06-06T09:14:42Z",
          "url": "https://github.com/fallow-rs/fallow/commit/e665f88427a70751a7c0b7cecc8f7379d00d3da4"
        },
        "date": 1780738160202,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 152,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 143,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 292,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 253,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 163,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 143,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 487,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 393,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1320,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1142,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 959,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 939,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 889,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 800,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 7565,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 7451,
            "unit": "ms"
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
          "id": "f7161395e92145f1e673fa5b9d99bf52e60ec28e",
          "message": "fix: tighten security source matching\n\nTighten framework-source detection and package-subpath provenance for the security catalogue follow-up.",
          "timestamp": "2026-06-07T09:20:22Z",
          "url": "https://github.com/fallow-rs/fallow/commit/f7161395e92145f1e673fa5b9d99bf52e60ec28e"
        },
        "date": 1780826722483,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 196,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 175,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 315,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 256,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 162,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 139,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 501,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 428,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1360,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1327,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 973,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 931,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 896,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 802,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 7998,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 7173,
            "unit": "ms"
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
        "date": 1780919967278,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 162,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 158,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 369,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 316,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 185,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 162,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 613,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 556,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1156,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1009,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 898,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 897,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 757,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 685,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 8180,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 7550,
            "unit": "ms"
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
          "id": "aceeecd930028947fd21302a3fa83b59cfb671c0",
          "message": "feat(telemetry): add outcome buckets\n\nRecord coarse result-count buckets and review/report truncation fields in workflow telemetry without exposing exact counts, paths, rule ids, finding names, or snippets.\n\nWire the bucket helpers from final command results, preserve the existing findings_present and failure_reason contracts, and document the inspect-mode payload.\n\nFixes #1080.",
          "timestamp": "2026-06-09T10:27:46Z",
          "url": "https://github.com/fallow-rs/fallow/commit/aceeecd930028947fd21302a3fa83b59cfb671c0"
        },
        "date": 1781001613723,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 182,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 175,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 282,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 257,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 162,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 144,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 500,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 397,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1383,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1218,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 977,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 959,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 904,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 818,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 7697,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 7229,
            "unit": "ms"
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
          "id": "47a0e4cfd291e8203480fba8ae1dc6edda00f906",
          "message": "fix: credit napi-rs optional prebuild packages\n\nnapi-rs packages declare generated platform prebuilds as optionalDependencies, but their runtime loader selects them dynamically instead of through static imports. Fallow now reads package.json napi metadata and credits only exact generated package names listed in the same optionalDependencies map.\n\nThe plugin registry now has a package.json metadata hook that runs for both root and workspace packages. These credits are scoped to the declaring package.json, so unrelated sibling workspace dependencies remain reportable.\n\nFixes #1164.",
          "timestamp": "2026-06-10T10:24:02Z",
          "url": "https://github.com/fallow-rs/fallow/commit/47a0e4cfd291e8203480fba8ae1dc6edda00f906"
        },
        "date": 1781089210859,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 237,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 222,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 425,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 398,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 262,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 231,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 1096,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 715,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1730,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1600,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1222,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1118,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1037,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 946,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 12431,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 11954,
            "unit": "ms"
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
          "id": "1f2bf7070b2650dd2e613fe0b84df61f7363a741",
          "message": "fix(cli): clear test-only-dependency findings under single-type filters (#1194)\n\nIssueFilters::apply() clears every issue category not selected by a single-type filter flag, but the --unused-deps clear arm omitted test_only_dependencies, so a focused run like `fallow dead-code --unused-files` on a project with a production dependency imported only from test files leaked that test-only finding alongside the requested issue type.\n\nThis groups test-only-dependency with the other dependency kinds under --unused-deps (matching type-only and the --file scope, which already cleared all five categories), sets filter_flag to --unused-deps for the test-only-dependency row in the capability manifest, regenerates the SKILL.md issue-types table, and adds a neuter-verified filter-parity regression test.\n\nFixes #1192.",
          "timestamp": "2026-06-11T10:56:25Z",
          "url": "https://github.com/fallow-rs/fallow/commit/1f2bf7070b2650dd2e613fe0b84df61f7363a741"
        },
        "date": 1781177310774,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 243,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 220,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 490,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 397,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 261,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 242,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 692,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 628,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1667,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1480,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1140,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1116,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1030,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 911,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 11637,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 11034,
            "unit": "ms"
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
        "date": 1781262467697,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 295,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 270,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 544,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 440,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 269,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 246,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 836,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 716,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1731,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1586,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1313,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1278,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1200,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1066,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 13445,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 12619,
            "unit": "ms"
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
          "id": "e96ae8e67c33c9b923483e9827424c93db2e4bab",
          "message": "feat(security): flag use-client cones that reach server-only code (#1231)\n\nStage 2 of the Next.js RSC differentiated-detection program.\n\nExtends the opt-in `fallow security` `client-server-leak` rule (default `off`) with a second sink predicate: a `\"use client\"` file whose transitive static-import cone reaches **server-only code**, emitted as a distinct `server-only-import` candidate category on the same rule, suppress kind, and finding shape. fallow catches this without requiring the `server-only` poison package and before a build (Next.js only errors at build time when the marker is present).\n\n- **Narrow sink set** (FP-conservative, no DB-client heuristic): a `\"use server\"` module, a `server-only` import, or a named server-only API (`next/headers` `cookies`/`headers`/`draftMode`, `next/server`, node `fs`/`child_process`, both `node:` and bare forms).\n- **`next/dynamic(..., { ssr: false })` aware**: a server module reached only through the sanctioned client-only dynamic import is not a leak. The extract layer captures those import spans on `ModuleInfo.client_only_dynamic_import_spans` (CACHE_VERSION bump) and the BFS skips an edge reached only through them.\n- **Direct case**: a `\"use client\"` file that itself imports a server-only sink is reported with a single self-hop trace; the transitive emit is gated so it reports once.\n- Opt-in and candidate-framed (never a verified vulnerability); `security_findings` stays out of bare `fallow` / `audit`. `SecuritySchemaVersion` bumped to V7 since `client-server-leak` findings can now carry the `server-only-import` category.\n\nTeam review: rust, json-output, mcp reviewers (zero BLOCKs); the direct-case coverage gap, the V7 schema bump, stale doc/schema descriptions, a misleading fixture comment, and thin sink-predicate fixtures were all addressed with new tests. Full workspace test, clippy, fmt, doc, codegen, and the security smoke (10 findings, `schema_version: 7`, zero under bare `fallow`) green.",
          "timestamp": "2026-06-13T09:39:29Z",
          "url": "https://github.com/fallow-rs/fallow/commit/e96ae8e67c33c9b923483e9827424c93db2e4bab"
        },
        "date": 1781344953082,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 290,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 226,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 481,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 398,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 263,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 235,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 750,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 671,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1696,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1454,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1331,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1246,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1147,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1033,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 12393,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 11302,
            "unit": "ms"
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
          "id": "d0f5b42e4588b0452eb5859c3578500a5695d05d",
          "message": "feat(nextjs): graduate route-collision to default error, keep dynamic-segment-name-conflict at warn\n\nroute-collision now defaults to error (it mirrors a next build failure, so a project hitting it was already red); dynamic-segment-name-conflict stays warn (a runtime crash next build does NOT catch) and graduates to error in a later release once field-proven.\n\nAlso corrects the dynamic-segment-name-conflict rustdoc (it wrongly claimed the build fails), rewrites the human and markdown conflict line to be crash-grade, and adds a monorepo-gate regression test proving the rule arms when next is declared only in a sub-app. Regenerated schema.json and re-accepted SARIF snapshots (route-collision rule level warning to error). No JSON schema or TS contract change.",
          "timestamp": "2026-06-14T09:32:57Z",
          "url": "https://github.com/fallow-rs/fallow/commit/d0f5b42e4588b0452eb5859c3578500a5695d05d"
        },
        "date": 1781433036932,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 243,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 235,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 421,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 392,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 260,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 229,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 667,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 607,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1589,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1478,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1340,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1230,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1127,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 992,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 12214,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 11186,
            "unit": "ms"
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
          "id": "2f636206b0f16edd8cac2f493331f2e8ba8dc322",
          "message": "fix: wire post-2.96.0 IssueKinds through LSP severity, VS Code, and CI summaries (#1262)\n\nThe cluster of new IssueKinds shipped since v2.96.0 was complete in the Rust output but under-wired in three surfaces outside the Rust drift gates.\n\nLSP: route-collision and dynamic-segment-name-conflict now emit ERROR severity to match their core default (were hardcoded WARNING), with regression tests. VS Code: the new kinds are now counted, rendered in the Issues tree, and filterable instead of silently dropped from the sidebar; dist rebuilt. CI: the five missing kinds plus route-collision and dynamic-segment-name-conflict now appear in the GitHub Action and GitLab CI summary, annotation, combined, and audit breakdowns, with jq tests added. A shared drift guard fails when a future dead-code IssueKind is absent from the summary scripts.",
          "timestamp": "2026-06-15T12:57:12Z",
          "url": "https://github.com/fallow-rs/fallow/commit/2f636206b0f16edd8cac2f493331f2e8ba8dc322"
        },
        "date": 1781529175102,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 257,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 233,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 490,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 416,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 270,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 243,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 771,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 702,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1707,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1562,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1368,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1319,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1173,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1079,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 13305,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 12535,
            "unit": "ms"
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
          "id": "d2ccd78184f74a2e6211d60d2a304cfe6182b28e",
          "message": "chore(ci): retrigger main checks",
          "timestamp": "2026-06-15T17:15:14Z",
          "url": "https://github.com/fallow-rs/fallow/commit/d2ccd78184f74a2e6211d60d2a304cfe6182b28e"
        },
        "date": 1781546144679,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 301,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 269,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 436,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 263,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 238,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 739,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 672,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1661,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1497,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1376,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1307,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1147,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1054,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 12675,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 11776,
            "unit": "ms"
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
          "id": "f2ac13530e8ae5d4c775c295b4d60e8d6567d14c",
          "message": "feat(health): React/JSX component-health suite\n\nA React/Preact component-health layer on a new JSX structural extraction pass\n(component functions, props, hooks, render edges), mirroring and extending the\nVue component-analysis stack. CACHE_VERSION 170.\n\nSignals (dep-gated on react/react-dom/next/preact):\n- unused-component-prop React arm (default warn): prop declared but read nowhere\n  in the component body; shares the Vue rule key / suppress token.\n- React-aware complexity: JSX nesting depth folds into cognitive, hook/prop\n  density into the per-component contribution; descriptive hook profile (kind\n  breakdown + max useEffect dep-array arity).\n- prop-drilling (opt-in, off): a prop forwarded unused through >= 3 pass-through\n  components; located per-chain records + small capped health penalty.\n- thin-wrapper (opt-in, off): a component whose whole body is a spread-forwarded\n  single child render.\n- duplicate-prop-shape (opt-in, off): 3+ components across 2+ files with an\n  identical significant prop-name set.\n- render fan-in: descriptive blast-radius metric (component-graph analogue of\n  module fan-in) with a located top-N list; headline is distinct render\n  locations, test/spec files excluded.\n\nThe shared ChildResolver lives in analyze/react_resolve.rs. Validated zero false\npositives across next.js, query, preact, and vrs-portals; duplicate-prop-shape\nfound 23 true positives on vrs-portals. Companion docs (fallow-docs,\nfallow-skills) updated separately.",
          "timestamp": "2026-06-16T11:53:37Z",
          "url": "https://github.com/fallow-rs/fallow/commit/f2ac13530e8ae5d4c775c295b4d60e8d6567d14c"
        },
        "date": 1781612128198,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 248,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 228,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 483,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 261,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 237,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 731,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 656,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1698,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1460,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1359,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1257,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1165,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1024,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 12588,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 11412,
            "unit": "ms"
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
          "id": "a7c8bc7de7abfc05e695aa28edd921df410b83af",
          "message": "test: improve coverage across CLI output helpers\n\nAdds focused coverage for CLI audit output, combined output helpers, cache notice lifecycle, and human report helper behavior.\n\nThe branch keeps production code unchanged and stops the coverage loop once the remaining gains became marginal.",
          "timestamp": "2026-06-17T11:10:47Z",
          "url": "https://github.com/fallow-rs/fallow/commit/a7c8bc7de7abfc05e695aa28edd921df410b83af"
        },
        "date": 1781696480543,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 253,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 235,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 442,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 430,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 264,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 238,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 775,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 652,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1746,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1515,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1404,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1323,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1184,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1083,
            "unit": "ms"
          },
          {
            "name": "next.js (cold)",
            "value": 13012,
            "unit": "ms"
          },
          {
            "name": "next.js (warm)",
            "value": 11861,
            "unit": "ms"
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
          "id": "88b0c6b8465dd6272d86f813f14e560e85430502",
          "message": "fix(ci): skip timed out benchmark projects\n\nKeep real-world benchmark runs from failing the whole workflow when one project hits the per-project watchdog; partial benchmark JSON is still produced with skip diagnostics.",
          "timestamp": "2026-06-18T15:28:02Z",
          "url": "https://github.com/fallow-rs/fallow/commit/88b0c6b8465dd6272d86f813f14e560e85430502"
        },
        "date": 1781797207840,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 512,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 817,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 714,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1738,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1433,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1532,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1329,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1230,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1124,
            "unit": "ms"
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
          "id": "eaeb0634af797a69bae7600d2020dd99b33402ad",
          "message": "chore: release v2.100.0",
          "timestamp": "2026-06-19T10:48:37Z",
          "url": "https://github.com/fallow-rs/fallow/commit/eaeb0634af797a69bae7600d2020dd99b33402ad"
        },
        "date": 1781868391044,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 412,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 512,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 513,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 920,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 817,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1744,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1637,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1531,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1435,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1332,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1232,
            "unit": "ms"
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
          "id": "64ead321071c8b461f7d479ab4a56f36eaa58ec1",
          "message": "refactor: split react prop module scan",
          "timestamp": "2026-06-20T10:06:20Z",
          "url": "https://github.com/fallow-rs/fallow/commit/64ead321071c8b461f7d479ab4a56f36eaa58ec1"
        },
        "date": 1781950110038,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 412,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 616,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 613,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1644,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1537,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1430,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1328,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1226,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1122,
            "unit": "ms"
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
          "id": "6fde7abc51cd3cc841cc981968bac245b22fce12",
          "message": "refactor: ratchet unit-size/interfacing clippy gates and bundle param clusters\n\nTighten the project's SIG-aligned unit-size and unit-interfacing gates to their next ratchet step and eliminate the resulting outliers with genuine refactors.\n\n- .clippy.toml: too_many_lines 150 to 100, too_many_arguments 7 to 6. Every production function is now under 100 lines; the 7-parameter outliers drop from 25 to 4 (frozen public/deprecated APIs with reasoned #[expect]).\n- Over-100-line production functions are split into cohesive private helpers; private 7-param functions are bundled into input/context structs (SarifCtx, HealthScanCtx, SecurityRankingInput, LoadConfigArgs, and several *Input structs).\n- Test fixtures keep their length via reasoned #[expect] rather than being fragmented.\n\nBehavior is unchanged: clippy --all-targets -D warnings clean at the new thresholds, full test suite green, output byte-identical across all formats.",
          "timestamp": "2026-06-21T09:55:42Z",
          "url": "https://github.com/fallow-rs/fallow/commit/6fde7abc51cd3cc841cc981968bac245b22fce12"
        },
        "date": 1782038251422,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 305,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 410,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 407,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 610,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 511,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1330,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1223,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1223,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1222,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1027,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 917,
            "unit": "ms"
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
        "date": 1782132949479,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 308,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 411,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 410,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 717,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 616,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1639,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1433,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1329,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1331,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1231,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1125,
            "unit": "ms"
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
          "id": "e585f058e59b78b2d64339c7d16e380fbbcfc484",
          "message": "refactor(review-app): namespace persisted state under fallow-review instead of fre\n\nRenames the review app's terse `fre` storage/env prefix to the explicit `fallow-review` namespace and drops the stopgap _typos.toml allowlist. String-literal-only; no persisted-state migration needed.",
          "timestamp": "2026-06-23T09:28:32Z",
          "url": "https://github.com/fallow-rs/fallow/commit/e585f058e59b78b2d64339c7d16e380fbbcfc484"
        },
        "date": 1782211202514,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 311,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 308,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 514,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 615,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 614,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1642,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1433,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1332,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1333,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1227,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1128,
            "unit": "ms"
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
          "id": "7de6b4ee07eb2635621af39a10e120bd8b75db8b",
          "message": "fix(config): normalize leading dot ignore patterns\n\nStrip a single leading ./ before compiling ignorePatterns and ignoreUnresolvedImports so user globs match the project-root-relative paths used by source discovery and unresolved-import filtering.\n\nAdd focused regression coverage for resolved matchers and the source walker so the silent no-match case stays fixed.\n\nFixes #1385.",
          "timestamp": "2026-06-24T09:56:51Z",
          "url": "https://github.com/fallow-rs/fallow/commit/7de6b4ee07eb2635621af39a10e120bd8b75db8b"
        },
        "date": 1782296798153,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 311,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 411,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 718,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 612,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1639,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1431,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1431,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1329,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1226,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1127,
            "unit": "ms"
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
        "date": 1782383015478,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 511,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 410,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1633,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1437,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1334,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1225,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1129,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1122,
            "unit": "ms"
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
        "date": 1782469659347,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 509,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1430,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1224,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1120,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1021,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1019,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 917,
            "unit": "ms"
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
        "date": 1782552959660,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 207,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 512,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 410,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1642,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1434,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1330,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1226,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1128,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1030,
            "unit": "ms"
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
        "date": 1782641112188,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 207,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 512,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1640,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1431,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1225,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1122,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1126,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1023,
            "unit": "ms"
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
          "id": "59b20c7565930a5cb0df3e62a3e711941b1cde69",
          "message": "fix(telemetry): note find-state for flags/watch and guard the workflow class\n\nFollow-up to #1650. flags and watch emit a code_quality_review telemetry event\n(the same workflow as combined fallow, which populates findings_present) but\nnever noted their find-state, so findings_present serialized as null. flags now\nnotes its feature-flag count and watch notes each cycle's issue count.\n\nFocused dead-code/dupes trace and impact-closure views early-return before the\nnormal note; they still run the full analysis, so they now record its result\ncount. findings_present reflects what the analysis surfaced independent of the\noutput view.\n\nAdds a structural guard: an exhaustive Workflow::surfaces_findings()\nclassification (a new workflow variant fails to compile until classified) plus a\ndebug-build invariant at the single telemetry event-emission point that fails\nfast if a finding-surfacing workflow records a non-failing event without noting\nfind-state. The guard caught the dead-code and dupes trace-mode gaps during this\nchange. No change to the telemetry payload shape.\n\nRefs #1650.",
          "timestamp": "2026-06-29T11:01:42Z",
          "url": "https://github.com/fallow-rs/fallow/commit/59b20c7565930a5cb0df3e62a3e711941b1cde69"
        },
        "date": 1782734661662,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 511,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1640,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1335,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1123,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1125,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1123,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1023,
            "unit": "ms"
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
          "id": "5c72d26939cd6f21847c0897de42f81873842184",
          "message": "fix(health): no implicit px on custom-property values in object CSS-in-JS (#1672)\n\nThe object serializer applied implicit-px to numeric custom-property values\n(`--space: 8` -> `--space:8px`), fabricating a unit the bundler never emits.\nEmotion's own serializer guards px with `!isCustomProperty(key)`\n(@emotion/serialize) and React does the same, so a numeric `--x` value stays\nunitless. Skip implicit px for `--*` properties so the lifted CSS matches the\nreal compiled output. Found while smoke-testing the emotion site's\n`<Global>` custom-property block on real public projects.",
          "timestamp": "2026-06-30T10:33:40Z",
          "url": "https://github.com/fallow-rs/fallow/commit/5c72d26939cd6f21847c0897de42f81873842184"
        },
        "date": 1782815881470,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 207,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 511,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1638,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1335,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1124,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1122,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1021,
            "unit": "ms"
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
        "date": 1782902989288,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 207,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 208,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 514,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1643,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1431,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1128,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1125,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1127,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1025,
            "unit": "ms"
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
        "date": 1782986681990,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 511,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1635,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1325,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1123,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1021,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1121,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1120,
            "unit": "ms"
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
        "date": 1783072914147,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 616,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1637,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1328,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1131,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1121,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1124,
            "unit": "ms"
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
          "id": "630722b016a5b785d6fa05ee54db6b339eb3c4fa",
          "message": "chore: release v3.0.0",
          "timestamp": "2026-07-04T08:58:59Z",
          "url": "https://github.com/fallow-rs/fallow/commit/630722b016a5b785d6fa05ee54db6b339eb3c4fa"
        },
        "date": 1783157160907,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 613,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1739,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1333,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1227,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1021,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1126,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1123,
            "unit": "ms"
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
          "id": "6b9eb819620baf9aaf892e2ec552e92eb8a63e2e",
          "message": "refactor(engine): route workspace discovery through engine",
          "timestamp": "2026-07-05T09:44:05Z",
          "url": "https://github.com/fallow-rs/fallow/commit/6b9eb819620baf9aaf892e2ec552e92eb8a63e2e"
        },
        "date": 1783244979279,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 509,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 406,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1427,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1223,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1016,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 916,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1017,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 915,
            "unit": "ms"
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
        "date": 1783337673889,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 614,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1739,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1329,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1229,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1331,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1225,
            "unit": "ms"
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
        "date": 1783420272641,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 614,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 410,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1739,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1332,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1224,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1223,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1121,
            "unit": "ms"
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
          "id": "df2f052a926c23937a359551ba771fcbe795cbe2",
          "message": "refactor(skill): split MCP catalogue to references/mcp.md; add vendored-skill drift gate (#1781)\n\nMove the MCP tool catalogue out of the always-loaded SKILL.md into references/mcp.md, add a CI gate keeping npm/fallow/skills in lockstep with canonical fallow-skills, and reconcile capabilities.json + both skill trees with the binary (adds plugin-check, fixing the stale contract bundle).",
          "timestamp": "2026-07-08T09:24:03Z",
          "url": "https://github.com/fallow-rs/fallow/commit/df2f052a926c23937a359551ba771fcbe795cbe2"
        },
        "date": 1783503194782,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 614,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1740,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1331,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1222,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1021,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1226,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1221,
            "unit": "ms"
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
          "id": "b39782a149db879383734aef60f1329e967b0317",
          "message": "docs(rules): document the PositionMapper byte-to-UTF-16 boundary convention",
          "timestamp": "2026-07-09T10:26:37Z",
          "url": "https://github.com/fallow-rs/fallow/commit/b39782a149db879383734aef60f1329e967b0317"
        },
        "date": 1783593102862,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 615,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1745,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1333,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1228,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1021,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1225,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1222,
            "unit": "ms"
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
        "date": 1783679290156,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 310,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 615,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 411,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1851,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1435,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1226,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1024,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1329,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1224,
            "unit": "ms"
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
        "date": 1783759938006,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 102,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 509,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1324,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1120,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 920,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 814,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1022,
            "unit": "ms"
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
        "date": 1783847325044,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 103,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 509,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 407,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1429,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1225,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1019,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 915,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1123,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1019,
            "unit": "ms"
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
          "id": "0f2de24d23679cca1568b85aa8028d9f4ea6ed38",
          "message": "chore: release v3.4.0",
          "timestamp": "2026-07-13T09:35:29Z",
          "url": "https://github.com/fallow-rs/fallow/commit/0f2de24d23679cca1568b85aa8028d9f4ea6ed38"
        },
        "date": 1783938433401,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 208,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 511,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1431,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1119,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 919,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 815,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1119,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1019,
            "unit": "ms"
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
          "id": "c802c2d5ddcfd6a3cee7bcceb4542a7842a34569",
          "message": "docs: document the --legacy-envelope removal and clear stale references\n\nAdds the missing v2.104.0 changelog entry for the --legacy-envelope / legacyEnvelope removal, re-vendors the skills SKILL.md without the stale flag mention, and rewrites the stale programmatic.rs bullet in the cli-crate rules (the module moved to fallow-api; napi no longer depends on fallow-cli).",
          "timestamp": "2026-07-14T08:29:28Z",
          "url": "https://github.com/fallow-rs/fallow/commit/c802c2d5ddcfd6a3cee7bcceb4542a7842a34569"
        },
        "date": 1784020274120,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 617,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1745,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1429,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1227,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1022,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1327,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1226,
            "unit": "ms"
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
          "id": "29e84905254d37b5c20577ecd31f4daba4349926",
          "message": "Merge pull request #1898 from fallow-rs/codex/fix-pnpm-audit-ci\n\nfix(ci): restore VS Code dependency audit",
          "timestamp": "2026-07-15T08:02:47Z",
          "url": "https://github.com/fallow-rs/fallow/commit/29e84905254d37b5c20577ecd31f4daba4349926"
        },
        "date": 1784107001254,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 615,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1644,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1427,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1223,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1022,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1230,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1224,
            "unit": "ms"
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
        "date": 1784193497489,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1328,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1122,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 918,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1124,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1119,
            "unit": "ms"
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
        "date": 1784279677807,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 616,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1737,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1328,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1227,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1022,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1330,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1230,
            "unit": "ms"
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
        "date": 1784364802649,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 616,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1742,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1329,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1225,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1121,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1333,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1226,
            "unit": "ms"
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
        "date": 1784452308678,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 102,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 509,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 406,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1223,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1124,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1019,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 917,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 914,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 815,
            "unit": "ms"
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
          "id": "dacea3780abcf5e2c5d98ac208a88a4cdeb3529e",
          "message": "fix: inherited-member (#1910) and tsconfig-alias (#1911) false positives\n\nTwo dead-code false-positive fixes: credit members reached through an inherited/generic base-class property (#1910), and activate the TypeScript plugin on tsconfig presence so paths aliases are not misreported as unlisted dependencies (#1911).\n\nCloses #1910\nCloses #1911",
          "timestamp": "2026-07-20T09:21:52Z",
          "url": "https://github.com/fallow-rs/fallow/commit/dacea3780abcf5e2c5d98ac208a88a4cdeb3529e"
        },
        "date": 1784541728567,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 614,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1746,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1328,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1228,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1124,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1327,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1226,
            "unit": "ms"
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
        "date": 1784626442170,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 614,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1640,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1428,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1228,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1022,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1328,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1224,
            "unit": "ms"
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
          "id": "ec6dca830f375ac5a0200ab2f417f160bb42bf0e",
          "message": "chore(deps): bump lucide-react in /apps/review-electron (#1978)\n\nBumps [lucide-react](https://github.com/lucide-icons/lucide/tree/HEAD/packages/lucide-react) from 1.21.0 to 1.24.0.\n- [Release notes](https://github.com/lucide-icons/lucide/releases)\n- [Commits](https://github.com/lucide-icons/lucide/commits/1.24.0/packages/lucide-react)\n\n---\nupdated-dependencies:\n- dependency-name: lucide-react\n  dependency-version: 1.24.0\n  dependency-type: direct:production\n  update-type: version-update:semver-minor\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>\nCo-authored-by: dependabot[bot] <49699333+dependabot[bot]@users.noreply.github.com>",
          "timestamp": "2026-07-22T09:19:28Z",
          "url": "https://github.com/fallow-rs/fallow/commit/ec6dca830f375ac5a0200ab2f417f160bb42bf0e"
        },
        "date": 1784712818717,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 616,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1639,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1330,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1226,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1021,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1331,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1226,
            "unit": "ms"
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
        "date": 1784799079523,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 207,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 617,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 511,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1846,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1433,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1328,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1123,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1329,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1329,
            "unit": "ms"
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
        "date": 1784885181323,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 614,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1743,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1429,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1231,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1123,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1327,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1224,
            "unit": "ms"
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
        "date": 1785057553777,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 614,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1744,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1332,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1234,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1024,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1328,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1225,
            "unit": "ms"
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
          "id": "20972d541b1deadad79108d780ffae6fd9c48163",
          "message": "feat: add stable type-aware TypeScript analysis\n\n* chore: open type-aware proof of concept branch\n\n* feat: prototype type-aware class member refinement\n\n* fix: harden type-aware proof of concept\n\n* fix: cover type-aware sidecar in git hook\n\n* fix: keep unpublished flags out of agent docs\n\n* feat: mature type-aware class member refinement\n\n* fix: harden type-aware refinement gates\n\n* test: isolate case-sensitive sidecar projects\n\n* fix: satisfy Windows process tree lint\n\n* fix: harden type-aware release evidence\n\n* fix: defer type-aware corpus dependencies\n\n* test: isolate optional sidecar dependencies\n\n* feat: add project-wide type-aware analysis\n\n* test: cover type-aware protocol adapter\n\n* fix: apply type-aware API surface results\n\n* feat: complete type-aware analysis integration\n\n* fix: make type-aware CI dependencies explicit\n\n* feat: expand type-aware semantic analysis\n\n* feat: refine type-aware unused exports\n\n* fix: update vulnerable VS Code dependency\n\n* fix: harden type-aware export evidence\n\n* refactor: stabilize type-aware architecture\n\n* perf: benchmark type-aware cold and warm paths\n\n* fix: await instrumented type-aware benchmarks\n\n* fix: use supported CodSpeed walltime runner\n\n* fix: run type-aware walltime on available runner\n\n* feat: recommend type-aware analysis for TypeScript\n\n* fix(ci): verify branded PR comment author",
          "timestamp": "2026-07-27T10:30:03Z",
          "url": "https://github.com/fallow-rs/fallow/commit/20972d541b1deadad79108d780ffae6fd9c48163"
        },
        "date": 1785149241756,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 615,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1639,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1329,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1228,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1021,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1329,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1225,
            "unit": "ms"
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
        "date": 1785231768538,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 615,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 509,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1639,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1330,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1226,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1327,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1322,
            "unit": "ms"
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
        "date": 1785318227940,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 102,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 406,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1224,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1017,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1019,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 913,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 917,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 912,
            "unit": "ms"
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
        "date": 1785404145817,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 615,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1740,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1327,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1228,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1123,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1326,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1329,
            "unit": "ms"
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
        "date": 1785491302127,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 308,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 615,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1646,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1430,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1227,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1121,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1325,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1328,
            "unit": "ms"
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
        "date": 1785575457474,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 613,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 509,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1739,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1334,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1225,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1023,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1327,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1224,
            "unit": "ms"
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
        "date": 1785662209938,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 714,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 509,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1637,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1332,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1228,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1030,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1429,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1326,
            "unit": "ms"
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
        "date": 1785753936186,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 614,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1427,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1225,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1120,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1018,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1117,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1118,
            "unit": "ms"
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
          "id": "7093ed409c5b7e64162bd70826dd62e7db88a08c",
          "message": "chore(docker): pin FALLOW_VERSION 3.14.0 with refreshed checksums",
          "timestamp": "2026-08-04T09:01:32Z",
          "url": "https://github.com/fallow-rs/fallow/commit/7093ed409c5b7e64162bd70826dd62e7db88a08c"
        },
        "date": 1785836781098,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 617,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1744,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1431,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1333,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1127,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1534,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1433,
            "unit": "ms"
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
        "date": 1785923004850,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 617,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 511,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1745,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1432,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1332,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1124,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1432,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1429,
            "unit": "ms"
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
        "date": 1786009507229,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 102,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 509,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 410,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1322,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1123,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 916,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1120,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1421,
            "unit": "ms"
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
          "id": "d8de238c7fce3f4805a1d671cf54b408e9e9d445",
          "message": "refactor: consolidate entry-point discovery on the fallow-core implementation (#2154)\n\nDeletes the engine's diverged copy of entry-point discovery (net +131/-1428) and routes the engine through core_backend pass-throughs, mirroring the discovery-walk consolidation. BackendAggregatedPluginResult wraps the core AggregatedPluginResult directly so plugin entry-point provenance survives without mirror-type reconstruction; the engine e2e tests moved to core before the fork was deleted; the orphaned regex and glob dependencies are removed. Behavior verified byte-identical against a pristine baseline binary on three real projects (dead-code, check, and list JSON, cold and warm cache). One log-only change: the skipped-entry warning dedupe is a single process-wide set.",
          "timestamp": "2026-08-07T07:14:05Z",
          "url": "https://github.com/fallow-rs/fallow/commit/d8de238c7fce3f4805a1d671cf54b408e9e9d445"
        },
        "date": 1786090893436,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 309,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 311,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 207,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 207,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 723,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 515,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 2268,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1852,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1564,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1332,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1650,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1548,
            "unit": "ms"
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
          "id": "dffec365267ba06a983bfa0944be5ff339f1fb64",
          "message": "refactor: harden duplication architecture\n\n* refactor: start duplication architecture hardening\n\n* refactor: harden duplication architecture\n\n* test: keep spread proptest out of miri",
          "timestamp": "2026-08-08T06:48:38Z",
          "url": "https://github.com/fallow-rs/fallow/commit/dffec365267ba06a983bfa0944be5ff339f1fb64"
        },
        "date": 1786175587507,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 613,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 410,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1748,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1336,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1227,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1427,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1327,
            "unit": "ms"
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
        "date": 1786262174094,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 406,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 405,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1217,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1012,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 912,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 810,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 912,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 914,
            "unit": "ms"
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
        "date": 1786350839950,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 511,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 407,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1536,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1326,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1121,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1124,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1122,
            "unit": "ms"
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
        "date": 1786436090608,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 617,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 411,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1741,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1432,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1328,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1124,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1433,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1326,
            "unit": "ms"
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
          "id": "354ee1c6a92bdf2e8f84c336cb35e22eff039dd8",
          "message": "chore(docker): pin FALLOW_VERSION 3.15.0 with refreshed checksums",
          "timestamp": "2026-08-12T07:51:43Z",
          "url": "https://github.com/fallow-rs/fallow/commit/354ee1c6a92bdf2e8f84c336cb35e22eff039dd8"
        },
        "date": 1786522910279,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 615,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 510,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1741,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1431,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1327,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1022,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1429,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1328,
            "unit": "ms"
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
        "date": 1786609486745,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 207,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 513,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1644,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1332,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1226,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1426,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1326,
            "unit": "ms"
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
        "date": 1786695569595,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 207,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 615,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1641,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1329,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1226,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1022,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1329,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1326,
            "unit": "ms"
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
        "date": 1786779356378,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 207,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 617,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1640,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1327,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1228,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1024,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1431,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1329,
            "unit": "ms"
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
        "date": 1786865784431,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 308,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 617,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1649,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1334,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1237,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1028,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1436,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1431,
            "unit": "ms"
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
          "id": "aba36fe9c341c4365ead772ba7ff274a74ecf8eb",
          "message": "chore(benchmarks): remove stale CodSpeed coverage\n\n* chore: start CodSpeed benchmark cleanup\n\n* chore(benchmarks): remove stale CodSpeed coverage",
          "timestamp": "2026-08-17T07:32:16Z",
          "url": "https://github.com/fallow-rs/fallow/commit/aba36fe9c341c4365ead772ba7ff274a74ecf8eb"
        },
        "date": 1786953278401,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 102,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 102,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 102,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 405,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 305,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1012,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1019,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 917,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 710,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 810,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 812,
            "unit": "ms"
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
        "date": 1787038948373,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 512,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1639,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1225,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1224,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1327,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1324,
            "unit": "ms"
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
          "id": "43b0526540c84f669ea1f4a43bf23dbba0c596da",
          "message": "perf(benchmarks): track viz rendering",
          "timestamp": "2026-08-19T07:01:34Z",
          "url": "https://github.com/fallow-rs/fallow/commit/43b0526540c84f669ea1f4a43bf23dbba0c596da"
        },
        "date": 1787125275419,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 509,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 407,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1328,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1118,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1019,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 916,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1122,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1123,
            "unit": "ms"
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
          "id": "1881f4d5fe0a9410f807e6c236d23537279a1a7b",
          "message": "perf(benchmarks): cover inspect evidence bundle\n\n* perf(benchmarks): cover inspect evidence bundle\n\n* perf(benchmarks): bound inspect simulation corpus",
          "timestamp": "2026-08-20T07:41:01Z",
          "url": "https://github.com/fallow-rs/fallow/commit/1881f4d5fe0a9410f807e6c236d23537279a1a7b"
        },
        "date": 1787211958855,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 615,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1637,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1330,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1229,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1021,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1432,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1329,
            "unit": "ms"
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
        "date": 1787298472727,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 513,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1638,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1332,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1227,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1328,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1327,
            "unit": "ms"
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
        "date": 1787384225786,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 102,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 103,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1122,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1023,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 816,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 713,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1018,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 917,
            "unit": "ms"
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
          "id": "0a95caca69955029a0c41c397f01f2b6b87b1b2f",
          "message": "fix(graph): credit star and namespace chains behind whole-module consumers and entry namespaces\n\n## What was broken\n\nSeveral consumer shapes observe every name on a module's namespace object but credited only the module's direct exports, so names the module only exposes through its own `export * from './deep'` or `export * as sub from './sub'` were reported as unused (#2372):\n\n- A namespace import the graph cannot narrow: `import * as ns from './barrel'` used as a whole object (`Object.values(ns)`, a spread, a destructure with rest), handed on without any member access, or exported under its own name.\n- An `export * as sub` binding imported by name and used as a whole object: `import { sub } from './barrel'` plus `Object.values(sub)`.\n- A dynamic-import pattern match: `import()` with a template, `import.meta.glob`, `require.context`.\n- A bare side-effect `require('./barrel')` with no binding.\n\nFor a real entry point, `export * as sub from './sub'` on the entry, on a barrel the entry reaches through plain `export *`, or named by the entry through a chain of named re-exports (`export { sub } from './barrel'`, or `import * as ns` plus `export { ns }`) credited every direct export of `sub.ts`, but neither sub's own `export * from './deep'` sources nor its own `export * as sub2` sources (#2373).\n\n## Root cause\n\nPhase 2 credits a whole-module consumer through `mark_all_exports_referenced_at_site`, which walks the target's direct export list only. The two phases that credit chains, star propagation (Phase 4, seeded by `collect_entry_star_targets`) and namespace re-export propagation (Phase 2c, gated by `exposes_namespace_object`), knew two seeds: entry points and the ambient-module star closure that #2357 added. `collect_entry_star_targets` also walked plain `export *` edges only, so an `export * as sub` source behind an entry was never treated as exposing a namespace object of its own.\n\n## The fix\n\nOne seed-agnostic closure replaces `collect_ambient_star_targets`:\n\n- `populate_references` (Phase 2) returns every target whose whole namespace object a consumer observed: the empty-local-name namespace branch of `attach_symbol_reference` (ambient stars, dynamic-import pattern matches, and a bare side-effect `require('./barrel')` with no binding) and every mark-all branch of `narrow_namespace_references` (whole-object use, no member access outside an entry point, binding exported under its own name). Both sites record the seed through `AttachContext::observe_whole_namespace_object`, which carries the invariant.\n- `ModuleGraph::collect_exposed_namespace_targets` seeds with those targets plus every `export * as ns` source whose name reaches a consumer the graph cannot enumerate, then closes over both `export *` and `export * as` chains. Phase 2c treats a member as exposing its `export * as` sources; Phase 4 unions the closure into `entry_star_targets`, so every member is treated like an entry barrel for its `export *` sources (named exports, never `default`).\n- A name reaches such a consumer three ways, and the closure applies **the same test Phase 2c applies**: it arrives on an entry point's own export surface, on a module already in the closure, or at a name some importer uses as a whole object. The namespace-edge seeds and the chain walk therefore run to a fixpoint against each other, because a target that joins the closure can itself expose the name a further `export * as ns` edge forwards to it. Each round widens the closure or stops, so the walk terminates, and the closure Phase 2c reads no longer stops one namespace level short of what Phase 2c credits.\n- Each member carries **how much of it is exposed**. A member whose whole namespace object is observed exposes every export, `default` included. A member reached through a plain `export *` exposes every export except `default`, because that is the one name a plain `export *` never forwards, so an `export * as default` declared on such a member hands its target's namespace object to nobody. An entry point exposes its own `default`: it is public API.\n- The surface is matched **by name**, and every hop must uniquely forward the binding. That rule is not restated: `ModuleGraph::forwards_binding` picks the namespace and then calls Phase 2c's own `uniquely_forwards_binding`, so the closure and the phase it pre-computes for cannot drift apart on what a hop forwards. A barrel that declares its own `ns`, or that receives `ns` from two `export *` sources at once, exports a different binding under that name and the chain stops there. Sitting on an entry point's plain-`export *` closure is not on its own proof that a name survives to the entry, so no hop is skipped for it: the shortcut applies to an entry point itself only. The check reads the value namespace whenever the source exports the name there and the type namespace otherwise, so `export type { ns } from './barrel'` on an entry point does not put the value namespace object on the surface.\n- Entry-point reachability gates the seeds this PR adds, and nothing else. A target observed by a consumer in this graph, where no entry point reaches the target, is not seeded: that consumer is unreachable too, the report already calls the target an unused file, and crediting its chain would only stack unused-export rows underneath the unused-file rows. The same holds for an `export * as ns` source no entry point reaches. Withholding those can only withhold credit the pre-existing closure never gave.\n- The ambient seeds from #2357 are deliberately **not** gated, and neither is the chain walk. A `declare module 'pkg'` body states the shape of an external module id: its observers are importers of that id, outside this graph, so where the shim and its target sit inside the graph says nothing about who looks. The chain behind an unreachable shim routinely re-enters a module an entry point imports directly, and gating it reported unused exports on files the report calls reachable. A re-export edge makes its source reachable whenever the barrel is, so only an ambient chain can ever walk out of an unreachable member in the first place. Entry-point reachability reads the edge list alone, so `ModuleGraph::build` computes it once, before the closure, and hands the same bitset to `mark_reachable`.\n- Two seed properties are deliberate and visible in reports, and both are written down in the CHANGELOG and `docs/reference/detection-internals.md`. The seed is namespace-agnostic, so `export type { ns }` seeds the closure exactly like `export { ns }` and credits the chain in the value namespace as well: `typeof ns.member` keeps a value declaration reachable through a type-only re-export. And the seed does not ask whether the re-export itself has a consumer, so a namespace binding exported under its own name credits the chain behind it even when the report calls that very export unused, the same self-inconsistency the unreachable-observer case has.\n\nThe closure is computed once in `ModuleGraph::build` and threaded into Phase 2c and Phase 4 instead of each phase rebuilding it; it reads only `re_exports`, the entry-point flags, the consumers' whole-object uses, and that reachability bitset, none of which a later phase mutates. Seeding short-circuits when the project has no `export * as` edge at all.\n\n## Performance\n\nThe name search runs outward from each `export * as` edge toward the acceptance points, over a reverse index of the edges that forward a single name, rather than inward from every name an entry point re-exports. Two cutoffs keep a pathological chain cheap: a module that no forwarding edge connects to an acceptance point answers in constant time however deep its own chains run, and an exhausted search remembers every state it visited within a round, so a forwarding chain shared by many namespace edges is walked once instead of once per edge.\n\nDebug-build minimum over repeated full `dead-code --no-cache` runs, identical output everywhere:\n\n| project | baseline (main) | this branch |\n| --- | --- | --- |\n| vitest monorepo (12 runs each) | 2426 ms | 2507 ms |\n| 400 namespace barrels behind a 20-link plain-star chain | 145 ms | 152 ms |\n| 400 namespace barrels behind a 400-link plain-star chain | 374 ms | 430 ms |\n\nThe last row is the accepted cost of the shadowing fix: an on-surface namespace edge no longer takes a shortcut, so its name walks up the plain-star chain with a uniqueness check per hop. Realistic chain depths are within noise; the inward-by-name search variant measured roughly twice the baseline on the vitest monorepo, which is why the search direction is what it is.\n\nThe seed's own credit keeps its shape: a runtime whole-module edge credits the namespace object, `default` included; the ambient star form credits the star surface without `default`. Member-narrowed namespace imports (`ns.one()`) never seed the closure. A binding placed in an exported object literal (`export const API = { ns }`) keeps the direct-export mark-all it had on main but seeds the closure only when it is also used as a whole object or exported under its own name: the namespace-object alias phase follows `API.ns.<member>` accesses precisely, and the existing `issue-310` multi-hop alias test pins that `unusedQuery` stays reported.\n\nThe mark-all sites that feed the closure keep crediting their target's own direct exports as before, reachable or not; reference-level reachability filters those at reporting time. This matches the pre-existing mark-all model and is stated in `docs/reference/detection-internals.md`, along with what an unreachable whole-object observer suppresses.\n\nThe closure fixpoint does not rebuild its reachability prune per round. The prune only grows, so a round extends it from the members the previous round added and each re-export edge is walked at most once across all rounds. What is left per round is a rescan of the namespace edges still pending, which a chain shaped to resolve exactly one edge per round can drive up. On the pathological alternating named/namespace chain a reviewer built for that shape, minimum of 11 debug runs on a loaded box: N=1200 baseline 283 ms against 310 ms here (down from the +77% measured before the prune became incremental), N=2000 baseline 428 ms against 619 ms here (down from +160%). Real projects are flat: `viz-frontend` 191 ms against 182 ms, `editors/vscode` 228 ms against 227 ms, minimum of 9.\n\n## Behavior change\n\n- Whole-object namespace uses, unnarrowed namespace bindings, a namespace binding exported under its own name, an `export * as` binding imported by name and used as a whole object, dynamic-import pattern matches, a bindingless `require()`, and `export * as` chains an entry point exposes now credit the full namespace object of their target: direct exports, the named exports of `export *` sources (never their `default`), and every export of `export * as` sources (`default` included), recursively. Fewer unused-export findings for star barrels consumed those ways. A barrel that one of those consumers observes and no entry point reaches keeps reporting as an unused file with nothing stacked underneath it; an ambient `declare module` shim keeps crediting its chain whatever its reachability.\n- **One shape reports one finding more.** A plain `export *` hop inside a chain no longer carries a downstream `export * as default` onward, because that star never forwards `default`. An `export * from './barrel'` whose barrel does `export * from './mid'` over a `mid` that does `export * as default from './target'` now reports target's exports. That includes the ambient form: the `issue-2357-ambient-star-reexport` fixture is byte-identical between the baseline and this branch, but an ambient chain with a plain-star hop before an `export * as default` is not. An `export * as default` declared directly on the ambient star's own target still credits its chain. Nothing else in the issue-2357 behaviour moves: an ambient chain is seeded and walked at any reachability, exactly as it was.\n- A namespace re-export on a reachable non-entry barrel that is off the entry surface and has no consumer still exposes nothing; a barrel that declares its own copy of a star-forwarded name, or that receives the same name from two stars at once, stops the chain, whether the entry names the barrel or reaches it through a plain `export *`; a plain `export *` still never forwards `default`, so `export * as default` behind one keeps reporting while the same declaration on the entry point itself is credited.\n\n## Cache invalidation\n\n- `GRAPH_CACHE_VERSION` 38 -> 39: the new references are baked into the persisted graph and a graph-cache hit skips the build entirely. Warm 38 caches carry only the direct-export credit, and they also predate the one direction that moves the other way. Version 39 is unreleased (main is at 38), so it still invalidates every warm cache a user can have.\n- Extraction is untouched; the extract cache version stays at 276.\n\nWarm-cache proof on a scratch copy of the `issue-2373-entry-namespace-chain` fixture: the baseline binary (main, graph 38) runs cold and writes the cache; this branch (graph 39) on that warm cache reports exactly its own cold `--no-cache` output, and a second warm run is identical.\n\n## How it was tested\n\n- Integration fixture `issue-2372-star-barrel-whole-module`: whole-object use in the entry point over a barrel with `export *` plus a three-level `export * as` chain; a non-entry whole-object shim; a binding handed on without member access; a binding re-exported from a non-entry module; an `export * as` binding imported by name and used as a whole object, with a name-precision control on the same barrel; a binding that is both an object-literal alias source and exported under its own name, against the object-literal-only negative control; an ambient `declare module` star whose target exposes an `export * as ns` (credited whole, `default` included) and whose plain-star hop drops a downstream `export * as default` (reported); `import.meta.glob`, a template `import()`, and `require.context` targets with star, named, and `export * as` re-exports; a member-narrowed namespace import as a negative control; a barrel that re-exports itself through an `export *` / `export * as` cycle; an `export * as default` on a plain-star member, whose chain stays reported; a whole-object use inside an unreachable file, whose dead subtree stays unused-file rows with no unused-export rows underneath; reference-shape assertions that the credit is routed through the barrel's star chain and the exposed namespace object.\n- Integration fixture `issue-2373-entry-namespace-chain`: the issue repro plus a third `export * as sub3` level and sub2's own `export *`, an `export * as top` directly on the entry, a namespace named by the entry through `export { named } from './named'` and through a rename hop, an `import * as bindNs` plus `export { bindNs }` on the entry, a name-precision control, an off-surface `export * as hidden` on a reachable non-entry barrel, an entry star cycle, `export * as default` on the entry itself (credited) against the same declaration on a plain-star barrel and behind an `export { x as default }` rename (both reported), a star-forwarded namespace name shadowed by a local declaration on the barrel in both the named-hop and the plain-star entry form, the same name arriving from two `export *` sources at once, and reference-shape assertions.\n- Fixture `issue-2372-star-barrel-whole-module` also pins the shape the reachability split exists for: `unreached-shim.ts` holds a `declare module` body in a plain `.ts` file nothing imports, `unreached-barrel.ts` and `unreached-mid.ts` behind it are unused files, and `unreached-reentry.ts` and `unreached-ns-reentry.ts` are imported directly by the entry point, so the chain's names must stay credited on modules that carry no unused-file row at all. Re-gating the seed makes that test fail.\n- Graph unit test `forwards_binding_agrees_with_phase_2c_on_rename_shadow_and_ambiguity`: a rename hop forwards, a local declaration on the barrel shadows, and the same name arriving from two `export *` sources is ambiguous, asserted for both `ModuleGraph::forwards_binding` and `uniquely_forwards_binding`.\n- Mutation matrix. With the whole branch's `crates/graph/src` reverted to main, fourteen of the twenty-one tests fail; the seven that pass are the declared negative controls (member narrowing, object-literal alias, unreachable observer, off-surface namespace, the two cycle pins, and the shadow/ambiguity pin, which guards a regression this branch introduced rather than a gap on main). With only this round's `crates/graph/src` reverted, the four tests this round adds for its own fixes fail (the plain-star shadow and ambiguity pin, the entry namespace binding pin, the whole-object named-import pin, and the alias-plus-named-export pin) while the ambient pin passes, since that behaviour landed in the branch's preceding commit and bites against main instead.\n- Exact issue repros through both binaries: #2372 case 1 reports `src/deep.ts:deepHelper` on the baseline and nothing with the fix; #2373 reports only `barrel.ts:default` and `deep.ts:default` with the fix (baseline added `deep.ts:deepX`, `sub2.ts:sub2X`, `sub2.ts:default`).\n- Adversarial probes replayed from review, all now matching ES semantics: a locally shadowed `export * as ns` behind a plain-star entry and behind a named hop; two `export *` sources exporting the same `ns`; `export * as default` behind an entry star, behind a plain star from a whole-object seed, behind an `export { ns as default }` rename, and on the entry point itself; an ambient chain with a plain-star hop before an `export * as default`.\n- Real-project smokes with the baseline and branch binaries, normalized `dead-code --format json --no-cache` output identical everywhere: the in-repo `viz-frontend` and `editors/vscode`, the vitest monorepo, a design-system monorepo, and a large product monorepo. The `issue-2357`, `issue-310`, `issue-2348`, `issue-269`, `issue-303`, `issue-324`, `issue-328`, and `issue-1373` namespace fixtures are also identical to main.\n- Gates on the rebased tree: cargo fmt check, clippy workspace all-targets with warnings denied, full workspace test suite (with the type-aware sidecar installed), bench check, cargo doc with warnings denied, typos, hidden-unicode scan, and comment-quality check.\n\n## Review round 2\n\nThe entry-reachability gate the previous round added narrowed the pre-existing #2357 closure: when the `declare module` shim is an unreachable non-`.d.ts` file, the branch stopped crediting the ambient star's chain and reported new unused exports on files an entry point imports directly. Three shapes a reviewer executed are byte-identical to the pre-change binary again:\n\n1. An unreachable shim over `impl.ts` -> `impl-deep.ts` where the entry point imports `implDeepOne` from `impl-deep.ts`: `impl-deep.ts:implDeepTwo` is no longer reported.\n2. The same in namespace form (`export * as ns` over a reachable `ns-target.ts`): `ns-target.ts:nsTwo` and `ns-target.ts:default` are no longer reported.\n3. A fully dead island: `impl-deep.ts:implDeepOne` is reported again, as on main.\n\nFixed by splitting the closure seeds (ambient targets ungated, in-graph observers gated) and dropping the reachability test from the chain walk, plus the incremental reachability prune, the `forwards_binding` delegation, and the CHANGELOG and detection-internals corrections described above.\n\nRebased onto `dc03fd148`.\n\nFixes #2372\nFixes #2373",
          "timestamp": "2026-08-23T06:43:58Z",
          "url": "https://github.com/fallow-rs/fallow/commit/0a95caca69955029a0c41c397f01f2b6b87b1b2f"
        },
        "date": 1787470730638,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 308,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 722,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1647,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1328,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1230,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1123,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1437,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1433,
            "unit": "ms"
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
          "id": "62752fa6df4feb1e4ad646f4b2b536c52f2b7db3",
          "message": "fix(extract): route ambient export type * through a type-only whole-module import\n\n## What was broken\n\n`export type *` and `export type * as ns` inside a `declare module '...'` body still created a file-level type-only star re-export on the declaring file. [#2357](https://github.com/fallow-rs/fallow/issues/2357) routed the plain `export *` and `export * as ns` spellings through a bindingless whole-module import so the declaring file gains no export surface, but that shape carried no type modifier: routing the type-only star through it as-is would have credited the value meaning of every target export, which the star erases.\n\nBoth directions were wrong, with `src/pair.ts` holding `export interface Foo { a: number }`, `export const Foo = 1`, `export const plain = 3` and `export default function d(): void {}`:\n\n- A non-entry `src/shim.ts` carrying `declare module 'pkg' { export type * from './pair' }` plus `export {}`, imported for side effects by the entry, reported the `Foo` interface, the `Foo` const, `plain` and `default`. Nothing credited the target, so the interface half was a false positive: the type star forwards it.\n- With the same declaration in an entry-point `src/ambient.d.ts`, the file-level star laundered the consts into the entry's public surface and only `default` reported: the `Foo` const, which the type star does not forward, reported nowhere.\n\n## Root cause\n\n`visit_export_all_declaration` took the ambient branch only for `!decl.export_kind.is_type()`, so the type-only spellings fell through to the file-level `ReExportInfo`. The whole-module shape the ambient branch records is read by `ImportedSymbol::is_ambient_star`, and `desired_import_namespaces` credits both namespaces for it, which is right for `export *` and wrong for `export type *`. Nothing in the persisted extract shape could tell the two apart.\n\n## The fix\n\nEarliest incorrect layer is extraction. `ImportInfo` and the graph's `ImportedSymbol` gain `is_type_only_star`. The extractor now takes the ambient branch for every star spelling and sets the flag from `export_kind.is_type()`, so `export type *` records exactly what `export *` records (one type-space `Namespace` import with an empty local name, plus a `Default` import for the `as ns` form, and no `ReExportInfo`) with the flag set.\n\nIn the graph, `ImportedSymbol::is_value_bearing_ambient_star` names the plain star alone, and `desired_import_namespaces` reads it, so a type-only star credits the target's star surface in the type namespace and nothing else. Everything else about the shape is unchanged: `mark_star_surface_referenced_at_site` still skips `default` for the plain form, the extra `Default` import of the `as ns` form still reaches `ns.default` (now in type space), and the exposed-namespace closure seed is the same, so the chain behind the target keeps exactly the credit the plain star gives it.\n\n## Behavior change\n\n- **The type half of a same-name type and value pair behind a non-entry shim stops reporting.** That is the false positive in the issue.\n- **The target's value-only exports behind such a shim stop reporting too.** `export type *` forwards them as type-only bindings, reachable as `typeof plain`, and the graph credits them through the type-space fallback lane. This is exactly the credit the ambient `export type { plain } from './pair'` form has given since [#2349](https://github.com/fallow-rs/fallow/issues/2349), verified against a binary built from main, so the two spellings stay consistent. The issue text expected these rows to survive; making the star stricter than the named form it generalizes would have been the inconsistency.\n- **The value half of a same-name pair behind an entry-point `.d.ts` starts reporting.** The laundered entry surface is gone, so a `const Foo` the type star does not forward is a finding again. That is a finding-count increase on upgrade for repositories with an entry-point ambient `export type *`.\n- `export type *` forwards no `default`, exactly like the plain star; `export type * as ns` forwards `ns.default` in type space.\n- Plain ambient stars, the ambient named re-export forms, and `import()` type references in TypeScript and JSDoc are unchanged. A bare-specifier `export type *` inside an ambient body stays type-only package usage, so `--production` classification does not move.\n\n### Known limitation\n\nThe chain behind the target (its own `export *` and `export * as sub` sources) is credited at full namespace-object exposure, in both namespaces, because the closure has no namespace dimension. A type star erases the value meanings of those names too, so that credit is more generous than the target's own surface. Keeping the seed as it is means no shape reports more than it did before this change; splitting the closure per namespace is filed separately.\n\n## Cache invalidation\n\n- extract `CACHE_VERSION` 279 -> 280: warm caches replay the file-level star re-export and lack the flag.\n- `GRAPH_CACHE_VERSION` 43 -> 44: the persisted graph carries the old `ReExportEdge`, the laundered entry surface, and the value-lane credits, and a graph-cache hit skips the build entirely.\n\nWarm-cache proof on a scratch copy of the fixture sharing one `.fallow` directory: a binary built from main (279/43) runs cold, writes `cache.bin` and `graph-cache.bin`, and reports the laundered and uncredited shapes; the patched binary (280/44) on that warm cache reports exactly its own cold output, twice.\n\n## How it was tested\n\n- Extract tests: `export type *` and `export type * as ns` inside an ambient body record the flagged whole-module shape, no file-level export and no `ReExportInfo`, and the `as ns` form adds the default import. The test that pinned the old file-level shape is replaced by these.\n- Cache test: the flag survives a `ModuleInfo` to `CachedModule` round trip, and a bound `import type { Foo }` does not carry it.\n- Graph unit tests: `desired_import_namespaces` returns type space only for the flagged `Namespace` and `Default` symbols; the type star credits the interface half of a pair in the type namespace, leaves the const half with no reference at all, credits a value-only export in type space, and leaves `default` unreferenced; the `as ns` form credits `default` in type space and nowhere else.\n- Integration fixture `issue-2375-ambient-type-star`: an entry-point `ambient.d.ts` with `export type * from './entry-pair'`, a reachable non-entry `shim.ts` with `export type * from './shim-pair'`, `ambient-ns.d.ts` with `export type * as ns` over a target carrying a type default (`export default interface`) and over a value-only target, and `ambient-value.d.ts` with a plain type star over a value-only target. Four tests assert the credited type surface, the value halves and defaults that keep reporting, the empty export surface and re-export list on all four declaring files (with the entry-point and non-entry roles pinned), and the namespace of the credited references.\n- Mutation matrix: reverting the extractor hunk fails both new extract tests and all four integration tests; reverting the lane hunk fails the three graph unit tests and the two integration tests that read lanes; reverting the cache mapping fails the round trip.\n- Issue reproduction through the patched binary, both directions: the non-entry shim reports the `Foo` const and `default` (was: those plus the interface and `plain`); the entry-point `.d.ts` reports the `Foo` const and `default` (was: `default` alone).\n- Regression pins re-run: the `issue-2357`, `issue-2349`, `issue-2372` and `issue-2373` fixtures all pass unchanged, and a chain probe confirms the type star credits a `export * from` plus `export * as sub` chain identically to the plain star.\n- Real-project smokes: `dead-code --format json` with the main and patched binaries on the in-repo `viz-frontend` and `editors/vscode`, normalized outputs identical.\n- Gates: cargo fmt check, clippy workspace all-targets with warnings denied, full workspace test suite, bench check, cargo doc with warnings denied, typos, hidden-unicode scan, comment-quality check, and the agent-adapter check, all green.\n\nFixes #2375",
          "timestamp": "2026-08-24T07:57:20Z",
          "url": "https://github.com/fallow-rs/fallow/commit/62752fa6df4feb1e4ad646f4b2b536c52f2b7db3"
        },
        "date": 1787558593518,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 306,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 207,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 516,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 511,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1638,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1226,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1228,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1021,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1326,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1326,
            "unit": "ms"
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
        "date": 1787644060699,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 207,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 508,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 407,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1326,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1119,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1022,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 915,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1120,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1121,
            "unit": "ms"
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
        "date": 1787730650444,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 203,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 509,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 405,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1220,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1014,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 916,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 812,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1013,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 915,
            "unit": "ms"
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
        "date": 1787854404750,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 204,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 206,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 309,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 208,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 615,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1742,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1334,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1328,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1123,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1430,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1328,
            "unit": "ms"
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
        "date": 1787945061096,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 617,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1638,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1330,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1226,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1025,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1535,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1433,
            "unit": "ms"
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
        "date": 1788008848453,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 207,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 309,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 615,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 408,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1742,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1331,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1225,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1020,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1433,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1329,
            "unit": "ms"
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
        "date": 1788093653252,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "preact (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "preact (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "fastify (cold)",
            "value": 307,
            "unit": "ms"
          },
          {
            "name": "fastify (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (cold)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "zod (warm)",
            "value": 205,
            "unit": "ms"
          },
          {
            "name": "vue-core (cold)",
            "value": 617,
            "unit": "ms"
          },
          {
            "name": "vue-core (warm)",
            "value": 409,
            "unit": "ms"
          },
          {
            "name": "svelte (cold)",
            "value": 1640,
            "unit": "ms"
          },
          {
            "name": "svelte (warm)",
            "value": 1330,
            "unit": "ms"
          },
          {
            "name": "query (cold)",
            "value": 1225,
            "unit": "ms"
          },
          {
            "name": "query (warm)",
            "value": 1023,
            "unit": "ms"
          },
          {
            "name": "vite (cold)",
            "value": 1438,
            "unit": "ms"
          },
          {
            "name": "vite (warm)",
            "value": 1430,
            "unit": "ms"
          }
        ]
      }
    ]
  }
}