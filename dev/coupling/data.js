window.BENCHMARK_DATA = {
  "lastUpdate": 1789157397798,
  "repoUrl": "https://github.com/fallow-rs/fallow",
  "entries": {
    "Module Coupling": [
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
        "date": 1787660374630,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 47,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 28,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.31,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 457,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1246,
            "unit": "count"
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
        "date": 1787672303452,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 48,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 28,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.3,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 460,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1252,
            "unit": "count"
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
        "date": 1787696473801,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 48,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 28,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.3,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 461,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1256,
            "unit": "count"
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
        "date": 1787738541473,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 48,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 28,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.3,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 461,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1256,
            "unit": "count"
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
        "date": 1787740302766,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 48,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 28,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.3,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 461,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1256,
            "unit": "count"
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
        "date": 1787741959521,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1787753710874,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1787755385044,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1787824946114,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1787833106370,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1787836746640,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1787860512832,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "b0585d4b6b699447066714569c1ea5e2a612c90c",
          "message": "fix(graph): scope a referenced tsconfig without include to its own directory (#2436)\n\nA tsconfig.json without `include` or `files` now applies only to files under its own directory when fallow follows project `references`, matching tsc's `**/*` default project scope. Previously such a referenced config matched every file in the repository, so its `paths` aliases leaked to files outside that directory and every referenced project was walked for every import.\n\nImports that only resolved through that leak are now reported as `unresolved-import` findings, which are error severity by default, and a file that was only reachable through such an import may now be reported as unused. Give the subdirectory config an explicit `include`, or move the shared aliases to a config whose directory contains the importing files. Root and workspace configs are unaffected.\n\nBumps GRAPH_CACHE_VERSION so a warm cache does not replay the old resolutions.\n\nThanks to @PatrickShaw for the contribution.",
          "timestamp": "2026-08-27T21:58:11+02:00",
          "tree_id": "51e192031afe2836b05808f641f708ca3cbd7f65",
          "url": "https://github.com/fallow-rs/fallow/commit/b0585d4b6b699447066714569c1ea5e2a612c90c"
        },
        "date": 1787860871068,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1787861168949,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1787865579860,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1787873050004,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1787876873466,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1787888268485,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1787983596876,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1788079544656,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1788083147367,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1788112916074,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1788125890995,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1788129918350,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
        "date": 1788137461868,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "48417c46faa284f18304c4f49f50a6edecdcfdb7",
          "message": "fix(cli): resolve project-local fallow in lefthook (#2465)\n\nA real installed Git hook preserves its caller's PATH and does not add\n`node_modules/.bin`, so a project with fallow pinned locally could have the\ngenerated Lefthook job exit successfully without auditing anything. The job now\nprefers a global `fallow`, then the project-local launcher, then the Yarn\nPlug'n'Play binary, and still skips when there is none.\n\nThe Yarn arm passes the audit arguments through the separator yarn requires.\nMeasured on yarn 1.22.22, `yarn exec fallow audit --base HEAD` reaches the\nbinary as `audit` alone, so without it the hook would audit the default base\nwith no gate marker and say nothing about it.\n\nCloses #2464\n\nThanks to @PrinceD96 for the report and the implementation.",
          "timestamp": "2026-08-31T13:33:06+02:00",
          "tree_id": "42edcfbe7c0f368abcdba2585faa7f1a02f617f8",
          "url": "https://github.com/fallow-rs/fallow/commit/48417c46faa284f18304c4f49f50a6edecdcfdb7"
        },
        "date": 1788176586271,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "d5a422c6206d00c813fe7c77d5038855d1051780",
          "message": "fix(telemetry): stop asserting a lock release is instantly visible\n\n`spool_lock_excludes_concurrent_acquire` demanded that the post-drop reacquire\nsucceed on its first attempt. Dropping the holder closes the descriptor, but\nthe kernel does not promise the release is visible to the next `flock` right\naway, and under a loaded parallel workspace run it measurably is not. Refs\n#2460.\n\nThe diagnostic added in #2459 is what pinned this down. The failure arrives as\n`Contended`, not `Unusable`, so the lock file opened fine and the lock was\nsimply still held a moment after its holder was gone. That rules out the\nenvironment explanations (a missing directory, a permissions denial, a\ndescriptor limit) and leaves release visibility, which is a property of the\nplatform rather than of this code.\n\nRetrying is not a mask, because production never needed the guarantee the test\nwas asserting. Both callers of `try_acquire`, the over-cap trim and the drain,\ntreat contention as \"skip, the next run picks it up\". A release that becomes\nvisible a few milliseconds later costs nothing there. What still matters is\nthat the lock does come free once its holder is gone, and that is still\nasserted: the test fails if it never reacquires across the full window.\n\nThe sibling assertion, that a second acquire contends while the first is held,\nis unchanged and still immediate, since that direction has no visibility delay\nto absorb.",
          "timestamp": "2026-08-31T14:28:52+02:00",
          "tree_id": "8a6961c9cdf1897e714ac1f552d596f80568273e",
          "url": "https://github.com/fallow-rs/fallow/commit/d5a422c6206d00c813fe7c77d5038855d1051780"
        },
        "date": 1788179480755,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "a67216a0461decae46b5f3a9c59ed4a4ca7c3e8e",
          "message": "test(cli): give each migrate test its own directory (#2490)\n\nTwenty-nine tests built their working directory from a fixed name under the\nsystem temp dir, and several deleted that directory on the way in, so two\nprocesses running the suite at once shared the same paths and one removed the\nfixture another was reading.\n\nMeasured with eight concurrent instances of the lib test binary filtered to\n`migrate::`: 8 of 8 runs failed before, 0 of 8 after, with the same tests\nfailing repeatedly rather than randomly. Deterministic given overlap, not load.\n\nEach test now takes a unique directory from `tempfile::tempdir()`, which\nremoves itself on drop and retires 35 hand-written cleanup calls.\n\nRefs #2460",
          "timestamp": "2026-08-31T14:38:38+02:00",
          "tree_id": "c563a4b1c288c6cd671f8013f15ec8453c6ba60b",
          "url": "https://github.com/fallow-rs/fallow/commit/a67216a0461decae46b5f3a9c59ed4a4ca7c3e8e"
        },
        "date": 1788180114220,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "173ee9b117aabba2a2d39795abacccc7f680bbaf",
          "message": "feat(sveltekit): recognize the SvelteKit 3 file conventions (#2488)\n\nCloses #2400. Thanks @filiabel for the heads-up ahead of the release.\n\nSvelteKit 3 is still 3.0.0-next.25 on npm, and the issue asks to wait for the\nofficial release before implementing the migration. This does not implement it.\nIt closes the gap a version 3 project hits today, measured rather than read off\nthe migration guide.\n\nThe current binary reports src/params.ts, src/instrumentation.server.ts and\nsrc/service-worker/index.ts as unused files, all wrong: each is loaded by the\nframework rather than imported. Every matcher exported from src/params.ts is\nreported as an unused export on top of that, because version 2 matchers each\nexported a fixed match from their own file while version 3 collapses them into\none file whose export names are the matcher names.\n\nThree entry patterns and one used-exports entry, additive, with no version 2\nshape touched. Verified by a new integration test against a new fixture, proven\nto fail without the plugin change, the existing SvelteKit tests, and an end to\nend probe going from three unused-file findings to zero.\n\nConfiguration moving from svelte.config.js into sveltekit() plugin options is\ndeliberately not covered: that option shape is still moving in the release\ncandidate, so alias resolution waits for 3.0.0 final.",
          "timestamp": "2026-08-31T15:31:39+02:00",
          "tree_id": "d7d44be14a8c3be0a4b9f326be6158aff05b7749",
          "url": "https://github.com/fallow-rs/fallow/commit/173ee9b117aabba2a2d39795abacccc7f680bbaf"
        },
        "date": 1788183373284,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "829a7cd490df0492fad6fdbd9b2059813050820d",
          "message": "fix(cli): resolve a project-local Fallow from the standalone commit gate (#2492)\n\nFollow-up to #2465, which fixed the generated Lefthook job: a real installed Git\nhook keeps its caller PATH and does not add node_modules/.bin, so command -v\nfallow misses a project-local install and the job exits 0 without auditing.\n\nThe standalone .claude/hooks/fallow-gate.sh never learned any of it. Its only\nproject-local arm was npx --no-install, which needs npx on the hook own PATH\nrather than the shell one, and cannot see a Plug and Play install at all,\nbecause Plug and Play has no node_modules/.bin for npx to look in. Both cases\nfell through to \"binary not found, skipping audit\", the same silent success\n#2464 was filed for.\n\nThe script now tries the same installs in the same order as the job it ships\nalongside: PATH, the node_modules/.bin launcher, Yarn Plug and Play, then npx\n--no-install as a last resort. A comment in each file points at the other, since\nthe two must stay in step.\n\nTwo execution tests run the real rendered script rather than asserting on a\nstring: a project-local launcher with no global install reachable, and a Plug\nand Play install with no node_modules directory at all. Both reduce PATH to the\nprobe directory plus the system ones and link jq into the probe directory, since\non a developer machine the jq directory also holds the global fallow and would\nsatisfy the very resolution step under test. An unreachable\nFALLOW_GATE_MIN_VERSION makes the chosen runner observable in the block message.\nBoth fail without the new arms.",
          "timestamp": "2026-08-31T16:29:15+02:00",
          "tree_id": "6a331d36aadb5ec386086234912afceff48c2285",
          "url": "https://github.com/fallow-rs/fallow/commit/829a7cd490df0492fad6fdbd9b2059813050820d"
        },
        "date": 1788187085203,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "8c0f2244fe0cf5543cec5c9d65abcf3616e73ab5",
          "message": "fix(graph): resolve root-relative HTML assets against a static directory (#2489)\n\nFound while checking SvelteKit support against the official SvelteKit RealWorld\napplication, which reported two findings that are both wrong.\n\nsrc/app.html links /conduit-theme.css, and the file it names is\nstatic/conduit-theme.css, exactly where SvelteKit expects it. Fallow reported an\nunresolved-import for /conduit-theme.css from src/app.html and an unused-file for\nstatic/conduit-theme.css: one cause, two findings, because the HTML\nroot-relative fallback in the resolver looked only in public/.\n\npublic/ is the Vite, Next and Create React App convention. SvelteKit, Gatsby and\nDocusaurus serve static/ at the site root instead, so every root-absolute asset\nreference in those projects missed and produced the same pair.\n\nBoth directories are now tried, in order. A candidate is accepted only when the\nfile is really on disk, so a project with neither directory, or with the\ndirectory but not the file, resolves exactly as before and cannot gain a\nreference it does not have. The traversal guard on the relative path is\nunchanged.\n\nGRAPH_CACHE_VERSION goes to 48: resolver output is persisted with the graph and\nthe cache key does not cover this, so a warm 47 cache would replay the earlier\nmiss as both findings.",
          "timestamp": "2026-08-31T17:31:12+02:00",
          "tree_id": "38efb7e9e56ccfa8dcb8bba78cdf862a5f46c6f8",
          "url": "https://github.com/fallow-rs/fallow/commit/8c0f2244fe0cf5543cec5c9d65abcf3616e73ab5"
        },
        "date": 1788190609666,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "e368c0d05060409b9bcea702972415a85193c914",
          "message": "fix(process): wait out an executable that is busy at spawn (#2496)\n\nUnix refuses to exec a file while any process holds it open for writing, and the\nholder is not always the process that opened it. `fs::write` closes its\ndescriptor, but a fork from any other thread during that open window carries a\nreference to the same open file description into the new child, and the inode\nkeeps counting a writer until that child reaches its own exec. A multi-threaded\nprocess that writes an executable and then runs it therefore races every other\nthread that spawns. That is how `cargo test -p fallow-api --lib` failed once in\nCI with `failed to spawn /tmp/.../fallow-similar-code: Text file busy`. The same\nshape reaches real users from the other side: an MCP tool call that re-runs the\nFallow binary while a package manager is still writing it.\n\nA new `crates/process/src/spawn_retry.rs` holds one retry schedule (200\nmicroseconds, doubling, capped at 20 milliseconds, one second total) behind two\nhelpers: a blocking one for `std::process::Command` and a Tokio one that awaits\n`tokio::time::sleep` so the pause yields instead of holding a worker. Both drive\nthe same struct, so the two paths cannot drift.\n\nWired into every managed spawn: `ScopedChild::spawn` and\n`ScopedChild::spawn_process_tree` (companion sidecars, audit helpers),\n`spawn_fallow` in the MCP tool path, and `spawn_managed_child` behind the MCP\ncode_mode tool. The last two re-run the Fallow binary itself. Each call site\nkeeps its exact error text, ProcessTree construction and cleanup behavior.\n`crates/engine/src/repo_refs.rs` deliberately keeps a plain spawn: it runs git\nfrom PATH, a binary Fallow never writes, so it cannot meet the condition.\n\nMeasured with a stress binary linked against the real crate, eight workers each\nwriting a stub and running it, 40000 spawns per row, on Linux:\n\n    Command::spawn (blocking, before)              1382 (3.455%)\n    ScopedChild::spawn_process_tree (after)           0\n    tokio::process::Command::spawn (before)         930 (2.325%)\n    spawn_tokio_retrying_busy_executable (after)      0\n\nSame wall time in both modes, so the retry costs nothing on the success path.\nTwo controls place the fault on the just-written target rather than on spawning:\nthe same loop single threaded fails 0 of 16000, and a thread that spawns a\npre-existing file under the same fork load fails 0 of 2000. On macOS an open\nwrite handle does not block exec at all, which is why this only ever failed on\nLinux CI.\n\nThe suite-level flake is not reproduced: the fallow-api lib suite is green over\n220 contended Linux runs before the change and 220 after. What is established is\nthe mechanism and the per-spawn rate, not a suite-level delta.",
          "timestamp": "2026-08-31T18:45:13+02:00",
          "tree_id": "df2f37a9849c34438fac9e1c90a43fdbf2c4d6e2",
          "url": "https://github.com/fallow-rs/fallow/commit/e368c0d05060409b9bcea702972415a85193c914"
        },
        "date": 1788194792370,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "88b1bb6193f8d66e263bbeaf2defd39b93ba7945",
          "message": "fix(audit): keep concurrent base worktree paths distinct and reclaim abandoned caches (#2500)\n\nTwo audits in one process could compose the same temporary base worktree name, because the name came from the process id plus a non-monotonic wall-clock reading, so the second git worktree add failed with 'already exists'. The name now carries a process-global monotonic counter, with the process id kept as the first segment for orphan reclamation.\n\nfallow audit-cache prune --max-age-days 0 documented that it still collects entries whose recorded owner root is gone, but a probed-dead owner fell through to the switched-off age gate. Those entries are abandoned and nothing else can reclaim them, so they now reclaim outright under a new owner-missing reason. Caches whose owner root still exists stay under their own project's policy.",
          "timestamp": "2026-08-31T19:34:37+02:00",
          "tree_id": "a2550cd69d4b4da6da571a82179d3015be943a9e",
          "url": "https://github.com/fallow-rs/fallow/commit/88b1bb6193f8d66e263bbeaf2defd39b93ba7945"
        },
        "date": 1788197961726,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "0fabc864081f9326259b2b31cf3c01e9f5357471",
          "message": "fix(mcp): bound Code Mode output and single-source its allowlist\n\nCode Mode capped the fallow JSON its host calls read but not the value the\nsnippet returns, which is the one thing that reaches the calling agent. A 3 MB\nreturn value came back whole under a 1 KB cap, bounded only by the 32 MB\nsandbox heap. An oversized result is now refused with ok:false plus truncated,\nresult_bytes, and a short result_preview, and an oversized thrown message is\nclamped the same way.\n\nThe sandbox advertised that it had no Function, but undefining the global\nbinding left the intrinsic reachable: (function(){}).constructor compiled and\nran code. The function, async, generator, and async-generator prototypes now\ncarry a non-configurable undefined constructor, and a test pins the sandbox\nglobal set so a future runtime cannot widen it unnoticed.\n\nThe Code Mode allowlist lived in five hand-kept lists that had to agree by\nhand. McpToolInfo now carries code_mode_alias and CODE_MODE_ONLY_TOOLS covers\nthe one helper with no standalone tool, the sandbox bindings project that data,\nand drift tests bind the enum to the manifest in both directions. The allowlist\nreaches fallow schema, the fallow://tools resource, and capabilities.json, so\nagents read reachability from data instead of parsing the tool description.\nTests that asserted contract facts against hardcoded prose now derive them from\nthe manifest.\n\nBacking is one value per tool rather than two overlapping predicates with an\ninline exception list, which removes four dispatch arms nothing could reach.\nAbandoned in-process calls are bounded: once one has been left running, the\nnext call takes the killable subprocess.\n\nHost calls are memoized per snippet on the tool and canonicalized params, and\nfallow.all fans out independent calls under one shared output budget. Refusals\nno longer spend the analysis budget, and the recorded tool name is clamped so a\nbogus 5 KB name cannot inflate the trace.\n\nAlso fixes a macOS process-group race in crates/process: killpg returns EPERM\nwhen the leader has become an unreaped zombie, and terminate() accepted that\nonly from a cached observation, which made the user-visible max_output_bytes\nrefusal message nondeterministic. It now asks the kernel at the moment of\nfailure.\n\nBehavior changes are recorded in docs/backwards-compatibility.md.",
          "timestamp": "2026-08-31T20:41:32+02:00",
          "tree_id": "f1a439f34afcb6fae39008ece811265d941dc35c",
          "url": "https://github.com/fallow-rs/fallow/commit/0fabc864081f9326259b2b31cf3c01e9f5357471"
        },
        "date": 1788202010280,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "702aaca8c7f81108ac85c702c31c5ca0d1b54325",
          "message": "fix(mcp): keep Code Mode refusals free of process-cleanup noise\n\nThe output-cap refusal and the deadline message are contract strings: the\nsnippet acts on them and the code_execute response envelope documents them.\nBoth were built with with_cleanup_errors, which appends best-effort\nprocess-teardown diagnostics, so on macOS a refusal intermittently read \"code\nmode host output exceeded 500 bytes; cleanup errors: failed to terminate\nsubprocess tree: Operation not permitted (os error 1)\". Terminating the\nprocess group is a cleanup concern, not part of the host call's outcome, and\nit fails whenever the leader has already become an unreaped zombie.\n\nThose two messages now keep their exact wording and the cleanup errors go to\ntracing::warn instead. Operational failures still carry their cleanup context\ninline, and structured programmatic errors still gain their cleanup_errors\nfield, so nothing is lost from a channel where it belongs. Three unit tests\npin the split.\n\nFollow-up to #2498, whose description claimed its crates/process change took\nthis from 1 failure in 15 runs to 0 in 15. That measurement was invalid: the\nload generators from the first half were still running during the second. Both\nvariants prebuilt and alternated under one constant load give 3 in 20 before\nthat change and 2 in 20 after, so it does not close the window; it remains\ncorrect on its own merit. With this change the message cannot vary by\nconstruction.",
          "timestamp": "2026-08-31T23:08:51+02:00",
          "tree_id": "94e9adcaa7cc7e4e7577a5c2c429ca1bbadbbe21",
          "url": "https://github.com/fallow-rs/fallow/commit/702aaca8c7f81108ac85c702c31c5ca0d1b54325"
        },
        "date": 1788210876691,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "8a2b5194d09dbfd012cc8388925e9060fd5cfd54",
          "message": "test: add component_cache bench shard for store load, save, and conversion (#2501)\n\nAdds CodSpeed coverage for the extraction cache store save, store load, and cached-module to module-info conversion. Benchmark only, no production code.",
          "timestamp": "2026-08-31T23:47:08+02:00",
          "tree_id": "5c9bbdf92a2d48121afb9fc543b50201d43ef463",
          "url": "https://github.com/fallow-rs/fallow/commit/8a2b5194d09dbfd012cc8388925e9060fd5cfd54"
        },
        "date": 1788213101603,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "79085fb4e009ff6768ee02eeb1139857231998b8",
          "message": "feat(brief): name files whose branching was split rather than removed (#2502)\n\n* feat(types): aggregate branching separately from unit count\n\nA per-function cyclomatic ceiling constrains a partition, not a quantity.\nMcCabe gives a function 1 plus one increment per decision point, so across a\nset of units the summed cyclomatic score is functions + branch points. Moving\nan if from one function into a new one removes an increment from the first and\nadds it to the second, so every per-unit metric improves while the branching is\nuntouched.\n\nFileBranching reports the two terms separately from the per-increment\nbreakdown that is already computed and cached for every unit. Cognitive\nexcludes PropCount and HookDensity: both are cognitive-only, and PropCount\nrecords an excess over a floor, so it is superlinear in a split and would move\nthe number with branching and nesting both flat.\n\nTests assert the set-level identity together with the absence of u16\nsaturation, since the identity alone holds vacuously once a unit saturates.\nOne test pins the known blind spot: no frame is pushed at module scope, so a\nbranch hoisted to the top level of a module lowers the count without removing\nany branching.\n\n* feat(engine): carry per-file branching totals on the health result\n\nThe audit needs branching totals from both revisions to compare them, and\nnothing carries them today: the health findings path drops every unit below\nits threshold, and it skips suppressed units before aggregation, so one\nfallow-ignore-next-line comment would remove a unit's branches from the total.\n\nbranching_by_file aggregates ModuleInfo directly, threshold-blind and\nsuppression-blind, and rides on HealthAnalysisResult. That type derives Debug\nonly and carries no schema version, so the carrier costs no wire contract.\nHealthReport is deliberately untouched.\n\nFiles whose module holds no accounted units are omitted rather than recorded as\nzero, so the payload stays proportional to the code that has units.\n\n* feat(cli): persist per-file branching totals in the audit base snapshot\n\nThe head-versus-base comparison needs both revisions' totals, so the snapshot\ncarries a branching payload keyed by root-relative path, and the cached form\nsorts it by path to keep the encoded bytes stable.\n\nRename handling needs its own line: remap_keys_for_renames rewrites path\nsegments inside opaque key strings, while this payload is keyed by a bare path.\n\nAUDIT_BASE_SNAPSHOT_CACHE_VERSION moves to 8, since a version-7 payload cannot\nanswer the comparison. The bump adds no cold pass beyond what a release already\ncauses: the cache key payload already includes cli_version.\n\nsave_cached_base_snapshot gains the size guard the loader already had. Without\nit an oversized snapshot was written once and then rejected on every read,\ncosting a cold base pass on every later run instead of just this one.\n\n* feat(output): branching conservation report for the review brief\n\nCompares branch points against the number of functions holding them, base\nversus head, over the audit accounting set.\n\nThe verdict rests on a transfer test rather than on the sign of the deltas.\nThe two sides describe different populations: the base cannot contain added\nfiles and the head does, and new JS/TS code is dominated by zero-branch\ncallbacks, so branching flat with function count up is the default state of any\ncommit that adds a file, not the signature of a split. A move is reported only\nwhen branching stayed flat inside a file whose peak fell, or when the fall in\npre-existing files approximately cancels what the added files carry.\n\nThere is no \"branching removed\" verdict. Increments outside every function are\ninvisible to the count, so a fall is equally consistent with a branch having\nbeen hoisted to module scope, and the tool must not claim what it cannot prove.\n\nScope reports the test-path share and the largest single file's share, because\ntest code and vendored bundles routinely dominate both terms. The per-file list\nis what keeps a mixed changeset readable: a set-level scalar cannot localize.\n\n* test(cli): cover the branching payload's cache and rename paths\n\nTwo gaps in the committed code. The bitcode round trip was never exercised, so\na field addition could silently fail to decode, and the sorted encoding that\nkeeps identical input byte-identical was unasserted.\n\nThe rename remap had no test at all, which is the one place a pure rename can\nread as branching arriving: without the remap the base entry keeps the old path\nand the head entry looks like a new file.\n\n* refactor(output): guard the branching subtraction against a later change\n\nThe added partition is a subset of the head partition by construction, so the\nsubtraction cannot underflow today. It is a latent trap on a line whose\ninvariant is not locally visible: a later change to how the partition is built\nwould wrap in release and panic in debug.\n\n* feat(brief): report branching conservation on the review brief\n\nThe block lands on both the brief payload and the wire envelope. Widening the\npayload alone would put it in the walkthrough digest and leave it out of\n`fallow audit --brief --format json`, because the wire struct is mapped field\nby field.\n\nBoth revisions are restricted to the changed files before comparing. The base\npass analyzes the whole base worktree, so an unrestricted comparison would\ndescribe the repository rather than the changeset.\n\nAdditive and optional, absent when no base comparison ran, so a consumer that\nnever had a base snapshot sees a byte-identical wire shape and no schema\nversion moves.\n\nThe human brief renders one line, and only when the comparison found a move.\nEvery sibling section is silent when it has nothing to say, and a flat result\nis not news.\n\n* fix(output): do not attribute a cognitive rise to removed branches\n\nRunning the real binary across a split routed through a nullish-coalescing\nchain produced branching up by four, cognitive up by one, and the label\n\"branches-removed\". Every fixture missed it because the attribution keyed on\nthe absolute size of the branching change and ignored its sign.\n\nThe field answers where a cognitive improvement came from, so it is now absent\nwhen there was no improvement, and \"branches-removed\" requires branching to\nhave actually fallen.\n\n* fix(output): pair the branching skip attributes with serde default\n\nRepository policy requires every JsonSchema Option field that skips\nserialization to also declare a default, so a consumer deserializing an\nenvelope without the field gets None instead of an error.\n\n* fix(output): stop the refactor note promising a reduction splitting cannot deliver\n\nThe complexity finding told agents to split a function \"to reduce complexity\",\nand the agent contract points them at that action. Splitting relocates\nbranching: it lowers the per-function score while the total is untouched, which\nis the behavior the branching section reports on the same envelope. The note\nnow says which of the two it moves.\n\n* fix(output): a move verdict now requires the set total to have held\n\nA transfer signature in one file said nothing about the changeset, yet it alone\ndecided the verdict. Measured: a changeset whose total branch points rose from\n15 to 205 reported branching-moved, and the human line then explained that\nsplitting relocates branching on top of numbers showing it had arrived. A\nchangeset that genuinely removed eleven branch points was captioned the same\nway.\n\nThe verdict now requires the set total to sit inside the tolerance as well, so\na relocation is reported only when the branching both held and demonstrably\nmoved. The three measured cases are tests.\n\nThe cognitive attribution no longer falls back to naming a nesting reset when\nneither branching nor nesting moved. That case reports mixed, because naming\neither cause would assert something the numbers do not show.\n\n* docs(brief): document the branching block and cover its wire shape\n\nThe block reached the envelope with nothing asserting it arrives there.\nWidening the payload struct alone would land it in the walkthrough digest and\nleave it out of the brief JSON, since the wire struct is mapped field by field,\nso the mapping now has a test.\n\nThe human line drops the peak clause when the peak did not move, rather than\nrendering \"peak per function 15 to 15\" beside a sentence about splitting.\n\nFileBranching::merge had no caller.\n\n* fix(brief): make the branching lines fit and read one direction\n\nThe human lines could not fit 80 columns at any input, reversed direction\nmid-line, and introduced a third name for a quantity the health reports already\ncall max cyclomatic. They are now split out from the printer so the wording and\nthe width are testable, and a test pins both lines under 80 columns at\nfour-digit counts.\n\nfiles_deleted also collected files that merely lost every accounted unit, so a\ncomplete removal of a file's branching was the one outcome filed under a field\nnamed for deletion. Renamed to files_only_in_base with the matching total.\n\nThe SetTooSmall doc promised a size threshold the code does not apply, and the\ncache size-guard comment claimed a benefit that does not hold: skipping the\nwrite costs a cold base pass either way, it just avoids leaving an unreadable\npayload on disk. The verdict doc now states the consolidation case both\ntransfer tests miss.\n\n* fix(output): a relocation claim now requires the rest of the changeset to be still\n\nThe previous gate required the set total to hold, but that total is a sum, so\nthree routes reached a move verdict without one. Measured: a genuine split in\none file while an unrelated file lost eight branch points and another gained\neight, which is two changes and not one relocation; a split beside a deleted\nfile carrying three hundred branch points, since base-only files enter neither\nside; and a changeset whose function count never rose at all.\n\nThe residual over the files that carried no transfer signature is now summed in\nabsolute terms, so opposing moves cannot cancel, and base-only branch points\nand a risen function count are checked too. A test pins that a split adding a\nguard or two of glue still qualifies, so the gate did not make the verdict\nunreachable.\n\nThe cognitive label branches-removed published the exact claim the verdict\nrefuses as unprovable. It is now fewer-branch-points, a statement about the\ncount rather than about the program.\n\n* refactor(output): make the branching claim per file instead of per changeset\n\nThree review rounds found eight ways to reach a wrong changeset-level verdict,\neach in the same predicate, and each fix added a conjunct that both leaked and\nover-blocked. The last version reported a relocation for a split that moved\neverything into a test file, and flipped from unchanged to moved when an empty\nconstants file was added.\n\nThe cause is structural: inferring \"a split happened here\" from aggregates over\na changeset that also contains arbitrary other work is an attribution problem,\nand aggregates cannot attribute.\n\nThe claim is now local. For each changed file present on both revisions, report\nwhether its branching held while it gained functions and its worst function\nshrank. That is true by construction, so unrelated work elsewhere cannot make\nit more or less true, and one test pins exactly that: the same split beside a\ncancelling pair, a deleted file carrying three hundred branch points, an added\nfile, and branching arriving elsewhere, all leaving the result unchanged.\n\nThe changeset totals stay in the JSON as context with no verdict attached, and\nthe human brief is silent about them.\n\nGone with the classifier: BranchingVerdict, BranchingInconclusiveReason,\nEvidence, decide, and the residual.\n\nThe human lines put the path on its own line, elided from the left, because a\ndeep path pushed the line to 138 columns.\n\n* fix(schema): drop the definitions the deleted verdict types left behind\n\nThe schema emitter merges its derived definitions with the committed file, so\na removed type keeps riding along until its entry is pruned by hand. The drift\ngate exists for exactly this and was the only check that caught it: contract\ngeneration and the doc-sync check both passed while two orphaned definitions\nsat in the published schema.\n\n* fix(brief): report both branch-point numbers and stop the header concluding\n\nThe rendered line showed the base revision's branch points and called them\nheld, so a file that went from two to none read as \"2 branch points held\". Both\nnumbers are now printed.\n\nThe header asserted that a split moves branching into new functions. The peak\nis a file-level maximum, so all three conditions are also satisfiable when the\nlargest function left the file while other functions arrived. The header now\nstates what was measured and leaves the conclusion to the reader.\n\nTest paths and files carrying synthetic template units no longer carry the\nclaim. A test file could take a slot from production code in the rendered list\nwhile its totals were already reported in scope, and template units sit outside\nevery count here, so a Vue file that gained nine template branch points read as\nhaving held six.\n\nFileBranching gained a field and rides in the bitcode snapshot cache, so\nAUDIT_BASE_SNAPSHOT_CACHE_VERSION moves to 9.\n\n* docs(brief): stop the branching text claiming more than it measures\n\nThe header said branching \"held\" while the line beneath it could print a fall\nfrom two to zero, which the tolerance permits. Two field docs still called a\nsplit a fact where the struct doc retracts that inference. And the branch-point\nmetric was documented as covering the surviving partition, which describes the\nbase side only: head includes files the changeset added and base cannot.\n\nGenerated and vendored paths are excluded from the claim as well. The rendered\nlist holds two files, so a bundle taking a slot costs a reader the production\nfile they needed. Deliberately a short unambiguous marker list rather than a\ngeneral heuristic, since a wrong exclusion drops a real finding in silence.",
          "timestamp": "2026-09-01T10:53:31+02:00",
          "tree_id": "27d628e7887b776c9c1550ea62a27006b7143017",
          "url": "https://github.com/fallow-rs/fallow/commit/79085fb4e009ff6768ee02eeb1139857231998b8"
        },
        "date": 1788253030488,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "05374a4d878f73f0b42337fe417b985d7732ced3",
          "message": "fix(cli): stop mutating the process environment from tests (#2510)\n\nCloses #2494.\n\ncrates/cli and crates/mcp tests called std::env::set_var / remove_var from inside\ntest functions. The harness runs tests as parallel threads in one process, so\nthose calls mutated state other threads were concurrently reading.\n\nThe issue filed this on soundness grounds with no claim it was breaking anything.\nIt was: crates/mcp links its unit tests and its end-to-end tests into one binary,\nso one test removed and repointed FALLOW_BIN while concurrent end-to-end tests\nread that same variable to locate the binary they spawn, and the coverage\nworkflow sets it for the whole workspace run.\n\nEvery environment read is now a thin one-line wrapper delegating to a pure inner\nfunction the test drives, following the pattern established for #2368.\nFALLOW_BOT_LOGIN needed a tri-state rather than an Option, because its three\nreaders distinguish set, set-but-not-unicode, and absent, and one reader is\nstricter than the other two. Semantics are byte-identical.\n\nA guard test walks the workspace and fails on a new mutation, matching both the\npath form and the call form so a glob import cannot slip past it.",
          "timestamp": "2026-09-01T12:03:02+02:00",
          "tree_id": "f570d80e28c88e31fd0399df4f2fad13325aa9bf",
          "url": "https://github.com/fallow-rs/fallow/commit/05374a4d878f73f0b42337fe417b985d7732ced3"
        },
        "date": 1788257534218,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "45bddc6859377636bc1e0ea0374cd318bd7f9e70",
          "message": "fix(deps): credit packages hoisted for a bundled private sibling workspace (#2512)\n\nRefs discussion #2244.\n\nA private, unpublished sibling workspace is not installed from a registry, so a\nconsumer that depends on it inlines its source and the package manager resolves\nthat sibling's own packages from the consumer's manifest. Hoisting them there is\nwhat makes the build work, but nothing in the consumer imports them, so each was\nreported as an unused dependency with `move-dependency` as the suggested action.\nFollowing that advice breaks the build, and the only escape was repo-global\n`ignoreDependencies`.\n\nFallow now walks the private-sibling closure, transitively and cycle-safe, and\ncredits the packages those siblings import. A published sibling brings its own\ndependency tree and is deliberately not followed. Only what a sibling both\nimports and declares in dependencies, optionalDependencies, or peerDependencies\ncounts; devDependencies never reach a consumer.\n\nPreviously `private` was not consulted at all: removing it produced a byte\nidentical finding. The old behavior was not conservative, it was uninformed.\n\nMeasured safe on a warm cache in both directions, and against suppression\nbaselines and regression gates, which detect increases only.",
          "timestamp": "2026-09-01T12:19:12+02:00",
          "tree_id": "3942da4a7cd4852d7abd3599ea4d4f151e981392",
          "url": "https://github.com/fallow-rs/fallow/commit/45bddc6859377636bc1e0ea0374cd318bd7f9e70"
        },
        "date": 1788258096606,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "fa40ef0946a90523ba1a7eb1366db0166b9cdd2d",
          "message": "fix(mcp): bound the Code Mode call trace\n\nMemoization gave a snippet a way to grow the response without limit. A memo\nhit spends no max_host_calls slot and no output budget, by design, but it\nstill appended an entry to calls[], and nothing bounded that array. A loop of\n5000 identical cached calls produced a 430 KB response carrying a four-byte\nresult, with every documented limit reporting as respected, which reads as\ncompliant and is worse for it. Before memoization the same loop stopped at\neight entries because the budget was derived from calls[] length, so this\narrived with the memo cache rather than preceding it.\n\ncalls[] now stops at max_recorded_calls entries, reported in limits. Past the\nbound the host calls still run and still return, so bounding the trace never\nchanges what the snippet sees; only the trace entries are dropped, and the\nadditive calls_omitted field reports how many. It is absent when nothing was\ndropped, so an ordinary response keeps the shape its consumers already parse.\nThe same loop now returns 5836 bytes with 64 entries and calls_omitted 4936,\nasserted by test rather than left to inspection.\n\ndocs/reference/mcp-internals.md said the rejection budget was what kept a\nsnippet from growing calls[] without limit. It never did, for memo hits.\nCorrected, with the trace bound documented alongside it.",
          "timestamp": "2026-09-01T12:24:58+02:00",
          "tree_id": "429de7eea2934d4379bf30827f2ef0c77f2e158f",
          "url": "https://github.com/fallow-rs/fallow/commit/fa40ef0946a90523ba1a7eb1366db0166b9cdd2d"
        },
        "date": 1788258953449,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1278,
            "unit": "count"
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
          "id": "c3c701f6783ac5798cd70fcd0aa47cdd3e812e49",
          "message": "feat(viz): analysis lenses with explicit availability states (#2515)\n\nSupersedes the #2411 draft, rebased onto current main with the contract surfaces\nclosed and the validation that branch never had.\n\nfallow viz covered four analyses; everything else fallow computes rendered as\nnothing, and an analysis that was switched off, not applicable, or unavailable\nlooked identical to one that ran and found no problems. Six primary lenses plus\nan adaptive More menu now carry an explicit availability state with a reason.\n\nThe security helpers move into crates/security rather than being duplicated:\nforce_security_rules, sarif_rule_id, fnv_hex and security_finding_id all existed\nat the merge base in the CLI, and this hoists them so the cross-crate viz lens\nshares one implementation, deleting 54 lines from the CLI.\n\nThree contract surfaces are corrected. The published skill contract still said\nviz had four lenses, in a curated cell that generate:contracts:check preserves\nverbatim, so CI passed while the text was false and the vendor job republished\nit to the companion repo. The coverage variables now name viz, including in the\nguard test that asserts the honoring surfaces. VIZ_SCHEMA_VERSION is dropped\nrather than gated, because render_html inlines the CSS, JS and payload into one\nfile so producer and consumer can never skew.\n\nAnd the gap the feature exists to close: viz passed runtime_coverage: None while\nthe Health lens could still report complete. Health now derives that state from\nthe report through a reason constant shared with the Security lens, so the two\ncannot drift.\n\nThe vendor gate is red by construction until fallow-skills carries the same\nskill text; the companion follows immediately.",
          "timestamp": "2026-09-01T13:21:53+02:00",
          "tree_id": "fb7ae0cc13676a638164f82711c6f5ee40510305",
          "url": "https://github.com/fallow-rs/fallow/commit/c3c701f6783ac5798cd70fcd0aa47cdd3e812e49"
        },
        "date": 1788262062002,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1279,
            "unit": "count"
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
          "id": "6c5bf5db3bc32c34b2e82f93e7c26099a60c21ea",
          "message": "fix(cli): keep syntactic findings when type-aware analysis cannot run (#2514)\n\nCloses #2499.\n\nThe semantic pass has a fixed two-minute ceiling, and a project large enough to\nreach it lost the entire report: every CLI surface exited 2 regardless of\ntypeAware.require, whose default is best-effort. The LSP already handled the\nidentical failure correctly, so the same condition produced a warning through\none surface and a hard error through the other.\n\nSyntactic analysis reports a superset and the semantic pass only removes\ncandidates it confirms are used, so continuing is the conservative outcome. check,\nwatch, health and both combined sites now warn and finish with the syntactic\nfindings, recording the reason in _meta.type_aware.warnings for CI consumers.\n\nfallow fix is the deliberate exception and still stops. It removes code, and the\nextra entries in the unrefined set are precisely the ones a working semantic pass\nwould have proven live, so widening a deletion is the opposite of conservative.\nIts error now names the way out.\n\nSix call sites, not the five originally scoped: run_combined_health has a second\none reached by a bare fallow --only health.\n\nThe CLI's own diagnostic was already being discarded. Under --format json it goes\nto stdout while the extension read stderr only, which is why this report and #2284\nboth say just \"code 2\". The extension now recovers it, says when results came from\na semantic pass that did not run, and exposes the ceiling as a setting.",
          "timestamp": "2026-09-01T13:33:19+02:00",
          "tree_id": "c74fe3b9cb37ed66138b09b7da2141488e2177ef",
          "url": "https://github.com/fallow-rs/fallow/commit/6c5bf5db3bc32c34b2e82f93e7c26099a60c21ea"
        },
        "date": 1788262480610,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1279,
            "unit": "count"
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
          "id": "9db28bf7c2ff6c83384baf66194a7a3f75777904",
          "message": "feat(extract): count module-scope branching as a synthetic unit (#2516)\n\nCloses #2503.\n\nNo frame was pushed at module scope, so decision points outside every function\ncontributed nothing to any complexity number fallow reports. Wider than the issue\ntext says: module-scope ??, || and ?. scored zero too, not just if ladders. The\nconsequence that motivated this is that \"this change removed branching\" was\nunprovable, because a fall was equally consistent with a branch having been\nhoisted out of a function.\n\nA synthetic per-file <module> unit, in the same family as <template>, but\naggregate-only: visible to vital signs, file scores and the branching section,\nand never producing a user-facing finding. \"Extract helper functions\" is\nmeaningless advice for module scope, and emitting findings would churn every\nsaved baseline for advice we cannot give. That single decision bounds the change.\n\nIt gets its own predicate rather than widening is_synthetic_template_unit,\nbecause FileBranching::from_units filters on that one and folding <module> in\nwould have left the branching section exactly as blind as before.\n\nValidated on five projects, old binary against new. Findings arrays are\ndeep-equal on all five; <module> appears zero times in any output format. Only\nthree vitals moved anywhere and each is decomposed: two are pure denominator\neffects with byte-identical numerators. Largest per-file deltas hand-verified\nagainst source, exact matches on both metrics. No baseline churn, warm cache\npicks the change up, and SFC files report both units without double counting.\n\nThe blast radius is much smaller than predicted: two of five projects showed zero\nvital movement and no score moved more than 0.2. Well-factored TypeScript keeps\nits boolean operators inside functions where they were already counted.",
          "timestamp": "2026-09-01T14:20:16+02:00",
          "tree_id": "e5570cb5d860f5fb6cc223f461d1ea67eaa45613",
          "url": "https://github.com/fallow-rs/fallow/commit/9db28bf7c2ff6c83384baf66194a7a3f75777904"
        },
        "date": 1788265923084,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1279,
            "unit": "count"
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
          "id": "a5b045900bc0ad7cd1f859b95d75cec27fa791a5",
          "message": "feat(api): cooperative cancellation, and a staleness gate for curated agent-doc cells\n\nAn MCP Code Mode host call that ran in this process could be answered on its\ndeadline but not stopped: the thread was abandoned and kept running a full\nanalysis inside a long-lived server. AnalysisOptions now carries an optional\ncancellation token, matching the Option<Arc<AtomicBool>> idiom the type-aware\ntransport already uses. It reaches the dead-code pipeline's stage boundaries\nand the per-file parse loop, which is the one place work stops per item rather\nthan per stage, and cancellation is always an error so a truncated module set\ncannot reach the graph.\n\nThe stop is cooperative and has no upper bound: duplication detection and the\ndead-code detectors hold no check once entered, so analyze, find_dupes,\ncheck_health and audit keep the killable subprocess, and every description\nthat touches this says which stops are promised and which are not. Threading a\ntoken into those loops was deliberately not done, because the suffix-array\nstage after the tokenize loop holds no check either, so the stop would have\nstayed unbounded while the change touched eight public entry points.\n\nMeasured on a 520-file project: every in-process route returns FALLOW_CANCELLED\nin a fraction of its uncancelled time, where three of them previously returned\na completed analysis. The engine test asserts work performed, a strictly\nsmaller module count, rather than how long the caller took to return.\n\nSeparately, the agent-doc generator prefers hand-written prose over its\ngenerated seed and preserves it forever, which is right except when the\nmanifest text a cell was written from moves later: the published cell then\ndescribes a surface that has changed and nothing says so. A record beside the\ngenerator holds the seed each of the 400 curated cells was last accepted\nagainst, outside the vendored skill tree so it never touches the public skills\nsurface. generate:contracts:check fails naming the cell and both seeds;\ngenerate:contracts re-records.",
          "timestamp": "2026-09-01T15:34:29+02:00",
          "tree_id": "65e54c61456f9199c92fc54a58bf7f4cb43a6fe6",
          "url": "https://github.com/fallow-rs/fallow/commit/a5b045900bc0ad7cd1f859b95d75cec27fa791a5"
        },
        "date": 1788272876124,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1279,
            "unit": "count"
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
          "id": "0beb9fb4ae56c799fca0088b7e167430d815254d",
          "message": "fix(cli): complete the knip and jscpd migration tables (#2523)\n\nCloses #2507.\n\nA systematic diff against knip 6.34.0 and @jscpd/core 4.2.5 found 128 knip gaps\nand 15 jscpd gaps. The serious category is not the missing entries: 16 keys were\nclaimed as auto-detected by fallow when no fallow plugin covers them, so\nmigration was telling users their tooling was handled when it was not. Those now\nreport honestly as unsupported.\n\nCoverage is exact. The covered and unsupported knip tables together are the 184\nreal keys, taken as the union of knip's Plugins map and its published schema.\njscpd is 6 mapped plus 31 reported across IOptions plus colors. Plausible but\nunreal mappings were rejected rather than padding the covered list: metro is\nonly reachable through the react-native plugin, i18next is the library rather\nthan the CLI, and parsing a file type is not plugin coverage.\n\nThe tables are now guarded rather than merely correct. A first pass claimed the\ntests pinned each table against the upstream key set; they asserted sorting and\neight hardcoded names and consulted nothing, so renaming a plugin would have\nleft every test green while migration promised auto-detection that would not\nhappen. Every covered key now resolves through a named alias table to the\nbuilt-in plugin roster, with the reverse check and a no-stale-alias check.\n\nUnknown keys no longer vanish either. Both migrators were allowlist-only with no\nelse branch, so a real knip root key produced zero warnings. They now report what\nneither table names, following the existing unmapped-rule-key wording.\n\nAlso maps knip's cycles rule to circular-dependencies, which fallow has and the\ntable missed.\n\nFallow still has no Marko plugin; only the table side is fixed here.\n\nThanks @VariableVince for the report, and for the hunch that more than Marko was\nmissing.",
          "timestamp": "2026-09-01T16:43:48+02:00",
          "tree_id": "4e92ea67e3ab4cccf52afdc3108411b5b2bcc67c",
          "url": "https://github.com/fallow-rs/fallow/commit/0beb9fb4ae56c799fca0088b7e167430d815254d"
        },
        "date": 1788274635013,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1279,
            "unit": "count"
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
          "id": "d4b0e6ab9a37ee22d70b9d7986288e6f2a821e63",
          "message": "docs(config): stop the rule-name guard claiming a check it does not make (#2536)\n\nKNOWN_RULE_NAMES drives the typo detector for user configs. Its doc comment said\nthe known_rule_names_count_matches_struct test fails when the lists drift. That\ntest asserts a length literal and never mentions RulesConfig, so adding a rule to\nthe struct and forgetting the list leaves every test green while the new rule\nname warns as an unknown key in real configs.\n\nNothing is drifted today; the only absent field is serde(skip) bookkeeping and\ncorrectly excluded.\n\nA real pin needs the provenance of all 98 entries, because the list also covers\nnames no RulesConfig field produces, so a field-count comparison would not even\nbe correct. That is deliberately not attempted here. The false assurance is\nseparable and is what misleads: the test is renamed to what it does, and both it\nand the constant now state what is and is not enforced. The guard is unchanged.\n\nThe four sibling guards whose names promise a comparison were checked and all\ngenuinely compare sources, so this was an isolated case.",
          "timestamp": "2026-09-01T17:35:32+02:00",
          "tree_id": "8b52d198cb772c19208fd0d246a3f1b57319114b",
          "url": "https://github.com/fallow-rs/fallow/commit/d4b0e6ab9a37ee22d70b9d7986288e6f2a821e63"
        },
        "date": 1788277323997,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1279,
            "unit": "count"
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
          "id": "a212b0f01d061898405c11dc5b908f54b9055b6a",
          "message": "fix(viz): gate every path-redaction site on rooted rather than absolute (#2538)\n\nFollow-up to #2537, which corrected one of four sites.\n\nThree siblings in the same file made the same is_absolute assumption. The JSON\nlayer left a rooted path without a drive letter untouched in the payload, and\ntwo join-onto-root decisions joined such a path onto the project root, so an\nexternal path rendered as project-relative rather than being redacted at all.\n\nAll four now gate on has_root. A value outside a path key, such as a route\nspecifier, is excluded by the key gate rather than by the absoluteness test, so\nbroadening the predicate does not widen what gets rewritten.\n\ncrates/engine/src/viz.rs now contains no is_absolute.",
          "timestamp": "2026-09-01T19:02:43+02:00",
          "tree_id": "f6d71789df67da0f04bd6bd7ab8cdca15f8123df",
          "url": "https://github.com/fallow-rs/fallow/commit/a212b0f01d061898405c11dc5b908f54b9055b6a"
        },
        "date": 1788282497953,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1279,
            "unit": "count"
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
          "id": "79a0e8d8dbcaa408dce348b3df14fc1e824f988b",
          "message": "chore: release v3.22.0",
          "timestamp": "2026-09-01T20:01:03+02:00",
          "tree_id": "8df4a6416fa304cf0788325200b367301015aa11",
          "url": "https://github.com/fallow-rs/fallow/commit/79a0e8d8dbcaa408dce348b3df14fc1e824f988b"
        },
        "date": 1788285760819,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1279,
            "unit": "count"
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
          "id": "c7163e9b5475baf95d2b607c531e4e390fda99a2",
          "message": "chore(napi): sync package.json / package-lock / index.js to v3.22.0",
          "timestamp": "2026-09-01T21:51:22+02:00",
          "tree_id": "a84f4e04a32804a83983a2fd13d349a2846d9e3c",
          "url": "https://github.com/fallow-rs/fallow/commit/c7163e9b5475baf95d2b607c531e4e390fda99a2"
        },
        "date": 1788292624355,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1279,
            "unit": "count"
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
          "id": "83b7d71bf8273c41809074b335246293fc6f259b",
          "message": "test(config): prove every listed rule name is reachable (#2541)\n\nThe forward direction was already guarded: known_rule_names_covers_every_struct_field\nasserts every serialized RulesConfig field appears in KNOWN_RULE_NAMES, and\nRulesConfig has no skip_serializing_if, so no field can hide from it.\n\nThe reverse was not guarded, so a removed or renamed rule left in the list stayed\nthere silently. That is not inert: closest_known_rule_name draws its suggestions\nfrom this list, so a stale entry gets offered to a user as the fix for their typo,\npointing at a rule that no longer exists.\n\nProvenance, previously unwritten: the 98 entries are the 53 canonical kebab-case\nnames (54 fields minus one serde(skip)) unioned with the 53 declared aliases.",
          "timestamp": "2026-09-02T10:11:01+02:00",
          "tree_id": "39eb4ea1275aa1762a0719ec4e0f08eecd86e583",
          "url": "https://github.com/fallow-rs/fallow/commit/83b7d71bf8273c41809074b335246293fc6f259b"
        },
        "date": 1788337010049,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1279,
            "unit": "count"
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
          "id": "0a827f8e8302f4d2187fcc5f75f1ca61e8518cf5",
          "message": "fix(brief): stop a hoist to module scope reading as an in-place split (#2539)\n\nThe split signature counts units as functions. Counting module-scope branching\nas a synthetic unit made a branch hoisted out of a function into that unit look\nlike a split: the branching holds, the count rises by one, and the worst\nfunction shrinks. Reproduced against a real binary on a two-commit repository,\nwhere the brief named the file and called it the shape a split leaves.\n\nBoth changes are right on their own; the interaction is not. The module unit\nbelongs in the branch-point total, because those decision points are real and\nrun at import time, and it must stay in the unit count so the conservation\nidentity holds. What it must not do is count towards the tax a split adds,\nsince nobody split a function into it.\n\nFileBranching therefore records that a module unit is present and the signature\njudges on authored functions. Fixing this in the counts instead would break the\nidentity, which the conservation test caught.\n\nTests pin both directions: a hoist is not a split, and a real split in a file\nthat also has module-scope branching still is one.",
          "timestamp": "2026-09-02T10:46:52+02:00",
          "tree_id": "f9d52a4b306bfae9de84fba3636e3c6f38904a80",
          "url": "https://github.com/fallow-rs/fallow/commit/0a827f8e8302f4d2187fcc5f75f1ca61e8518cf5"
        },
        "date": 1788338886727,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1279,
            "unit": "count"
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
          "id": "08aac92679c9d3729d10e0068296f3e020c31357",
          "message": "feat(lsp): add Zed diagnostic muting\n\n* chore: initialize Zed parity work\n\n* feat(lsp): add Zed diagnostic muting",
          "timestamp": "2026-09-02T15:47:08+02:00",
          "tree_id": "d86915316d49d0c4b7a0944db095d87e3df8c825",
          "url": "https://github.com/fallow-rs/fallow/commit/08aac92679c9d3729d10e0068296f3e020c31357"
        },
        "date": 1788357168874,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1279,
            "unit": "count"
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
          "id": "320786e34e688499abb39a7655623f1e5191c2cc",
          "message": "chore(agents): clean up the agent-facing instruction surface after a prompt audit (#2544)\n\nCleanup of the agent-facing instruction surface after a prompt audit of the skill tree, reviewer definitions, and MCP descriptions.\n\n- Skill tree: rotted numbers removed (gate version floor, clippy thresholds, struct sizes), `--changed-since` described as a scope filter, the six-step Instructions block replaced by a client-neutral arguments line, phase labels and migration-relative wording dropped from the reviewers, the two orphaned incident logs deleted with their one live mechanic moved into `open-draft-pr`.\n- MCP: every tool parameter now carries a description taken from the CLI help, `guard` states its contract (empty rule set for unzoned paths, config-only, no analysis), and the server instructions route to `tools/list` and the resources instead of re-listing all tools. The two tests that asserted the old enumeration now assert routing and resource coverage.\n- Reviewer roster: the GitHub and GitLab reviewers are one `ci-integration-reviewer` that decides the provider from the touched paths.\n- `.claude/agents/*.md` are generated from `.agents/agents/*.md` by the adapter script, with drift check and tests; the diverged copies were reconciled with the richer version winning per file.\n- `.agents/skills/fallow` is a generator target next to the npm skill, wired into the contract surfaces and both CI path filters. The `gotchas`, `patterns`, and `cli-reference` references under `.agents` were left as-is and still lag the npm copies; that is a follow-up.\n\nVerification: `node --test scripts/*.test.mjs`, `cargo test -p fallow-mcp`, clippy, `lint:js`, `fmt:js:check`, `generate:contracts:check`, `check:agent-adapters` all pass; both drift checks fail on a deliberate one-character drift.",
          "timestamp": "2026-09-02T15:52:44+02:00",
          "tree_id": "05580f95f2c0194fc16bd0d2f5cf008903ad9dfa",
          "url": "https://github.com/fallow-rs/fallow/commit/320786e34e688499abb39a7655623f1e5191c2cc"
        },
        "date": 1788357256660,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 51,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 469,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1279,
            "unit": "count"
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
          "id": "541e048480866080382f3401a899f3338b947f92",
          "message": "feat: add project readiness doctor and Oxlint JS plugin compatibility\n\n* chore: start Ultracite doctor work\n\n* feat: add doctor and Ultracite Oxlint compatibility\n\n* fix: harden doctor readiness checks",
          "timestamp": "2026-09-03T13:33:53+02:00",
          "tree_id": "6e17fc4af40d2427dd581c18248718aefb76e33f",
          "url": "https://github.com/fallow-rs/fallow/commit/541e048480866080382f3401a899f3338b947f92"
        },
        "date": 1788435591158,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 470,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1281,
            "unit": "count"
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
          "id": "c9ecdc7c0897b746d560bc14714ef50b04c0cb91",
          "message": "fix: recognize class members through object properties\n\n* chore: start issue 2546 implementation\n\n* fix: recognize class members through object properties\n\n* fix: keep object binding extraction MSRV compatible",
          "timestamp": "2026-09-04T01:37:53+02:00",
          "tree_id": "42abe1502f830b90b9904abe0ebe7a5c83a15f83",
          "url": "https://github.com/fallow-rs/fallow/commit/c9ecdc7c0897b746d560bc14714ef50b04c0cb91"
        },
        "date": 1788479011395,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 470,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1282,
            "unit": "count"
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
          "id": "3f062cf9f5b08c2747aff0bd73f4a3c458e0bb98",
          "message": "fix: follow class members through object containers\n\n* chore: start object property member follow-ups\n\n* fix: follow class members through object aliases",
          "timestamp": "2026-09-04T10:20:10+02:00",
          "tree_id": "e6107b0379260ebc141ab8acc71bfba4bdaee5f5",
          "url": "https://github.com/fallow-rs/fallow/commit/3f062cf9f5b08c2747aff0bd73f4a3c458e0bb98"
        },
        "date": 1788510338338,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 470,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1282,
            "unit": "count"
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
          "id": "b226fe78b40ef0e93cc1708f9b1fb989cd06fe37",
          "message": "fix(scripts): credit binaries invoked through varlock run",
          "timestamp": "2026-09-05T20:50:52+02:00",
          "tree_id": "46c86e106d73e7a5a28dab10ecc8d2a8ab3948b6",
          "url": "https://github.com/fallow-rs/fallow/commit/b226fe78b40ef0e93cc1708f9b1fb989cd06fe37"
        },
        "date": 1788634544215,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 470,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1282,
            "unit": "count"
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
          "id": "802c7fe381daadb6190f46c9c856e96ba8dbdb27",
          "message": "fix: preserve scoped package names in review brief\n\nFixes #2553.",
          "timestamp": "2026-09-05T21:26:10+02:00",
          "tree_id": "c114fbeaa525687e473f2746c407184d2bf9db73",
          "url": "https://github.com/fallow-rs/fallow/commit/802c7fe381daadb6190f46c9c856e96ba8dbdb27"
        },
        "date": 1788636668638,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 470,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1282,
            "unit": "count"
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
          "id": "02381b837dfa93df5c845b293359cb99d2a19c72",
          "message": "fix: honor pnpm catalog next-line suppressions\n\nFixes #2548.",
          "timestamp": "2026-09-05T21:45:59+02:00",
          "tree_id": "b5836aff546faa0300fe76ee240f77a1face7047",
          "url": "https://github.com/fallow-rs/fallow/commit/02381b837dfa93df5c845b293359cb99d2a19c72"
        },
        "date": 1788637622388,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 470,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1282,
            "unit": "count"
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
          "id": "f6dbe8cda5f898b7fe6913ede4748406a37a868d",
          "message": "fix(ci): show clone evidence in inline reviews\n\nShow stable clone handles and repository-relative peer ranges in inline reviews.",
          "timestamp": "2026-09-05T22:04:12+02:00",
          "tree_id": "1c4979e1f75c77530e4d790d8db0ea8b1b86e89e",
          "url": "https://github.com/fallow-rs/fallow/commit/f6dbe8cda5f898b7fe6913ede4748406a37a868d"
        },
        "date": 1788638988849,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.28,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 470,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1282,
            "unit": "count"
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
          "id": "23bb9a7eceb6467336422db710ee0f5d92258c30",
          "message": "fix: preserve quoted content in catalog and script parsing\n\nPreserve quoted YAML scalar content and shell argument boundaries so suppression directives and wrapper commands are interpreted accurately.",
          "timestamp": "2026-09-05T23:40:33+02:00",
          "tree_id": "c3f57391b0aba6f41a239994898383c9f040c373",
          "url": "https://github.com/fallow-rs/fallow/commit/23bb9a7eceb6467336422db710ee0f5d92258c30"
        },
        "date": 1788644504120,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1284,
            "unit": "count"
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
          "id": "7e225b5d4061bbb16b5dcb4dad46c37dd9a95ca7",
          "message": "refactor: remove redundant helpers and strengthen behavioral tests\n\nRemove redundant private helpers, duplicated tests and unused app scaffolding. Strengthen process cleanup and artifact-integrity tests against actual production paths. Full repository verification, public-project comparisons and pull-request CI pass.",
          "timestamp": "2026-09-06T08:40:21+02:00",
          "tree_id": "a1b5ffb5723ff7d7c87380982c6f4450a113bdf5",
          "url": "https://github.com/fallow-rs/fallow/commit/7e225b5d4061bbb16b5dcb4dad46c37dd9a95ca7"
        },
        "date": 1788677164119,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1284,
            "unit": "count"
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
          "id": "d1b5d427aea98c17c5fe94e4753f0d3af864614a",
          "message": "refactor: remove redundant work and test runtime contracts\n\n## What\n\nRemove unused test implementations and forwarding code, reuse owned report data, and add a reusable slop-audit workflow with checked documentation routes.\n\nGit failures now reach the review app's error state instead of appearing as empty diffs. The signed setup launcher preserves termination status, and the sidecar cache always performs its required integrity check.\n\n## Why\n\nSome tests exercised a separate test-only implementation or failed before reaching the behavior they claimed to cover. They now exercise production parsing, graph queries, initialization, cleanup, command registration, and serialized reports. Report rendering also avoids unnecessary deep copies while retaining output and error compatibility.\n\n## Test plan\n\n- Canonical `npm run verify:full`, including workspace, wrapper, conformance, documentation, benchmark compilation, and native Node checks.\n- Affected editor, Electron, sidecar, and GitLab suites, including actual extension-host and review-app end-to-end behavior.\n- Fault injection confirms the old assertions accepted broken production behavior and the replacement assertions reject it.\n- Public Fastify and SvelteKit output parity, real MCP/LSP stdio, and the original scoped-package, varlock, and catalog-suppression regression matrices.\n- Matched allocation probes with identical dependencies and inputs; health JSON remains identical.",
          "timestamp": "2026-09-06T10:28:31+02:00",
          "tree_id": "e9659c188329beb6863c6de291a24c1bec9829da",
          "url": "https://github.com/fallow-rs/fallow/commit/d1b5d427aea98c17c5fe94e4753f0d3af864614a"
        },
        "date": 1788683645149,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1284,
            "unit": "count"
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
          "id": "1bc3eca3cd714ca2cef9f34ab7ae81835c7988fb",
          "message": "fix: preserve Unicode framing and checkout-independent reports\n\nMoving an unchanged checkout could reorder tied health findings and assign a duplicate collision handle to a different group. Health now resolves metric ties by source location, and duplicate collision ordinals use canonical locations instead of absolute-path digests.\n\nCorrected full-hash collisions use `dup:<16hex>-rN`. Ordinary handles are unchanged. Legacy numeric collision keys remain valid input but cannot alias another group: affected suppressions and normalized baseline findings resurface for review. The config schema and migration documentation describe refreshing those keys and upgrading shared-config consumers.\n\nThe VS Code integration fixture now frames LSP messages by bytes. Its subprocess regressions and actual extension-host suite exercise Unicode workspace paths and navigation.\n\nValidation: failing-before/passing-after regressions; relocated pinned Fastify and SvelteKit reports across cache modes and parser threads; actual trace, suppression, baseline and saved-report format checks; original issue regressions (#2553, #2551 and #2548); real Fastify CLI/LSP editor-host smoke; full editor suite; `npm run verify:fast` and `npm run verify:full`. All passed.\n\nPublic documentation: https://github.com/fallow-rs/docs/pull/23.",
          "timestamp": "2026-09-06T13:04:22+02:00",
          "tree_id": "0e8fe7fa32b01c3afe389586aa6fbc926fab330d",
          "url": "https://github.com/fallow-rs/fallow/commit/1bc3eca3cd714ca2cef9f34ab7ae81835c7988fb"
        },
        "date": 1788692932023,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1284,
            "unit": "count"
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
          "id": "c55ba1ba3c98c6e4ad48f0aed0e0f256c256d959",
          "message": "chore: release v3.23.0",
          "timestamp": "2026-09-07T11:55:06+02:00",
          "tree_id": "1f269de77c6b7cddd4303d410efd02c25d511185",
          "url": "https://github.com/fallow-rs/fallow/commit/c55ba1ba3c98c6e4ad48f0aed0e0f256c256d959"
        },
        "date": 1788775581746,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1284,
            "unit": "count"
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
          "id": "cdc5c0ee05031c77cb420b635de403164733d958",
          "message": "chore(docker): pin FALLOW_VERSION 3.23.0 with refreshed checksums",
          "timestamp": "2026-09-07T14:06:24+02:00",
          "tree_id": "0cdd881d3c9e5ed647639962932bae7ed312e739",
          "url": "https://github.com/fallow-rs/fallow/commit/cdc5c0ee05031c77cb420b635de403164733d958"
        },
        "date": 1788783173582,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1284,
            "unit": "count"
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
          "id": "a6afda2f0136f645cea233b546a150ac79011115",
          "message": "feat(review): replace the duplicated blast-radius list with a count and a rollup (#2563)\n\nThe review-brief envelope carried the impact closure's affected-but-not-in-diff paths twice, in full: graph_facts.reachable_from was a verbatim clone of impact_closure.affected_not_shown, and neither was capped. On a one-file change to colinhacks/zod the two lists were 28,372 of 52,676 bytes while the focus map and decision surface were 1,276.\n\ngraph_facts.reachable_from is removed; it had no reader. impact_closure now reports affected_count (exact, computed before capping), a ten-path sorted-prefix sample, and affected_by_dir: {dir, count} rows heaviest first, capped at 25 with affected_by_dir_omitted counting the rest. A prefix sample alone would mislead: on a 20-file zod diff a 25-path prefix covers one of 24 directories while the weight sits in two others.\n\nBoth human renderers read affected_count, so their totals are unchanged. Decisions, ranks, verdicts and exit codes are untouched; the decision surface takes its blast metric from the uncapped engine closure. Brief schema_version moves to 9.\n\nEnvelope on the same reproduction: 52,676 -> 25,818 bytes.",
          "timestamp": "2026-09-07T15:01:24+02:00",
          "tree_id": "9caf7344e24e187a754859e296c4a9f8888f158e",
          "url": "https://github.com/fallow-rs/fallow/commit/a6afda2f0136f645cea233b546a150ac79011115"
        },
        "date": 1788786431307,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1284,
            "unit": "count"
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
          "id": "4503f09c0f324f4c55e2cff6c581f7fd92c36d6f",
          "message": "fix(guard): state the coverage requirement for unzoned files\n\nAn unzoned file under boundaries.coverage.requireAllFiles was told it was\nunrestricted while an analysis run would report it as a boundary-coverage\nviolation. guard_notes never read coverage_required, which the JSON already\ncarried, so the human output contradicted the JSON for the same file.\n\nThe unrestricted note now names what it actually covers, import and call\nchecks, and a second note states the coverage requirement when it applies.\nPaths exempted by allowUnmatched keep the old single note.",
          "timestamp": "2026-09-07T15:19:35+02:00",
          "tree_id": "10fefeb404697cc57b5fa643833ee776d735b09b",
          "url": "https://github.com/fallow-rs/fallow/commit/4503f09c0f324f4c55e2cff6c581f7fd92c36d6f"
        },
        "date": 1788787664445,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1284,
            "unit": "count"
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
          "id": "17c82aa7c56b2e946b6d0905355e532d4c7b5d77",
          "message": "fix(cli): isolate reconcile failures per fingerprint so every stale thread resolves (#2562)\n\nA staged provider lifecycle is a flat list across every stale fingerprint, and both apply loops returned on the first error, so one failed mutation discarded every remaining operation, including the resolution replies of unrelated fingerprints. A failure now blocks only the remaining operations of the same fingerprint, on GitHub and on GitLab alike, and a failed thread resolve still blocks that fingerprint's own marker reply so a later run can tell a failed resolve from a reopened lifecycle.\n\npost-review now reports failed_fingerprints and unapplied_fingerprints, both omitted when empty, so a dropped resolution is visible instead of silent, and both review.sh wrappers name the unapplied fingerprints in their warning. The GitHub wrapper's warning gate was dead: jq binds | looser than or, so the unparenthesised condition always raised 'boolean has no length' and the error was swallowed by the redirect. It now matches the GitLab form.\n\nNo review-mutating endpoint is added: the only PATCH targets the sticky issue comment. The content-free 'reviewed' row that follows a resolution reply is GitHub's own wrapper around a standalone review-comment reply, documented in the code and in cli-internals. Retry policy and the all-or-nothing preflight gate are deliberately unchanged.",
          "timestamp": "2026-09-07T15:51:35+02:00",
          "tree_id": "c274ceeb7e4bbe5dc5e81b32c8bdc91f6f43d649",
          "url": "https://github.com/fallow-rs/fallow/commit/17c82aa7c56b2e946b6d0905355e532d4c7b5d77"
        },
        "date": 1788789450824,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1284,
            "unit": "count"
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
          "id": "dc8656e74ff5e58df08ccd03bc8aabac4966c594",
          "message": "test: cover guard multi-file batches and candidate loader errors\n\nTwo gaps salvaged from unlanded optimization branches, without the\noptimizations themselves.\n\nEvery existing guard test passes exactly one file, so nothing caught state\nleaking across files in a batch. The new test asserts a four-file report\nserializes identically to the four single-file reports concatenated, and pins\nthe per-file rule-id ordering, including that an unparsable files glob applies\nto every file.\n\nload_candidate_map had no coverage of its three rejection paths. The verdict\nside was tested, the candidate side was not, so a missing security_findings\narray, a malformed finding, and a duplicate finding_id were all unguarded.",
          "timestamp": "2026-09-07T17:03:32+02:00",
          "tree_id": "63b88877e3eddebf22d21f468e060de7c6727e75",
          "url": "https://github.com/fallow-rs/fallow/commit/dc8656e74ff5e58df08ccd03bc8aabac4966c594"
        },
        "date": 1788793626309,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1284,
            "unit": "count"
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
          "id": "9e690609e1f918887937d3ff5d890adba4f2feba",
          "message": "fix(cli): bound the coordination-gap lines on the human review brief (#2565)\n\nThe brief printed one line per gap, joining every consumed symbol and both full paths. On a zod change to a module a barrel re-exports, one line rendered at 955 columns; a project with thirty out-of-diff consumers produced sixty lines at ~96 columns. They sat directly under an impact-closure summary that holds to eighty.\n\nThe section now states how many consumers sit outside the diff, walks the three that take the most symbols (the consumer on its own line, the contract it consumes on the next, as a branching split renders), and closes with the remainder and where to read it. Paths shorten from the left through elide_path; the symbol list fills a budget and cuts with a +N more suffix, shortened from the right because a symbol's leading characters identify it.\n\nOrdering by symbols taken rather than by path matters: the JSON gap list is path-sorted with no ranking, so an alphabetical prefix collapsed the 26-symbol barrel consumer behind the remainder. The header says 'use exports of changed files' because collect_coordination_gaps never verifies the export itself changed.\n\nRendering only. impact_closure.coordination_gap still carries every gap with every symbol, so the JSON contract and schema_version are untouched, and a new test pins that invariant next to the sibling fields that are capped.\n\nzod: 955 columns -> 77. Thirty consumers: 60 lines -> 9, none over 78.",
          "timestamp": "2026-09-07T17:26:51+02:00",
          "tree_id": "4a4fb606e3ddc28d76caf0267d08b4b45f27b3f6",
          "url": "https://github.com/fallow-rs/fallow/commit/9e690609e1f918887937d3ff5d890adba4f2feba"
        },
        "date": 1788794967630,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1284,
            "unit": "count"
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
          "id": "ca508ed0aaca90e0ac3d23bb61d8f60f943ed935",
          "message": "refactor: replace copied helpers with one definition each\n\nFour report surfaces in the graph crate each carried a byte-identical\nrelativize, and their doc comments named the other copies as the thing to\nkeep in sync. A cross-platform key invariant shared by four outputs was\nenforced by prose. One pub(super) helper now owns it.\n\nranges_to_gaps and push_region were copied whole from sfc into astro, which\nalready imports SourceRegion from sfc. The sorted-input precondition the\nfunction depends on was implied only by the sort call sitting above it, so it\nis now written down.\n\nline_range_from_byte_col was defined twice, and hover wrapped\nutf16_col_span in a forwarder that added nothing. Both move to position.rs,\nwhich already owns the byte-column to UTF-16 boundary.",
          "timestamp": "2026-09-07T18:46:01+02:00",
          "tree_id": "4e1407a84e73c484101ba6e0d89ec4709332933c",
          "url": "https://github.com/fallow-rs/fallow/commit/ca508ed0aaca90e0ac3d23bb61d8f60f943ed935"
        },
        "date": 1788799870869,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1284,
            "unit": "count"
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
          "id": "f63c4dbff717c1e9037d3290174d425d282340e7",
          "message": "fix(cli): wrap the decision-surface and focus lines to eighty columns (#2566)\n\nThe two sections the brief leads with were the two that wrapped unpredictably in a terminal. The decision surface printed its question, trade-off and expert list on single unbounded lines: on zod, a question naming every widened export ran to 159 columns. The focus map put an un-elided path and an unbounded reason on one row, reaching 103.\n\nBoth now wrap under a hanging indent. Prose is re-flowed rather than cut: a decision question ends in the actual ask, so truncating it would drop the question. Only a single word that alone overruns its line is shortened, from whichever end identifies it: a path keeps its tail, an owner identity keeps its head. The continuation indents give the block a 2 / 5 / 7 hierarchy so column 5 stays the key column.\n\nTwo defects found in review and fixed: the trailing blank line after a populated decision block was dropped, so the apex ran straight into the drill-down header; and the ask line double-counted its bus-factor reservation and elided owner identities from the wrong end, rendering a 52-character email as '.../ame.lastname@engineering.example.com' with fifteen columns free.\n\nRendering only; no field, format or schema moves. Zero lines over eighty across three fixtures, in both focus branches.",
          "timestamp": "2026-09-07T19:17:01+02:00",
          "tree_id": "9821252e4f154e68aaaf5b19df1d0b7dfdd9b64d",
          "url": "https://github.com/fallow-rs/fallow/commit/f63c4dbff717c1e9037d3290174d425d282340e7"
        },
        "date": 1788801498925,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1284,
            "unit": "count"
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
          "id": "cd9b2e46a8a4f19ceab9c5d6e060abe133c16dcd",
          "message": "refactor: remove test-only shadows and copied catalogues\n\nAn audit across eight domains, each challenged by an independent reviewer,\nfound the same shape repeatedly: logic copied into a second place, then\nasserted against itself.\n\nSeveral test modules re-implemented the production function they claimed to\ncover, so the tests passed against their own copy while the real code was\nnever exercised. report/ci/severity.rs was an entirely cfg(test) shadow of\nmappings owned by fallow-output and fallow-config; sarif.rs, codeclimate.rs\nand serde_path.rs each kept a second copy of a function and asserted it\nagainst that copy. Each removal names the executed assertion elsewhere that\nstill covers the contract.\n\nThe React runtime dependency gate existed in six detectors under four names,\nthe security binding-trace catalogue in two, and the jsonc dialect catalogue\nin two crates. Forwarding wrappers that added nothing to their callee are\ngone, as are the three is_config_fixable tests left behind when the function\nmoved to fallow-config.\n\nBehavior is unchanged throughout. The whole workspace suite, the JS suites,\nthe contract-drift check and the agent-adapter check all pass.",
          "timestamp": "2026-09-07T20:08:45+02:00",
          "tree_id": "9e1227668769b378099f057f2fe83efd032c26f3",
          "url": "https://github.com/fallow-rs/fallow/commit/cd9b2e46a8a4f19ceab9c5d6e060abe133c16dcd"
        },
        "date": 1788805637831,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 52,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.27,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 472,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1291,
            "unit": "count"
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
          "id": "0cc81b8761da239b721b3d70c3a26cc4db48d412",
          "message": "fix: report what fallow does not know (#2568)\n\nfix: report what fallow does not know",
          "timestamp": "2026-09-08T13:02:05+02:00",
          "tree_id": "ca9780cfd6bea5be4a4375ad44e5957fae7a1082",
          "url": "https://github.com/fallow-rs/fallow/commit/0cc81b8761da239b721b3d70c3a26cc4db48d412"
        },
        "date": 1788865670242,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "16667610c8543f862d6b0900fd3ed762bfca027a",
          "message": "fix: recognize destructured class-member usage (#2567)\n\nfix(extract): preserve class-member usage through destructuring",
          "timestamp": "2026-09-08T13:28:25+02:00",
          "tree_id": "764c005b828dc7eec58483ce9c82c93f7a178b17",
          "url": "https://github.com/fallow-rs/fallow/commit/16667610c8543f862d6b0900fd3ed762bfca027a"
        },
        "date": 1788867239817,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "c178aafaa0545cf499c9b9cc9a2f32cdd0b33843",
          "message": "fix: expose cyclomatic metric populations (#2569)\n\nfix: explain cyclomatic complexity populations",
          "timestamp": "2026-09-08T13:47:23+02:00",
          "tree_id": "981d36c6070bf96456e3867ca8577b10993f4ea6",
          "url": "https://github.com/fallow-rs/fallow/commit/c178aafaa0545cf499c9b9cc9a2f32cdd0b33843"
        },
        "date": 1788868122618,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "1c8b422fa93c14b8d6a047f5749d2c54a5717061",
          "message": "fix: explain cyclomatic populations in compact health output (#2589)\n\nfix: disclose populations in compact health output",
          "timestamp": "2026-09-08T17:15:19+02:00",
          "tree_id": "9b51140d2a25bc08a0f388cd34c2adff588d8989",
          "url": "https://github.com/fallow-rs/fallow/commit/1c8b422fa93c14b8d6a047f5749d2c54a5717061"
        },
        "date": 1788881145455,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "147004e79ecb8cbe0b8059036021c09596942fae",
          "message": "Merge pull request #2590 from fallow-rs/feat/dependabot-batch\n\nchore: batch Dependabot updates",
          "timestamp": "2026-09-08T18:13:16+02:00",
          "tree_id": "8548dc61538a4e6f585ecbbc514d4ac74197ed5f",
          "url": "https://github.com/fallow-rs/fallow/commit/147004e79ecb8cbe0b8059036021c09596942fae"
        },
        "date": 1788884292085,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "e5e92c787465f81e6fcd8f84a33be5b7f6ee93ac",
          "message": "fix: make the caveat reach every surface, and the gates that guard it fail\n\n* fix: disclose incomplete evidence on unused store members\n\nEight of the nine dead-code finding types tell a reader when their verdict\nrests on a file the run never fully read. Store members were the ninth, left\nout on the argument that they expose no mutation on any surface.\n\nThat argument is correct and still holds: a store member has no `Fix` action,\nno LSP code action, and `store_members_never_offer_unverified_line_deletions`\npins the absent review suggestion. There is nothing here to withhold.\n\nBut the caveat is a disclosure before it is a gate. A reader deciding by hand\nwhether to delete a store member deserves the same hint as the eight arrays\nbeside it, and a run that reports eight caveated findings and one bare one\nreads as though the bare one were better evidenced.\n\nStore members take the member rule unchanged, the same one class and enum\nmembers take: member usage is collected by one walk over the accesses of every\nmodule the run parsed, reachable or not, so any module analyzed incompletely\ncan hold the access that credits it.\n\nThe added test asserts the disclosure and the absence of a mutation together,\nso if a store-member mutation is ever introduced the gate has to be reasoned\nabout rather than inherited silently.\n\nThe field is additive-optional and absent on a clean run, so no envelope moves.\n\n* fix: make the caveat reach every surface, and the gates that guard it fail\n\nA slop audit of the output-honesty work, four reviewers on disjoint domains,\neach building their own binaries and reproducing before reporting.\n\nThe store-member caveat reached the JSON wire and almost nothing else. Eight\nrender sites hardcoded an empty caveat slice in the store-member branch, left\nfrom before store members were registered, and because the four pr-comment and\nreview formats render from the CodeClimate description, that one site cascaded\ninto all four. Nine of eleven surfaces. The eighth site was not in the reported\nlist: the human summary rollup chained eight arrays and omitted the ninth, so\n`--summary` undercounted.\n\nIt shipped green because no fixture on any of those surfaces built a member\nfinding carrying a caveat. The fixtures now do, everywhere, so the class cannot\nreturn silently.\n\nTwo gates could not fail. `every_machine_consumed_format_carries_the_caveat`\nomitted `sarif` from its list, the format GitHub code scanning ingests, so\ndeleting the caveat from every SARIF message left it green. The summary rollup\ntest gave each finding exactly one caveat, making per-token over-counting\nundetectable, though a finding whose own file is degraded really carries two.\nBoth now go red under the mutation that previously passed.\n\nThe MCP byte gate was measuring a number no commit ever had: the recorded mark\nwas arrived at by summing deltas by hand rather than running the gate, which\nleft the stale-headroom guard passing vacuously with a 602-byte dead zone. It\nis re-pinned from a real measurement, and the comment says how.\n\n`max_output_bytes` told agents that exceeding the cap returns a preview. It\nreturns a refusal. An agent lowering the cap to bound its context was planning\non a bounded result and getting a run that returns no data.\n\nNine claims in the compatibility document and changelog were falsified by\nrunning what they described, including the combined envelope having no\n`dead_code` block, a counter documented as counting removals that counts files,\nand a changelog that still said store members stay out twenty lines from its\nown sentence saying they do not. The three entries are rewritten from about\n1790 words to about 875, structured as what changed, which findings, what you\nmust do, what stays the same, with the archaeology moved out.\n\nAlso: two dependency findings in one package.json shared a SARIF fingerprint,\nso GitHub collapsed two alerts into one; the human directory rollup labelled a\nfile as a directory; and four test names promised more than their bodies\nchecked.",
          "timestamp": "2026-09-08T22:39:13+02:00",
          "tree_id": "57ebe14e0632c954e383a18e842ee6a390cdcf3a",
          "url": "https://github.com/fallow-rs/fallow/commit/e5e92c787465f81e6fcd8f84a33be5b7f6ee93ac"
        },
        "date": 1788900302253,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "c716033fb8a61549d7b801a97bbcbd7cac643038",
          "message": "fix: stop reading a jq filter as an entry glob, and print forward slashes\n\nTwo reports, one release-blocking.\n\nA quoted jq filter in a CI `run:` block was harvested as a file path and then\nfed to the entry-pattern globber, so a valid workflow warned `invalid entry\npattern ... unclosed character class` on dead-code, health and audit alike.\nTwo defects met there. The harvester treated any non-flag token containing a\nslash as a path, and a jq filter contains one from the `//` alternative\noperator alone; it now also requires the token to carry no internal\nwhitespace, which a real positional path argument never does. And the\ndot-segment normalizer dropped empty segments silently, so `//` collapsed to\n`/` and the string was already corrupt by the time it reached the globber; a\ndoubled separator is not path syntax, so the candidate is dropped rather than\nrewritten. Script paths named in `run:` blocks are still harvested, because\nignoring `.github/**` would lose that dependency evidence.\n\n`fallow fix` rendered the platform separator in its human lines and in its\nJSON `path` and `file` fields, so a Windows user was told `Would remove export\nfrom src\\util.ts` while every other fallow surface said `src/util.ts`. The\nrelease validation caught it as a Windows-only test failure. Thirty-eight\nsites across the fix module now normalize the way the rest of the CLI already\ndid. The private `__target` correlation field a fixer later opens on disk\nstays native, because that path goes back to the operating system rather than\nto a reader.\n\nCloses #2592",
          "timestamp": "2026-09-09T08:55:21+02:00",
          "tree_id": "134716d85678517f8bcdb7512ecb61d1f44ddbe8",
          "url": "https://github.com/fallow-rs/fallow/commit/c716033fb8a61549d7b801a97bbcbd7cac643038"
        },
        "date": 1788937199139,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "85615b646a1adb45a30bf24a45ffa55dfa088737",
          "message": "fix(dupes): fall back to content when the fingerprint cannot be trusted\n\nAdding ctime to the extract fingerprint closed a real staleness bug, but the\nduplication token cache reacted to a missing ctime by returning nothing rather\nthan by falling back. ctime is always absent on Windows, so that cache never\nhit there: correct results, and a permanently cold cache on an entire platform.\nThe release validation caught it; a reviewer had noted the behaviour earlier as\na clean miss, which is true and was too light a reading.\n\nIt now takes the same fast-path and slow-path split the extract cache already\nhad. When the timestamps can be trusted, an exact match or a miss, unchanged.\nWhen they cannot, read the file and compare content hashes against the source\nthe entry already stores, so no cache version moves.\n\nThe invariant the ctime work exists to protect survives, because the fallback\ntrusts bytes rather than metadata: a size-preserving edit with a restored mtime\nstill misses, on every platform. The new tests build a fingerprint without a\nctime directly, so a macOS run proves the Windows path rather than skipping it.\n\nThe graph cache was checked and is unaffected: it keys on content hashes and\ndeliberately avoids ctime, since cp -Rp and CI cache restores preserve mtime\nbut reset it.",
          "timestamp": "2026-09-09T12:16:58+02:00",
          "tree_id": "58042834076bade476e9312b54f764a1dd71f3f8",
          "url": "https://github.com/fallow-rs/fallow/commit/85615b646a1adb45a30bf24a45ffa55dfa088737"
        },
        "date": 1788949379505,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "9dc043deca19489fdbc7dfd382e53d95c5c8e114",
          "message": "fix(coverage): point the post-upload dashboard link at the repository route\n\nCloses #2597",
          "timestamp": "2026-09-09T12:49:28+02:00",
          "tree_id": "c1144f3de6279ebaf6a926203038832f530859fa",
          "url": "https://github.com/fallow-rs/fallow/commit/9dc043deca19489fdbc7dfd382e53d95c5c8e114"
        },
        "date": 1788951328888,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "0b2922747231a43c269b2e555e0f366f6025e294",
          "message": "fix(coverage): match cloud runtime functions through path prefixes and runtime names\n\nCloses #2593.\n\nThe cloud join compared the full runtime file path against the repo-relative\nstatic index and required both sides to spell the function name the same way.\nThe busiest functions failed both tests. A containerized service reports\n/app/src/a.ts, which never equals src/a.ts, so whole files were dropped; and\nruntime instrumentation names a function from its surroundings, so an anonymous\ncallback arrives under the name of the callee it was passed to and an accessor\nkeeps its get prefix, neither of which the static index spells that way. Most of\nthe payload landed in the cloud_functions_unmatched warning instead of in\nfindings, and the hot-path list was led by whatever incidental helper survived.\n\nRuntime paths are now rebased onto the local tree by file name plus a\nsegment-wise suffix comparison, and a function whose name disagrees is matched\non position within the resolved file. Both tiers refuse an ambiguous answer\nrather than guess: two local files equally entitled to one runtime path, or two\ndefinitions opening on one line with no end line to separate them, stay\nunmatched. Stable-id matching is unchanged and still runs first.\n\n--debug-unmatched lists what remains on stderr, highest traffic first, so the\nresidue can be read without a debugger; stdout stays machine-readable.\n\nThe fixture test builds a real static index from a project holding a top-level\narrow, an object-literal method, an accessor and two callee-named callbacks,\nthen merges a snapshot whose paths carry a container prefix. Without the path\nrebase all six functions are dropped; with the rebase alone the three\nruntime-named ones are still dropped.",
          "timestamp": "2026-09-09T13:42:19+02:00",
          "tree_id": "3aa835b93bf50be2c00205eaf4792c758f62b7fe",
          "url": "https://github.com/fallow-rs/fallow/commit/0b2922747231a43c269b2e555e0f366f6025e294"
        },
        "date": 1788954480635,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "adeca23ebe17e714a27e7a4f57300cdefd3d283b",
          "message": "fix: let license refresh fall back to a full-access API key\n\nCloses #2595",
          "timestamp": "2026-09-09T14:43:18+02:00",
          "tree_id": "888f0016c2a13aeacb9a60ab716f4e87e075cbe7",
          "url": "https://github.com/fallow-rs/fallow/commit/adeca23ebe17e714a27e7a4f57300cdefd3d283b"
        },
        "date": 1788958140922,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "4e9a9c39b74edaacd7945addb493ec35f9287d28",
          "message": "fix(dupes): let content settle a cache lookup the timestamps cannot\n\nThe first pass gave the token cache a content fallback only when the\nfingerprint was untrustworthy, which fixed Windows and left the two platforms\nbehaving differently: a touch re-tokenized on Unix and hit on Windows.\n\nMetadata is the fast path, not the verdict. A match settles the lookup without\ntouching the disk; a mismatch only means the timestamps cannot settle it, so\ncontent decides, on every platform. A file whose bytes never moved now survives\na touch or a checkout that rewrites timestamps.\n\nThe staleness invariant is unchanged because content is the authority in both\nbranches: a size-preserving edit with a restored mtime still misses.\n\nOne test asserted the old rule, that a metadata mismatch means a miss. It is\nreplaced by the two properties that are actually worth holding: timestamps\nmoving alone hits, timestamps and content moving together misses.",
          "timestamp": "2026-09-09T14:51:53+02:00",
          "tree_id": "c79a453c179bbba0c3304652645d9a19f31bec2b",
          "url": "https://github.com/fallow-rs/fallow/commit/4e9a9c39b74edaacd7945addb493ec35f9287d28"
        },
        "date": 1788958930855,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "8b875bdf06105317ab1b94c4493cf639c9e4839b",
          "message": "chore: release v3.24.0",
          "timestamp": "2026-09-09T15:58:39+02:00",
          "tree_id": "634fecbf4bb35acf7ebd8db7434cea2872ccce3b",
          "url": "https://github.com/fallow-rs/fallow/commit/8b875bdf06105317ab1b94c4493cf639c9e4839b"
        },
        "date": 1788962727351,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "5cd805c63be517cecff3e0ddcc7c52e7422555b7",
          "message": "feat(mcp): read runtime coverage from fallow cloud\n\nAdds get_cloud_runtime_context, an MCP tool that pulls a repository's runtime facts from fallow cloud and returns the same runtime_coverage block the local runtime-coverage tools return, backed by fallow coverage analyze --cloud --format json. The API key is read from FALLOW_API_KEY in the server environment and is never a tool parameter; a call without one is refused before any subprocess starts with code cloud_api_key_missing and the CLI's own remediation sentence, now shared through fallow-types.\n\nCloses #2596",
          "timestamp": "2026-09-09T17:16:46+02:00",
          "tree_id": "4050e0734283a649176268b4df2849375a2863d8",
          "url": "https://github.com/fallow-rs/fallow/commit/5cd805c63be517cecff3e0ddcc7c52e7422555b7"
        },
        "date": 1788967359203,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "4163a2a7664eee3bb37b371fa97bc1ad39156496",
          "message": "fix(coverage): stop calling a test-only export safe to delete under --production\n\nCloses #2594",
          "timestamp": "2026-09-09T18:36:40+02:00",
          "tree_id": "4f0a6315d5dcd9ed69a1d13d618115155f6bb47f",
          "url": "https://github.com/fallow-rs/fallow/commit/4163a2a7664eee3bb37b371fa97bc1ad39156496"
        },
        "date": 1788972147569,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "0565d3ec6ad58777270553feb504c00af7ce0790",
          "message": "chore: pin the container and the schema baseline to v3.24.0",
          "timestamp": "2026-09-09T19:33:56+02:00",
          "tree_id": "815dab8449f1c82ec4ef9a916ea122560e090272",
          "url": "https://github.com/fallow-rs/fallow/commit/0565d3ec6ad58777270553feb504c00af7ce0790"
        },
        "date": 1788975619651,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "c2da9fcae5388226a87e986b1812f9c872d288c6",
          "message": "chore: release v3.24.1",
          "timestamp": "2026-09-09T21:36:33+02:00",
          "tree_id": "082d4b12b54247a074954b50c7c31d0af8936e9b",
          "url": "https://github.com/fallow-rs/fallow/commit/c2da9fcae5388226a87e986b1812f9c872d288c6"
        },
        "date": 1788991291516,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "59ce79f4a493ea85aeb10b02d8cfa8f2278f5c56",
          "message": "chore(napi): sync package.json / package-lock / index.js to v3.24.1",
          "timestamp": "2026-09-10T02:10:55+02:00",
          "tree_id": "16c2dec30963674f025618b2bb7ece4297be3ef6",
          "url": "https://github.com/fallow-rs/fallow/commit/59ce79f4a493ea85aeb10b02d8cfa8f2278f5c56"
        },
        "date": 1788999428561,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "69d8d818a9d6b094545dae2f5c476cb0870feaa3",
          "message": "feat: support positional PATH scope across file commands\n\nAdds an optional positional [PATH] to bare fallow, check, dupes, health, audit, security, fix, list, and similar-code. The scope narrows reported findings to the file or directory while the full project graph is still built, so cross-file facts stay sound.\n\nResolution is root-first for bare relative paths, honors ./ and ../ as current-directory claims, and rejects missing or outside-root paths with an actionable exit-2 error. Scope composes with --workspace as one more workspace root and intersects with --changed-since and --diff-file. Audit narrows its changed-file universe so verdict and base attribution stay coherent; its base pass stays unscoped because it runs in another worktree.",
          "timestamp": "2026-09-10T09:51:56+02:00",
          "tree_id": "19d0ed01da89ec323b163b9b7d303bc03dfbc6d9",
          "url": "https://github.com/fallow-rs/fallow/commit/69d8d818a9d6b094545dae2f5c476cb0870feaa3"
        },
        "date": 1789027054038,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "b7722aff930fdad053d4d811b8e5fb055ff1ef44",
          "message": "feat(coverage): report the caller-edge size guard in the inventory blob\n\nCloses #2607",
          "timestamp": "2026-09-10T10:31:42+02:00",
          "tree_id": "11ef3a131bf22aca2845d8069ed0526ea2b43240",
          "url": "https://github.com/fallow-rs/fallow/commit/b7722aff930fdad053d4d811b8e5fb055ff1ef44"
        },
        "date": 1789029166885,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1302,
            "unit": "count"
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
          "id": "423ce06892b814c76c22d9e745df44aafa25d732",
          "message": "fix(coverage): index instrumenter-named callbacks and object members for the cloud join\n\nCloses #2606",
          "timestamp": "2026-09-10T09:47:57Z",
          "tree_id": "ca11dae5f453b957d1c02ac10cf747052331515c",
          "url": "https://github.com/fallow-rs/fallow/commit/423ce06892b814c76c22d9e745df44aafa25d732"
        },
        "date": 1789033938856,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1303,
            "unit": "count"
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
          "id": "03aad8caad3c563481e2b00b5c0d7c67f2a371d8",
          "message": "fix(cli): print forward slashes in the check and health human output\n\nThe two human renderers rendered the platform path separator, so a Windows\nuser was told `src\\a.ts` while `dupes`, `list`, `fix`, and every JSON\nsurface said `src/a.ts`. The damage was not only cosmetic: the dimmed-directory\n/ bold-filename split keys on `/`, so a native-separator path also lost its\nemphasis and rendered as one bold blob. Every path the check and health\nrenderers put on screen now goes through the existing display helper, the way\nthe rest of the CLI already did. On-disk path handling is untouched; only the\nrendered text changes, and it is byte-identical on POSIX.\n\n`scope_path_tests` caught this as three Windows-only failures, and its\nnegative assertions were the reason it caught no more: `!contains(\"other/c.ts\")`\npassed vacuously while the file was on screen as `other\\c.ts`. The captured\noutput is now normalised once before every assertion, so a leaked out-of-scope\nfile trips the suite under either separator. Two renderer unit tests pin the\nforward-slash convention itself, since the integration tests no longer can.\n\nCloses #2611",
          "timestamp": "2026-09-10T14:35:15+02:00",
          "tree_id": "8c55301281961a9e4bd58b54ef7b236a3b08eb9a",
          "url": "https://github.com/fallow-rs/fallow/commit/03aad8caad3c563481e2b00b5c0d7c67f2a371d8"
        },
        "date": 1789044056937,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1303,
            "unit": "count"
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
          "id": "30167b42257ded3bef683f6c91b4aa4017a1ff59",
          "message": "chore: release v3.25.0",
          "timestamp": "2026-09-11T09:05:59+02:00",
          "tree_id": "beca44a718db4073802dc0e467c40169e0df45a3",
          "url": "https://github.com/fallow-rs/fallow/commit/30167b42257ded3bef683f6c91b4aa4017a1ff59"
        },
        "date": 1789110736316,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1303,
            "unit": "count"
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
          "id": "bb3893aa62cc51a0be60037ea7ab5ccf9ade3d5a",
          "message": "chore: advance the schema policy baseline to v3.25.0",
          "timestamp": "2026-09-11T10:55:09+02:00",
          "tree_id": "e72ba4e1cece94196473dee09cb72bb686e081b5",
          "url": "https://github.com/fallow-rs/fallow/commit/bb3893aa62cc51a0be60037ea7ab5ccf9ade3d5a"
        },
        "date": 1789117291657,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1303,
            "unit": "count"
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
          "id": "5c1add6b22533b9d580932edbd728dded915643e",
          "message": "fix(audit): scope base snapshot to sparse cone and analysis subdir\n\nfallow audit hung to the CI timeout on GitHub Actions runners for sparse checkouts of large monorepos. The raw object materialization introduced in 3.4.2 reads every blob in the base commit, so on a blobless partial clone each out-of-cone blob triggers a lazy promisor fetch.\n\nmaterialize_committed_tree now filters the committed tree through a MaterializationScope before touching blobs. Verified against a local blobless partial clone: the old binary fetched all 300 out-of-cone blobs, the fixed binary only in-cone blobs, with identical audit output apart from the telemetry id. verify:fast green, rust-review APPROVE.\n\nFixes #2615.",
          "timestamp": "2026-09-11T21:03:09+02:00",
          "tree_id": "47abbcb18e5c6ecfe0b2ca98a45a1f1b60441e8d",
          "url": "https://github.com/fallow-rs/fallow/commit/5c1add6b22533b9d580932edbd728dded915643e"
        },
        "date": 1789153513168,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 476,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1303,
            "unit": "count"
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
          "id": "1ee9062b92e2b4a002844bf4f778917433901e1b",
          "message": "feat(plugins): add built-in Oxfmt plugin\n\nBuilt-in Oxfmt plugin mirroring the Oxlint plugin: oxfmt.config.ts (and siblings) are marked always-used and static imports from TS configs are credited. Knip migration reports oxfmt sections as auto-detected.\n\nFixes #2614.",
          "timestamp": "2026-09-11T22:03:57+02:00",
          "tree_id": "b098fc0afebc4b7f9dabc7650fde84b911c42308",
          "url": "https://github.com/fallow-rs/fallow/commit/1ee9062b92e2b4a002844bf4f778917433901e1b"
        },
        "date": 1789157393632,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Max Fan-In (non-framework)",
            "value": 54,
            "unit": "deps"
          },
          {
            "name": "Max Fan-Out (non-framework)",
            "value": 29,
            "unit": "deps"
          },
          {
            "name": "Modules >20 Fan-In (%)",
            "value": 1.26,
            "unit": "%"
          },
          {
            "name": "Total Modules",
            "value": 477,
            "unit": "count"
          },
          {
            "name": "Total Edges",
            "value": 1306,
            "unit": "count"
          }
        ]
      }
    ]
  }
}