window.BENCHMARK_DATA = {
  "lastUpdate": 1788137491802,
  "repoUrl": "https://github.com/fallow-rs/fallow",
  "entries": {
    "Fallow Allocations": [
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "12a15fb2155d56dea28dc2128b9f6682e6d0722f",
          "message": "feat(health): drop CRAP from template units and score Svelte {#snippet} blocks as their own units (#2258)\n\n* feat(health): drop CRAP from template units and score Svelte snippets\n\nSynthetic template-family units leave the CRAP dimension in every dialect\n(Closes #2235). A template carries no measurable coverage, so its CRAP was\na hidden second cyclomatic gate at 5 / 10 / 28 depending on the estimate\ntier. Template findings no longer carry crap, coverage_pct, coverage_tier,\ncoverage_source, or inherited_from; template units no longer count toward\ncrap_max, crap_above_threshold, refactoring targets, or Istanbul match\nstatistics. A maxCrap override scoped to a template unit reports a matched\nstale crap-dimension row with the CRAP value absent and copy saying the\nentry can be removed, instead of regressing to no_match. Component rollups\nare built from extracted template complexity rather than the findings\nlist, so a component whose template stops producing its own finding keeps\nits rollup. The unreachable template arm of the CRAP coverage action is\ndeleted, and the explain, MCP, and SARIF rule-help contract surfaces are\nupdated to match.\n\nTop-level Svelte {#snippet name(params)} blocks become their own\n<snippet:name> complexity units (Closes #2227). The body is scored with\nnesting rebased to zero and no longer accumulates into the parent\n<template>, so in-file snippet extraction now scores like the equivalent\nfile split instead of carrying a nesting surcharge. Snippet units are\nexact-match keys for health.thresholdOverrides[].functions, suppress with\nthe SFC markup comment above the reported anchor line, and render with a\ndistinct display name; the .svelte refactor advice names the snippet lever\nbefore a file split. Nested or unnameable snippets keep the folded\nbehavior and an unclosed snippet keeps the all-or-nothing\nmalformed-template drop. Extraction CACHE_VERSION bumps to 270.\n\nNew snippet unit names create new health-baseline buckets and overflow a\npre-existing baseline in both identity and count modes; the changelog\ntells adopters to re-save baselines on upgrade. The CRAP removal alone is\nabsorbed by baselined severity allowances and does not flip a gate.\n\n* docs(changelog): state the intentional template and snippet line-count overlap",
          "timestamp": "2026-08-13T13:04:08+02:00",
          "tree_id": "843c48061d498ca53b2bdd45f0e36d048b71bcea",
          "url": "https://github.com/fallow-rs/fallow/commit/12a15fb2155d56dea28dc2128b9f6682e6d0722f"
        },
        "date": 1786619284337,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10345715,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 53039,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1018706,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7270,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "e5bf4e24fafe7798e97255aa458e4b62d8660ea3",
          "message": "fix: collect Rust walltime benchmark results\n\n* fix(benchmarks): use CodSpeed macro runner\n\n* fix(benchmarks): collect Rust walltime results",
          "timestamp": "2026-08-13T14:57:24+02:00",
          "tree_id": "7529a1fb2b352f9f9b7574ce36a879a04cc79876",
          "url": "https://github.com/fallow-rs/fallow/commit/e5bf4e24fafe7798e97255aa458e4b62d8660ea3"
        },
        "date": 1786626105161,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10294859,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 53018,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1022747,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7560,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "93e765835bf3aa5b3048468753ad0fd228bbef15",
          "message": "perf: skip side-effect export indexes",
          "timestamp": "2026-08-13T15:41:30+02:00",
          "tree_id": "65970b121ab3e0902b67919ecf298a124e4db4eb",
          "url": "https://github.com/fallow-rs/fallow/commit/93e765835bf3aa5b3048468753ad0fd228bbef15"
        },
        "date": 1786628798962,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10310066,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 52965,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1024347,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7578,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "04b49162b342f0889b8eb50e3c69ba010f18aee5",
          "message": "perf: compact source discovery globs",
          "timestamp": "2026-08-13T16:08:41+02:00",
          "tree_id": "354e7483492f34085f8391b63dff92dc0be7dd4b",
          "url": "https://github.com/fallow-rs/fallow/commit/04b49162b342f0889b8eb50e3c69ba010f18aee5"
        },
        "date": 1786630446062,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9741098,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48469,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1026456,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7596,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "74f5a847a45709ef4d09a5d8f7918d6fcb70cb9d",
          "message": "perf(core): cache default entry matchers",
          "timestamp": "2026-08-13T16:57:26+02:00",
          "tree_id": "5f23453ab749809c83ad7f68b0ce87b612840b0f",
          "url": "https://github.com/fallow-rs/fallow/commit/74f5a847a45709ef4d09a5d8f7918d6fcb70cb9d"
        },
        "date": 1786633364497,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9744539,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48504,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1030804,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7606,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "400835b0ed607faeb8bca1443d39647223bab41e",
          "message": "fix(audit-cache): stop counting deregistered legacy entries as reclaimed (#2260)\n\nA pre-#1815 registration at the current cache path is only deregistered\nand stays warm on disk. Prune now reports it as kept with reason\nlegacy-deregistered, surfaces an additive deregistered count in the JSON\nenvelope plus a matching human summary line, and excludes its size from\nreclaimed_bytes and the human reclaim total. Released SHA-keyed\nregistrations are genuinely removed and stay counted.\n\nThe audit-cache prune long help now states that --dry-run previews the\npolicy; no generated contract surface embeds the nested subcommand help,\nso the generated contracts are unchanged.\n\nNew-only duplication demotion diff sources gain coverage beyond the\nWorktree state: integration tests for shared-diff precedence over the\nmerge-base worktree diff, the skipped-state --explain line, and the\ndemotion note wording, plus unit tests for the diff-source labels and\nthe retained shared-diff source label.",
          "timestamp": "2026-08-13T17:28:55+02:00",
          "tree_id": "e55cbd805de3d86dcd4413e74351be99849a981a",
          "url": "https://github.com/fallow-rs/fallow/commit/400835b0ed607faeb8bca1443d39647223bab41e"
        },
        "date": 1786635164792,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9744515,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48501,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1034772,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7636,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "008e2b2056841859745afae6b92047f847e5553e",
          "message": "test(audit): surface verdict context in the reshaped-clone demotion assert",
          "timestamp": "2026-08-13T18:13:44+02:00",
          "tree_id": "68eb1f43b3c9b57f7f28fb4514448f3c7ba877c2",
          "url": "https://github.com/fallow-rs/fallow/commit/008e2b2056841859745afae6b92047f847e5553e"
        },
        "date": 1786637883017,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9758819,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48522,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1028405,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7552,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "defaf599b13af50aa35df2138c3772c6a93de032",
          "message": "test(audit): dump base duplicate-export keys in the demotion assert",
          "timestamp": "2026-08-13T18:29:55+02:00",
          "tree_id": "429b863690666afbf6eba0ad37f74e8aa5acfe4a",
          "url": "https://github.com/fallow-rs/fallow/commit/defaf599b13af50aa35df2138c3772c6a93de032"
        },
        "date": 1786638846696,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9750955,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48511,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1036395,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7593,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "88507a56c272a0ec00cd59cc33c0405563f0ebf4",
          "message": "test(audit): dump full base snapshot keys in the demotion assert",
          "timestamp": "2026-08-13T18:41:16+02:00",
          "tree_id": "e32ded3879f3fba7b45ba73d38937df13e0571ed",
          "url": "https://github.com/fallow-rs/fallow/commit/88507a56c272a0ec00cd59cc33c0405563f0ebf4"
        },
        "date": 1786639474256,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9737306,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48473,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1027269,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7601,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "fb44ef467c8a032dc12453fb3b2842c115ec10da",
          "message": "fix(audit): keep base attribution when the focus remap fails\n\nThe base-snapshot focus set is built from `git rev-parse --show-toplevel`,\nwhose spelling can differ from the caller's canonicalized root (Windows 8.3\ncomponents and drive-letter case, verbatim prefixes), so a literal\nstrip_prefix mapped no path at all. The base dead-code results were then\nfiltered against an empty focus set and every base finding disappeared, which\nmade each inherited finding look introduced and failed `--gate new-only` on\npre-existing findings.\n\nThe remap now compares simplified and canonicalized forms before giving up on\na path, and a base run whose focus set cannot be expressed leaves its results\nunfiltered instead of filtering them against an empty set.",
          "timestamp": "2026-08-13T20:01:56+02:00",
          "tree_id": "a32c69c82618dc6eee83c07431f87f5eed349ee2",
          "url": "https://github.com/fallow-rs/fallow/commit/fb44ef467c8a032dc12453fb3b2842c115ec10da"
        },
        "date": 1786644377680,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9753850,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48491,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1015114,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7566,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "aec0bd07a5e2532ad242a3587459b7aaade345df",
          "message": "test(audit-cache): ignore lazy directory mtimes in the prune snapshot",
          "timestamp": "2026-08-13T20:17:19+02:00",
          "tree_id": "93e3fee7ae6c8e79248acf298c739fe8ecdbc395",
          "url": "https://github.com/fallow-rs/fallow/commit/aec0bd07a5e2532ad242a3587459b7aaade345df"
        },
        "date": 1786645288407,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9738002,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48465,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1031843,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7581,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "45fd28766199acb1f939f6862274a37aad12770b",
          "message": "chore: release v3.16.0",
          "timestamp": "2026-08-13T21:13:19+02:00",
          "tree_id": "75fc52ec7f7b8ff5a32073a0c7f8f8145dd2f75a",
          "url": "https://github.com/fallow-rs/fallow/commit/45fd28766199acb1f939f6862274a37aad12770b"
        },
        "date": 1786648677752,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9755307,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48522,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1018706,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7270,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "c64f703114d452f52077a3a1737696a22a342ec9",
          "message": "perf(graph): reserve named export index capacity",
          "timestamp": "2026-08-13T23:53:47+02:00",
          "tree_id": "bdbad5be9280f810e1c0cf7527b6a71fc6d80ef4",
          "url": "https://github.com/fallow-rs/fallow/commit/c64f703114d452f52077a3a1737696a22a342ec9"
        },
        "date": 1786658334992,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9760211,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48528,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1018706,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7270,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "c2b6c9efff2e4de57a32bfad828bc5a3a3574070",
          "message": "perf(output): insert root kind in place",
          "timestamp": "2026-08-14T00:15:47+02:00",
          "tree_id": "9aa3e9c8065828466f34323fb9593765664c96cf",
          "url": "https://github.com/fallow-rs/fallow/commit/c2b6c9efff2e4de57a32bfad828bc5a3a3574070"
        },
        "date": 1786659673390,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9758691,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48523,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1040409,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7635,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "8b32aa4f6a285d1ae6897986f99dde834ecb7579",
          "message": "chore(docker): pin FALLOW_VERSION 3.16.0 with refreshed checksums",
          "timestamp": "2026-08-14T00:38:42+02:00",
          "tree_id": "362c1fbc84741f399cd199621f74bde72b1d11cf",
          "url": "https://github.com/fallow-rs/fallow/commit/8b32aa4f6a285d1ae6897986f99dde834ecb7579"
        },
        "date": 1786660972519,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9751147,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48520,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1029918,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7588,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "95a7ae9faf9e987616fe2366b74e99626dfd58c6",
          "message": "fix: surface star-export ambiguity instead of blaming the sources (#2268)\n\nWhen two star re-export sources supply the same name, the barrel exports nothing under that name. Unused-export and unused-type findings are now suppressed for the declarations that contribute to such a collision, instead of blaming both source files for a mistake in the barrel. Traces carry an optional star_export_ambiguity block naming the contributing files and namespaces, so an ambiguous name is no longer indistinguishable from a misspelled one. The unrendered-component and unprovided-inject headers now state the guarantee the code actually offers, including the abstain carve-out that remains. The value-derived type fallback lane is seeded lazily, which makes barrel-chain resolution roughly ten percent cheaper.\n\nCloses #2262\nCloses #2263\nCloses #2264",
          "timestamp": "2026-08-14T08:35:39+02:00",
          "tree_id": "daf704ab51453c6880b4fb3c5d8775efa97cafce",
          "url": "https://github.com/fallow-rs/fallow/commit/95a7ae9faf9e987616fe2366b74e99626dfd58c6"
        },
        "date": 1786689583048,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9712147,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48503,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1039022,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7621,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "6ab2c847bad9bc88a85e6fa29139a811db7203a0",
          "message": "fix(type-aware): bound generic scans and identify Svelte host gaps\n\n* chore: start type-aware issue fixes\n\n* fix: harden type-aware generic and Svelte analysis",
          "timestamp": "2026-08-14T11:14:51+02:00",
          "tree_id": "e9541817bdca9b64870fd20eab3e4911020d11d0",
          "url": "https://github.com/fallow-rs/fallow/commit/6ab2c847bad9bc88a85e6fa29139a811db7203a0"
        },
        "date": 1786699202217,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9718139,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48512,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1023571,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7582,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "7a8442440410c9e3211ac28028fd415e8ee4e303",
          "message": "perf(types): bucket clone spread locations",
          "timestamp": "2026-08-15T23:51:41+02:00",
          "tree_id": "5f52d8af626c7e62b2ce1239d7963bbfc93e8c92",
          "url": "https://github.com/fallow-rs/fallow/commit/7a8442440410c9e3211ac28028fd415e8ee4e303"
        },
        "date": 1786831009344,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9718283,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48512,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1018706,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7270,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "4084184901c496642c96f0b8937ce933342cfa2e",
          "message": "perf(extract): deduplicate local type declarations once",
          "timestamp": "2026-08-15T22:01:25Z",
          "tree_id": "2b57ead18d3817001821489850716112f9185888",
          "url": "https://github.com/fallow-rs/fallow/commit/4084184901c496642c96f0b8937ce933342cfa2e"
        },
        "date": 1786831533761,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9711815,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48701,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1030952,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7608,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "8437d52e6688cd1ce823d5da8c6670e7a23f839f",
          "message": "fix: close analysis and audit follow-ups\n\n* chore: start issue follow-up batch\n\n* chore: start issue follow-up batch\n\n* fix: close analysis and audit follow-ups",
          "timestamp": "2026-08-16T01:07:16+02:00",
          "tree_id": "84124fa62f20213b3fbefff9d07a20d84d5a33fd",
          "url": "https://github.com/fallow-rs/fallow/commit/8437d52e6688cd1ce823d5da8c6670e7a23f839f"
        },
        "date": 1786835482982,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9730199,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48919,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1031285,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7608,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "jernej.barbaric@gmail.com",
            "name": "Jerc92",
            "username": "Jerc92"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "78632bbbada683198be1cc47e1ea9023c7c8cebc",
          "message": "feat(ci): render type-aware feedback from saved results\n\n* feat(ci): render type-aware feedback from saved results\n\n* fix(ci): harden saved report rendering\n\n---------\n\nCo-authored-by: Jernej Barbaric <jernej.barbaric@login5.org>\nCo-authored-by: Bart Waardenburg <bart@waardenburg.dev>",
          "timestamp": "2026-08-16T18:20:58Z",
          "tree_id": "b5d9359e208820c8c0ff7ecb0ee38be2d3824e5b",
          "url": "https://github.com/fallow-rs/fallow/commit/78632bbbada683198be1cc47e1ea9023c7c8cebc"
        },
        "date": 1786904766683,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9717727,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48900,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1038668,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7621,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "e08e3e05fe9909916f92b089fb7e8ae5ff544e32",
          "message": "chore: release v3.17.0",
          "timestamp": "2026-08-16T23:47:53+02:00",
          "tree_id": "5418df7130ee9f4317fffae9b8c21e2f27282181",
          "url": "https://github.com/fallow-rs/fallow/commit/e08e3e05fe9909916f92b089fb7e8ae5ff544e32"
        },
        "date": 1786917254095,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9669487,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48883,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1021945,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7478,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "fc234ba804a29edc872f2ace40045b6254da5c81",
          "message": "chore(docker): pin v3.17.0 assets",
          "timestamp": "2026-08-17T02:40:35+02:00",
          "tree_id": "0c18118291abdea71c5f0798efb0c53da759674a",
          "url": "https://github.com/fallow-rs/fallow/commit/fc234ba804a29edc872f2ace40045b6254da5c81"
        },
        "date": 1786927555052,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9722191,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48904,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1020292,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7476,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "aba36fe9c341c4365ead772ba7ff274a74ecf8eb",
          "message": "chore(benchmarks): remove stale CodSpeed coverage\n\n* chore: start CodSpeed benchmark cleanup\n\n* chore(benchmarks): remove stale CodSpeed coverage",
          "timestamp": "2026-08-17T09:32:16+02:00",
          "tree_id": "1ae690447248416f42d7419692136895a319c362",
          "url": "https://github.com/fallow-rs/fallow/commit/aba36fe9c341c4365ead772ba7ff274a74ecf8eb"
        },
        "date": 1786952246326,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9728295,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48911,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1040349,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7637,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "6ebdf806df1a1ce1389026401af225d8fc7ab38f",
          "message": "perf(engine): coalesce duplicate line ranges",
          "timestamp": "2026-08-17T16:44:24+02:00",
          "tree_id": "2c1af25ee8f999aa236ef4107e1b91d6d82d24ed",
          "url": "https://github.com/fallow-rs/fallow/commit/6ebdf806df1a1ce1389026401af225d8fc7ab38f"
        },
        "date": 1786978559980,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9722039,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48911,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1022404,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7587,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f44fa8e5f47a37383c5b48bea7df41ab8729087c",
          "message": "perf(engine): preallocate health candidate paths",
          "timestamp": "2026-08-18T10:37:56+02:00",
          "tree_id": "f17d09561f443acf7ee864bb1c79077f03ab38fa",
          "url": "https://github.com/fallow-rs/fallow/commit/f44fa8e5f47a37383c5b48bea7df41ab8729087c"
        },
        "date": 1787042594418,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9714999,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48911,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1027694,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7596,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "00be8921cfd7397d579693a592e60e2944fc1a2a",
          "message": "perf(benchmarks): cover saved report rendering",
          "timestamp": "2026-08-18T11:20:35+02:00",
          "tree_id": "b3f35a4ca0b97461757bbdcf219e9fa16a1e8b6b",
          "url": "https://github.com/fallow-rs/fallow/commit/00be8921cfd7397d579693a592e60e2944fc1a2a"
        },
        "date": 1787045157006,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9708861,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 48830,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1024618,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7561,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "21b304caa4484b4395588e526df4663db8a1d7aa",
          "message": "perf(core): cache discovery file type matchers",
          "timestamp": "2026-08-18T22:20:39+02:00",
          "tree_id": "d8ecea9f32a90f11c1f4b6fb18ae40000db1ba7e",
          "url": "https://github.com/fallow-rs/fallow/commit/21b304caa4484b4395588e526df4663db8a1d7aa"
        },
        "date": 1787084757615,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9749387,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49223,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1190179,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8443,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "7e1e075d314031e000c552e8af2f5de26084d0a1",
          "message": "chore(deps-dev): bump ovsx to 1.1.1 and rolldown to 1.2.3 (#2322)",
          "timestamp": "2026-08-18T22:33:27Z",
          "tree_id": "c80bf47d3b43378343eb88ec2a0df0d9c4b08206",
          "url": "https://github.com/fallow-rs/fallow/commit/7e1e075d314031e000c552e8af2f5de26084d0a1"
        },
        "date": 1787092749047,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9745099,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49222,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1186637,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8405,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "65c0f80a2e29e607eda9eafb5f9457f92c3b3127",
          "message": "perf(benchmarks): track circular dependency command",
          "timestamp": "2026-08-19T01:42:41+02:00",
          "tree_id": "ba73445056731c31c7ea2a97760c02a5aaef20b6",
          "url": "https://github.com/fallow-rs/fallow/commit/65c0f80a2e29e607eda9eafb5f9457f92c3b3127"
        },
        "date": 1787096828860,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9709571,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49211,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1191935,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8429,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "9d14001d7fefd519a89116d156a07149b51b4314",
          "message": "perf(benchmarks): track feature flags command",
          "timestamp": "2026-08-19T02:12:42+02:00",
          "tree_id": "55c0cc4f0e9285a1d46628492bbec72928b07ca7",
          "url": "https://github.com/fallow-rs/fallow/commit/9d14001d7fefd519a89116d156a07149b51b4314"
        },
        "date": 1787098668512,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9680137,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49117,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1170982,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8378,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "d6e16368e9a9b04f1d232b2b35442021146945a4",
          "message": "perf(benchmarks): track guard policy resolution",
          "timestamp": "2026-08-19T02:40:52+02:00",
          "tree_id": "c463c7ff2489653698a1ba698389908c45313317",
          "url": "https://github.com/fallow-rs/fallow/commit/d6e16368e9a9b04f1d232b2b35442021146945a4"
        },
        "date": 1787100286553,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9685265,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49121,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1179974,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8403,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "69e4446d9494e464b304317ee9a9c6920f25150a",
          "message": "perf(benchmarks): track trace symbol chains",
          "timestamp": "2026-08-19T03:09:11+02:00",
          "tree_id": "e573c4f84e583b25e1ccef0d6d964de61685d0f0",
          "url": "https://github.com/fallow-rs/fallow/commit/69e4446d9494e464b304317ee9a9c6920f25150a"
        },
        "date": 1787102061324,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9741491,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49211,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1195708,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8447,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "d52e91244c091b25b097d15050635516cc3d0c28",
          "message": "perf(benchmarks): track suppression inventory",
          "timestamp": "2026-08-19T03:38:28+02:00",
          "tree_id": "ae26aa0d130443e83e0b96cdc5d4280477f3f00f",
          "url": "https://github.com/fallow-rs/fallow/commit/d52e91244c091b25b097d15050635516cc3d0c28"
        },
        "date": 1787103821048,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9731147,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49206,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1192260,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8450,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "3bf4a7f1f0b53e3c025ccca573ed82962c20c6a9",
          "message": "perf(benchmarks): track fix dry runs",
          "timestamp": "2026-08-19T04:56:33+02:00",
          "tree_id": "1bc7a2047619aa006aeb8a9704bd05b8b83951dd",
          "url": "https://github.com/fallow-rs/fallow/commit/3bf4a7f1f0b53e3c025ccca573ed82962c20c6a9"
        },
        "date": 1787108429468,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9741035,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49221,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1184690,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8391,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "3f861d310a867a7381e62bf7547f887c8c1ab811",
          "message": "perf(benchmarks): track security analysis",
          "timestamp": "2026-08-19T05:45:09+02:00",
          "tree_id": "893d9eefae1082c561cc69b29ec7e8d09708a6ef",
          "url": "https://github.com/fallow-rs/fallow/commit/3f861d310a867a7381e62bf7547f887c8c1ab811"
        },
        "date": 1787111384805,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9709723,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49212,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1191168,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8421,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f6ce30f8c3663ffa8d042a61e792ed8e03c11b09",
          "message": "perf(benchmarks): track list inventory",
          "timestamp": "2026-08-19T06:29:00+02:00",
          "tree_id": "c10d95c4467ed06288792bf590e5d29f82278358",
          "url": "https://github.com/fallow-rs/fallow/commit/f6ce30f8c3663ffa8d042a61e792ed8e03c11b09"
        },
        "date": 1787113991707,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9712371,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49218,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1183709,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8424,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "43b0526540c84f669ea1f4a43bf23dbba0c596da",
          "message": "perf(benchmarks): track viz rendering",
          "timestamp": "2026-08-19T09:01:34+02:00",
          "tree_id": "15e94a0e35a06b619a40ab796fbb2d25b45a0ef0",
          "url": "https://github.com/fallow-rs/fallow/commit/43b0526540c84f669ea1f4a43bf23dbba0c596da"
        },
        "date": 1787123148422,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9686225,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49123,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1172126,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8367,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f7764202547193e2c1bfefafc4b67f38a22d14b3",
          "message": "perf(benchmarks): track rule-pack policy analysis",
          "timestamp": "2026-08-19T10:56:00+02:00",
          "tree_id": "aa5e39f8fd6ce18b82128714fe2420d60613c655",
          "url": "https://github.com/fallow-rs/fallow/commit/f7764202547193e2c1bfefafc4b67f38a22d14b3"
        },
        "date": 1787130003186,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9683089,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49119,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1174625,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8346,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "c2f8b129689a7c12abe7f35e7727d4e18f670d0a",
          "message": "perf(benchmarks): track dead-code JSON pipeline",
          "timestamp": "2026-08-19T11:48:45+02:00",
          "tree_id": "a3efe51eadca4bb26f3e02b73b1284f3092f8419",
          "url": "https://github.com/fallow-rs/fallow/commit/c2f8b129689a7c12abe7f35e7727d4e18f670d0a"
        },
        "date": 1787133155426,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9736955,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49205,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1185456,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8391,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "2ed7d02a94c6ff2d3578406a2dc269f31ce3667f",
          "message": "perf(benchmarks): cover audit impact closure",
          "timestamp": "2026-08-19T13:54:04+02:00",
          "tree_id": "1647e61081d7d2497306d0e0d9ff4f2171407e7a",
          "url": "https://github.com/fallow-rs/fallow/commit/2ed7d02a94c6ff2d3578406a2dc269f31ce3667f"
        },
        "date": 1787140685645,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9688985,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49128,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1178137,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8378,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "0bae5b1d371aeadeea21373ced69584dfdcacbcd",
          "message": "perf(benchmarks): cover recommend workspace JSON",
          "timestamp": "2026-08-19T15:16:31+02:00",
          "tree_id": "e8991454566277d66ccf2ef7e58cdab167cb8b81",
          "url": "https://github.com/fallow-rs/fallow/commit/0bae5b1d371aeadeea21373ced69584dfdcacbcd"
        },
        "date": 1787145642493,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9736883,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49205,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1190541,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8447,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "aed49b4addf9549b6271997b0c0a2e38dfe14b08",
          "message": "perf(benchmarks): cover warm coverage gaps",
          "timestamp": "2026-08-19T20:03:53+02:00",
          "tree_id": "b03c21c46732dd8858caa5eccf4f8258be7cd6ac",
          "url": "https://github.com/fallow-rs/fallow/commit/aed49b4addf9549b6271997b0c0a2e38dfe14b08"
        },
        "date": 1787162879832,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9700811,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49201,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1187182,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8434,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "fecc8c221bd0c75346720209d8e6ec18d78f8495",
          "message": "perf(benchmarks): cover hotspot ownership churn",
          "timestamp": "2026-08-19T21:24:21+02:00",
          "tree_id": "949f3bf700e92734ca6877fb6549b0ee69f2adfa",
          "url": "https://github.com/fallow-rs/fallow/commit/fecc8c221bd0c75346720209d8e6ec18d78f8495"
        },
        "date": 1787167793420,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9736555,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49210,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1187415,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8419,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "08565e1db1c5166af5dc4f2907894657fd347dda",
          "message": "perf(benchmarks): track runtime coverage analysis",
          "timestamp": "2026-08-20T09:08:41+02:00",
          "tree_id": "8539fd70ef84c9dad35bfc6506a6a6046805f8c6",
          "url": "https://github.com/fallow-rs/fallow/commit/08565e1db1c5166af5dc4f2907894657fd347dda"
        },
        "date": 1787210036501,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9747915,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49229,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1191527,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8443,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "1881f4d5fe0a9410f807e6c236d23537279a1a7b",
          "message": "perf(benchmarks): cover inspect evidence bundle\n\n* perf(benchmarks): cover inspect evidence bundle\n\n* perf(benchmarks): bound inspect simulation corpus",
          "timestamp": "2026-08-20T09:41:01+02:00",
          "tree_id": "36252efcca8f4b75152b56a06c9e7b891022acd3",
          "url": "https://github.com/fallow-rs/fallow/commit/1881f4d5fe0a9410f807e6c236d23537279a1a7b"
        },
        "date": 1787211938167,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9714107,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49217,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1184463,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8430,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "9450f933307dcbad05b7ee2e80df5251221d8ebf",
          "message": "perf(benchmarks): cover list boundaries",
          "timestamp": "2026-08-20T10:14:48+02:00",
          "tree_id": "c7534b59d2992a42abe31ae713b31cd36ab90199",
          "url": "https://github.com/fallow-rs/fallow/commit/9450f933307dcbad05b7ee2e80df5251221d8ebf"
        },
        "date": 1787213995240,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9746115,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49218,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1186637,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8405,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "efb888b3c84f2a377fc65f0d644e8ad530ca9595",
          "message": "perf(benchmarks): cover watch filter initialization",
          "timestamp": "2026-08-20T11:02:37+02:00",
          "tree_id": "bfd82c23323f328eb35b78f057a8d4b93dc3e448",
          "url": "https://github.com/fallow-rs/fallow/commit/efb888b3c84f2a377fc65f0d644e8ad530ca9595"
        },
        "date": 1787216877163,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9750475,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49222,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1224649,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8614,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "1a6cdc5cc00c6158571362cf00126e5ac8112e0d",
          "message": "perf(benchmarks): cover audit review brief assembly\n\n* perf(benchmarks): cover audit review brief assembly\n\n* perf(benchmarks): route audit output changes",
          "timestamp": "2026-08-20T13:05:39+02:00",
          "tree_id": "434c60962ccb0793c4e3dafd6e55159cfc2f2244",
          "url": "https://github.com/fallow-rs/fallow/commit/1a6cdc5cc00c6158571362cf00126e5ac8112e0d"
        },
        "date": 1787224252168,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9713737,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49162,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1177428,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8353,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "7472746b01cba1e7e3436fc1837b4c130334be4c",
          "message": "perf(benchmarks): cover trace family outputs\n\nAdd stable CodSpeed coverage for graph trace output and clone tracing by location and fingerprint.",
          "timestamp": "2026-08-20T15:07:08+02:00",
          "tree_id": "0fbf54f488d8ba7b6390fe7dfb00fafb89619834",
          "url": "https://github.com/fallow-rs/fallow/commit/7472746b01cba1e7e3436fc1837b4c130334be4c"
        },
        "date": 1787231491001,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9711611,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49213,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1190501,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8415,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "1380b8d61a8e874f8d7368ae0b88aff15edf36cc",
          "message": "perf(benchmarks): cover derived security outputs\n\nAdd stable CodSpeed coverage for survivors verdict joins and unresolved-callee blind-spot grouping.",
          "timestamp": "2026-08-20T15:41:34+02:00",
          "tree_id": "bbd2cdf817c722b6e5173159b821a190e61d61ed",
          "url": "https://github.com/fallow-rs/fallow/commit/1380b8d61a8e874f8d7368ae0b88aff15edf36cc"
        },
        "date": 1787233615000,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9743771,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49222,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1146577,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7055,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "69dc2c13221ee32b578617d659352c5218191888",
          "message": "perf(benchmarks): cover Istanbul health CRAP matching\n\nAdd stable CodSpeed coverage for Istanbul ingestion, declaration-alias matching, CRAP scoring, and health report assembly.",
          "timestamp": "2026-08-20T16:38:05+02:00",
          "tree_id": "0469723ab0df950cb17a1ebf35b5cc0a04eef186",
          "url": "https://github.com/fallow-rs/fallow/commit/69dc2c13221ee32b578617d659352c5218191888"
        },
        "date": 1787236989998,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9750723,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49224,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1191487,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8444,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f83974cf024b987f2f4dc5ba51a5a83fa6744976",
          "message": "fix(extract): record JSX namespace member tags as member accesses\n\nThe syntactic dead-code scan only recorded plain-expression member accesses, so a namespace import rendered exclusively through JSX member tags (<SC.Wrapper />) kept its exports reported as unused, and in entry-point files every sibling export of the namespace target was falsely flagged. JSX member-expression tags now record member accesses like their plain-expression spelling, covering nested receivers, this receivers, and star re-export resolution. Behavior change: namespace imports in non-entry consumers narrow to the members actually used instead of marking every export used, so genuinely unused siblings surface for the first time. Extraction and graph cache versions were bumped, so the first run after upgrading performs one cold re-analysis.\n\nFixes #2348",
          "timestamp": "2026-08-21T22:14:36+02:00",
          "tree_id": "1b98e2f31f26ddbfb312439022ea6a7c60ca9c0e",
          "url": "https://github.com/fallow-rs/fallow/commit/f83974cf024b987f2f4dc5ba51a5a83fa6744976"
        },
        "date": 1787344042264,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9725915,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49196,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1190645,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8431,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "cbd9cf4b5d8507f8a4d7b49e964c4b76eca2e894",
          "message": "fix(extract): keep non-exported namespace members out of file-level exports\n\n## What was broken\n\nA namespace declared without the `export` keyword had its inner `export` declarations recorded as file-level exports. For the issue's snippet\n\n```ts\n// src/ns.ts\nnamespace Foo {\n  export const inner = 1;\n}\nexport {};\n```\n\n`fallow dead-code` reported `inner` as an unused export with an auto-fixable remove-export action. Following that advice removes `inner` from the namespace's public surface and breaks every consumer of `Foo.inner`. The same leak hit `declare namespace Foo {}`, legacy `module Foo {}`, dotted `namespace A.B.C {}`, a namespace exported from inside a local namespace, and a local namespace exported afterwards through `export { Foo }` or `export = Foo` (its members were reported unused even while a consumer used `Foo.member`).\n\n## Root cause\n\n`namespace_depth` is only raised inside `visit_export_named_declaration`, so a `TSModuleDeclaration` reached as a plain statement never entered namespace mode and its inner `export` statements fell through to the file-level export recorder. Re-using `namespace_depth` for these bodies was not an option: that path also routes inner declarations into `pending_namespace_members`, which expects an exported owner to attach them to, and a local namespace has none (TS2395 forbids merging a local namespace with an exported declaration of the same name).\n\n## The fix\n\nEarliest incorrect layer is extraction. `ModuleInfoExtractor` gains a separate `local_namespace_depth` counter (documented on the field, issue #2356). `visit_ts_module_declaration` raises it for an identifier-named namespace reached while `namespace_depth == 0` and `ambient_module_depth == 0`, which is exactly a namespace without an exported owner: `export namespace Foo` raises `namespace_depth` before its declaration is walked, and `declare module '...'` raises `ambient_module_depth`. While the counter is non-zero, `visit_export_named_declaration` walks the statement without recording an export and without queueing namespace members, so imports referenced inside the body keep their credit and nested namespaces still reach the module-declaration arm. `namespace_depth` is untouched, so the scope-binding helpers treat the inner statements exactly as before.\n\nExported namespaces keep their existing member extraction, `declare module '<specifier>'` bodies keep the #2349 behaviour, and direct `declare global { export ... }` bodies keep today's behaviour (`declare global` is its own AST node and never enters the namespace arm). A namespace nested inside `declare global` (`declare global { namespace NodeJS { export interface ProcessEnv {} } }`) reaches the arm with no exported owner and now follows the local rule; previously `ProcessEnv` was reported as an unused type in a `.ts` file.\n\n## Behavior change\n\nNarrowing only: local namespace members stop producing `unused-export` and `unused-type` findings and their remove-export actions. Existing `fallow-ignore` suppressions placed above these declarations as a workaround surface as stale-suppression findings and can be removed. Genuinely unused exports next to a local namespace and unused exported namespaces keep reporting.\n\n## Cache invalidation\n\nBoth cache layers are bumped with doc comments naming #2356:\n\n- extract `CACHE_VERSION` 273 -> 274, so warm extraction caches re-extract the export set;\n- `GRAPH_CACHE_VERSION` 35 -> 36, because unused-export verdicts are read off the persisted export set.\n\nWarm-cache proof on the fixture, two debug binaries sharing one `.fallow` directory:\n\n1. origin/main binary, cold run: reports `inner`, `value`, `viaSpecifier`, `unusedSibling`, `Exported` and writes the cache.\n2. patched binary on that warm cache: reports only `unusedSibling` and `Exported`.\n3. second warm run of the patched binary: identical.\n4. origin/main binary again on the patched cache: its version gate rejects the newer cache and it replays its own stale result, confirming the verdict is version-gated rather than accidental.\n\n## How it was tested\n\n- Extract-level tests (all failed before the fix): the issue snippet, `declare namespace` with const/interface/type/function/class/enum members, legacy `module Foo {}`, dotted `namespace A.B.C {}`, `namespace A { export namespace B { ... } }` staying entirely local, body references keeping value and type import credit, and a namespace nested inside `declare global`.\n- Pins written before the code change: `export namespace Foo { ... }` still records one export with members `x`, `Bar`, `y`; direct `declare global { export ... }` bodies still record file-level exports.\n- Integration test on fixture `issue-2356-local-namespace`: `inner`, `viaSpecifier`, and `value` are not reported; `unusedSibling` and the unused exported namespace `Exported` still report; `helper` (only referenced inside a local namespace body) keeps its credit and `helper.ts` stays reachable.\n- Mutation matrix: with the non-test source hunks stashed, the seven extract repro tests and the integration test fail; the two pins pass by design.\n- CLI run of the issue's exact snippet with the patched binary: no findings (baseline reported `inner` with an auto-fixable remove-export action).\n- Real-project smokes: `dead-code --format json` with the baseline and patched binaries on the in-repo viz-frontend and editors/vscode projects, both complete cleanly with no finding differences (neither project declares a non-exported namespace).\n- Gates: cargo fmt check, clippy workspace with warnings denied, workspace tests (`--lib --bins --tests --examples`), bench check, cargo doc with warnings denied, typos, hidden-unicode scan, and comment-quality check.\n\nFixes #2356",
          "timestamp": "2026-08-22T10:00:23+02:00",
          "tree_id": "42c1f9923d8ab608248d91b9c0ce9ce2a9095460",
          "url": "https://github.com/fallow-rs/fallow/commit/cbd9cf4b5d8507f8a4d7b49e964c4b76eca2e894"
        },
        "date": 1787385996319,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9716299,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49221,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1177098,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8403,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "c3df8857ec0fca10f1b359946ec89a0208a50715",
          "message": "perf(benchmarks): cover Bun lock override analysis",
          "timestamp": "2026-08-22T09:29:16Z",
          "tree_id": "43e370925077f83b46ba7073598895fb2f8ae382",
          "url": "https://github.com/fallow-rs/fallow/commit/c3df8857ec0fca10f1b359946ec89a0208a50715"
        },
        "date": 1787391169992,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9762907,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49241,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1146577,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7055,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "bfeee75e2d04e637a1bab843241eb988ebbe1f42",
          "message": "fix(extract): credit ambient-module star re-exports without a file-level surface\n\n## What was broken\n\n`declare module 'pkg' { export * from './impl' }` (and the `export * as ns from './impl'` form) was still recorded as a file-level star re-export of the declaring file. Two shapes went wrong, both surfaced in the panel review of #2351:\n\n- When the declaring `.d.ts` was an entry point (a `package.json` `types` declaration), every export of `./impl` was laundered into that entry's public surface: `inspect --file src/ambient.d.ts` listed a `*` re-export edge the file does not have, and a genuinely unused `export const helperA` in `./impl` was credited through it.\n- When the declaring file was a reachable non-entry module (a `.ts` file carrying the augmentation), nothing in `./impl` was credited at all, so its exports were reported as unused even though the ambient declaration states that all of them are reachable through `pkg`.\n\nSeparately, `declare module 'shim' { export * from 'some-dep' }` counted as runtime usage of `some-dep`, while the named form (`export { A } from 'some-dep'`) has counted as type-only usage since #2349. The classification was undecided and untested.\n\nReview of the earlier versions of this change found defects in the credit shape, all reproduced with executed fixtures and fixed here:\n\n- The whole-module import credited the target only in the type namespace, so a value export sharing its name with a type (`interface User` plus `const User`, zod-style `const User` plus `type User`) lost its value credit.\n- Plain `export *` inside an ambient body credited the target's `export default`, which ES star re-exports never forward.\n- The both-namespace credit was then applied to every type-only import without a binding, so the named ambient form from #2349, explicitly type-only ambient re-exports (`export type { X } from`, `export { type X } from`), and `import('./impl').X` type references in TypeScript and JSDoc stopped reporting the value half of such a pair, a behaviour change outside the star forms.\n- The ambient star chain followed the target's `export *` sources but not its `export * as sub` sources, so an entry-point ambient `.d.ts` pointing at a barrel with `export * as sub from './sub'` newly reported every export of `sub.ts` (main credited them through laundering), and the non-entry shim variant stayed a false positive.\n\n## Root cause\n\n`visit_export_all_declaration` had no ambient-module guard: it pushed a `ReExportInfo` regardless of `ambient_module_depth`, so the graph built a `ReExportEdge` on the declaring module and routed the target's exports through entry-point star propagation (laundering) or through per-name consumer propagation (nothing credited, because an ambient body never imports a name).\n\nIn the graph, `desired_import_namespaces` restricts every `is_type_only` import to type space, which is right for every type-only form except the ambient star: the star forwards every export in both meanings. The namespace re-export phase (`propagate_namespace_re_exports`) credits every export of an `export * as sub` source only when the barrel is an entry point, and the star-chain seed only followed plain `export *` edges, so the `sub` namespace behind an ambient target was never treated as exposed.\n\n## The fix\n\nEarliest incorrect layer is extraction. Inside an ambient body, `export *` and `export * as ns` now record one type-space `ImportedName::Namespace` import with an empty local name and push no `ReExportInfo`. `export * as ns` additionally records one type-space `ImportedName::Default` import with the same span, because the namespace object it forwards exposes `ns.default`. `export type *` inside the body keeps the pre-existing file-level type-only star re-export: the whole-module shape carries no type modifier. A side-effect edge was deliberately not used: it carries no symbol and would have dropped the credit for every target export.\n\nIn the graph, `ImportedSymbol::is_ambient_star` names exactly that shape (type-only, no local binding, `Namespace` or `Default`). `desired_import_namespaces` credits both the type and the value namespace for it and for nothing else, so `import type { x }`, the ambient named form, explicitly type-only ambient re-exports, and `import()` type references keep their type-space credit exactly as on main. The empty-local-name namespace branch of `attach_symbol_reference` routes the ambient star to `mark_star_surface_referenced_at_site`, which skips `ExportName::Default`; runtime whole-module edges (dynamic-import patterns) keep the full `mark_all_exports_referenced_at_site` and behave exactly as on main.\n\n`ModuleGraph::collect_ambient_star_targets` computes the closure of modules an ambient star reaches through the target's own `export *` and `export * as sub` chains. Star propagation treats every member like an entry barrel for its `export *` sources (named exports in both namespaces, never `default`), and the namespace re-export phase treats every member as exposing its `export * as sub` sources, crediting every export of `sub`, `default` included, because `sub.default` is reachable. The entry-point closure and dynamic-import pattern targets are untouched.\n\nBare-specifier re-exports inside ambient bodies are type-only package usage for both the named and the star form (ambient bodies are erased at runtime). A production dependency referenced solely through such a re-export yields the existing type-only-dependency finding in `--production` mode.\n\n## Behavior change\n\n- Ambient star forms (`export *`, `export * as ns` inside `declare module '...'`) credit the target's full ES star surface in both namespaces without creating an export surface on the declaring file: every named export, including names reached through the target's `export *` and `export * as sub` chains, and every export (`default` included) of each such `sub` namespace. Plain `export *` never credits a `default` export (same as main); `export * as ns` does. Fewer unused-export findings for targets behind a non-entry ambient declaration and for `export * as sub` chains behind an entry-point ambient declaration; `inspect --file` and `dead-code --trace` no longer show a re-export edge on the declaring file.\n- An unreferenced `.ts` file whose only content is an ambient star declaration is now reported as an unused file, while its ambient star still credits the target's exports. On main the laundered star re-export edge kept such a shim off the unused-file list; the named ambient form from #2349 already reported this way, so the star form now matches it and the existing rule that references from an unreachable module never save a target's export.\n- Named and type-only ambient re-exports are unchanged from #2349, and `import()` type references in TypeScript and JSDoc are unchanged from main: the value half of a same-name type and value pair reached only that way keeps reporting.\n- `declare module 'x' { export * from 'some-dep' }` now classifies as type-only usage. A production dependency referenced only that way surfaces as a type-only-dependency finding in production mode (finding-count increase for that shape; `ignoreDependencies` or moving the package to `devDependencies` resolves it). Documented under Changed in the CHANGELOG.\n\n## Cache invalidation\n\n- extract `CACHE_VERSION` 274 -> 275: warm caches would replay the star `ReExportInfo` and the runtime classification of a bare-specifier ambient star.\n- `GRAPH_CACHE_VERSION` 36 -> 37: the persisted graph carries the old `ReExportEdge`, the laundered references, the runtime package usage, and type-lane-only credits; a graph-cache hit skips the build entirely.\n\nWhile this PR was in review, #2356 landed on main and took 274/36, the values this branch had chosen from 273/35. The branch is rebased onto current main and bumps past #2356; both doc comments name #2357 and state that #2356 took the previous values.\n\nWarm-cache proof on a scratch copy of the fixture sharing one `.fallow` directory: a binary built from current main (c3df8857e, cache versions 274/36) runs cold, writes `cache.bin` and `graph-cache.bin`, and reports the laundered and uncredited shapes; the rebased binary (275/37) on that warm cache reports exactly its own cold output, twice, with `inspect --file src/ambient-ns.d.ts` showing `export_count: 0` and `re_exports: []`, and production mode reporting `named-dep` and `star-dep` as type-only.\n\n## Known nit\n\n`ModuleGraph::collect_ambient_star_targets` is computed twice per graph build, once in Phase 2c (`propagate_namespace_re_exports`) and once in Phase 4 (`collect_entry_star_targets`). Each pass is linear in edges plus re-exports and returns an empty set on projects without an ambient star. Sharing one result needs the closure threaded through both phase signatures (the two call sites live in different build functions and the Phase 2c function has its own unit tests), so it is left as is here.\n\n## How it was tested\n\n- Extract-level tests: ambient `export *` records no file export, no `ReExportInfo`, and exactly one type-only `Namespace` import with an empty local name; `export * as ns` records that import plus one type-only `Default` import; `export type *` keeps the file-level type-only star re-export; ambient bare-specifier named and star re-exports are type-only imports of their package.\n- Graph unit tests: `desired_import_namespaces` returns both namespaces for the unbound type-only `Namespace` and `Default` symbols and type-only for an unbound `Named` symbol and for a bound type-only import; the ambient star credits `interface Foo` in type space and `const Foo` in value space and leaves `default` unreferenced; the extra default import credits `default`; an unbound named type-only import credits the interface half only; a non-entry ambient star into a barrel with `export * as sub` credits every export of `sub.ts` (`default` included) and the named exports of `sub.ts`'s own `export *` source, never that source's default.\n- Integration fixture `issue-2357-ambient-star-reexport` (seven tests): entry-point `ambient.d.ts` `export * from './impl'` where `impl.ts` is a star barrel with `export * as sub`; `ambient-ns.d.ts` `export * as impl`; reachable non-entry `shim.ts` whose target has `export * as shimSub`; `ambient-named.d.ts` `export { Pair }`; `ambient-typed.d.ts` with `export type { TypedPair }` and `export { type InlinePair }`; `import-type.ts` with a TS `import()` type and a JSDoc `@type {import()}`. Asserts: every name on the star surfaces is credited, both halves of the `Merged`, `DeepMerged`, `User`, and `SubPair` pairs keep their credit in their own namespace; the value halves of `Pair`, `TypedPair`, `InlinePair`, `ImportPair`, and `JsdocPair` keep reporting while their type halves are credited; the `impl.ts`, `impl-deep.ts`, and `sub-deep.ts` defaults keep reporting while the `ns-impl.ts`, `sub.ts`, `sub-nested.ts`, and `shim-sub.ts` defaults are credited; the three declaring files have empty `exports` and `re_exports`; `sub.ts` and `shim-sub.ts` exports carry namespace-import references from their barrels and `sub-deep.ts` carries the star-chain reference; `unusedSibling` and `namedSibling` keep reporting; production mode classifies `named-dep` and `star-dep` as type-only.\n- Mutation matrix: every test that pins new behaviour fails with the branch's non-test source hunks reverted to main; the tests that pin behaviour unchanged from main fail with only the last round's source hunks reverted and pass on main by design.\n- Review attack fixtures from all rounds re-run with the main and patched binaries: nested `export * as` chains behind an entry-point and a non-entry ambient star credit fully (main reported the `sub.ts` level on the non-entry shape and both deeper levels everywhere); the real-entry control, `import.meta.glob` barrels, bound `import type`, explicitly type-only ambient re-exports, `export type *`, and `import()` type references are identical to main; cycles through `export * as` terminate.\n- `dead-code --trace` on the fixture agrees with the findings: credited exports report their namespace-import or re-export reference, unused defaults and the value halves of the non-star pairs report no references.\n- Real-project smokes: `dead-code --format json` with the main and patched binaries on the in-repo `viz-frontend` and `editors/vscode`; normalized outputs identical.\n- Rebased onto main c3df8857e (#2356, #2358, #2359 and the Bun lock benchmark commit). The five conflicts (both cache constants, the integration test module list, the extract visitor tests, and extract-internals.md) were resolved by hand keeping both sides; `git range-diff` against the pre-rebase head differs only in the cache numbers, the #2356 context, and the placement of the #2357 entries after main's #2356 entries. The #2348, #2349, #2356 and #2357 integration fixtures all pass on the rebased head.\n- Gates: cargo fmt check, clippy workspace all-targets with warnings denied, full workspace test suite, bench check, cargo doc with warnings denied, typos, hidden-unicode scan, and comment-quality check, all green.\n\nFixes #2357",
          "timestamp": "2026-08-22T15:02:35+02:00",
          "tree_id": "e9e3eb60035a83893357d49e1052ef1c58d8bde8",
          "url": "https://github.com/fallow-rs/fallow/commit/bfeee75e2d04e637a1bab843241eb988ebbe1f42"
        },
        "date": 1787404073275,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9737539,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49209,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1203831,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8437,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "be0b5d7b4e51e2cee2d959f22e7f2e71cfa447c9",
          "message": "fix(extract): record Astro and MDX member-expression tags as member accesses\n\n## What was broken\n\n#2348 taught the JSX visitor to record member-expression tags (`<SC.Wrapper />`) as member accesses, but two template pipelines never reached that visitor:\n\n- **Astro**: the markup scan credited only the tag root, so `<SC.Card />` kept `SC` alive as an import binding while the `SC.Card` access was never recorded. An entry-point `.astro` page with a namespace import still reported every export of the namespace target as unused (the pre-#2348 false-positive shape), and a non-entry `.astro` component fell back to crediting every export.\n- **MDX**: the extractor parsed only `import` / `export` lines, so JSX member tags in the prose body were invisible to usage crediting entirely.\n\n## Root cause\n\n`narrow_namespace_references` reads `ResolvedModule.member_accesses`, which the graph copies verbatim from `ModuleInfo.member_accesses`. Neither `parse_astro_to_module` nor `parse_mdx_to_module` ever populated it from markup, so the namespace binding arrived with an empty accessed-member list: entry consumers narrowed to nothing, non-entry consumers marked everything.\n\nNarrowing is only sound on a complete access stream, and a text scanner of a template cannot promise completeness shape by shape. Earlier revisions recorded dotted tags alone, then added the expression regions, then a structural guard over the template; review of that revision found two remaining gaps: the guard did not cover the script side (a namespace passed bare in the Astro frontmatter or on an MDX statement line still narrowed), and MDX prose recorded dotted chains for any root, so a documentation sentence naming `process.env.API_KEY` turned the MDX module into a secret source for `fallow security`. This revision closes both.\n\n## The fix\n\nThe guarantee, for Astro and MDX consumers: **a namespace import (or a CSS module default import, the other binding whose exports the graph narrows by member access) narrows only when every mention of the binding in the whole file was structurally understood; otherwise every export is credited.** Structurally understood means: in Astro markup a component tag root or a parsed `{ ... }` expression region; in MDX a prose line outside code; in the Astro frontmatter or on an MDX `import` / `export` line a static dotted access whose `(root, member)` pair the visitor recorded, or a JSX tag root.\n\n- `crates/extract/src/template_expression_scan.rs`\n  - `record_unexplained_mentions` (template side, unchanged): every identifier-boundary mention of an import binding outside the byte spans the structured template passes classified records a whole-object use.\n  - `record_unexplained_script_mentions` (new, script side): the visitor records a bare identifier as a whole-object use only for an allow-list of positions (spread, `Object.keys(NS)`, `for ... in`, computed non-string access, rest destructuring), so an alias (`const N = NS`), a cast (`NS as T`), a call argument (`pick(NS)`), `Object.assign({}, NS)`, an array literal (`[NS]`), a props object (`{ all: NS }`), `export const all = NS`, or a JSX attribute on a statement line (`<Callout all={NS} />`) left no trace. The guard walks the raw script text, skips the import declaration spans, and explains a mention only by a recorded static dotted access or a JSX tag root; everything else records a whole-object use. Set membership against the visitor's recorded pairs (instead of a mention count) keeps it independent of how many times one access is recorded and of the deduplication applied to template accesses. `narrowable_import_locals` scopes this guard to namespace imports and CSS-module default imports, so a class or enum named in a frontmatter type annotation or `new` expression keeps the visitor's member crediting (the markup guard still covers every import binding, as before).\n  - `scan_template_usage` in MDX prose mode records a dotted chain only when its root is an import local of the file. Every crediting consumer (namespace, CSS module, enum and class member) is keyed by import locals, so nothing is lost; the security secret-source index, which also reads member accesses, no longer sees `process.env.X` / `import.meta.env.X` pairs spelled in prose.\n- `crates/extract/src/astro.rs`: `extend_unexplained_frontmatter_mentions` runs the script guard over the frontmatter body after the template passes.\n- `crates/extract/src/mdx.rs`: the statement lines are kept with their source offsets and `collect_unexplained_statement_mentions` runs the script guard over them after the prose scan.\n- `crates/extract/src/visitor/mod.rs`: `extend_whole_object_uses` skips names already present, so the region pass, the template guard, and the script guard cannot grow duplicates in the persisted record.\n\n## Behavior change\n\n- A namespace import in a non-entry Astro or MDX consumer whose every mention is a dotted tag, a parsed expression, or a recorded dotted script access narrows to the members actually accessed (the same widening #2348 introduced for `.tsx`), so genuinely unused siblings surface. Every other mention keeps mark-all crediting: a `define:vars` or `set:html` directive on a `<style>` / `<script>` tag, an HTML comment, text content, an attribute string, an expression the parser rejects, an MDX fenced code block or inline code span, a template literal inside an MDX expression, and a script-side alias, cast, call argument, `Object.assign`, array or object literal element, or JSX attribute value. Mark-all can hide an unused sibling; it never reports a used one.\n- Entry-point pages follow the same rule: a dotted-only namespace narrows (the rendered members are credited, the rest report), and an unexplained mention credits every export.\n- Astro expression accesses reach the consumers that already read frontmatter accesses (CSS module default-import narrowing, enum and class member crediting), matching `.tsx`. The markup guard covers those bindings too: an enum or class mentioned in an HTML comment, text content, or an attribute string of an Astro or MDX template credits every member (the `.tsx` visitor would not). The script guard does not cover them, so class and enum member crediting on the frontmatter and statement side stays at `.tsx` parity.\n- MDX prose records dotted chains only for import locals, so `fallow security` no longer reports a `client-server-leak` for a `\"use client\"` file importing an MDX document whose prose mentions `process.env.API_KEY`.\n\n## Cache invalidation\n\n- `CACHE_VERSION` 274 -> 276 (`crates/extract/src/cache/types.rs`)\n- `GRAPH_CACHE_VERSION` 36 -> 38 (`crates/graph/src/cache/mod.rs`)\n\nRebased onto `main` after #2356 took 274 / 36 and again after #2357 took 275 / 37, so the final values are 276 / 38. Warm-cache proof (executed at the first rebase, before the second bump) on a copy of the fixture: the `main` binary (274 / 36) ran cold and wrote `.fallow/`, the rebased binary (275 / 37) on that cache reported the fixed set, a second warm run and a fresh `--no-cache` run were identical after dropping `analysis_run_id` / `elapsed_ms`.\n\n| run | unused exports |\n|---|---|\n| `main` cold and warm | every export of the entry pages' namespaces (65 findings, incl. `UsedStyle`, `Layout`, `UsedBlock`, and all `ea-*` / `em-*` shape members) |\n| rebased warm 1, warm 2, cold | `ActuallyUnusedStyle, UnusedSibling, UnusedMdBlock, UnusedDocSibling, AttrUnused, CallUnused, MdAttrUnused, MdCallUnused, MdMultiUnused` plus the four `DottedUnused` precision rows and the seven exports the two `script-shapes.mdx` documents declare themselves (`all`, `Demo`, `moon`, `default`; reported by `main` too), 20 findings |\n\n## How it was tested\n\n- Scan unit tests (`template_expression_scan.rs`): prose chains with a foreign root (`process.env.API_KEY`, `items.map`) record nothing while import-local roots still record; the script guard explains a mention only by a recorded dotted access or a tag root (alias, cast, call argument, `Object.assign`, literal element, property value, `export const all = NS`, JSX attribute, unrecorded `NS.Sun`, `NS?.Moon`, `NS[\"Moon\"]`, spread all keep mark-all; recorded `NS.Moon`, `NS.Star.Deep`, `<NS.Moon>`, `</NS.Moon>`, `outer.NS`, `NSX` do not); import declaration spans excluded in file coordinates; `narrowable_import_locals` covers namespace and CSS-module bindings only.\n- Astro unit test: alias, cast, call argument, `Object.assign`, array literal, props object, and a CSS module passed in an object each record one whole-object use; a dotted-only frontmatter use records none; a class in a type annotation and `new` expression is not guarded; `Object.keys(AL)` next to `const N = AL` records `AL` once.\n- MDX unit tests: `export const all = NS`, `<Callout all={NS} />` on an exported component, and a default-export layout each record a whole-object use while a dotted-only statement use stays precise; an unfenced prose `process.env.API_KEY` records no member access.\n- Integration fixture `tests/fixtures/issue-2355-astro-mdx-member-tags`: `astro_and_mdx_script_mentions_keep_mark_all` pins the six frontmatter shapes and the three statement shapes for an entry page and a non-entry consumer of each kind (no `Star` / `Moon` / `Shielded` reported), plus a dotted-only namespace per consumer whose `DottedUnused` sibling must still report. Fixture `tests/fixtures/issue-2355-mdx-prose-env-mention`: `mdx_prose_env_mention_is_not_a_secret_source` runs `client-server-leak` on a `\"use client\"` page importing an MDX document with an unfenced env mention (no finding) next to a control client importing a module that really reads `process.env.API_KEY` (finding).\n- Mutation evidence: six targeted neutralisations at HEAD (prose root filter off, script guard off, dotted mention always explained, import span not excluded, named imports guarded, whole-object dedup removed) each fail the tests that pin that piece; with every round-4 source hunk reverted the two new integration tests fail and the earlier ones pass.\n- Reviewer fixtures from the previous round (alias, as-cast, call-arg, Object.assign, array literal, props pass, MDX statement whole / attr whole / default whole): zero unused-export findings with the fixed binary where the baseline had zero; the `.tsx` twins are unchanged (pre-existing visitor gap, tracked as a follow-up). Security probe (Next.js `\"use client\"` page importing an MDX with `Set process.env.API_KEY before running`): baseline 0, previous revision 1, fixed 0.\n- Real-world runs, baseline vs fixed, `dead-code` and `security` with `--no-cache --format json`: Starlight `docs/` (25 `.astro`, 289 `.mdx`), `packages/starlight` (61 `.astro`, 12 `.mdx`), and the Docusaurus website (1326 `.mdx`) produce identical normalized output. None of the trees uses a namespace import or a CSS module from `.astro` / `.mdx`, and no `\"use client\"` module imports an MDX document there, so there is nothing for the narrowing, the guards, or the prose root filter to move.\n- Gates on the rebased tree: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace --lib --bins --tests --examples`, `cargo check --workspace --benches`, `cargo doc` with `-D warnings`, `typos`, hidden-unicode scan, comment-quality check.\n\n## Known, deferred\n\n- `push_member_tag_accesses` is a third text-side dotted-chain splitter next to `glimmer::emit_chain_member_accesses` and the Vue/Svelte dotted-tag path; same per-hop semantics, left for a separate consolidation.\n- Pre-existing, to be filed as follow-ups: in `.ts` / `.tsx` a namespace passed whole through a JSX attribute, a plain call argument, or an assignment (`const N = NS`) is not a whole-object use in the visitor, so the namespace narrows to its dotted accesses (the Astro and MDX guards now cover those shapes on the template and script side); an MDX prose line that starts with `import ` or `export ` is classified as a statement and breaks the import parse of the file.\n\nFixes #2355",
          "timestamp": "2026-08-22T15:09:03+02:00",
          "tree_id": "927eccb97a6701dee89af76b0ad15d34d10d95d9",
          "url": "https://github.com/fallow-rs/fallow/commit/be0b5d7b4e51e2cee2d959f22e7f2e71cfa447c9"
        },
        "date": 1787404484658,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9750723,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49230,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1188606,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8429,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "dc03fd1484261a623e044e1dbe97378095bae4ac",
          "message": "fix(core): analyze package.json resolutions as a bun dependency-override source\n\n## What was broken\n\nbun honours Yarn-style `resolutions` in `package.json` as an alias of `overrides`, but the dependency-override analyzer only read the top-level `overrides` object, `pnpm.overrides`, and `pnpm-workspace.yaml`. A bun repository that pins transitive versions under `resolutions` was never analyzed: no `unused-dependency-overrides` or `misconfigured-dependency-overrides` findings with any lockfile, and since #2362 no `bun-lockb-override-resolution-skipped` diagnostic next to a `bun.lockb` either, because no override state was gathered. Both repros from the issue (a `resolutions` manifest next to a `bun.lockb`, and a `resolutions` manifest next to a text `bun.lock` that resolves only `ws`) produced nothing: no findings, no diagnostic, no stderr warning.\n\n## Root cause\n\n`gather_pnpm_override_state` builds its state from three parsers (`pnpm-workspace.yaml`, `pnpm.overrides`, and the npm `overrides` object) and returns `None` when all three are empty, so the detectors and the bun.lockb skip path never ran for a `resolutions`-only manifest. The npm parser also hard-codes the `overrides` key and flattens nested objects, and it does not understand yarn's `parent/child` and `**/child` path keys, which bun accepts under `resolutions`.\n\n## The fix\n\n- `fallow_config::parse_bun_package_json_resolutions` parses the top-level `resolutions` object through the shared override entry shape. It reuses the npm line scanner (now parameterised by section key, recording only direct keys for `resolutions` so a nested object bun rejects cannot shift a later key's line) and maps bun's key dialect: bare packages, `@scope/pkg`, `pkg@<2`, the yarn paths `parent/child`, `**/child`, and `parent/**/child` (with `@scope/name` spanning two segments), and the pnpm `parent>child` form, using bun's delimiter rule so `pkg@>=1` keeps its selector. Shapes bun warns about and skips (more than one parent level, a bare scope, a trailing `**`, a non-string value) stay entries without a parsed key or value so the misconfigured detector reports them. `//` comment keys are skipped.\n- The core analyzer gathers `resolutions` only for bun repositories: the root `packageManager` names bun, or no recognised `packageManager` is declared and a `bun.lock` or `bun.lockb` sits at the root (a manifest naming npm, pnpm, or yarn is never a bun repository, even next to a leftover bun lockfile, mirroring the packageManager-first rule the transitive hint already uses).\n- bun precedence, cited in a code comment: `OverrideMap::parse_append` in bun's `src/install/lockfile/OverrideMap.rs` takes the `overrides` property when it exists, whatever its value, and falls through to `resolutions` only when `overrides` is absent. The analyzer therefore ignores `resolutions` whenever the manifest has an `overrides` key, including an empty one.\n- `resolutions` entries run through both detectors with `source: \"package.json\"` (the declaring-file label the field is documented as), the entry's line, and a bun hint that names `resolutions`: `declared under `resolutions`, which bun applies as an alias of `overrides`; may target a transitive dependency; bun install --frozen-lockfile is the ground truth`. A `resolutions`-only manifest next to a `bun.lockb` without a parseable text lockfile now records the existing skip diagnostic.\n- yarn repositories keep the current stance, documented in the module docs: `resolutions` is not parsed, and the inert-`overrides` hint is unchanged.\n- The root manifest is parsed once in `gather_pnpm_override_state` and the declared package manager is passed into `collect_lockfile_packages` instead of being re-derived from the source string.\n\n## Behavior change\n\nAdditive findings only, for bun repositories that declare `resolutions` without `overrides`: new `unused-dependency-overrides` and `misconfigured-dependency-overrides` entries, and the `bun-lockb-override-resolution-skipped` diagnostic next to a `bun.lockb`. npm, pnpm, and yarn repositories, and bun repositories with an `overrides` key, are unchanged. No contract change: `DependencyOverrideSource` keeps its two values (the policy in `docs/backwards-compatibility.md` bumps the envelope for a value added to an existing enum-valued required field, and `source` is documented as the declaring-file label, which `package.json` already is), so only the rustdoc descriptions of `DependencyOverrideSource::PnpmPackageJson` and `UnusedDependencyOverride` changed and `docs/output-schema.json` was regenerated from them. Suppression works with the existing `ignoreDependencyOverrides: [{ \"package\": \"...\", \"source\": \"package.json\" }]` rule.\n\n## Cache invalidation\n\nNone. Override analysis reads the manifest and lockfiles on every run; nothing about it is persisted in the extract or graph caches. Warm-cache proof on the fixture: baseline binary cold run writes `.fallow` and reports nothing, the fixed binary on that warm cache reports `left-pad` and `**/trim-newlines`, and a second warm run is stable.\n\n## How it was tested\n\n- Config unit tests (`npm_overrides::tests`): flat entries with lines; every yarn path shape and the pnpm delimiter form; shapes bun rejects are unparsable or valueless and a nested object does not shift a later key's line; comment keys are skipped; the `resolutions` parser ignores `overrides` and nested `resolutions`, and the npm parser ignores `resolutions`.\n- Core unit tests (`unused_overrides::tests`, mirroring the #2362 set): resolutions-only next to `bun.lockb` records the skip diagnostic once and stays deduplicated; resolutions resolve against a text `bun.lock` (left-pad flagged, `source`, `path`, line, hint); an `overrides` key (even empty) shadows `resolutions`; yarn, npm (with a leftover `bun.lock`), pnpm, and lockfile-less repositories ignore `resolutions`; a root bun lockfile without a `packageManager` field enables them; yarn path keys credit a declared parent and the shapes bun rejects reach the misconfigured detector.\n- Integration test on fixture `tests/fixtures/issue-2367-bun-resolutions` (bun repo, `resolutions` with `ws`, `left-pad`, and `**/trim-newlines`, text `bun.lock` resolving `ws`): both unresolved pins report at their lines with the `package.json` source and the resolutions hint, no misconfigured findings, no skip diagnostic; the `ignoreDependencyOverrides` rule with `source: \"package.json\"` suppresses an entry.\n- CLI test: `dead-code --format json` on the fixture carries both findings with `source` and `path` `package.json` and the resolutions hint, and no workspace diagnostics.\n- Issue repros through the baseline binary (built from e65a9083e) and the fixed binary: the `bun.lockb` repro reports the skip diagnostic in JSON and on stderr on the fixed binary only; the text `bun.lock` repro reports `left-pad` with the bun hint on the fixed binary only; a manifest with both `overrides` and `resolutions` reports nothing on either.\n- Real-project parity: `dead-code --format json --no-cache` on the in-repo `viz-frontend` (package-lock.json) and `editors/vscode` (pnpm-lock.yaml); outputs identical apart from `analysis_run_id` and `elapsed_ms`, stderr identical apart from timestamps.\n- Mutation matrix: with the non-test hunks of `unused_overrides.rs` reverted, the core unit tests, the integration test, and the CLI test fail; with the non-test hunks of the config parser reverted, the config tests and the core crate fail to compile on the missing parser.\n- Gates: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --lib --bins --tests --examples`, `cargo check --workspace --benches`, `RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps --document-private-items`, `typos`, the hidden-unicode scan, the comment-quality check, and `npm run generate:contracts:check`.\n\nFixes #2367",
          "timestamp": "2026-08-23T01:33:15+02:00",
          "tree_id": "a771091012a3f984217d960a2cfd23f1c0a4e4b1",
          "url": "https://github.com/fallow-rs/fallow/commit/dc03fd1484261a623e044e1dbe97378095bae4ac"
        },
        "date": 1787441900290,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9697704,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49211,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1142433,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7053,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "0a95caca69955029a0c41c397f01f2b6b87b1b2f",
          "message": "fix(graph): credit star and namespace chains behind whole-module consumers and entry namespaces\n\n## What was broken\n\nSeveral consumer shapes observe every name on a module's namespace object but credited only the module's direct exports, so names the module only exposes through its own `export * from './deep'` or `export * as sub from './sub'` were reported as unused (#2372):\n\n- A namespace import the graph cannot narrow: `import * as ns from './barrel'` used as a whole object (`Object.values(ns)`, a spread, a destructure with rest), handed on without any member access, or exported under its own name.\n- An `export * as sub` binding imported by name and used as a whole object: `import { sub } from './barrel'` plus `Object.values(sub)`.\n- A dynamic-import pattern match: `import()` with a template, `import.meta.glob`, `require.context`.\n- A bare side-effect `require('./barrel')` with no binding.\n\nFor a real entry point, `export * as sub from './sub'` on the entry, on a barrel the entry reaches through plain `export *`, or named by the entry through a chain of named re-exports (`export { sub } from './barrel'`, or `import * as ns` plus `export { ns }`) credited every direct export of `sub.ts`, but neither sub's own `export * from './deep'` sources nor its own `export * as sub2` sources (#2373).\n\n## Root cause\n\nPhase 2 credits a whole-module consumer through `mark_all_exports_referenced_at_site`, which walks the target's direct export list only. The two phases that credit chains, star propagation (Phase 4, seeded by `collect_entry_star_targets`) and namespace re-export propagation (Phase 2c, gated by `exposes_namespace_object`), knew two seeds: entry points and the ambient-module star closure that #2357 added. `collect_entry_star_targets` also walked plain `export *` edges only, so an `export * as sub` source behind an entry was never treated as exposing a namespace object of its own.\n\n## The fix\n\nOne seed-agnostic closure replaces `collect_ambient_star_targets`:\n\n- `populate_references` (Phase 2) returns every target whose whole namespace object a consumer observed: the empty-local-name namespace branch of `attach_symbol_reference` (ambient stars, dynamic-import pattern matches, and a bare side-effect `require('./barrel')` with no binding) and every mark-all branch of `narrow_namespace_references` (whole-object use, no member access outside an entry point, binding exported under its own name). Both sites record the seed through `AttachContext::observe_whole_namespace_object`, which carries the invariant.\n- `ModuleGraph::collect_exposed_namespace_targets` seeds with those targets plus every `export * as ns` source whose name reaches a consumer the graph cannot enumerate, then closes over both `export *` and `export * as` chains. Phase 2c treats a member as exposing its `export * as` sources; Phase 4 unions the closure into `entry_star_targets`, so every member is treated like an entry barrel for its `export *` sources (named exports, never `default`).\n- A name reaches such a consumer three ways, and the closure applies **the same test Phase 2c applies**: it arrives on an entry point's own export surface, on a module already in the closure, or at a name some importer uses as a whole object. The namespace-edge seeds and the chain walk therefore run to a fixpoint against each other, because a target that joins the closure can itself expose the name a further `export * as ns` edge forwards to it. Each round widens the closure or stops, so the walk terminates, and the closure Phase 2c reads no longer stops one namespace level short of what Phase 2c credits.\n- Each member carries **how much of it is exposed**. A member whose whole namespace object is observed exposes every export, `default` included. A member reached through a plain `export *` exposes every export except `default`, because that is the one name a plain `export *` never forwards, so an `export * as default` declared on such a member hands its target's namespace object to nobody. An entry point exposes its own `default`: it is public API.\n- The surface is matched **by name**, and every hop must uniquely forward the binding. That rule is not restated: `ModuleGraph::forwards_binding` picks the namespace and then calls Phase 2c's own `uniquely_forwards_binding`, so the closure and the phase it pre-computes for cannot drift apart on what a hop forwards. A barrel that declares its own `ns`, or that receives `ns` from two `export *` sources at once, exports a different binding under that name and the chain stops there. Sitting on an entry point's plain-`export *` closure is not on its own proof that a name survives to the entry, so no hop is skipped for it: the shortcut applies to an entry point itself only. The check reads the value namespace whenever the source exports the name there and the type namespace otherwise, so `export type { ns } from './barrel'` on an entry point does not put the value namespace object on the surface.\n- Entry-point reachability gates the seeds this PR adds, and nothing else. A target observed by a consumer in this graph, where no entry point reaches the target, is not seeded: that consumer is unreachable too, the report already calls the target an unused file, and crediting its chain would only stack unused-export rows underneath the unused-file rows. The same holds for an `export * as ns` source no entry point reaches. Withholding those can only withhold credit the pre-existing closure never gave.\n- The ambient seeds from #2357 are deliberately **not** gated, and neither is the chain walk. A `declare module 'pkg'` body states the shape of an external module id: its observers are importers of that id, outside this graph, so where the shim and its target sit inside the graph says nothing about who looks. The chain behind an unreachable shim routinely re-enters a module an entry point imports directly, and gating it reported unused exports on files the report calls reachable. A re-export edge makes its source reachable whenever the barrel is, so only an ambient chain can ever walk out of an unreachable member in the first place. Entry-point reachability reads the edge list alone, so `ModuleGraph::build` computes it once, before the closure, and hands the same bitset to `mark_reachable`.\n- Two seed properties are deliberate and visible in reports, and both are written down in the CHANGELOG and `docs/reference/detection-internals.md`. The seed is namespace-agnostic, so `export type { ns }` seeds the closure exactly like `export { ns }` and credits the chain in the value namespace as well: `typeof ns.member` keeps a value declaration reachable through a type-only re-export. And the seed does not ask whether the re-export itself has a consumer, so a namespace binding exported under its own name credits the chain behind it even when the report calls that very export unused, the same self-inconsistency the unreachable-observer case has.\n\nThe closure is computed once in `ModuleGraph::build` and threaded into Phase 2c and Phase 4 instead of each phase rebuilding it; it reads only `re_exports`, the entry-point flags, the consumers' whole-object uses, and that reachability bitset, none of which a later phase mutates. Seeding short-circuits when the project has no `export * as` edge at all.\n\n## Performance\n\nThe name search runs outward from each `export * as` edge toward the acceptance points, over a reverse index of the edges that forward a single name, rather than inward from every name an entry point re-exports. Two cutoffs keep a pathological chain cheap: a module that no forwarding edge connects to an acceptance point answers in constant time however deep its own chains run, and an exhausted search remembers every state it visited within a round, so a forwarding chain shared by many namespace edges is walked once instead of once per edge.\n\nDebug-build minimum over repeated full `dead-code --no-cache` runs, identical output everywhere:\n\n| project | baseline (main) | this branch |\n| --- | --- | --- |\n| vitest monorepo (12 runs each) | 2426 ms | 2507 ms |\n| 400 namespace barrels behind a 20-link plain-star chain | 145 ms | 152 ms |\n| 400 namespace barrels behind a 400-link plain-star chain | 374 ms | 430 ms |\n\nThe last row is the accepted cost of the shadowing fix: an on-surface namespace edge no longer takes a shortcut, so its name walks up the plain-star chain with a uniqueness check per hop. Realistic chain depths are within noise; the inward-by-name search variant measured roughly twice the baseline on the vitest monorepo, which is why the search direction is what it is.\n\nThe seed's own credit keeps its shape: a runtime whole-module edge credits the namespace object, `default` included; the ambient star form credits the star surface without `default`. Member-narrowed namespace imports (`ns.one()`) never seed the closure. A binding placed in an exported object literal (`export const API = { ns }`) keeps the direct-export mark-all it had on main but seeds the closure only when it is also used as a whole object or exported under its own name: the namespace-object alias phase follows `API.ns.<member>` accesses precisely, and the existing `issue-310` multi-hop alias test pins that `unusedQuery` stays reported.\n\nThe mark-all sites that feed the closure keep crediting their target's own direct exports as before, reachable or not; reference-level reachability filters those at reporting time. This matches the pre-existing mark-all model and is stated in `docs/reference/detection-internals.md`, along with what an unreachable whole-object observer suppresses.\n\nThe closure fixpoint does not rebuild its reachability prune per round. The prune only grows, so a round extends it from the members the previous round added and each re-export edge is walked at most once across all rounds. What is left per round is a rescan of the namespace edges still pending, which a chain shaped to resolve exactly one edge per round can drive up. On the pathological alternating named/namespace chain a reviewer built for that shape, minimum of 11 debug runs on a loaded box: N=1200 baseline 283 ms against 310 ms here (down from the +77% measured before the prune became incremental), N=2000 baseline 428 ms against 619 ms here (down from +160%). Real projects are flat: `viz-frontend` 191 ms against 182 ms, `editors/vscode` 228 ms against 227 ms, minimum of 9.\n\n## Behavior change\n\n- Whole-object namespace uses, unnarrowed namespace bindings, a namespace binding exported under its own name, an `export * as` binding imported by name and used as a whole object, dynamic-import pattern matches, a bindingless `require()`, and `export * as` chains an entry point exposes now credit the full namespace object of their target: direct exports, the named exports of `export *` sources (never their `default`), and every export of `export * as` sources (`default` included), recursively. Fewer unused-export findings for star barrels consumed those ways. A barrel that one of those consumers observes and no entry point reaches keeps reporting as an unused file with nothing stacked underneath it; an ambient `declare module` shim keeps crediting its chain whatever its reachability.\n- **One shape reports one finding more.** A plain `export *` hop inside a chain no longer carries a downstream `export * as default` onward, because that star never forwards `default`. An `export * from './barrel'` whose barrel does `export * from './mid'` over a `mid` that does `export * as default from './target'` now reports target's exports. That includes the ambient form: the `issue-2357-ambient-star-reexport` fixture is byte-identical between the baseline and this branch, but an ambient chain with a plain-star hop before an `export * as default` is not. An `export * as default` declared directly on the ambient star's own target still credits its chain. Nothing else in the issue-2357 behaviour moves: an ambient chain is seeded and walked at any reachability, exactly as it was.\n- A namespace re-export on a reachable non-entry barrel that is off the entry surface and has no consumer still exposes nothing; a barrel that declares its own copy of a star-forwarded name, or that receives the same name from two stars at once, stops the chain, whether the entry names the barrel or reaches it through a plain `export *`; a plain `export *` still never forwards `default`, so `export * as default` behind one keeps reporting while the same declaration on the entry point itself is credited.\n\n## Cache invalidation\n\n- `GRAPH_CACHE_VERSION` 38 -> 39: the new references are baked into the persisted graph and a graph-cache hit skips the build entirely. Warm 38 caches carry only the direct-export credit, and they also predate the one direction that moves the other way. Version 39 is unreleased (main is at 38), so it still invalidates every warm cache a user can have.\n- Extraction is untouched; the extract cache version stays at 276.\n\nWarm-cache proof on a scratch copy of the `issue-2373-entry-namespace-chain` fixture: the baseline binary (main, graph 38) runs cold and writes the cache; this branch (graph 39) on that warm cache reports exactly its own cold `--no-cache` output, and a second warm run is identical.\n\n## How it was tested\n\n- Integration fixture `issue-2372-star-barrel-whole-module`: whole-object use in the entry point over a barrel with `export *` plus a three-level `export * as` chain; a non-entry whole-object shim; a binding handed on without member access; a binding re-exported from a non-entry module; an `export * as` binding imported by name and used as a whole object, with a name-precision control on the same barrel; a binding that is both an object-literal alias source and exported under its own name, against the object-literal-only negative control; an ambient `declare module` star whose target exposes an `export * as ns` (credited whole, `default` included) and whose plain-star hop drops a downstream `export * as default` (reported); `import.meta.glob`, a template `import()`, and `require.context` targets with star, named, and `export * as` re-exports; a member-narrowed namespace import as a negative control; a barrel that re-exports itself through an `export *` / `export * as` cycle; an `export * as default` on a plain-star member, whose chain stays reported; a whole-object use inside an unreachable file, whose dead subtree stays unused-file rows with no unused-export rows underneath; reference-shape assertions that the credit is routed through the barrel's star chain and the exposed namespace object.\n- Integration fixture `issue-2373-entry-namespace-chain`: the issue repro plus a third `export * as sub3` level and sub2's own `export *`, an `export * as top` directly on the entry, a namespace named by the entry through `export { named } from './named'` and through a rename hop, an `import * as bindNs` plus `export { bindNs }` on the entry, a name-precision control, an off-surface `export * as hidden` on a reachable non-entry barrel, an entry star cycle, `export * as default` on the entry itself (credited) against the same declaration on a plain-star barrel and behind an `export { x as default }` rename (both reported), a star-forwarded namespace name shadowed by a local declaration on the barrel in both the named-hop and the plain-star entry form, the same name arriving from two `export *` sources at once, and reference-shape assertions.\n- Fixture `issue-2372-star-barrel-whole-module` also pins the shape the reachability split exists for: `unreached-shim.ts` holds a `declare module` body in a plain `.ts` file nothing imports, `unreached-barrel.ts` and `unreached-mid.ts` behind it are unused files, and `unreached-reentry.ts` and `unreached-ns-reentry.ts` are imported directly by the entry point, so the chain's names must stay credited on modules that carry no unused-file row at all. Re-gating the seed makes that test fail.\n- Graph unit test `forwards_binding_agrees_with_phase_2c_on_rename_shadow_and_ambiguity`: a rename hop forwards, a local declaration on the barrel shadows, and the same name arriving from two `export *` sources is ambiguous, asserted for both `ModuleGraph::forwards_binding` and `uniquely_forwards_binding`.\n- Mutation matrix. With the whole branch's `crates/graph/src` reverted to main, fourteen of the twenty-one tests fail; the seven that pass are the declared negative controls (member narrowing, object-literal alias, unreachable observer, off-surface namespace, the two cycle pins, and the shadow/ambiguity pin, which guards a regression this branch introduced rather than a gap on main). With only this round's `crates/graph/src` reverted, the four tests this round adds for its own fixes fail (the plain-star shadow and ambiguity pin, the entry namespace binding pin, the whole-object named-import pin, and the alias-plus-named-export pin) while the ambient pin passes, since that behaviour landed in the branch's preceding commit and bites against main instead.\n- Exact issue repros through both binaries: #2372 case 1 reports `src/deep.ts:deepHelper` on the baseline and nothing with the fix; #2373 reports only `barrel.ts:default` and `deep.ts:default` with the fix (baseline added `deep.ts:deepX`, `sub2.ts:sub2X`, `sub2.ts:default`).\n- Adversarial probes replayed from review, all now matching ES semantics: a locally shadowed `export * as ns` behind a plain-star entry and behind a named hop; two `export *` sources exporting the same `ns`; `export * as default` behind an entry star, behind a plain star from a whole-object seed, behind an `export { ns as default }` rename, and on the entry point itself; an ambient chain with a plain-star hop before an `export * as default`.\n- Real-project smokes with the baseline and branch binaries, normalized `dead-code --format json --no-cache` output identical everywhere: the in-repo `viz-frontend` and `editors/vscode`, the vitest monorepo, a design-system monorepo, and a large product monorepo. The `issue-2357`, `issue-310`, `issue-2348`, `issue-269`, `issue-303`, `issue-324`, `issue-328`, and `issue-1373` namespace fixtures are also identical to main.\n- Gates on the rebased tree: cargo fmt check, clippy workspace all-targets with warnings denied, full workspace test suite (with the type-aware sidecar installed), bench check, cargo doc with warnings denied, typos, hidden-unicode scan, and comment-quality check.\n\n## Review round 2\n\nThe entry-reachability gate the previous round added narrowed the pre-existing #2357 closure: when the `declare module` shim is an unreachable non-`.d.ts` file, the branch stopped crediting the ambient star's chain and reported new unused exports on files an entry point imports directly. Three shapes a reviewer executed are byte-identical to the pre-change binary again:\n\n1. An unreachable shim over `impl.ts` -> `impl-deep.ts` where the entry point imports `implDeepOne` from `impl-deep.ts`: `impl-deep.ts:implDeepTwo` is no longer reported.\n2. The same in namespace form (`export * as ns` over a reachable `ns-target.ts`): `ns-target.ts:nsTwo` and `ns-target.ts:default` are no longer reported.\n3. A fully dead island: `impl-deep.ts:implDeepOne` is reported again, as on main.\n\nFixed by splitting the closure seeds (ambient targets ungated, in-graph observers gated) and dropping the reachability test from the chain walk, plus the incremental reachability prune, the `forwards_binding` delegation, and the CHANGELOG and detection-internals corrections described above.\n\nRebased onto `dc03fd148`.\n\nFixes #2372\nFixes #2373",
          "timestamp": "2026-08-23T08:43:58+02:00",
          "tree_id": "1065175448d339092faf29d21c3a754373070880",
          "url": "https://github.com/fallow-rs/fallow/commit/0a95caca69955029a0c41c397f01f2b6b87b1b2f"
        },
        "date": 1787467691581,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9732376,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49220,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1186749,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8404,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "2862dbe4f96a2e801a576b63028da97c73b8f6b9",
          "message": "fix(graph): credit the default export however each side spells the specifier\n\n## What was broken\n\n`default` is one importable name that either side may spell two ways, and no pairing that mixed the two spellings ever met, so the target's default export kept reporting as unused:\n\n- `import { default as X } from './impl'` (the issue's reproduction).\n- `declare module 'pkg' { export { default } from './impl' }`, and the mixed `export { default as Impl, Y as Z } from './impl'` inside an ambient body, where `Y` was credited and the default was not.\n- A plain `import x from './impl'` against a module whose default is written `export { x as default }`, which the extractor keeps under its written name. This is the mirror image of the first case and reproduces on the same lookup.\n- The same mirror for CommonJS: `exports.default = x` is recorded under its written name too, so the default of every transpiled CommonJS module reported as unused against a plain default import.\n- Two further producers of a named `default` specifier: a JSDoc `import('./impl').default` type reference, and a destructured `const { default: X } = require('./impl')`.\n\nA `export { default } from` chain consumed by a `default` specifier lost the credit at every hop, so a two-hop chain reported three unused defaults instead of none.\n\n## Root cause\n\n`ExportNameIndex` (`crates/graph/src/graph/build.rs`) is the per-module lookup Phase 2 uses to find the exports an incoming edge symbol references. It keyed on the spelling rather than on the name: `ExportName::Default` went into a dedicated slot, `ExportName::Named(\"default\")` into the string map, and `matches` read the slot only for `ImportedName::Default`. Several producers record a `default` specifier as `ImportedName::Named(\"default\")`: `handle_import_specifier` for a plain import, the ambient named re-export branch of `visit_export_named_declaration` (which records one named type-space import per specifier, #2349), the JSDoc `import()` member path, and the destructured-`require` path. None could reach a slot the index never filled from their side.\n\n## The fix\n\nOne default slot, read and written under both spellings. `sync` routes `ExportName::Named(\"default\")` into the slot instead of the string map, so the slot holds every declaration form in ascending export order and the named map never carries a `\"default\"` key; `matches` reads the slot for `ImportedName::Named(\"default\")` as well as for `ImportedName::Default`.\n\nThe index is the right layer, not the extractor:\n\n- Every other `ImportedName` consumer in the graph already collapses `Default` and `Named(\"default\")` to the same string key `\"default\"` (`imported_name_label`, the consumer and whole-object maps in `re_exports`, `reference_reaches_surface`, and `attach_direct_export_references`'s own `imported_name` binding). This lookup was the one that did not, so it is the defect site.\n- Normalising the two producers to `ImportedName::Default` instead would make an ambient `export { default } from` byte-identical to the `export * as ns` default-member import #2357 records (`ImportedSymbol::is_ambient_star` is type-only plus an empty local name plus `Namespace | Default`), silently promoting the named ambient form from type-space credit to both-namespace credit. That is exactly the regression #2357 fixed. It would also flip `reference_kind_for` and `narrowable_import_locals` for a CSS-module default alias.\n\n### The collapse is scoped to modules whose `default` is a default export\n\n`NamedDefaultSpelling` picks the rule per target module from its path. A CSS Module exports one `ExportName::Named` per class and never an `ExportName::Default` (`crates/extract/src/css.rs` even asserts it), so for a stylesheet a class spelled `.default` is an ordinary class, not a module default. Folding there let any plain `import styles from './x.module.css'` credit that class unconditionally, before the CSS member narrowing that owns crediting for that import ever ran, and a genuinely unused class silently stopped reporting. Stylesheets keep an empty default slot and `narrow_css_module_references` remains the only thing that credits their classes, from the member accesses the consumer actually writes.\n\nThe stylesheet side is decided by `is_css_module_stylesheet`, which tracks the extractor's own CSS Module set: `.module.css`, `.module.scss`, `.module.sass` and `.module.less`. It is deliberately a separate predicate from `is_css_module_path`, which gates CSS member narrowing on the `.css` / `.scss` pair alone and is left untouched, so `.less` and `.sass` class maps gain an empty default slot without gaining member narrowing (a behavior change well past this fix, and pinned as unchanged by the pre-existing `css_module_path_less_not_matched` test).\n\nA CommonJS `exports.default = x` keeps folding, deliberately. The extractor records it as `Named(\"default\")` exactly like `export { x as default }` and the graph cannot tell the two apart, so the choice is one or the other for both. Crediting is correct for the dominant shape (transpiled CommonJS carrying `__esModule`, where `import x from './m'` binds exactly that property) and leaving it uncredited was a measurable false positive on the baseline. The same fold reaches a hand-written `module.exports = { default: ... }` object key, which is defensible under the same interop rule and is disclosed in the changelog.\n\n## Behavior change\n\nUnused-export findings decrease. Repositories using any of the affected shapes lose unused-export findings on the target's default export, including every hop of a `export { default } from` chain.\n\nTotal finding count is not guaranteed to decrease. A default export credited for the first time becomes reachable, so member-level detectors can now report on it: a `export default class C { value = 1 }` reached only through `import { default as C }` swaps its `unused_exports ... default` row for an `unused_class_members ... value` row. Both rows are correct; the changelog says so.\n\nThree neighbours are deliberately untouched and pinned by fixtures:\n\n- A plain `export * from './impl'` still does not forward `default`, however the target spells it. Two negative controls cover both spellings: `export default fn` in the main fixture, and `export { hidden as default }` (the spelling the fold actually relocates) in `issue-2374-star-named-default`. Each proves the star edge is live by keeping the target's named export credited.\n- A named re-export chain still forwards exactly the specifiers written on it: the sibling next to the default in `chain-deep.ts` keeps reporting.\n- A CSS Module class named `.default` still reports when no consumer accesses it, in all four stylesheet syntaxes, while a `.module.css` class the consumer does access stays credited.\n\nThe #2357, #2372, and #2373 fixtures keep their exact verdicts.\n\n## Cache invalidation\n\n`GRAPH_CACHE_VERSION` 39 to 40. Unused-export verdicts are read off the persisted graph and reference attachment bakes the credited references into it at graph build, so a warm 39 `.fallow` would replay the uncredited verdict verbatim. The extract cache is untouched: this change records nothing new during extraction, so `CACHE_VERSION` stays at 276.\n\nWarm-cache proof on the crediting fixture:\n\n1. Baseline binary (graph v39, no fix), cold run: 12 unused exports, writes the cache.\n2. Patched binary on that warm v39 cache: 5 unused exports, the fixed verdict.\n3. Second patched run on the now-v40 cache: identical to step 2.\n\nThe same three steps on a `.module.less` plus `.module.sass` project hold the verdict at the baseline's 5 unused exports throughout, so the stylesheet guard survives a cache round trip.\n\n## How it was tested\n\n- Graph unit tests on `ExportNameIndex`: both spellings read the same slot, in ascending order, with an unrelated name unaffected; a module that only declares `export { x as default }` answers a plain default import; a CSS-module index keeps `default` in the string map with an empty default slot; `is_css_module_stylesheet` covers all four CSS Module extensions and stays wider than the narrowing gate; and `NamedDefaultSpelling::for_target` classifies `.module.css`, `.module.scss`, `.module.sass` and `.module.less` apart from ordinary source paths (`.css`, `.less`, `.sass` and `.module.ts` included).\n- Extract unit test pinning that both producers keep recording the written name `Named(\"default\")`, which is the shape the index fix reads. Deliberately passes on both sides of the fix.\n- Integration tests on fixture `issue-2374-default-specifier` covering `import { default as X }`, the two ambient forms, the two-hop chain, the `export { x as default }` mirror, and the negative controls. Three boundary fixtures hold the cases that each declare a further `Named(\"default\")`, which the pre-existing `duplicate_exports` grouping would otherwise report as a collision against the main fixture's `export { inner as default }`: `issue-2374-css-module-default-class` (now carrying `.module.css`, `.module.less` and `.module.sass` siblings), `issue-2374-cjs-default-property`, and `issue-2374-star-named-default`.\n- Mutation matrix. Reverting the whole `build.rs` source change fails the three crediting integration tests and leaves the negative controls passing. Pointing `for_target` back at `is_css_module_path` fails the `.module.sass` case of the `for_target` unit test and the CSS integration test on `classes.module.less`. Forcing `for_target` never to fold fails the same two.\n- Differential run of the baseline and patched binaries over every top-level fixture project (505 with a `package.json`, `dead-code --format json --no-cache`): exactly two differ, the crediting fixture (12 findings to 5) and the CommonJS boundary fixture (2 to 1). The CSS-module fixtures, the new star fixture, and the #2355 / #2357 / #2372 / #2373 pins are byte-identical.\n- Real-project runs, baseline versus patched, `dead-code --format json --no-cache`: `viz-frontend` and `editors/vscode` are byte-identical, as is a third real project that uses `export { default as X } from`. None of them uses the affected shapes.\n- Scratch-project probes on both binaries for each reported shape: a `.module.less` and a `.module.sass` each carrying a `.default` class (the branch before this round dropped both findings; patched output is now byte-identical to the baseline), `.module.css` and `.module.scss` equivalents, the counter-case where the consumer writes `styles.default` (credited on both), `exports.default = fn` (baseline false positive, cured), a `module.exports = { default: ... }` object key, a JSDoc `import('./m').default`, a destructured `const { default: X } = require('./m')`, and a class default reached through `import { default as C }` (the unused-export row becomes an unused-class-member row).\n- The issue's own reproduction through the patched binary: `src/impl.ts:default` is gone, the genuinely unused `untouched` still reports.\n- Gates: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --lib --bins --tests --examples`, `cargo check --workspace --benches`, `RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps --document-private-items`, `typos`, `scripts/scan-hidden-unicode.py --mode committed --staged`, `scripts/check-comment-quality.mjs --staged`.\n\n## Known gap, not in scope\n\n`narrow_css_module_references` is gated on `is_css_module_path`, so it never runs for `.module.less` or `.module.sass`: every class in such a stylesheet reports as unused even when the consumer accesses it. Identical on the baseline and patched binaries, so pre-existing and untouched here. Tracked as a separate follow-up.\n\nFixes #2374",
          "timestamp": "2026-08-23T15:00:28+02:00",
          "tree_id": "c9491d5c6fdee45adc600453e01e2e1ccb010920",
          "url": "https://github.com/fallow-rs/fallow/commit/2862dbe4f96a2e801a576b63028da97c73b8f6b9"
        },
        "date": 1787490337234,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9741272,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49231,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1178563,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8345,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "28b7be16fd895a1d937922b2ead2a77ad552aab2",
          "message": "fix(extract): keep MDX prose lines that only look like statements out of the parser\n\n## What was broken\n\nAn MDX file whose prose contains a sentence opening with the word \"import\" or \"export\" lost **every** import of the file. The reproduction from the issue, an Astro page rendering\n\n```mdx\nimport * as NS from '../components/ns'\n\n<NS.Star />\n\nimport the thing and render <NS.Moon /> here.\n```\n\nreported `src/components/ns.ts` as an unused file (and therefore no unused export for its genuinely unused `Unused` sibling). The same shape hits any documentation line that starts with those words, including a paragraph that wraps onto a line beginning with \"export\" and a shell `export FOO=bar` outside a fenced block.\n\n## Root cause\n\n`is_statement_start` in `crates/extract/src/mdx.rs` accepted any trimmed line starting with `import ` / `import{` / `export ` / `export{`, so the prose sentence joined the statement lines. Those lines are concatenated into one buffer and parsed as a single program; oxc aborts on the first fatal error and returns an empty program (`panicked`), so nothing at all was extracted: no imports, no exports, no member accesses from the body.\n\n## The fix\n\nThree layers, all in `crates/extract/src/mdx.rs`.\n\n1. **Classification.** A candidate line is a statement when it carries a shape only a real statement has: a source clause, a brace specifier list (on the line or continued below), a star specifier, a string-literal side-effect import (`import './x'`), or, after `export`, a brace list, a star, or a declaration keyword (`const`, `let`, `var`, `function`, `class`, `async`, `default`, plus the TypeScript heads `type`, `interface`, `enum`, `namespace`, `declare`, `abstract`). The keyword itself still has to be followed by whitespace or `{`, so `important` is not a candidate. A source clause is a `from` bounded by whitespace on the left and followed by its specifier quote on the right, immediately or after whitespace, so `from './x'`, `from'./x'`, `from` + tab, a no-break space and a multi-space run all qualify, while a `from` inside a word (`fromage`, `from_the_api`) or inside a string does not. The TypeScript heads are recognised as statement shapes; the JSX source type the statement body is parsed with rejects them, so they are handled by the fallback below rather than surviving as statements.\n\n2. **Parse probe (false negatives).** The shape list is a fast path, not the definition of the language. A candidate line that opens with the keyword and matches nothing in the list is handed to the parser on its own, and a line that parses is a statement whatever its shape. That keeps heads no specifier pattern names out of prose: `import /* set up styles */ './global.css'`, `export /* keep */ const x = 1`, a top-level dynamic `import ('./x')`. A prose sentence cannot slip through, because a sentence does not parse. The probe costs one parse of one line, and only for a candidate the shape list does not recognise.\n\n3. **Parse fallback (false positives).** The scan collects statement *blocks* (an opening line plus the continuation lines a multi-line specifier list collected) instead of loose lines. When the parser rejects the assembled body, every block the parser also rejects on its own is demoted to prose and the remaining blocks are re-parsed. The retry only ever adds statements back: the rejected body was an empty program to begin with. Demotion is per block, so a multi-line specifier list is never split, and demoted lines are merged back into the prose list in source order.\n\nThe #2355 completeness guard is preserved: a demoted line goes through the prose scan like any other body line, so a namespace or CSS-module binding mentioned on it records a whole-object use and keeps its mark-all crediting instead of narrowing to the tags the scan happened to see.\n\n## Behavior change\n\n- MDX documents with such a line resolve their imports again: their targets leave the unused-file list, the members their bodies render are credited, and their genuinely unused siblings surface as unused exports.\n- The affected document's own exports are read too, so an `export const` nothing consumes now reports as an unused export on the `.mdx` file itself. Measured: a scratch Astro project whose `notes.mdx` carries `export const docsHelper` plus the prose line moves from `unused_files ['src/components/ns.ts']`, `unused_exports []` on the baseline binary to `unused_files []`, `unused_exports [('src/docs/notes.mdx', 'docsHelper')]` on the patched one.\n- A multi-line MDX declaration whose continuation line carries the word `from` inside a string (`summary: 'Written from scratch'`) is collected whole rather than cut one line short, so that declaration parses and its unconsumed export reports as well. Measured on a two-document scratch project: the baseline binary reports `unused_exports [('src/docs/tab.mdx', 'meta')]` and the patched one `[('src/docs/space.mdx', 'spaced'), ('src/docs/tab.mdx', 'meta')]`. The space form of that truncation predates this branch.\n- Duplication reads the same statement body, so clones inside an affected document, and the duplication share of its health score, now surface where the file previously tokenized to nothing. Measured: two MDX documents sharing a 25-key `export const meta` block under the prose line go from 0 clone groups / 0 tokens on the baseline binary to 1 group / 114 tokens / 83.9% on the patched one.\n- A TS-only `import type { X } from './x'` clause, which the JSX source type the MDX statement body is parsed with does not accept, used to take the whole file's imports with it. It now costs only itself. (Parsing MDX statements with a TS-aware source type is a separate question, filed as a follow-up.)\n- A line that parsed cleanly before still parses cleanly: a candidate is only sent to prose when it neither carries a statement shape nor parses as JavaScript on its own, and the block fallback only runs after a rejected parse. Verified over 6800 real `.mdx` files on this machine: the narrowed source clause changes the collected statement lines of zero of them, and the 31 distinct candidate lines the shape list rejects (shell `export VAR=value`, `export = contents;`, `import json, sys`, prose wraps) are all lines the parser rejects too, so all of them stay prose.\n\n## Cache invalidation\n\n- extract `CACHE_VERSION` 276 -> 277: warm caches hold the import-less module for every affected MDX file.\n- `GRAPH_CACHE_VERSION` 39 -> 40: unused-file and unused-export verdicts are read off the persisted graph, and the newly resolved imports are baked into it at graph build, so a warm graph cache would replay the false positive verbatim.\n- `DUPES_CACHE_VERSION` 10 -> 11: the duplication tokenizer reads MDX through `extract_mdx_statements`, so the classification change moves MDX token streams, and the token store is keyed only on `dupes-tokens-v{DUPES_CACHE_VERSION}` plus the file fingerprint.\n\nWarm-cache proof on the issue fixture: the `origin/main` binary (276 / 39) ran cold and wrote `.fallow/` reporting `commented.ts`, `lazy.ts`, `ns.ts` and `rejected.ts` as unused files with no unused exports; the patched binary on that same cache reported the fixed set (no unused files, `ns.ts:Unused`, `commented.mdx:docNote`, `commented.ts:commentedSetup`, `lazy.ts:Lazy`), and a second warm run was stable. The `.fallow/` directory was removed afterwards.\n\nWarm-cache proof for the token store, on a two-document fixture with `minCorpusSizeForTokenCache: 1` where one document carries a tab-separated `export` head: the baseline binary wrote a `dupes-tokens-v10` store and reported 0 clone groups / 114 tokens. Before the bump, the patched binary on that store still reported 0 groups / 114 tokens while a cold run reported 1 group / 171 tokens. After the bump the patched binary reports 1 group / 171 tokens with the v10 store still on disk, writes its own `dupes-tokens-v11`, and repeats that result on a second warm run.\n\n## How it was tested\n\n- **Unit tests** (`crates/extract/src/mdx.rs`): a classification table pinning real statement shapes and prose sentences; a whitespace table for the source clause (tab before and after `from`, a no-break space, a multi-space run, and negative cases where `from` is inside a word); the issue reproduction at module level (import survives, both body tags credited, namespace stays precise); the tab-separated default import resolving to an edge; the parse probe recovering the commented-keyword import, the commented-keyword export declaration and the spaced dynamic import, with every candidate shape a scan of real MDX corpora turns up pinned as prose; a `from` inside a string on a continuation line not ending the block, in both the tab and the space form; the fallback on a rejected single line (both imports survive, the mention on the demoted line keeps its namespace on mark-all, an unmentioned namespace stays precise); a rejected multi-line block demoted whole; a real multi-line import surviving a demoted sibling; the `import type` case; and the file's inline suppressions surviving the fallback re-parse.\n- **Integration fixture** `tests/fixtures/issue-2376-mdx-import-prose` (Astro entry page rendering three MDX documents): `mdx_import_prose_line_keeps_the_files_imports` pins that `ns.ts` is not an unused file, `Star` / `Moon` stay credited and `Unused` reports; `rejected_statement_block_is_demoted_and_keeps_mark_all` pins that the target behind a rejected line is used and that both its exports stay credited through the mark-all guard; `statement_shapes_without_a_specifier_pattern_keep_their_edges` pins that the commented side-effect import and the spaced dynamic import keep their targets off the unused-file list and that the multi-line export with `from` inside a string surfaces.\n- **Mutation matrix**: with the parse probe removed, the four probe tests fail plus the new integration test; with the source clause reverted to the round 1 character class, the string-`from` test fails plus the same integration test; with the fallback disabled, the five fallback tests fail plus `rejected_statement_block_is_demoted_and_keeps_mark_all`; with every non-test source hunk reverted to `origin/main` (main's source spliced in front of this branch's test module), twelve unit tests and all three integration tests fail. Restored tree green and clean each time.\n- **Real-world evidence**, baseline vs patched, `--format json --no-cache`: the in-repo `viz-frontend` and `editors/vscode` are identical on `dead-code`, and the public documentation site of this project (73 MDX files) is identical too, in all three cases down to every field except `analysis_run_id` and `elapsed_ms`. Both reviewer reproductions from round 1 now match the baseline exactly: the commented side-effect import project reports `unused_files []` on both binaries, and the spaced dynamic import project reports `unused_files ['src/components/b.ts']`, `unused_exports [('src/components/a.ts', 'A')]` on both. A state-machine simulation of the line scanner over all 6800 local `.mdx` files finds exactly one file whose collected statement lines change, the new fixture document added by this branch.\n- **Gates**: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --lib --bins --tests --examples` (with the type-aware sidecar installed), `cargo check --workspace --benches`, `cargo doc` with `-D warnings`, `typos`, hidden-unicode scan, comment-quality check.\n- **Docs**: `docs/reference/extract-internals.md` records the classification rule, the quote-bounded source clause, the parse probe, the parse fallback, and that the fallback covers the dead-code path only, next to the MDX completeness guard.\n\n## Review round 1\n\n- `DUPES_CACHE_VERSION` 10 -> 11, with the live stale-cache divergence reproduced before the bump and the fix proven after it.\n- The source clause is matched on whitespace boundaries instead of the literal strings with a leading space, so a valid `import X from` + tab + `'./x'` keeps its edge instead of becoming a false unused file.\n- `parse_mdx_to_module` no longer clones the parsed suppressions on every MDX file; the fallback path discards the rejected module info, so its suppressions move on to the retry.\n- The CHANGELOG names the two further finding movements above, both measured on the patched binary.\n- The export keyword table and the extraction reference say the TypeScript heads are statement shapes handled by the parse fallback rather than surviving as statements.\n\n## Review round 2\n\n- **The classifier now has a false-negative safety net.** A line whose keyword is followed by a block comment (`import /* set up styles */ './global.css'`, `export /* keep */ const x = 1`) parsed on main and carried none of the recognised shapes, so round 1 sent it to prose and lost its edge or its declaration with no way back: the block fallback only rescues lines the classifier wrongly accepts. A spaced dynamic `import ('./x')` was lost the same way. Both are recovered by the parse probe, and both scratch projects now match the baseline binary exactly.\n- **The source clause requires its specifier quote.** The word `from` inside a string on a continuation line no longer ends a multi-line block one line early and cost the declaration it belongs to. That also removes the space form of the same truncation, which predates this branch.\n- **The stability claim is now accurate.** The \"no change for files that parsed cleanly before\" sentence was false against the commented-keyword shape; it is replaced by the precise rule and backed by the 6800-file corpus scan above.\n- **Test inputs retargeted.** Tightening the source clause moved `import data from the API using X` out of the fallback and into plain classification, which silently cost three fallback tests and the `rejected.mdx` fixture their bite (the fallback mutation dropped from five failures to two). They now use a line carrying a real source clause with prose trailing it, which the classifier still accepts and the parser still rejects.\n\nFixes #2376",
          "timestamp": "2026-08-23T15:08:31+02:00",
          "tree_id": "92243c49027c355d7a817eb5e58a6b22a768d998",
          "url": "https://github.com/fallow-rs/fallow/commit/28b7be16fd895a1d937922b2ead2a77ad552aab2"
        },
        "date": 1787490838367,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9745408,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49232,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1195882,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8442,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "02ea35062bf121e07f7404888a28c2c096284bb8",
          "message": "fix(engine): report the type-lane credit of a value-only export in dead-code --trace\n\n`dead-code --trace` reported a value export as unused when its only credit was a bound `import type` of that export, contradicting the `dead-code` verdict on the same input.\n\n## The bug\n\n`dead-code` credits `export const helper` referenced through `import type { helper }`: the graph's type lane falls back to the value declaration when no type declaration of that name exists, and the unused-export analyzer counts the reference regardless of namespace. `trace_export` selected the value namespace for the value export, read only value-lane references, and printed `is_used: false` with an empty `direct_references`. An agent following the \"trace before deleting\" guidance got two contradictory answers for one symbol.\n\n## The fix\n\nThe trace now reports the references that credit the traced declaration. The preferred namespace wins whenever its lane carries a reference. When it carries none and the other namespace resolves to the **same effective binding**, the trace reports that lane's references and sets `namespace` to the lane that carries them.\n\nOn the repro, `--trace src/impl.ts:helper` returns `namespace: \"type\"`, `is_used: true`, and the `import type` consumer in `direct_references`; the human trace prints `USED` and `Namespace: type`.\n\nPreserved by construction and pinned by tests:\n\n- both lanes carrying references keeps the value namespace and the value consumer,\n- an unreferenced export still reports `is_used: false` and stays on its preferred lane,\n- a distinct same-name declaration in the other lane keeps the preferred lane, because its references credit that other declaration and `dead-code` still reports the traced one,\n- a declaration merge that stays one binding (`class Widget` next to `namespace Widget`) is covered; a merge that splits across lanes (`interface Foo` next to `class Foo`) is not, and is documented as a carve-out plus a follow-up.\n\n`is_used` keeps following the listed references alone, so file reachability stays a separate axis: an export in an unreachable file can read `is_used: true` next to `file_reachable: false`, exactly as the value lane already behaved. The rustdoc, the changelog and the compatibility note say so rather than claiming the trace always matches the verdict.\n\n## Consumers that inherit the correction\n\n`trace_export` (MCP), the typed API, the `trace_symbol` root trace, `fallow inspect --symbol` (its `identity.is_used` / `identity.reason` and its `evidence.trace_export` block, in the CLI and in the `inspect` MCP tool), and the class-member trace. The LSP reference lens already counted the type-only use.\n\n`ClassMemberTrace` gains an additive `owner_namespace` naming the lane that credits its owner (optional in the schema, like `ExportTrace.namespace`), and the human member trace prints an `Owner namespace:` line, so the member payload no longer hides the crediting lane from JSON consumers.\n\n## Keeping the type-aware payload readable\n\nThe checker proof beside a trace covers only the lane the declaration itself occupies, so it can report `no-references-found` for a credit the root trace lists. That is a pre-existing sidecar limitation, unchanged here. This PR makes the payload say so instead of contradicting itself:\n\n- `semantic.target.namespace` names the lane the proof covers,\n- both the export and the member human proof line append that lane (`value namespace only`) when such a proof lists no references of its own; a proof that carries its own evidence stays unqualified,\n- the in-band `_meta.field_definitions.semantic` note and the `trace_symbol` MCP tool description now state that a root trace listing a reference the proof omits is the wider evidence rather than a stale one. Without that, an agent obeying the in-band guidance would discard the corrected root evidence and delete a used export, which is exactly the reported failure.\n\n## Scope note\n\nAn earlier revision of this branch also changed the type-aware sidecar so the checker credited cross-lane imports. That moves the `--type-aware` finding counts on real projects and belongs in its own reviewed change; it has been removed here and the branch no longer touches `tools/type-aware-sidecar`. A `--type-aware --include-entry-exports` run on `editors/vscode` is byte-identical to the previous release apart from `elapsed_ms`.\n\n## Compatibility\n\n`namespace` may now be `type` for a value export; consumers should read it as the lane the listed references use. Both `namespace` and the new `owner_namespace` stay optional in the schema. No cache version changed: the trace is a read-only query over the graph, verified by running the corrected trace against a `.fallow` directory written by the previous release.\n\n## Verification\n\n- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --lib --bins --tests --examples`, `cargo check --workspace --benches`, `RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps --document-private-items`, `typos`, hidden-unicode and comment-quality scans, `CI=true npm run generate:contracts:check`, `tsc --noEmit` on the VS Code extension: all pass.\n- The sidecar's own `node --test` suite passes unchanged.\n- Mutation matrix: reverting the engine fallback, the renderer qualifier, the member namespace threading, the in-band `_meta` carve-out or the tool-description carve-out each fails the matching test; forcing the qualifier on unconditionally fails the three negative controls.\n- Baseline versus branch on `viz-frontend` and `editors/vscode`: identical findings, only `elapsed_ms` differs.\n\nPlease squash-merge with an explicit body rather than the default commit concatenation.\n\nFixes #2371.",
          "timestamp": "2026-08-23T17:58:24+02:00",
          "tree_id": "1b427fd0be7fc93764f174d200fdd8c2cfd7b14c",
          "url": "https://github.com/fallow-rs/fallow/commit/02ea35062bf121e07f7404888a28c2c096284bb8"
        },
        "date": 1787501023300,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9750784,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49244,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1175419,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8086,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "969689e6de5675acebfdffd6e0538bc9e56372eb",
          "message": "fix(extract): record a bare namespace-import reference as a whole-object use\n\n## What was broken\n\nA namespace import that is handed over whole and also read through one dotted access narrowed to that member alone, so every sibling the receiver can still reach was reported as an unused export. The reported shapes:\n\n```tsx\n// src/whole.tsx\nimport * as Icons from './whole-icons';\nexport const Whole = () => (<div><Icons.Star /><Callout icons={Icons} /></div>);\n```\n\n```ts\n// src/arg.ts\nimport * as Icons from './arg-icons';\nexport const useArg = (): number => { Icons.Star(); return register(Icons); };\n```\n\nBoth `Moon` exports reported, plus the same for an alias (`const N = NS`) and a statement initializer (`export const all = NS`).\n\n## Root cause\n\n`narrow_namespace_references` narrows a namespace binding to the members the consumer accessed unless the consumer recorded a whole-object use. The visitor recorded one for an allow-list of positions only: `Object.keys/values/entries/getOwnPropertyNames(NS)`, a spread, `for ... in`, a computed non-string access, a rest destructure, and a few type positions. Every other reference to the local left no trace at all, so a file with one dotted access and one bare pass looked exactly like a file with one dotted access.\n\nAstro and MDX consumers were already covered: since #2360 their whole-file completeness guard turns any mention the structured passes did not classify into a whole-object use. Only `.ts` / `.tsx` / `.js` / `.jsx` consumers were affected.\n\n## The fix\n\nEarliest incorrect layer is extraction. A reference to an `import * as NS` local that the visitor cannot resolve to one member is now a whole-object use (`record_bare_namespace_reference` in `crates/extract/src/visitor/visit_impl.rs`), which puts the binding back on the graph's mark-all path. That is the conservative direction: it over-credits rather than reporting a used export.\n\nThe resolved positions are the exclusions, and they keep narrowing:\n\n- the object of a static access (`NS.member`, `NS?.member`),\n- the object of a string-computed access (`NS['member']`), which the parser resolves exactly even though the Astro and MDX text guards cannot,\n- the root of a JSX member tag, opening and closing (`<NS.Card>` / `</NS.Card>`),\n- the left side of a dotted type name (`NS.Type`),\n- a destructure initializer (`const { Star } = NS`), whose members the visitor records (a rest element keeps its own whole-object use),\n- a re-export specifier local (`export { NS }`), which the graph already credits in full through the rule added by #2373, so the two do not double-count,\n- a placement in an object literal bound to a local (`const api = { NS }`), whose `api.NS.member` path the object-binding resolver follows and which #2372 deliberately keeps off the whole-module closure. A bare reference to that local (`hand(api)`) hands the namespace on in turn and does record the use.\n\nThe locals are pre-registered from the program's top-level statement list before the body walk, because an import declaration is legal after the code that reads it. The local of the binding's own `import * as NS` is a binding identifier, never a reference, so the import declaration needs no exclusion. The rule is scoped to `import * as` locals: named imports, and namespace objects bound by `require` or a dynamic import, keep the allow-list they had.\n\nSide effect worth naming: `whole_object_uses` is now deduplicated at the recording site. Every consumer asks membership, never a count, and the record is persisted, so a name recorded opaquely many times is one entry instead of one per mention.\n\n## Behavior change: fewer findings\n\nThis widens crediting for every TS and TSX consumer that hands a namespace over, so it is a finding-count **decrease** on upgrade, and it lands in regression baselines and `--max-issues` gates. An export credited for the first time also becomes reachable, so member-level detectors can report on it where the unused-export finding used to stand in its place.\n\nMeasured with `dead-code --no-cache --format json`, baseline binary built from `28b7be16f` against the patched binary:\n\n| Project | Baseline | Fixed | Delta |\n|---|---|---|---|\n| in-repo `viz-frontend` | 0 | 0 | none |\n| in-repo `editors/vscode` | 1 | 1 | none |\n| `vuejs/core` v3.5.30 | 206 | 206 | none |\n| `withastro/starlight` (main) | 34 | 32 | 2 fewer |\n\nBoth Starlight changes are true-positive removals on `packages/starlight/integrations/remark-rehype.ts`, `isUnifiedProcessor` and `remarkDirectivesRestoration`. `__tests__/markdown-processor/plugin-registration.test.ts` does `import * as unifiedIntegration from '../../integrations/remark-rehype'` and hands it over whole, both as a bare call argument (`registerDirectivesRestoration(processor, unifiedIntegration)`) and as a positional argument to `applyStarlightMarkdownPlugins`. The callees reach `unified.isUnifiedProcessor(...)` and `unified.remarkDirectivesRestoration`, so both exports are used. Nothing new is reported in any of the four projects.\n\n## Cache invalidation\n\nBoth layers are bumped:\n\n- extract `CACHE_VERSION` 277 -> 278, because extraction output changes (new `whole_object_uses` entries, and the deduplicated record);\n- `GRAPH_CACHE_VERSION` 41 -> 42, because narrowing verdicts are baked into the persisted graph at build time; a warm graph cache would replay the narrowed verdict verbatim.\n\nWarm-cache proof on the fixture:\n\n1. baseline binary (extract 277, graph 41), cold run: reports all seven `*Moon` siblings and writes `.fallow`.\n2. patched binary on that same warm cache: only `DottedMoon` reports, the precision control.\n3. second warm run with the patched binary: identical output.\n\n## How it was tested\n\n- Extract-level tests: every handover position records the whole-object use while the dotted access is kept; every resolved position does not; the JSX member tag stays narrowed while a JSX attribute pass does not; a reference above its own import declaration is still recorded and one name yields one entry; a named import stays out of scope; and the object-literal placement plus the destructure keep narrowing while a rest element does not.\n- Integration test on fixture `issue-2377-whole-object-use`, one consumer per handover shape (JSX attribute value, call argument, alias, array literal element, object literal value, return value), each against its own namespace target so a miscredit cannot be masked, plus the precision control that a dotted-only namespace still narrows and reports its unused sibling.\n- Mutation matrix: with the non-test source hunks reverted, the four positive tests and the integration test fail; the three negative controls (`resolved_namespace_reference_keeps_narrowing`, `resolved_namespace_placement_keeps_narrowing`, `bare_named_import_reference_is_not_a_whole_use`) pass either way by design, since they pin behavior the fix must preserve. Restoring the hunks makes everything green again.\n- CLI run of the issue's exact reproduction: only the one genuinely unused export remains.\n- Real-project runs and warm-cache proof as above.\n- Gates: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --lib --bins --tests --examples`, `cargo check --workspace --benches`, `RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps --document-private-items`, `typos`, `scripts/scan-hidden-unicode.py --mode committed --staged`, `scripts/check-comment-quality.mjs --staged`, and `scripts/check-knowledge-architecture.mjs` all green.\n\n## Known adjacent gap\n\nA CSS module default import has the same defect shape (`import styles from './x.module.css'` plus one `styles.a` access plus a whole pass narrows to `a`). It is deliberately out of scope here and filed separately.\n\nFixes #2377",
          "timestamp": "2026-08-23T19:44:03+02:00",
          "tree_id": "5866bafe3208443f9fe663ea3d0bc54581bbf261",
          "url": "https://github.com/fallow-rs/fallow/commit/969689e6de5675acebfdffd6e0538bc9e56372eb"
        },
        "date": 1787507369089,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9747856,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49231,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1194695,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8425,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f951395d7eca5f4ac3fabb338f28c5b2c2c0a869",
          "message": "fix(output): carry analysis-stage workspace diagnostics on every envelope that can hold them\n\nAnalysis-stage workspace diagnostics now reach every JSON envelope that can carry them, and every envelope that folds two observation points together agrees with the others.\n\nFixes #2366\n\n## The bug\n\nA malformed `pnpm-workspace.yaml` (`malformed-pnpm-workspace-yaml`) and the bun.lockb override-resolution skip (`bun-lockb-override-resolution-skipped`) warned on stderr but reached no JSON field on the bare combined `fallow --format json` run. Two causes: the combined envelope had no diagnostics field at all, and every per-analysis config reload replaced the diagnostics registry while preserving only the source-discovery kinds, wiping what the analyze pass had recorded before the envelope was built.\n\n## What the branch changes\n\n**Carrier.** `CombinedOutput` gains an optional top-level `workspace_diagnostics[]`. The carrier is the envelope root rather than a section, so `--skip check`, `--only health`, and `--only dupes` report what their analyses recorded, and the `check`, `dupes`, and `health` sections stay report bodies that never repeat the array. The value is the union of what every analysis in the run recorded, deduplicated by `fallow_types::workspace::merge_workspace_diagnostics`.\n\n**Preserve and refresh.** `WorkspaceDiagnosticKind::is_analysis_stage` classifies the two analyze-stage kinds with an exhaustive match, so a new kind must be classified before it compiles. `stash_workspace_diagnostics` preserves them across config reloads the way it preserves source-discovery kinds, and `clear_analysis_stage_diagnostics` runs at the start of every dead-code analyze pass, so a watch-mode or engine-session rerun drops the entry once the YAML is fixed or a text `bun.lock` exists.\n\n**One list per analysis.** The source walk writes its whole source-discovery set in one registry operation and hands the list back; `AnalysisSession` snapshots that list instead of reading the registry back, and `DupesResult` carries the same per-analysis snapshot `CheckResult` and `HealthResult` already carry. A combined run under a per-analysis `production` split walks the project twice in parallel (`rayon::join`), so a single registry read used to answer \"whichever walk wrote last\" and the union came out in a different order between runs of the same command.\n\n**Filtered registry leg.** Every fold now closes with `fallow_config::registry_diagnostics_to_fold` instead of a raw registry read. It drops the walk-recorded kinds (`skipped-large-file`, `skipped-minified-file`), which reach an envelope only from the walk that recorded them, and keeps every other kind, which is what still lets a fold pick up a parse-stage `source-read-failure` or an analysis-stage kind recorded after the section captured its list. Without the filter, `fallow audit --format json` under `production: { deadCode: true }` reported the oversized test file that only the non-production health and dupes walks skipped, while the MCP `audit` tool and the standalone `dead-code` envelope reported none. The engine session's live read, the CLI audit family, the CLI combined root and the programmatic combined root all use it, and an architecture guard pins all four.\n\n**Programmatic parity.** `serialize_combined_programmatic_json` folds its typed sections in the same order and closes with the same registry read as the CLI. The health run records the analysis-stage kinds inside its own dead-code precompute, after `HealthProgrammaticOutput::workspace_diagnostics` is captured, so `run_combined` with `dead_code: false` returned an empty root array where the CLI's `--skip check` and `--only health` report the skip.\n\n**Payload-aware dedupe, folded at discovery.** The fold keys on the whole typed `kind` plus `path`, not on the kind id, so two overlapping workspace globs (`[\"pkgs/*\", \"pkgs/a*\"]`) still report the same package-less directory once per `pattern` the way the standalone envelopes do. Because the payload decides identity, a declared glob's no-op `./` prefix is normalised out of the recorded `pattern`, and the recorded `path` drops the matching no-op `.` component that the `./pkgs/*` spelling joins into every match. That normalisation alone would have made a glob declared in two manifests produce byte-identical duplicates rather than distinct ones, so workspace discovery now deduplicates its own list before returning it, in `collect_workspaces_and_diagnostics`, not only in the process registry. `package.json` `workspaces`, `pnpm-workspace.yaml` `packages`, `deno.json` `workspace` and the root `tsconfig.json` references are additive sources; one glob declared in two of them is walked twice, and the fold is what makes the returned list, the workspace listing envelope, the MCP `project_info` tool, the analysis and audit envelopes, and the aggregated stderr warning tell one story. The warning matters most: it groups by `pattern` and counts the group, so before the fold it claimed 4 package-less directories where 2 existed and named one of them twice among its examples. The same fold repairs the case where both manifests spell the glob identically, which produced byte-identical duplicates before this branch as well, and it makes a malformed member reached through both an npm glob and a root `tsconfig.json` reference one diagnostic instead of one per source. That second shape is a finding-count decrease against the released binary on every envelope that carries the array, not only against this branch's earlier commits, and the CHANGELOG and `docs/backwards-compatibility.md` name it.\n\nOne pattern is exempt from the `./` strip: a glob spelled exactly `\"./\"`, the project root itself. Stripping it left an empty `pattern` field and a warning that quoted no glob at all, so `canonical_glob_pattern` now strips only when a non-empty remainder survives.\n\n**Workspace listing paths.** `WorkspaceDiagnostic::into_root_relative` normalises at the typed layer, and the two builders that assemble the `list-workspaces` envelope apply it, so `workspace_diagnostics[].path` is project-relative there like everywhere else. A path outside the project root, which canonicalisation crossing a symlink can produce, stays absolute, matching how the human-readable `message` renders it.\n\n**Schema description.** The typed `workspace_diagnostics[]` description said the kinds are \"surfaced during config load\", wrong since source-discovery diagnostics joined the array in #1086 and wrong again since the two analysis-stage kinds joined it. `CheckOutput::workspace_diagnostics` now names all three recording stages with their kinds, states that analysis-stage kinds only reach envelopes whose run includes a dead-code analyze pass, and states the path and empty-array contract; `HealthOutput`, `DupesOutput`, and `WorkspacesOutput` point at it.\n\n## Behaviour changes\n\n| Surface | Before | After |\n| --- | --- | --- |\n| `fallow --format json` (combined root) | no `workspace_diagnostics` field | optional union of every analysis's list, omitted when empty |\n| `fallow audit --format json`, `fallow review --format json`, `fallow audit --brief --format json` | `dead_code.workspace_diagnostics[]` carried the workspace-discovery and source-discovery kinds | additionally the two analysis-stage kinds and `undeclared-workspace`; no longer reports a skip only another analysis's walk saw |\n| MCP `audit` tool, MCP code-mode `combined`, NAPI, embedders | narrower than the CLI under a `production` split | same answer as the CLI |\n| the `list-workspaces` envelope (`fallow list --workspaces --format json`, `fallow workspaces --format json`, bare `fallow list --format json`) and MCP `project_info` | absolute `workspace_diagnostics[].path` | project-relative with forward slashes |\n| bare `fallow list --format json` and the envelopes built from an engine session snapshot (`project_info`, `find_dupes`, `check_health`, programmatic dead-code and combined) | collapsed two overlapping globs matching one package-less directory into one entry | one entry per matching pattern, agreeing with `list --workspaces` |\n| every envelope, and the aggregated stderr warning, on a repository declaring one glob in two manifests | every matched directory reported once per spelling; the summary counted the duplicates and repeated an example | once per directory; the summary names the true count with each example once; the recorded `pattern` has no `./` prefix and the recorded `path` no `.` component |\n| the human workspace listing (`fallow workspaces`, `fallow list --workspaces`, bare `fallow list`) and the `N workspace discovery diagnostics` summary line every human-format command prints | one entry per declaration: the block header counted the duplicates and the lines quoted both spellings | one entry per directory, quoting the canonical spelling |\n| SARIF, markdown, compact, badge, CodeClimate, cache format | unchanged | unchanged |\n\nNo `schema_version` moves: the combined root field is additive and optional, the audit-family arrays are the already-typed `CheckOutput` array, and no field changes type. No cache bump.\n\n## Tests\n\n- `combined_json_root_carries_analysis_stage_workspace_diagnostics`, `combined_json_carries_workspace_diagnostics_without_a_check_section`, `combined_json_only_dupes_carries_workspace_discovery_diagnostics`\n- `combined_json_root_unions_workspace_diagnostics_across_split_production_modes`, `combined_json_root_carries_a_diagnostic_only_the_dupes_walk_recorded`, `combined_json_root_workspace_diagnostics_are_byte_identical_across_repeat_runs`, `combined_json_root_keeps_both_overlapping_glob_diagnostics`\n- `audit_json_dead_code_section_carries_analysis_stage_workspace_diagnostics`, `review_json_dead_code_section_carries_analysis_stage_workspace_diagnostics`, `audit_json_dead_code_section_carries_the_analyze_appended_undeclared_workspace`\n- `audit_json_dead_code_section_carries_a_skip_only_the_dead_code_walk_saw` and its inverse `audit_json_dead_code_section_omits_a_skip_only_another_walk_saw`\n- `audit_json_dead_code_section_keeps_both_overlapping_glob_diagnostics`, `list_json_keeps_both_overlapping_glob_diagnostics`, `one_glob_declared_in_two_manifests_reports_one_diagnostic`\n- the two-manifest repository fixture, one glob declared `\"./pkgs/*\"` in `package.json` and `pkgs/*` in `pnpm-workspace.yaml` over two package-less directories: `every_envelope_reports_one_entry_per_directory_for_a_glob_in_two_manifests` (ten commands: bare combined, `--skip check`, `--only health`, `dead-code`, `check`, `health`, `dupes`, `list`, `list --workspaces`, `workspaces`), `two_manifest_glob_warning_names_the_true_directory_count_once_each`, `two_manifest_glob_warning_counts_each_directory_once` (the pure warning planner), `one_glob_declared_in_two_manifests_reports_each_directory_once` (discovery), `project_info_reports_one_entry_per_directory_for_a_glob_in_two_manifests` (MCP agreement)\n- `two_manifest_glob_human_listing_prints_each_directory_once` pins the one non-JSON surface the fold reaches: the block header counts the deduplicated entries and each directory gets one line quoting `pkgs/*`\n- `a_dotted_glob_declared_once_reports_the_undotted_spelling_everywhere` isolates the `./` normalisation from the fold, over eight invocations (the four standalone envelopes, `list`, `list --workspaces`, `workspaces`, bare combined): one manifest, one declaration, one glob spelled `./pkgs/*`, and every envelope reports `pattern: pkgs/*`, `path: pkgs/aaa` and a message quoting the canonical spelling\n- controls: `overlapping_globs_still_report_one_directory_once_per_pattern`, `dedupe_keeps_first_of_each_pair_and_every_distinct_payload`, `new_stores_one_spelling_for_a_directory_reached_through_a_dotted_glob`, `new_keeps_a_root_only_glob_spelling_and_still_strips_a_real_prefix`\n- `manifest_cache_replays_the_same_err_on_every_visit` pins the memoized-`Err` replay on the cache itself. The end-to-end `malformed_member_reached_via_two_sources_is_diagnosed_once` cannot: `load_tsconfig_workspace_package` maps both `Ok(None)` and `Err` to the directory-name fallback, and the discovery fold hides a second diagnostic, so a cache that swallowed the replay would still pass it\n- `combined_programmatic_json_carries_diagnostics_without_a_dead_code_section` (api), `merge_folds_two_spellings_of_one_glob_into_one_diagnostic` and `merge_keeps_two_diagnostics_that_share_a_kind_id_and_path` (types)\n- `list_workspaces_json_emits_project_relative_diagnostic_paths`, `stash_preserves_recorded_analysis_stage_diagnostics_across_restash`, `later_session_drops_stale_analysis_stage_diagnostic_after_cause_is_fixed`\n- architecture guards: single registry writer, by-value session snapshot, analysis-stage recorders confined to the dead-code analyze pass, and the filtered registry leg at all four fold sites\n\nMutation matrix: removing the discovery fold fails the discovery test, the warning-planner test, both CLI two-manifest tests and the human-listing test, while `a_dotted_glob_declared_once_reports_the_undotted_spelling_everywhere` still passes, which is what makes the two shapes independently pinned; making `canonical_glob_pattern` the identity fails the dotted test on its `pattern` assertion and the human-listing test on its quoted spelling; making `normalise_diagnostic_path` the identity fails the dotted test on its `path` assertion alone; making the path normalisation the identity fails the types spelling test and the CLI envelope test; dropping `into_root_relative` from the workspace listing builder fails both `project_info` tests; dropping the non-empty-remainder guard from `canonical_glob_pattern` fails `new_keeps_a_root_only_glob_spelling_and_still_strips_a_real_prefix` on the `\"./\"` assertion; memoizing only `Ok` outcomes in `load_member_package_manifest_cached` fails `manifest_cache_replays_the_same_err_on_every_visit` while `malformed_member_reached_via_two_sources_is_diagnosed_once` still passes, which is exactly why the replay needed its own test. Every other test above fails with its own source hunk reverted.\n\n## Validation\n\n`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --lib --bins --tests --examples`, `cargo check --workspace --benches`, `RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps --document-private-items`, `typos`, hidden-unicode scan, comment-quality check and `CI=true npm run generate:contracts:check` all pass on the tree rebased onto `main`.\n\nCommand sweep, baseline binary versus this branch, over five fixtures and fifteen commands each (bare combined, `--skip check`, `--only health`, `--only dupes`, `--production`, `dead-code`, `check`, `health`, `dupes`, `list`, `list --workspaces`, `workspaces`, `audit`, `audit --brief`, `review`), every fixture a git repository with a working-tree change so the audit family has a diff to report on. Each cell is the total/unique count of `workspace_diagnostics[]` entries found anywhere in the envelope, baseline to branch:\n\n| fixture | standalone `dead-code` / `check` / `health` / `dupes` | audit family | bare combined and its four splits | `list`, `list --workspaces`, `workspaces` | stderr WARN |\n| --- | --- | --- | --- | --- | --- |\n| one glob `pkgs/*`, one manifest | 1/1 to 1/1, byte-identical | 1/1 to 1/1, byte-identical | absent to 1/1 | 1/1 to 1/1, path now project-relative | unchanged |\n| two overlapping globs `[\"pkgs/*\", \"pkgs/a*\"]`, one manifest | 2/2 to 2/2, byte-identical | 2/2 to 2/2, byte-identical | absent to 2/2 | bare `list` 1/1 to 2/2, the two listing commands unchanged apart from the path | unchanged |\n| one glob `./pkgs/*`, one manifest, one source | 1/1 to 1/1, `pattern`, `path` and `message` all move | 1/1 to 1/1, same movement | absent to 1/1 | 1/1 to 1/1, same movement | quoted spelling moves |\n| `./pkgs/*` in `package.json` plus `pkgs/*` in `pnpm-workspace.yaml`, two package-less directories | 4/4 to 2/2 | 4/4 to 2/2 | absent to 2/2 | 4/4 to 2/2, bare `list` 2/2 to 2/2 | 3 to 2 |\n| declared `packages/*` plus an undeclared `tools/extra` | 0/0 to 0/0 | absent to 1/1 | absent to 1/1 | 1/1 to 1/1 | unchanged |\n\nRow three is the reviewers' repro for the byte-identity claim: a repository that declares one glob, once, through one source still moves when that glob is spelled `./pkgs/*`, because the `./` normalisation does not depend on duplication. The CHANGELOG and `docs/backwards-compatibility.md` now name the `./` spelling and the duplicate declaration as two independent causes and promise byte-identity on the four standalone envelopes only when neither is present; rows one and two are the confirmation that the promise holds when neither is.\n\nRow five is a divergence worth naming: the combined root and the audit family now report the `undeclared-workspace` entry that `list` and `workspaces` always reported, while the standalone `dead-code`, `check`, `health` and `dupes` envelopes still report none, because they read the process registry a later per-analysis stash has already replaced. That standalone-side gap reproduces identically on the baseline binary and predates this branch; the branch removes it from the three envelopes it touches rather than adding it. It is filed as a follow-up.\n\nNon-JSON sweep on the two-manifest and the dotted fixture: `dead-code`, `check`, `health` and `dupes` in `markdown`, `sarif`, `compact`, `badge` and `codeclimate`, plus combined `markdown` and `compact`, are byte-identical on stdout and stderr. Every surface that does move is human stderr: the aggregated warning, the `N workspace discovery diagnostics` summary line, and the workspace-listing block, which goes from four lines under two spellings to two lines under one on the two-manifest fixture and re-quotes the glob on the dotted one. No stdout of any format changes.\n\nReal projects: `viz-frontend` and `editors/vscode` record no diagnostic and are unchanged. On a public design-system monorepo that declares `\"./apps/**\"` in `package.json` and `apps/**` in `pnpm-workspace.yaml`, `fallow list --workspaces --format json` goes from 276 entries (276 unique, all absolute paths, 138 with a `./` pattern) to 138 entries, 138 unique, no dotted or absolute path and no empty pattern. The aggregated stderr warnings drop from six lines to three, and the surviving glob line names 136 directories on both binaries. The probe ran with `--no-cache` and left the project's existing `.fallow` byte-identical.\n\nA warm `.fallow` written by the baseline binary yields the identical array under this branch, cold and warm.\n\n## Follow-ups\n\nTwo gaps are pre-existing, reproduce on the released binary, and are filed rather than fixed here:\n\n- the standalone `dead-code`, `check`, `health` and `dupes` envelopes read the process registry through `workspace_diagnostics_for_output`, which a later per-analysis stash has already replaced, so they miss the `undeclared-workspace` entry the analyze pipeline appends. Row five of the sweep is the repro.\n- `replace_source_discovery_diagnostics` clears the parse stage's `source-read-failure` entries along with the walk-recorded kinds, because `is_source_discovery` covers that kind too, so a concurrent walk on the same root can transiently drop another analysis's read failures. Self-healing today (each parse re-records its own set and the closing registry leg runs after both walks); this branch documents it in the rustdoc rather than narrowing the retain.\n\n## Trailers\n\n`Fixes #2366` appears once across the branch. Every other commit carries `Refs #2366`.",
          "timestamp": "2026-08-24T07:10:23+02:00",
          "tree_id": "d79ff22f49ed0c7ea16fdbef35b5c2f29f84574d",
          "url": "https://github.com/fallow-rs/fallow/commit/f951395d7eca5f4ac3fabb338f28c5b2c2c0a869"
        },
        "date": 1787548572947,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9740004,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49224,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1197550,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8457,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "8eb190505e6a310544e897eb841dc398dac73e36",
          "message": "fix(extract): record import X = require as a CommonJS import edge\n\n## What was broken\n\n`import X = require('./x')` produced no import at all. `crates/extract/src/visitor/` had no arm for a `TSImportEqualsDeclaration` with an external module reference: only `visit_impl_structural.rs` saw the node, and only to shadow a function-type alias name. With neither an `ImportInfo` nor a require call, the target was reported as an unused file and nothing reached through the binding was credited.\n\nReproduction from the issue, `package.json` with `\"main\": \"src/index.ts\"`:\n\n```ts\n// src/assigned.ts\nnamespace Assigned {\n  export const viaAssignment = 1;\n}\nexport = Assigned;\n\n// src/index.ts (entry)\nimport Assigned = require('./assigned');\nconsole.log(Assigned.viaAssignment);\n```\n\n`fallow dead-code --format json --quiet --no-cache` reported `src/assigned.ts` as an unused file. Replacing the consumer line with `import Assigned from './assigned'` made it reachable, so the gap was specific to the import-equals form.\n\n## The fix\n\nA `visit_ts_import_equals_declaration` arm on the `Visit` impl hands the declaration to `handle_import_equals_declaration`, which pushes one non-destructured `RequireCallInfo` (the specifier, the `require('./x')` reference span, the specifier string span, no destructured names, the local binding) plus the local name onto `namespace_binding_names`. That is the same shape `handle_require_declaration` records for `const X = require('./x')`; the two paths run in parallel rather than one calling the other, because a `TSExternalModuleReference` carries a `StringLiteral` where the variable form carries a `CallExpression`.\n\nRecording it as a require call rather than as a hand-rolled `ImportInfo` is the point. `resolve_single_require` already turns a non-destructured require into a `Namespace` import against a target carried through `into_commonjs_require()`, so reusing it gives the CommonJS mechanism, `narrow_namespace_references`, the whole-object namespace seeds from #2372, and the specifier-anchored `source_span`, with no second spelling of the same edge to keep in sync.\n\nThe arm sits on the visitor, so it fires wherever the declaration appears: at file scope, and inside a `declare module '...'` body.\n\n### Both semantic namespaces\n\nLiving outside `ModuleInfo.imports` cost the binding its type/value classification: `compute_semantic_usage_with_candidates` only walked `imports`, so `type_referenced_import_bindings` never named a require-derived binding and `desired_import_namespaces` fell through to value only. A type reached through the binding was therefore left uncredited, and the target's interface surfaced as an `unused-type` row the `import * as` twin never produces.\n\nThe extractor now carries an `import_equals_bindings` vector and feeds it into the same semantic classification an ESM namespace binding gets, so `T.SomeType` in an annotation credits type space and `T.value` credits value space independently. Classification reads the root scope only, exactly the restriction the `imports` loop beside it has: a binding declared inside a `declare module '...'` body is not classified and stays value-only, the same way an `import * as X` binding in that position does.\n\n### An unreferenced binding credits nothing\n\nA binding with no resolved reference anywhere in the file is now reported as an unused import binding, which is the verdict the `imports` loop reaches for an unreferenced `import * as X from './x'`. TypeScript elides both declarations completely, so neither may buy the target a whole-object credit.\n\nCrediting it was strictly worse than the missing edge this PR started from: `import Utils = require('./utils')` in a file that never mentions `Utils` again deleted every `unused-export` and `unused-type` row on `utils.ts`, rows `main` reports correctly, and `import type Shapes = require('./shapes')` deleted both lanes at once. The edge itself is untouched, so the target is still a reachable file, exactly as it is behind an unreferenced `import * as X`.\n\n`export import X = require('./x')` is exempt: it has no local reference by construction and the binding is the file's public API.\n\n### The erased spelling\n\n`import type X = require('pkg')` is the one require spelling TypeScript erases completely: the emitted JavaScript holds no `require` call at all, so the package is a type-space reference and never a runtime import. `RequireCallInfo` now carries `is_type_only`, set from `decl.import_kind.is_type()` and resolved through to the `ImportInfo`, so dependency classification treats the declaration the way it treats `import type * as X from 'pkg'`.\n\nWithout that, a type-only devDependency reported `dev-dependency-in-production` with the note \"production code imports this at runtime\" and an auto-action telling the user to move the package into `dependencies`, where the byte-equivalent ESM twin reported nothing. The unerased `import X = require('pkg')` still reports it, and still matches its own twin.\n\n### The exported form\n\n`export import X = require('./x')` recorded no export either. `narrow_namespace_references` derives `is_re_exported` from an export whose `local_name` matches the binding, so it stayed false, `is_entry_with_no_access` fired on an entry point, and every export of the target became a false `unused-export` row. That is a direct coherence break with #2373 on the shape the declaration exists for.\n\nThe exported form now earns a whole-object use of the binding. That is the credit `import * as X from './x'; export { X }` already produces: the module object reaches consumers the graph cannot enumerate, so every name on it is observed, including the names the target only exposes through its own `export *` chain. No export row is invented for the binding itself, so the change adds no new finding class.\n\n`whole_object_uses` is keyed by bare name, so the credit is only safe while the name resolves to this one binding, and the visitor cannot know that: scope information first exists in the semantic pass. The credit is therefore **granted** there, only for a name the file binds once, rather than pushed at walk time and withdrawn afterwards. Withdrawing by name deleted a whole-object use the file genuinely wrote:\n\n```ts\nexport import Config = require('./config');\nexport const readConfig = (): number => Object.values(Config).length;\nexport const parseConfig = (Config: { n: number }): number => Config.n;\n```\n\nThe unrelated parameter made the name shadowed, the withdrawal removed the entry `Object.values(Config)` had recorded, and both exports of `src/config.ts` turned into false `unused-export` rows with an auto-fixable remove-export action. Renaming only the parameter made them disappear, and `tsc` is clean on the file. Granting instead of withdrawing also composes with #2377, which deduplicates `whole_object_uses` at the recording site: with one entry per name, removing \"the provisional one\" would have removed the only one.\n\n### A bare handover credits the whole object\n\nThe binding is registered as a namespace-import local, so a reference the visitor cannot resolve to one member (a call argument, an alias, a return value) records a whole-object use, exactly as #2377 does for `import * as X`. Without it, a consumer that writes one dotted access plus one handover narrowed to that member alone and reported every sibling the receiver still reaches:\n\n```ts\nimport Icons = require('./icons');\nconst register = (value: unknown): number => Object.keys(value as object).length;\nexport const handedOver = (): number => Icons.star + register(Icons);\n```\n\n`src/icons.ts: moon` reported, where the `import * as Icons` twin reports nothing. That false positive is created by this PR (before it, the target was simply unreachable), so it is closed here rather than left to a follow-up.\n\n`import X = Some.Namespace` keeps today's behaviour. An entity-name reference aliases a binding declared in the same file, not a module, so the require-call guard, the whole-object guard and the namespace-local registration all return before recording anything, and the qualified-name walk that credits `Some.Namespace` is untouched.\n\n## Design decisions\n\n1. **Reuse the require path, do not invent an `ImportInfo`.** The require path already carries the CommonJS mechanism, namespace narrowing, the whole-object seeds and the specifier span. A second spelling of the same edge would have to be kept in sync with all of it.\n2. **The whole-object credit, not an `ExportInfo` row, for `export import`.** The credit reaches the outcome the fix needs (every export of the target credited on an entry point that only re-exports the binding) without adding a finding class to a fix whose purpose is to remove false rows. Recording an export row for the binding would also start reporting a re-export nothing consumes as an unused export, which is a behaviour change of its own; it is left out rather than taken here.\n3. **Grant the whole-object credit in the semantic pass, never withdraw it.** Withdrawal is name-keyed and cannot tell a provisional mark from a genuine `Object.values(X)`, and #2377's deduplication leaves exactly one entry to remove. Granting is the same outcome for every unshadowed case and strictly safer for the shadowed one.\n4. **The unreferenced binding follows the ESM twin, not `const X = require()`.** The declaration is spelled `import` and TypeScript elides it like an import. Following the `const` model instead deleted rows `main` reports, which is a false negative introduced by this PR rather than a pre-existing gap it inherits.\n5. **The namespace-body call site stays, as leniency only.** `namespace N { export import X = require('./x') }` is TS1147, so no compiling project reaches it. fallow parses leniently and still has to behave, so the arm is kept and pinned by one clearly labelled extract test. The fixture covers the two spellings TypeScript accepts instead: file level, and inside a `declare module '...'` body.\n6. **Type-only is a flag on the require call, not a separate lane.** One extra bool keeps the erased spelling on the same edge, narrowing and reachability path as the unerased one, and only changes what dependency classification concludes.\n\n## Parity with the `import * as X` twin\n\nEvery shape this PR touches now produces the same findings as its ESM twin. Measured on one scratch project holding each import-equals shape next to a byte-equivalent `import * as` shape against its own target, `dead-code --no-cache --format json`, patched binary:\n\n| Shape | import-equals target | `import * as` twin target |\n|---|---|---|\n| unreferenced binding | `utils.ts`: `a1`-`a5` + `UtilsShape` | `utils-esm.ts`: `b1`-`b5` + `UtilsEsmShape` |\n| unreferenced `import type` binding | `shapes.ts`: `shapeValue` + `ShapesShape` | `shapes-esm.ts`: `shapeEsmValue` + `ShapesEsmShape` |\n| exported form, genuine whole-object use, shadowed name | `config.ts`: nothing | `config-esm.ts`: nothing |\n| dotted access plus bare handover | `icons.ts`: nothing | `icons-esm.ts`: nothing |\n| entry-point re-export with a type export | `re.ts`: `ReShape` | `re-esm.ts`: `ReEsmShape` |\n\nThe same project on the pre-review revision of this branch, rebased onto the same `main`, reported `config.ts: alpha`, `config.ts: beta` and `icons.ts: moon` as false positives and was missing `utils.ts: a1`-`a5`, `shapes.ts: shapeValue` and `shapes.ts: ShapesShape`.\n\nOne asymmetry remains, listed below.\n\n## Behavior change\n\nUnused-file and unused-export findings decrease for repositories using the form. A file that becomes reachable for the first time has its exports narrowed against the members the consumer writes, so a sibling nothing accesses surfaces as an unused export where the unused-file row used to stand in its place.\n\nA binding re-exported through `export import` credits its whole target instead of narrowing to the members the declaring file happens to read. Narrowing there was unsound: a consumer holding the re-exported module object can reach any member of the target. This matches what `export { X }` already credits for a namespace import, and it is what removes the entry-point false positives above.\n\nA type-only devDependency reached only through `import type X = require('pkg')` is no longer reported as production usage.\n\nTotal finding count is not guaranteed to decrease. The edge is now visible to every check that reads imports, so an import-equals whose specifier does not resolve reports `unresolved-import`, and a bare specifier no manifest lists reports `unlisted-dependency`, exactly as the equivalent `import X from './x'` already did.\n\n## Known deviation\n\n- **No export row for the re-exported binding.** `export import X = require('./x')` records no export for `X` itself, so a re-export nothing consumes is not reported, where the `import * as X; export { X }` twin does report `X` as an unused export. Verified on the patched binary with a non-entry consumer: the twin reports `src/mid-esm.ts: ReEsm`, the import-equals form reports nothing. A miss, never a false positive, and it matches `main`. Worth a follow-up, not taken here.\n\nThe `export = <binding>` deviation listed on the previous revision no longer reproduces: with #2377 on `main`, `import T = require('./t'); export = T;` and its `import * as T` twin both credit the target in full, and both report nothing.\n\n## Cache invalidation\n\n- extract `CACHE_VERSION` 278 to 279: extraction records a require call, its type-only flag, a type/value binding classification, an unused-binding verdict and a whole-object use it did not record before, so a warm 278 module replays without them. `CachedRequireCall` grows by the flag, and its size assertion moves from 88 to 96.\n- `GRAPH_CACHE_VERSION` 42 to 43: the edge and the references it credits are baked into the persisted graph.\n\nBoth are exactly one above `main` at 969689e6d. `DUPES_CACHE_VERSION` is untouched.\n\nWarm-cache proof on the scratch parity project, against a binary built from that same `main` commit:\n\n1. `main` binary (extract 278, graph 42) cold run writes `.fallow`: `src/config.ts`, `src/icons.ts`, `src/shapes.ts` and `src/utils.ts` reported as unused files, plus every ESM twin's rows.\n2. Patched binary on that warm cache: the fixed verdict, byte-identical to its own cold `--no-cache` run.\n3. Second patched run on the now 279/43 cache: identical to step 2.\n\n## How it was tested\n\n- Extract unit tests: the require call and its local binding, the require and specifier spans, member accesses through the binding, the type and value classification of the binding (both together and type-only), the unused-import-binding verdict on an unreferenced binding and on the erased `import type` spelling (each against its own `import * as` twin, with a referenced binding as the positive control), the exemption of the exported form (against its `import * as X; export { X }` twin), the type-only flag on the erased spelling against the unerased one, the require call from inside a `declare module` body, the whole-object credit for the file-level exported form, its withholding when the name is shadowed (with an unshadowed positive control), the survival of a genuine `Object.values(X)` under a shadowed name plus a one-name-one-entry assertion, the bare-handover whole-object use against its twin with a dotted-only negative control, and negative controls pinning that an unexported binding, an ambient-module member and an entity-name import-equals record no whole-object use. One test is labelled as a deliberate lenient-parse pin for the TS1147 namespace-body spelling.\n- Integration tests on fixture `issue-2365-import-equals`: reachability through the binding, value narrowing and type narrowing each held against an equivalent `import * as` twin declared with the same shape, object destructuring off the binding held against its twin, a whole-object use crediting the target's own `export *` chain, the entry-point `export import` form held against the `import * as X; export { X }` twin, an unreferenced binding crediting neither exports nor types (held against its twin, reachability asserted first so the assertion cannot hold vacuously), a genuine whole-object use surviving a shadowed exported binding (held against its twin), the `declare module` body crediting its package, the type-only devDependency held against its `import type * as` twin with an unerased devDependency as the positive control, and the entity-name negative control.\n- Mutation matrix for this revision's three source changes, applied one at a time to the committed tree and restored afterwards: neutralising the unreferenced-binding report fails `unreferenced_import_equals_reports_an_unused_import_binding` and `an_unreferenced_import_equals_binding_credits_nothing`; restoring the push-then-withdraw shape fails `a_genuine_whole_object_use_survives_a_shadowed_export_import_equals` and `a_shadowed_export_import_equals_keeps_a_genuine_whole_object_use` (which reports `[\"shadowedAlpha\", \"shadowedBeta\"]`, the defect verbatim); dropping the namespace-local registration fails `a_bare_import_equals_reference_is_a_whole_object_use`. `exported_import_equals_is_not_an_unused_import_binding` and the other negative controls pass either way by design, since they pin behaviour the fix must preserve. The full suite is green with every hunk restored.\n- Real projects, `main` at 969689e6d versus patched, `dead-code --no-cache --format json`. `viz-frontend` (0), `editors/vscode` (1), `vitest` (810), `rijkshuisstijl-community` (234), `vue-core` (160) and `next.js` (24272) are identical row for row; the two large ones differ only in a `next_steps` recommendation, which reads local run history rather than the analysis. On the TypeScript compiler repository, which carries the form throughout its test corpus, `unused_files`, `unused_exports`, `unused_types` and every other finding list are identical; only `unresolved_imports` (1453 to 1693) and `unlisted_dependencies` (201 to 300) move. Every new row sits on an `import X = require(...)` line in `tests/baselines/reference/**`, where a TypeScript emit baseline embeds the original source next to the emitted JavaScript and the specifier resolves to nothing, so an `import X from './foo'` on the same line would already report the same way.\n- Rebased onto `main` at 969689e6d, which carries #2377. Conflicts in `CHANGELOG.md`, `crates/extract/src/cache/types.rs`, `crates/graph/src/cache/mod.rs` and `crates/extract/src/visitor/mod.rs` were resolved by hand, keeping both sides and stacking the cache bumps one above `main`. The `assert_cached_type_size!(CachedRequireCall, 96)` assertion still holds after the rebase.\n- Gates on the rebased tree: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --lib --bins --tests --examples`, `cargo check --workspace --benches`, `RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps --document-private-items`, `typos`, `scripts/scan-hidden-unicode.py --mode committed --staged`, `scripts/check-comment-quality.mjs --staged`. No serde or schemars type changed, so no generated contract surface is implicated.\n\nFixes #2365",
          "timestamp": "2026-08-24T07:17:45+02:00",
          "tree_id": "c659ac17eacda497e6b5cb61f2eb951b52773e0c",
          "url": "https://github.com/fallow-rs/fallow/commit/8eb190505e6a310544e897eb841dc398dac73e36"
        },
        "date": 1787548965210,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9746540,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49238,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1146577,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7055,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "62752fa6df4feb1e4ad646f4b2b536c52f2b7db3",
          "message": "fix(extract): route ambient export type * through a type-only whole-module import\n\n## What was broken\n\n`export type *` and `export type * as ns` inside a `declare module '...'` body still created a file-level type-only star re-export on the declaring file. [#2357](https://github.com/fallow-rs/fallow/issues/2357) routed the plain `export *` and `export * as ns` spellings through a bindingless whole-module import so the declaring file gains no export surface, but that shape carried no type modifier: routing the type-only star through it as-is would have credited the value meaning of every target export, which the star erases.\n\nBoth directions were wrong, with `src/pair.ts` holding `export interface Foo { a: number }`, `export const Foo = 1`, `export const plain = 3` and `export default function d(): void {}`:\n\n- A non-entry `src/shim.ts` carrying `declare module 'pkg' { export type * from './pair' }` plus `export {}`, imported for side effects by the entry, reported the `Foo` interface, the `Foo` const, `plain` and `default`. Nothing credited the target, so the interface half was a false positive: the type star forwards it.\n- With the same declaration in an entry-point `src/ambient.d.ts`, the file-level star laundered the consts into the entry's public surface and only `default` reported: the `Foo` const, which the type star does not forward, reported nowhere.\n\n## Root cause\n\n`visit_export_all_declaration` took the ambient branch only for `!decl.export_kind.is_type()`, so the type-only spellings fell through to the file-level `ReExportInfo`. The whole-module shape the ambient branch records is read by `ImportedSymbol::is_ambient_star`, and `desired_import_namespaces` credits both namespaces for it, which is right for `export *` and wrong for `export type *`. Nothing in the persisted extract shape could tell the two apart.\n\n## The fix\n\nEarliest incorrect layer is extraction. `ImportInfo` and the graph's `ImportedSymbol` gain `is_type_only_star`. The extractor now takes the ambient branch for every star spelling and sets the flag from `export_kind.is_type()`, so `export type *` records exactly what `export *` records (one type-space `Namespace` import with an empty local name, plus a `Default` import for the `as ns` form, and no `ReExportInfo`) with the flag set.\n\nIn the graph, `ImportedSymbol::is_value_bearing_ambient_star` names the plain star alone, and `desired_import_namespaces` reads it, so a type-only star credits the target's star surface in the type namespace and nothing else. Everything else about the shape is unchanged: `mark_star_surface_referenced_at_site` still skips `default` for the plain form, the extra `Default` import of the `as ns` form still reaches `ns.default` (now in type space), and the exposed-namespace closure seed is the same, so the chain behind the target keeps exactly the credit the plain star gives it.\n\n## Behavior change\n\n- **The type half of a same-name type and value pair behind a non-entry shim stops reporting.** That is the false positive in the issue.\n- **The target's value-only exports behind such a shim stop reporting too.** `export type *` forwards them as type-only bindings, reachable as `typeof plain`, and the graph credits them through the type-space fallback lane. This is exactly the credit the ambient `export type { plain } from './pair'` form has given since [#2349](https://github.com/fallow-rs/fallow/issues/2349), verified against a binary built from main, so the two spellings stay consistent. The issue text expected these rows to survive; making the star stricter than the named form it generalizes would have been the inconsistency.\n- **The value half of a same-name pair behind an entry-point `.d.ts` starts reporting.** The laundered entry surface is gone, so a `const Foo` the type star does not forward is a finding again. That is a finding-count increase on upgrade for repositories with an entry-point ambient `export type *`.\n- `export type *` forwards no `default`, exactly like the plain star; `export type * as ns` forwards `ns.default` in type space.\n- Plain ambient stars, the ambient named re-export forms, and `import()` type references in TypeScript and JSDoc are unchanged. A bare-specifier `export type *` inside an ambient body stays type-only package usage, so `--production` classification does not move.\n\n### Known limitation\n\nThe chain behind the target (its own `export *` and `export * as sub` sources) is credited at full namespace-object exposure, in both namespaces, because the closure has no namespace dimension. A type star erases the value meanings of those names too, so that credit is more generous than the target's own surface. Keeping the seed as it is means no shape reports more than it did before this change; splitting the closure per namespace is filed separately.\n\n## Cache invalidation\n\n- extract `CACHE_VERSION` 279 -> 280: warm caches replay the file-level star re-export and lack the flag.\n- `GRAPH_CACHE_VERSION` 43 -> 44: the persisted graph carries the old `ReExportEdge`, the laundered entry surface, and the value-lane credits, and a graph-cache hit skips the build entirely.\n\nWarm-cache proof on a scratch copy of the fixture sharing one `.fallow` directory: a binary built from main (279/43) runs cold, writes `cache.bin` and `graph-cache.bin`, and reports the laundered and uncredited shapes; the patched binary (280/44) on that warm cache reports exactly its own cold output, twice.\n\n## How it was tested\n\n- Extract tests: `export type *` and `export type * as ns` inside an ambient body record the flagged whole-module shape, no file-level export and no `ReExportInfo`, and the `as ns` form adds the default import. The test that pinned the old file-level shape is replaced by these.\n- Cache test: the flag survives a `ModuleInfo` to `CachedModule` round trip, and a bound `import type { Foo }` does not carry it.\n- Graph unit tests: `desired_import_namespaces` returns type space only for the flagged `Namespace` and `Default` symbols; the type star credits the interface half of a pair in the type namespace, leaves the const half with no reference at all, credits a value-only export in type space, and leaves `default` unreferenced; the `as ns` form credits `default` in type space and nowhere else.\n- Integration fixture `issue-2375-ambient-type-star`: an entry-point `ambient.d.ts` with `export type * from './entry-pair'`, a reachable non-entry `shim.ts` with `export type * from './shim-pair'`, `ambient-ns.d.ts` with `export type * as ns` over a target carrying a type default (`export default interface`) and over a value-only target, and `ambient-value.d.ts` with a plain type star over a value-only target. Four tests assert the credited type surface, the value halves and defaults that keep reporting, the empty export surface and re-export list on all four declaring files (with the entry-point and non-entry roles pinned), and the namespace of the credited references.\n- Mutation matrix: reverting the extractor hunk fails both new extract tests and all four integration tests; reverting the lane hunk fails the three graph unit tests and the two integration tests that read lanes; reverting the cache mapping fails the round trip.\n- Issue reproduction through the patched binary, both directions: the non-entry shim reports the `Foo` const and `default` (was: those plus the interface and `plain`); the entry-point `.d.ts` reports the `Foo` const and `default` (was: `default` alone).\n- Regression pins re-run: the `issue-2357`, `issue-2349`, `issue-2372` and `issue-2373` fixtures all pass unchanged, and a chain probe confirms the type star credits a `export * from` plus `export * as sub` chain identically to the plain star.\n- Real-project smokes: `dead-code --format json` with the main and patched binaries on the in-repo `viz-frontend` and `editors/vscode`, normalized outputs identical.\n- Gates: cargo fmt check, clippy workspace all-targets with warnings denied, full workspace test suite, bench check, cargo doc with warnings denied, typos, hidden-unicode scan, comment-quality check, and the agent-adapter check, all green.\n\nFixes #2375",
          "timestamp": "2026-08-24T09:57:20+02:00",
          "tree_id": "5830a45096c226b9118819d3c43f8213fdf53a26",
          "url": "https://github.com/fallow-rs/fallow/commit/62752fa6df4feb1e4ad646f4b2b536c52f2b7db3"
        },
        "date": 1787558872504,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9719003,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49187,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1175955,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8364,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "9532b3fb45443be3ae29053e4df4cbf6cab432a7",
          "message": "test(config): compare the per-instance dedupe key with a platform-native tail (#2398)",
          "timestamp": "2026-08-24T08:43:49Z",
          "tree_id": "8c9cb762e2e4501d578bc6caec727fa26eeff5dc",
          "url": "https://github.com/fallow-rs/fallow/commit/9532b3fb45443be3ae29053e4df4cbf6cab432a7"
        },
        "date": 1787561267815,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9745836,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49234,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1194668,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8401,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bartwaardenburg@gmail.com",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "cac2a1438c82eec446fe856f54bb9ae3efe77463",
          "message": "fix(ci): complete provider lifecycle reconciliation",
          "timestamp": "2026-08-24T12:16:13+02:00",
          "tree_id": "a5ea7303285fc5ddc225741194979ad5f3e6469c",
          "url": "https://github.com/fallow-rs/fallow/commit/cac2a1438c82eec446fe856f54bb9ae3efe77463"
        },
        "date": 1787566812872,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9742312,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49231,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1190541,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8445,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "edf720b9b43955058fdcbab71089ee589d7f9c4c",
          "message": "fix: close remaining analysis consistency gaps\n\n* chore: start correctness sweep\n\n* fix(extract): align Convex and MDX statement discovery\n\n* fix(graph): align export crediting across spellings\n\n* fix(trace): align syntactic and type-aware evidence\n\n* fix(workspace): align diagnostics and override analysis\n\n* docs: document correctness sweep contracts\n\n* fix: resolve correctness review findings\n\n* fix(type-aware): remove stale project binding\n\n* style(type-aware): format barrel regressions\n\n* fix: close final review edge cases\n\n* fix(extract): close shadow and mutation gaps\n\n* fix(extract): close wrapped CommonJS mutations\n\n* test(type-aware): satisfy cancellation lint\n\n* fix(extract): preserve scoped namespace owners\n\n* fix(extract): register namespace binding owners",
          "timestamp": "2026-08-24T12:20:11Z",
          "tree_id": "526a9cd87dc8684c7beae3ce63f9142890fc9970",
          "url": "https://github.com/fallow-rs/fallow/commit/edf720b9b43955058fdcbab71089ee589d7f9c4c"
        },
        "date": 1787574330495,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9696323,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49265,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1189489,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8442,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "77cf2cf659697e89748c49c2a4c421fc0924e129",
          "message": "perf(trace): omit redundant namespace evidence",
          "timestamp": "2026-08-24T16:12:29+02:00",
          "tree_id": "17c89c3de031d78c1fd8693b22dd7ccc20e35d03",
          "url": "https://github.com/fallow-rs/fallow/commit/77cf2cf659697e89748c49c2a4c421fc0924e129"
        },
        "date": 1787581068605,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9734539,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49279,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1179060,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8407,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "e3a0f9854fdecddae851226049f6e0fad6ca103c",
          "message": "test(mcp): bound env route analysis threads",
          "timestamp": "2026-08-24T18:02:10+02:00",
          "tree_id": "2c557e29ef716fd3a8bfb509430370010537cfe0",
          "url": "https://github.com/fallow-rs/fallow/commit/e3a0f9854fdecddae851226049f6e0fad6ca103c"
        },
        "date": 1787587574594,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9748365,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49347,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1182613,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8360,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bartwaardenburg@gmail.com",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "4330a637d8a19aa094386828fa530acf1ddcde31",
          "message": "Merge pull request #2402 from fallow-rs/feat/contract-doc-drift\n\nfix: align generated contracts and agent guidance",
          "timestamp": "2026-08-24T19:33:32+02:00",
          "tree_id": "7653d731cf7c44d9d58302b23b97b8adeb427e8f",
          "url": "https://github.com/fallow-rs/fallow/commit/4330a637d8a19aa094386828fa530acf1ddcde31"
        },
        "date": 1787593124085,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9739787,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49282,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1171504,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8338,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "2262b4b6321374ae91ae6c4ae8df046035a83210",
          "message": "fix(engine): detach Git probes from MCP stdin\n\n* test(mcp): serialize env route servers\n\n* test(mcp): allow nested env route work\n\n* fix(mcp): isolate typed analysis workers\n\n* test(mcp): preserve default threads in env routes\n\n* test(mcp): isolate type-aware sidecar fixture\n\n* fix(ci): isolate Windows MCP test suite\n\n* style: format workflow policy test\n\n* test(mcp): trace Windows typed worker phases\n\n* test(ci): run MCP diagnostics first\n\n* test(ci): build CLI before MCP diagnostics\n\n* test(mcp): compare debug and release worker entry\n\n* fix(ci): order Windows MCP process suites\n\n* fix(api): prepare discovery outside analysis pools\n\n* test(api): diagnose Windows analysis pool stall\n\n* test(api): isolate Windows output assembly stall\n\n* fix(engine): detach Git probes from protocol stdin",
          "timestamp": "2026-08-24T22:58:11+02:00",
          "tree_id": "92426dbdfcd76bfeaa7bf4c6de4da080769c1e75",
          "url": "https://github.com/fallow-rs/fallow/commit/2262b4b6321374ae91ae6c4ae8df046035a83210"
        },
        "date": 1787605404694,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9756245,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49377,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1173500,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8090,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "536f96da2ec0b83aa5ee676fd2a2cdd56c124139",
          "message": "chore: release v3.18.0",
          "timestamp": "2026-08-25T00:00:27+02:00",
          "tree_id": "a7c5e143f1b082b6655fc9afc2c4031b47a825e0",
          "url": "https://github.com/fallow-rs/fallow/commit/536f96da2ec0b83aa5ee676fd2a2cdd56c124139"
        },
        "date": 1787609079254,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9777405,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49385,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1175572,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8091,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "a2931f099f4f8a8088dd7be40b46c7c0e8aa33e8",
          "message": "chore: prepare v3.18.0 post-release sync",
          "timestamp": "2026-08-25T07:34:32+02:00",
          "tree_id": "0ad2781ea27d70dddae618a993f18f2152b7b9fa",
          "url": "https://github.com/fallow-rs/fallow/commit/a2931f099f4f8a8088dd7be40b46c7c0e8aa33e8"
        },
        "date": 1787636598694,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9753884,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49326,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1139974,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7034,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "1edd39e794b2c16dbc64fedda368c3df1ff903bc",
          "message": "fix(impact): name a newer store in the statusline\n\nAn older fallow reading an Impact store written by a newer release printed the generic data-unavailable line, which read as lost history. The statusline now prints data from newer fallow with an upgrade hint for that case; corrupt or unreadable stores keep the data-unavailable line.",
          "timestamp": "2026-08-25T11:29:38+02:00",
          "tree_id": "5ac3f059ffa9b0ec43bd3637213d3dbdff380b0b",
          "url": "https://github.com/fallow-rs/fallow/commit/1edd39e794b2c16dbc64fedda368c3df1ff903bc"
        },
        "date": 1787650564546,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9766677,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49371,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1175572,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8091,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "7a28a05378a747cb78c3bd5e95fd59618eabb1e4",
          "message": "fix(dead-code): collapse React Native platform families in duplicate-exports (#2416)\n\nWith the react-native or expo plugin active, an import of ./UserMenu credits every Metro platform-extension member, so UserMenu.tsx and UserMenu.ios.tsx shared an importer and surfaced as a duplicate pair. Each family now folds into one representative (the base file, otherwise the lowest path) before the importer partition. A genuine duplicate in an unrelated file is still reported against that representative. Without those plugins the output is unchanged.\n\nCloses #2407",
          "timestamp": "2026-08-25T14:06:16+02:00",
          "tree_id": "d72f29057e2625beedc7a9b33edc6364b35e7cbf",
          "url": "https://github.com/fallow-rs/fallow/commit/7a28a05378a747cb78c3bd5e95fd59618eabb1e4"
        },
        "date": 1787659925277,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9706675,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49278,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1171130,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8382,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bartwaardenburg@gmail.com",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "80812c0fa5ce43fe950653fe67e9a5dc44f140bf",
          "message": "Merge pull request #2408 from fallow-rs/feat/semantic-clone-conformance\n\nfeat: add local similar code intelligence",
          "timestamp": "2026-08-25T17:32:47+02:00",
          "tree_id": "159932b59cc074669c41d52e5fc239af8286fdde",
          "url": "https://github.com/fallow-rs/fallow/commit/80812c0fa5ce43fe950653fe67e9a5dc44f140bf"
        },
        "date": 1787672281350,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9769809,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49375,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1182275,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8417,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bartwaardenburg@gmail.com",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "da6a0486f88623d045799b47ad9b13faed04c362",
          "message": "Merge pull request #2434 from fallow-rs/fix/similar-code-review-findings\n\nfix: harden similar-code evidence and companion verification",
          "timestamp": "2026-08-26T00:15:40+02:00",
          "tree_id": "bfcc1155f6e6bf8b0e5beff461f27b9f680d7d9d",
          "url": "https://github.com/fallow-rs/fallow/commit/da6a0486f88623d045799b47ad9b13faed04c362"
        },
        "date": 1787696468884,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9754317,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49354,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1190974,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8451,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "10cc20b72382fd1b0f0ef19efa30f44b8c5913ec",
          "message": "feat: harden similar-code agent discovery workflow\n\nHarden scoped semantic discovery, snapshot-stable inspection, cache and provider lifecycle safety, programmatic contracts, conformance evidence, and release gates.",
          "timestamp": "2026-08-26T11:56:44+02:00",
          "tree_id": "fea78994dffea9ee1054f9024a1ac0a3474672fc",
          "url": "https://github.com/fallow-rs/fallow/commit/10cc20b72382fd1b0f0ef19efa30f44b8c5913ec"
        },
        "date": 1787738509245,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9757845,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49361,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1218940,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8535,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bartwaardenburg@gmail.com",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "dfa135e5a59103dab969e063789ff8ebd5533be9",
          "message": "Merge pull request #2439 from fallow-rs/feat/stylex-theme-styling\n\nfeat: complete StyleX theme styling support",
          "timestamp": "2026-08-26T12:26:06+02:00",
          "tree_id": "ead5dcae2432185db584b5bfe74b6a93afe024a6",
          "url": "https://github.com/fallow-rs/fallow/commit/dfa135e5a59103dab969e063789ff8ebd5533be9"
        },
        "date": 1787740295093,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9759365,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49363,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1188674,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8436,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f9cb3758ec1e69d9b6def5e0ef6da87a208ab994",
          "message": "feat(cli): add fallow agent install for one-pass harness onboarding\n\n`fallow agent install` wires the coding-agent harnesses a project uses in one pass, with `agent status` and `agent uninstall` covering the same surfaces. It detects Claude Code, Codex, and Cursor from project files, the home directory, and session variables (or takes `--harness`), then writes what each reads: an AGENTS.md task map plus a CLAUDE.md import, the fallow skill (a pointer to node_modules/fallow when present, otherwise the tree gzip-embedded at build time), the MCP registration in .mcp.json, .codex/config.toml, or .cursor/mcp.json, and the commit/push gate. Nothing is fabricated when no harness is detected.\n\nEvery write carries a versioned fallow:agent-install marker and re-runs are byte-stable. MCP entries are owned by shape (a hand-written `fallow` entry is refused and never removed without --force), --force on an unparsable config file saves the old bytes as <file>.fallow-bak first, JSON edits keep the file's indentation, uninstall deletes config files it emptied, and authored AGENTS.md or CLAUDE.md files are deleted only while they still hash to what fallow wrote. Claude MCP pre-approval stays opt-in through --approve (it also clears an earlier rejection) and is refused when .claude/settings.local.json is tracked. The JSON envelope carries kind, schema_version, fallow_version, evidence, steps with a closed reason set, and next_actions with a mutating flag.\n\n`init --agents` and `hooks install --target agent` are unchanged and remain the single-piece commands underneath; `setup-hooks` is deprecated with a stderr warning and is removed in the next major.",
          "timestamp": "2026-08-26T12:54:23+02:00",
          "tree_id": "b64e7ab718c21664d57bbe469e24989aacce1ef8",
          "url": "https://github.com/fallow-rs/fallow/commit/f9cb3758ec1e69d9b6def5e0ef6da87a208ab994"
        },
        "date": 1787741967017,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9755484,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49333,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1191797,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8440,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "999350fc29dceea509bfef1259977fb6c35c5fdf",
          "message": "feat(mcp): expose reference material as MCP resources\n\n`fallow-mcp` now declares the `resources` capability and serves its reference material as read-only, cacheable resources: `fallow://tools` (the tool manifest with CLI fallbacks), `fallow://issue-types` (every issue type with default severity, fixable flag, and docs URL), `fallow://explain` (index) plus the `fallow://explain/{issue_type}` template (the same document as `fallow explain --format json`), `fallow://task-matrix` (which read-only command to run before a task), and `fallow://schema/config`, `fallow://schema/plugin`, and `fallow://schema/rule-pack` (byte-identical to the CLI schema documents). Everything renders in-process from shared crates; no subprocess and no analysis run.\n\nThe server version travels in each content item's `_meta.fallow_version`, so payloads stay plain (the schema resources are valid strict JSON Schema) and a cached copy is self-describing. Resources carry exact `size`, `title`, and `audience: [\"assistant\"]` annotations with a higher priority on the tool manifest and task matrix; no `subscribe` or `listChanged` since the catalogue is compile-time constant. Unknown URIs and issue types return a structured error whose `data` lists the known URIs or the nearest issue types (`-32002` before protocol 2026-07-28, `-32602` after).\n\n`fallow schema` gains a matching `mcp_resources` block, the shipped skill reference gains a generated resource table, and the task matrix data moves to `fallow-types` so the MCP server can project it without depending on the CLI crate.",
          "timestamp": "2026-08-26T16:05:15+02:00",
          "tree_id": "94fbe8848e30d8692b70c15528698994f5669e1e",
          "url": "https://github.com/fallow-rs/fallow/commit/999350fc29dceea509bfef1259977fb6c35c5fdf"
        },
        "date": 1787753625502,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9767745,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49374,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1175572,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8091,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "8eaa92c8e95f33ebfc8148bb9cf81706fbba21a6",
          "message": "fix: keep main green on Windows and within the bundled skill line cap\n\nThe similar-code provider environment test read `Command`'s Debug output,\nwhich lists environment entries on Unix only, so it failed on Windows; it\nnow inspects `get_envs()` directly. The bundled SKILL.md had grown to 502\nvalidator lines after the agent and MCP resource additions; three blank\nlines after headings are dropped so it stays under the 500-line limit.",
          "timestamp": "2026-08-26T16:36:58+02:00",
          "tree_id": "cf55203ca01402bf9c8dcdfa4403e0b15facb1fb",
          "url": "https://github.com/fallow-rs/fallow/commit/8eaa92c8e95f33ebfc8148bb9cf81706fbba21a6"
        },
        "date": 1787755387623,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9714115,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49289,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1167835,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8341,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "bf98a4270ab5ce6fb9aca1e5c92a51da6dde2023",
          "message": "chore(docker): pin Dockerfile to v3.19.0",
          "timestamp": "2026-08-27T11:55:55+02:00",
          "tree_id": "55c52365376ec9d37614cbefdb1f12aac052f1eb",
          "url": "https://github.com/fallow-rs/fallow/commit/bf98a4270ab5ce6fb9aca1e5c92a51da6dde2023"
        },
        "date": 1787824924010,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9748781,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49349,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1190764,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8455,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "bcf333d8f9fe0753be4df17bf1a8a20be4fbdb53",
          "message": "feat(review): author actions, test adjacency, slices, and dependency decisions (#2446)\n\nReview-brief schema 7 -> 8, all additive.\n\n- Judgments carry an author-action label (block, address, consider, fyi) validated on reentry (invalid-action with invalid_value, checked after the anchor) and echoed fenced; the guide publishes action_vocabulary and concern_vocabulary.\n- Direction units carry test_adjacency (none, untouched, changed); both tours badge NO-DIRECT-TEST; root-level test/ and tests/ count as test paths; a project with no tests gets no claims.\n- The partition reports independent_slices (connected components of the inter-unit graph) when there are two or more.\n- The dependency decision arm fires on both the CLI and the typed/MCP route: added entries and major bumps per changed package.json, batched per manifest per kind, weighted by in-repo importers (union, value and type-only), section-tagged, rename-aware, npm: aliases read at their range; a major bump ranks with a public-API change; no comment-based suppress action on a manifest anchor.\n- The human and markdown tours show decisions whose anchor is not a staged unit, so a dependency-only change never renders as \"0 files\".\n- Review app: action on judgments and feed items, invalid_value, schema pin as a floor.\n\nVerified with unit and e2e tests on the real binary, real-project runs (fallow's vscode extension bump commit, a monorepo worktree with major bumps), verify:fast, and the contract drift gate. Companion PRs fallow-skills #42/#43 and fallow-docs #21 land with the release that ships schema 8.",
          "timestamp": "2026-08-27T14:11:13+02:00",
          "tree_id": "f0fafc776f5a2ca0062607ff01b7d6baaa008884",
          "url": "https://github.com/fallow-rs/fallow/commit/bcf333d8f9fe0753be4df17bf1a8a20be4fbdb53"
        },
        "date": 1787833000484,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9750941,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49363,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1182157,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8364,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "9406571ba1749fc34c0a516720c9fb167ed7a233",
          "message": "chore(napi): sync transitive similar-code platform pins to v3.19.0",
          "timestamp": "2026-08-27T15:13:00+02:00",
          "tree_id": "d82aef00df0a8cd605ed955cc2e2e2023522d411",
          "url": "https://github.com/fallow-rs/fallow/commit/9406571ba1749fc34c0a516720c9fb167ed7a233"
        },
        "date": 1787836716352,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9760629,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49363,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1185754,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8400,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "Patrick.Leong.Shaw@gmail.com",
            "name": "Patrick Shaw",
            "username": "PatrickShaw"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "8c3c5b7d9a26ff5e0a6c7393edcde3aaeab1397f",
          "message": "perf(graph): cut tsconfig cache lock contention on large monorepos (#2437)\n\nImport resolution on large project-reference monorepos is faster: the per-run tsconfig and canonicalize caches no longer serialize every lookup behind a single lock or deep-copy the parsed document on every hit. Cached entries are handed back as Arc values, so a hit costs a refcount bump instead of cloning the whole parsed tsconfig on each hop of a chain that is walked several times per import specifier.\n\nThanks to @PatrickShaw for the contribution.",
          "timestamp": "2026-08-27T21:49:35+02:00",
          "tree_id": "c0afb4d8d82e1cf3503e5d52ea9af25403799106",
          "url": "https://github.com/fallow-rs/fallow/commit/8c3c5b7d9a26ff5e0a6c7393edcde3aaeab1397f"
        },
        "date": 1787860505779,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9705227,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49271,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1168106,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8347,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "Patrick.Leong.Shaw@gmail.com",
            "name": "Patrick Shaw",
            "username": "PatrickShaw"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "d877424ca12c71cb9490bfd1d05be4975285a195",
          "message": "feat(graph): resolve Yarn Plug'n'Play projects through the PnP manifest (#2435)\n\nA Yarn Plug'n'Play install has no populated node_modules, so every bare import used to miss and fall through to the much slower tsconfig fallback. fallow now detects `.pnp.cjs` at the analyzed root or one of its ancestors, enables oxc's PnP resolution, and anchors manifest discovery to that directory, so runs started outside the project and editor sessions resolve the same way.\n\nManifests that are not inlined (`pnpEnableInlining: false`) are not supported and stay on the fallback path. The generated `.pnp.cjs` and `.pnp.loader.mjs` files are no longer discovered as project source. Bumps GRAPH_CACHE_VERSION so a warm cache does not replay the old unresolved imports.\n\nThanks to @PatrickShaw for the contribution.\n\nCloses #2444",
          "timestamp": "2026-08-27T22:01:43+02:00",
          "tree_id": "d12445987c0624ba10651f706fe82f4bf5e70479",
          "url": "https://github.com/fallow-rs/fallow/commit/d877424ca12c71cb9490bfd1d05be4975285a195"
        },
        "date": 1787861348871,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9704307,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49329,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1171497,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8385,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "8d53657a9e18fa00f8b8a83337428bf912de8c23",
          "message": "fix(check): fail strict runs on findings the override path let through (#2447)\n\nWith any per-path `overrides` entry configured, the exit-code check switches to per-file severity resolution. That path never consulted import-direction boundary violations, so an error-severity `boundary-violation` was reported but the run exited 0. The same path started from unpromoted base rules, so the warn-to-error promotion of `--fail-on-issues` and `--ci` never reached it and a `warn` rule plus any override exited 0 as well.\n\nBoth now resolve per file and promote after override resolution. Because the override path handles every file once any `overrides` entry exists, this affects all warn-severity rules in a project that configures overrides, not only the rules set inside the override block: a strict run that previously exited 0 can now exit 1 on those findings. To keep the previous outcome, set the rule to `off` rather than `warn`, or drop the strict flag for that job. The findings themselves are unchanged; only the exit code is.\n\nThanks to @DeLuke84 for the precise repro.\n\nCloses #2445",
          "timestamp": "2026-08-27T23:13:15+02:00",
          "tree_id": "25ac228449682b492e8e57a9748a5966b4c37934",
          "url": "https://github.com/fallow-rs/fallow/commit/8d53657a9e18fa00f8b8a83337428bf912de8c23"
        },
        "date": 1787865478876,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9741341,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49434,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1182171,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8404,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "53633741+PrinceD96@users.noreply.github.com",
            "name": "Daniel Morales",
            "username": "PrinceD96"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "fe3fdd2321d3445df6695126a3d29a77384ac533",
          "message": "fix(health): match Istanbul coverage by body location and skip bodyless declarations (#2443)\n\nOverload signatures, abstract members, and `declare function` declarations no longer count as functions, so function counts drop for every file that carries them and its file score moves without any code change. A file whose declarations are all bodyless leaves the file-score table entirely.\n\nCoverage matching improves at the same time: each coverage-map function entry now contributes up to three candidate positions (the producer's own, the declaration start, and the body start), so a function whose only structural match was its body location scores against real coverage instead of a static estimate. Because an expression-bodied arrow's recorded body is the next arrow in a curried chain, a body-start candidate yields to a declaration at the same position, which keeps every arrow of a middleware chain, a higher-order component, or a curried class property matchable.\n\nRegression baselines are unaffected. Re-save health baselines if you run with `--coverage`, because a newly matched function can cross the CRAP ceiling.\n\nThanks to @PrinceD96 for the report and the contribution.\n\nCloses #2442",
          "timestamp": "2026-08-28T01:18:22+02:00",
          "tree_id": "f91a88e6e20b319d0ffef97e69fd76eea960cc70",
          "url": "https://github.com/fallow-rs/fallow/commit/fe3fdd2321d3445df6695126a3d29a77384ac533"
        },
        "date": 1787873022486,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9775997,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49435,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1191956,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8444,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "5a2a685eb56790043daaf15245ecad2ab5901387",
          "message": "chore: release v3.20.0",
          "timestamp": "2026-08-28T02:21:21+02:00",
          "tree_id": "d12b08d634a7b9adc277279b0efd4614b43f99f0",
          "url": "https://github.com/fallow-rs/fallow/commit/5a2a685eb56790043daaf15245ecad2ab5901387"
        },
        "date": 1787876852848,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9708667,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49333,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1177786,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8358,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "802bfd2c4e2e397409e16b5e87d7914464bdae11",
          "message": "chore(napi): sync package.json / package-lock / index.js to v3.20.0",
          "timestamp": "2026-08-28T05:31:51+02:00",
          "tree_id": "50069b7aeaf722d1d81d0b3d1b636e322394165c",
          "url": "https://github.com/fallow-rs/fallow/commit/802bfd2c4e2e397409e16b5e87d7914464bdae11"
        },
        "date": 1787888251497,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9756981,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49415,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1185970,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8404,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "53633741+PrinceD96@users.noreply.github.com",
            "name": "Daniel Morales",
            "username": "PrinceD96"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "df15924cb3ace9155aa431625f7e151d445617a6",
          "message": "fix(health): attribute Istanbul coverage to the function that owns the position (#2449)\n\nIstanbul coverage now reaches the functions whose extracted position falls\nbetween the producer's declaration and its body: a class member carrying a\ndecorator and a wrapped parameter list, and the innermost arrow of a curried\nchain formatted one per line. The header span identifies those, and it is read\nonly when exactly one anonymous record covers the position and no other\nfunction is declared inside it.\n\nAttribution is tightened at the same time. A member whose parameter list holds\na function no longer reports that function's coverage, a private class member\ntakes the static estimate rather than the coverage of whatever encloses it, and\na named function expression is resolved against the real source rather than a\nguess at the keyword's width. Coverage maps with project-relative keys join\nfrom any working directory, and the fallbacks are bounded by line indexes so a\nmap that does not join no longer costs a full scan per function.\n\nCloses #2448\n\nThanks to @PrinceD96 for the report and the implementation.",
          "timestamp": "2026-08-29T08:01:05+02:00",
          "tree_id": "f9149d4ff6be4fc046834d0a2ad9e53870db824e",
          "url": "https://github.com/fallow-rs/fallow/commit/df15924cb3ace9155aa431625f7e151d445617a6"
        },
        "date": 1787983586405,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9773469,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49437,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1201343,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8426,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "4e21f86a386ceae6a2a9dce0ebdd0607e7c2e33d",
          "message": "fix(health): stop a coverage map from lowering scores it never measured (#2458)\n\nA function whose file tests reach, and which no record in the coverage map\ncould be attributed to, scored as though it were fully covered, while the same\nfunction without a coverage map kept the static estimate. Passing real coverage\ndata could take a function under `--max-crap` that failed the gate without it.\nBoth paths now use the same estimate, so a map only moves a score for a\nfunction it actually measured.\n\nThe summary also reports how much of the coverage file joined. A map written\nfor a different root, a container path prefix, or an older checkout used to\nread exactly like code with no tests. `istanbul_files_matched` and\n`istanbul_files_total` separate the two, and the human report adds one line\nwhen they differ.\n\nCloses #2453\nCloses #2455",
          "timestamp": "2026-08-30T10:40:07+02:00",
          "tree_id": "71a6cbf7cc170e0f2a026b3ddb58282a0f31a86c",
          "url": "https://github.com/fallow-rs/fallow/commit/4e21f86a386ceae6a2a9dce0ebdd0607e7c2e33d"
        },
        "date": 1788079470789,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9715341,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49400,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1186230,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8446,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "7a3fcb30d2c6650bd5db529276a835add84710b1",
          "message": "fix(health): read the coverage maps other producers actually write (#2461)\n\nc8, nyc in v8 mode, and older vitest versions write a coverage map in which the\nimplicit else of a bare `if` carries `column: -1`. Positions are unsigned, so\none unplaceable coordinate in `branchMap`, a section the CRAP path never reads,\naborted the run with exit 2. Unplaceable coordinates are now clamped on a retry\nthat only runs after the strict parse has failed.\n\nRaw V8 coverage and `oxc-coverage-instrument` record an accessor as `get area`\nwhere istanbul-lib-instrument leaves the record anonymous, and fallow extracts\nthe unit as `area`. A covered accessor read as unmeasured under the first two\nproducers. A record now answers to its property name as well as to the\nspelling the producer chose.\n\nThe MCP coverage fixture asserted a body span the instrumenter does not emit\nfor its own source, and `coverage_tier` now documents what it describes when\nnothing measured the function.\n\nCloses #2454\nCloses #2456",
          "timestamp": "2026-08-30T11:41:03+02:00",
          "tree_id": "254c5dcd9162208570cdb11ccd12a5eec29ce063",
          "url": "https://github.com/fallow-rs/fallow/commit/7a3fcb30d2c6650bd5db529276a835add84710b1"
        },
        "date": 1788083192263,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9695051,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49313,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1159177,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8316,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "935f1ca88e7d0925c75bcb159df915d6e55db84c",
          "message": "feat: report skipped hidden directories that hold source files (#2450)\n\nCloses #461.\n\nSource discovery skips dot-prefixed directories outside a small convention\nallowlist, and the skip was silent, so first-party code under a directory such\nas `.claude/hooks/` was invisible with no explanation and no config field to\nreach it.\n\nA `skipped-source-dotdir` workspace diagnostic and one aggregated stderr note\nnow name each skipped directory that holds source files the project has not\nexcluded, state that its imports and exports are not analyzed, and give the two\nreal remedies: `fallow --root <dir>`, or `ignorePatterns` to silence it.\nTraversal is unchanged. Classification is bounded and deterministic.\n\nTwo containment defects are fixed alongside it. A `package.json` script\nreference now scopes the exact root-relative path it names instead of every\ndirectory of that name in the tree, with the scope's match mode carried across\nthe engine boundary. `.pnpm` joins the script-scope denylist beside\n`.pnpm-store`, along with 17 further generated-output and VCS directories.\n\nThe diagnostic kind is additive under the open-set exception for\n`workspace_diagnostics[].kind`, so no envelope moves its `schema_version`.",
          "timestamp": "2026-08-30T19:57:32+02:00",
          "tree_id": "52406d8eac7daee188cacd29942cf996175779af",
          "url": "https://github.com/fallow-rs/fallow/commit/935f1ca88e7d0925c75bcb159df915d6e55db84c"
        },
        "date": 1788112984705,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9764877,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49430,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1186790,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8410,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "d69e459ad2b8f98a1d3fec3a59b3d3013262be76",
          "message": "test(cli): stop copying a fixture's cache directory into the copy (#2483)\n\nTwo tests in the same binary share `tests/fixtures/coverage-gaps`. One runs the\nreal binary with that fixture as its root, so the binary writes and renames\ncache files under `.fallow/`. The other copies the fixture into a temp\ndirectory, walking every entry it finds, and fails with a not-found when the\nwriter renames a cache file mid-walk. It surfaced as an unrelated red check on\na dependabot pull request that only bumped a devDependency.\n\nA fixture's cache directory is not part of the fixture, and a copied project\nwants a cold cache anyway, so both copy helpers skip it.",
          "timestamp": "2026-08-30T23:33:51+02:00",
          "tree_id": "19c59e682f2491adf62e4f7a2cd1cbdbbe3886d7",
          "url": "https://github.com/fallow-rs/fallow/commit/d69e459ad2b8f98a1d3fec3a59b3d3013262be76"
        },
        "date": 1788125877079,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9774572,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49410,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1185720,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8430,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "3446f413b252cf0950ac63782e0b0ff25a1162df",
          "message": "chore: release v3.21.0",
          "timestamp": "2026-08-31T00:42:39+02:00",
          "tree_id": "e583367048294079d0a67e90746aefe8047d0ec9",
          "url": "https://github.com/fallow-rs/fallow/commit/3446f413b252cf0950ac63782e0b0ff25a1162df"
        },
        "date": 1788130054449,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9783341,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49451,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1195753,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8399,
            "unit": "allocations"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "committer": {
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "d0ebe05d32fa21bdb2ba7af3a02ef1c8efdde620",
          "message": "chore(docker): pin FALLOW_VERSION 3.21.0 with refreshed checksums",
          "timestamp": "2026-08-31T02:45:21+02:00",
          "tree_id": "c3ea82b32c10073b069cbde044f0d614e93e14bc",
          "url": "https://github.com/fallow-rs/fallow/commit/d0ebe05d32fa21bdb2ba7af3a02ef1c8efdde620"
        },
        "date": 1788137487988,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9741461,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 49391,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1189575,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8442,
            "unit": "allocations"
          }
        ]
      }
    ]
  }
}