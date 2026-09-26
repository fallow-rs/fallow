window.BENCHMARK_DATA = {
  "lastUpdate": 1790433476953,
  "repoUrl": "https://github.com/fallow-rs/fallow",
  "entries": {
    "Fallow Coverage": [
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
        "date": 1789157403611,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.5,
            "unit": "%"
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
          "id": "8622017e6effcad51ddff304032914e08366de27",
          "message": "chore(audit): drop unused derives on MaterializationScope\n\nSlop-audit follow-up to the sparse base-snapshot change: the private scope struct is only borrowed, so Debug, Clone, and Default have no users.",
          "timestamp": "2026-09-12T11:02:14+02:00",
          "tree_id": "d54e94b9a600e1b58a89a66a6514e25c5da7be53",
          "url": "https://github.com/fallow-rs/fallow/commit/8622017e6effcad51ddff304032914e08366de27"
        },
        "date": 1789204119192,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.5,
            "unit": "%"
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
          "id": "a013bc4285df07f9e706b1357de421222e9ebb4e",
          "message": "chore(deps): update rustls to 0.23.45 for RUSTSEC-2026-0285 (#2630)\n\nrustls 0.23.37 is affected by RUSTSEC-2026-0285: TLS 1.3 handshake messages are accepted across encryption level boundaries. Lockfile-only bump to 0.23.45, pulling rustls-webpki 0.103.13 to 0.103.15; both arrive transitively through ureq and neither is pinned.\n\nCargo Deny and Security Audit are required checks that only run on Rust paths, so this failed every open pull request touching a Rust file while main stayed green.",
          "timestamp": "2026-09-14T21:47:13+02:00",
          "tree_id": "7257b3005d74f4838d53fbaca6906f003d2f1c3a",
          "url": "https://github.com/fallow-rs/fallow/commit/a013bc4285df07f9e706b1357de421222e9ebb4e"
        },
        "date": 1789415880697,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.5,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "maritz.hans@gmail.com",
            "name": "Hans-Albert Maritz",
            "username": "Freakazo"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "08bcec083b24159ad948f90c666744c39d76b120",
          "message": "perf(core): reuse workspace ownership for dependency checks (#2624)\n\nResolve each graph file's deepest owning workspace once per analysis into a dense FileId-indexed lookup, and reuse it for both the unused-dependency and unlisted-dependency checks instead of walking the workspace list for every package-usage entry and import site.\n\nNested-workspace resolution and the root-dependency fallback are preserved, and the duplicated path-based lookup helper is removed.\n\nMaintainer validation: findings are byte-identical to main on five real workspace monorepos, and the analyze phase is measurably faster across repeated runs. fmt, clippy, the full workspace test suite, bench check and rustdoc are green locally.",
          "timestamp": "2026-09-15T01:15:28+02:00",
          "tree_id": "2ebfb9e3b05763943167bbc1c85e42c66c29e257",
          "url": "https://github.com/fallow-rs/fallow/commit/08bcec083b24159ad948f90c666744c39d76b120"
        },
        "date": 1789428379268,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.5,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "randall@bleeds.info",
            "name": "Randall Leeds",
            "username": "tilgovi"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "851eadf906da389cc32d0dc0864b4e446ffdf16c",
          "message": "feat(plugins): recognize the Expo Router SuspenseFallback export (#2618)\n\nAdd SuspenseFallback to the Expo Router plugin's route-file exports, so a route file exporting it is no longer reported as an unused export.\n\nExpo Router SDK 56 added a customizable Suspense fallback; LoadedRoute declares SuspenseFallback alongside the exports the plugin already knew, such as ErrorBoundary and unstable_settings.\n\nMaintainer validation: the fixture integration test fails without the plugin change and passes with it, and fmt, clippy, the full workspace test suite, bench check and rustdoc are green locally.",
          "timestamp": "2026-09-15T01:29:15+02:00",
          "tree_id": "7b26f36723396e9fdfe4e188318965a6b95c9fae",
          "url": "https://github.com/fallow-rs/fallow/commit/851eadf906da389cc32d0dc0864b4e446ffdf16c"
        },
        "date": 1789429361610,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.5,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "maritz.hans@gmail.com",
            "name": "Hans-Albert Maritz",
            "username": "Freakazo"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "75ac87337420156a40bd622c43f4245a4f2ba1d4",
          "message": "fix(graph): preserve empty dynamic pattern cache rows (#2626)\n\nPreserve one resolver-cache target row per dynamic-import pattern, including patterns that match no files and patterns whose glob fails to compile. Empty rows still contribute no graph edges, and the graph cache version stays at 50 because legacy sparse rows take the existing safe cache-miss path.\n\nBefore this change a project with a single zero-match pattern never reused its graph cache: the cached row list was shorter than the pattern list, restoration reported a changed file set on every run, and imports were re-resolved from scratch each time.\n\nMaintainer validation: on a real Next.js project carrying a zero-match template-literal import, the old binary rejected its own cache on every warm run while the fixed binary reuses it. A cache written by the fixed binary is accepted by the old one, and a cache written by the old binary is refused by the fixed one through the length check, so both crossing directions are safe. Cold-run output is otherwise identical.",
          "timestamp": "2026-09-15T01:43:25+02:00",
          "tree_id": "3eff75c806035985055dd50874d635c0c2248762",
          "url": "https://github.com/fallow-rs/fallow/commit/75ac87337420156a40bd622c43f4245a4f2ba1d4"
        },
        "date": 1789429955205,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.5,
            "unit": "%"
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
          "id": "68c6e4b87d2743479bdeb98b98d24cef47b046b8",
          "message": "docs(changelog): record the contributor changes awaiting release (#2628)\n\nRecords the four changes that landed without a changelog entry of their own: the Expo Router route exports from #2618 and #2629, the workspace-ownership reuse in the dependency checks from #2624, and the empty dynamic-pattern rows in the resolver cache from #2626.",
          "timestamp": "2026-09-15T02:18:04+02:00",
          "tree_id": "9bb3333bd7d384dba8c1b5f8aefc2059c77072d9",
          "url": "https://github.com/fallow-rs/fallow/commit/68c6e4b87d2743479bdeb98b98d24cef47b046b8"
        },
        "date": 1789431962614,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.5,
            "unit": "%"
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
          "id": "c00331886f81edf7fe4dbf8e5ab4b784d9f363e8",
          "message": "fix(lsp): resolve per-path rule overrides on the editor analysis path (#2635)\n\nRule severity resolution, including overrides[].rules, lived in the check\ncommand, so the language server published diagnostics for rules a project had\nturned off for the matching path while fallow dead-code filtered them out.\nThe pass now lives in fallow-engine next to the sibling dead-code result\nfilters and runs inside EditorAnalysisSession, once per analyzed project slice\nand again after type-aware refinement because reconciliation can add\nfindings. The check command delegates to the same code, so CLI output is\nunchanged. The programmatic runtime behind MCP does not run the pass yet.\n\nFixes #2621",
          "timestamp": "2026-09-15T08:29:22+02:00",
          "tree_id": "7a08998b33bfa08ce5fbac050fa82773e1a0cfe7",
          "url": "https://github.com/fallow-rs/fallow/commit/c00331886f81edf7fe4dbf8e5ab4b784d9f363e8"
        },
        "date": 1789454217472,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.5,
            "unit": "%"
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
          "id": "2c58e501eb11aaae5749310e6c8c2608b7992603",
          "message": "docs(changelog): record the five external-issue fixes awaiting release (#2639)\n\nEntries for the JSONC schema hint (#2623), the recursive build ignore\n(#2622), the dead-code baseline staleness warning and its scope guard\n(#2627), the istanbul nested-function ownership fix (#2620) and the editor\nrule-override fix (#2621). Kept out of the fix branches so they do not\nconflict with each other on this file.",
          "timestamp": "2026-09-15T08:40:29+02:00",
          "tree_id": "2096c6d41d801d80818ba8c7f40240ccea2b91e0",
          "url": "https://github.com/fallow-rs/fallow/commit/2c58e501eb11aaae5749310e6c8c2608b7992603"
        },
        "date": 1789454812004,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.5,
            "unit": "%"
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
          "id": "2e878e711b42b9a56f5b92c72bb97d92375182d1",
          "message": "docs(changelog): record the three follow-up fixes awaiting release (#2669)\n\nEntries for the rule-override surfaces (#2636), the baseline staleness gate and parity fixes (#2637) and the built-in exclusion diagnostics (#2638), kept out of the fix branches so they do not conflict on CHANGELOG.md.",
          "timestamp": "2026-09-15T20:16:38+02:00",
          "tree_id": "64837e7acf1f0554ae5dde96ee245b664b9b322a",
          "url": "https://github.com/fallow-rs/fallow/commit/2e878e711b42b9a56f5b92c72bb97d92375182d1"
        },
        "date": 1789496689279,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "80ea4e5198b68bd42ef527d2e638dad44081374a",
          "message": "test(lsp): canonicalize the watched-config test root through the Windows-safe helper\n\nstd::fs::canonicalize yields a verbatim path on Windows, so the file URI the test waited on never matched the URI the server publishes and the Windows job failed on the first diagnostic. The test now uses the same helper as the other LSP tests.",
          "timestamp": "2026-09-15T20:43:15+02:00",
          "tree_id": "f56f96766246835fd2bb79ec57975d23410251d5",
          "url": "https://github.com/fallow-rs/fallow/commit/80ea4e5198b68bd42ef527d2e638dad44081374a"
        },
        "date": 1789498290161,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "8102e22e0afcc70fb518cd26ea1a2f18f5d710ff",
          "message": "chore: release v3.26.0",
          "timestamp": "2026-09-15T21:40:27+02:00",
          "tree_id": "6e3085f55cd4967cfb8184193c170c651b79f792",
          "url": "https://github.com/fallow-rs/fallow/commit/8102e22e0afcc70fb518cd26ea1a2f18f5d710ff"
        },
        "date": 1789501869473,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "4f3bd9720ca6b22c8547adb7f1d225f57fc502d4",
          "message": "fix(similar-code): update rustls to 0.23.45 in the sidecar lockfile\n\nThe standalone similar-code sidecar lockfile still pinned rustls 0.23.43, which RUSTSEC-2026-0285 covers, so the sidecar audit step rejected the release build. The workspace lockfile moved to 0.23.45 in #2630; the sidecar now matches it.",
          "timestamp": "2026-09-15T23:20:57+02:00",
          "tree_id": "c0b58478ea704b9d0f17ccd9f1a04325d7888b47",
          "url": "https://github.com/fallow-rs/fallow/commit/4f3bd9720ca6b22c8547adb7f1d225f57fc502d4"
        },
        "date": 1789507817857,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "601d8ee39a0f0211a85dc522101058f60cda1484",
          "message": "chore: advance the schema policy baseline to v3.26.0",
          "timestamp": "2026-09-16T02:09:30+02:00",
          "tree_id": "96f2462287f3c7ac8efa8097502153e3febd26cf",
          "url": "https://github.com/fallow-rs/fallow/commit/601d8ee39a0f0211a85dc522101058f60cda1484"
        },
        "date": 1789517777277,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "49699333+dependabot[bot]@users.noreply.github.com",
            "name": "dependabot[bot]",
            "username": "dependabot[bot]"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "4528f4c94c92e9f98fd9d1606a4d886e6ab4ab39",
          "message": "chore(deps-dev): bump vitest from 4.1.11 to 5.0.0 in /editors/vscode (#2646)\n\nBumps [vitest](https://github.com/vitest-dev/vitest/tree/HEAD/packages/vitest) from 4.1.11 to 5.0.0.\n- [Release notes](https://github.com/vitest-dev/vitest/releases)\n- [Changelog](https://github.com/vitest-dev/vitest/blob/main/docs/releases.md)\n- [Commits](https://github.com/vitest-dev/vitest/commits/v5.0.0/packages/vitest)\n\n---\nupdated-dependencies:\n- dependency-name: vitest\n  dependency-version: 5.0.0\n  dependency-type: direct:development\n  update-type: version-update:semver-major\n...\n\nSigned-off-by: dependabot[bot] <support@github.com>\nCo-authored-by: dependabot[bot] <49699333+dependabot[bot]@users.noreply.github.com>",
          "timestamp": "2026-09-16T06:45:51+02:00",
          "tree_id": "9fc4e6e77a1d30e97e5f761e42bfb35f8fcf564a",
          "url": "https://github.com/fallow-rs/fallow/commit/4528f4c94c92e9f98fd9d1606a4d886e6ab4ab39"
        },
        "date": 1789534342290,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "9c6910da437645531da221119494b32313165ca9",
          "message": "chore(deps): bump @tanstack/intent to 0.4.0 (#2670)\n\nMove the root and npm/fallow devDependency together so the bundled skill\nvalidation keeps resolving from the root lockfile, and advance the pinned\nversion the workflow policy test asserts.",
          "timestamp": "2026-09-16T07:21:27+02:00",
          "tree_id": "4746e8c8e2926ea81934160c15c9896ae3536b36",
          "url": "https://github.com/fallow-rs/fallow/commit/9c6910da437645531da221119494b32313165ca9"
        },
        "date": 1789536564058,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "1080fe084b5a0fbd61e05f808664c82290427cd2",
          "message": "chore(deps-dev): bump rolldown to 1.2.7 in viz-frontend (#2671)\n\nThe viz bundle is checked in and byte-compared in CI, so the bundler bump\ncarries the rebuilt crates/cli/viz-assets/viz.js with it.",
          "timestamp": "2026-09-16T07:30:04+02:00",
          "tree_id": "99601523abb138f4109ab7329fb19cd109438a60",
          "url": "https://github.com/fallow-rs/fallow/commit/1080fe084b5a0fbd61e05f808664c82290427cd2"
        },
        "date": 1789537023341,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "de90d3109e5eee2bb7729f487c65476a3d0a2df2",
          "message": "fix(action): surface baseline staleness and propagate the stale-baseline gate in CI (#2674)\n\nThe dead-code baseline staleness warning and the opt-in stale-baseline gate shipped in 3.26.0 were unreachable through the GitHub Action and the GitLab template: both run with `--quiet --format json`, the warning is quiet-suppressed, stderr went to `::debug::`, and the CLI exit code was dropped whenever stdout parsed as JSON.\n\n- dead-code, dupes and health envelopes publish one shared `baseline_staleness` object (health's existing member names plus `current_findings`, `warning` and `gate_trips`); no schema version moves and the `HealthBaselineStaleness` TypeScript name stays as a deprecated alias.\n- The action and the GitLab template read the verdict from that object, never from the exit code, so `fail-on-issues: false` keeps its meaning; a new `fail-on-stale-baseline` input (`FALLOW_FAIL_ON_STALE_BASELINE` on GitLab) fails the job on a tripped gate.\n- On pull requests the primary run is change-scoped, so a second unscoped, report-discarding run judges the baseline whenever the action itself narrowed the run; the advisory lands as `::warning::`, a tripped gate as `::error::`, a stand-down as `::warning::`, and a line in the step summary.\n- Fail open on an older binary or a command without staleness; invalid input combinations are rejected at validation.\n\nFixes #2673",
          "timestamp": "2026-09-16T18:44:44+02:00",
          "tree_id": "04ca6836a569a821d7453c67930f0a5ce3c3bdc1",
          "url": "https://github.com/fallow-rs/fallow/commit/de90d3109e5eee2bb7729f487c65476a3d0a2df2"
        },
        "date": 1789577465242,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "5cebdde102efc436f89b7fc9878990c1b3647e0e",
          "message": "feat(output): publish gate_outcomes on every analysis envelope (#2692)\n\nEvery gate the CLI arms now publishes its outcome in the JSON envelope, computed once by the rule that decides the exit code, so a consumer that runs `--quiet --format json` and drops the exit code can still read the verdict.\n\n- Root optional `gate_outcomes` object keyed by gate name on dead-code, dupes, health, audit, security, combined and grouped envelopes, absent when no gate was armed. Each entry carries `status` (`pass`, `warn`, `fail`, `skipped`), `enforced` (whether the outcome affects the exit code on this run, false under `--report-only` and on combined machine output), and `observed`, `threshold` and `threshold_label` where a message needs numbers. The default exit rule of the command is always included when the object exists, so the object explains the exit code.\n- `workspace_diagnostics[].degrades_analysis` marks the diagnostics that mean the run analyzed less than the project, and a run that found no source files says so with its own diagnostic.\n- `fallow report --from` renders a neutral \"Gate outcomes\" line on the GitHub summary, annotations, PR comment and review targets, and the GitLab MR note, live and saved paths identical; the check-run conclusion and the exit code are unchanged.\n- No schema version moves; the gate name set is open on the wire. The 3.26.0 sentence that `--fail-on-stale-baseline` changes nothing but the exit code is superseded: it now also sets `enforced` on the stale-baseline entry.\n\nFixes #2682\nRefs #2680 #2681 #2683 #2684 #2685 #2686",
          "timestamp": "2026-09-17T02:33:28+02:00",
          "tree_id": "8433d161d981a823ae88dfb066c62a83aa25d36e",
          "url": "https://github.com/fallow-rs/fallow/commit/5cebdde102efc436f89b7fc9878990c1b3647e0e"
        },
        "date": 1789605762556,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "1645a5ee564500e38a37e774748c0f4d66c93bac",
          "message": "fix(action): read every gate verdict from the envelope and fail the job the inputs asked for (#2693)\n\nThe GitHub Action and the GitLab template read every gate verdict from the envelope's `gate_outcomes` object instead of the CLI exit code, which both dropped whenever stdout parsed as JSON. Each gate fails the job only when the input that owns it asked for it and the CLI reports the gate as failed and enforced; a gate that tripped through `args` alone warns instead, so `fail-on-issues: false` stays authoritative.\n\n- `fail-on-regression`, `threshold`, `min-severity` and the security gate inputs now deliver the verdict they promised; a new `min-score` input runs health with `--complexity` so findings, annotations and the comment are not emptied; `--dupes-threshold` reaches the bare command.\n- One failure accumulator prints every failing gate and exits once, after outputs, artifacts and the summary are written; the inline `Check threshold` step and the early return before the security check are gone, so both paths are covered by tests.\n- Behaviour change: a repository with a security gate configured and `fail-on-issues: false` now fails on a tripped gate; not configuring the gate is the opt-out. `command: audit` with `gate: all` and `fail-on-issues: false` stays a reporting configuration.\n- A binary without `gate_outcomes` falls back to `regression.exceeded`, `gate.verdict`, `baseline_staleness.gate_trips` and the type-aware meta; gates with no older field fail open with one warning, only when their input asked for them.\n- Workspace diagnostics that degrade analysis land as one aggregated warning, an empty analysis warns by default and fails behind `fail-on-empty-analysis` (`FALLOW_FAIL_ON_EMPTY_ANALYSIS` on GitLab), and gate outcomes are exposed as step outputs and a GitLab dotenv report.\n\nFixes #2680 #2681 #2683 #2684 #2685 #2686\nRefs #2682 #2692",
          "timestamp": "2026-09-17T03:57:56+02:00",
          "tree_id": "adc91d7a23a127af177427766993f17e444c98cd",
          "url": "https://github.com/fallow-rs/fallow/commit/1645a5ee564500e38a37e774748c0f4d66c93bac"
        },
        "date": 1789610834114,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "b846e626b559c75e021e61df59857b51e09b7f9a",
          "message": "feat(mcp): state the run's gate and baseline verdicts in the tool result (#2694)\n\nMCP tool results now state the run's verdicts that the CLI expressed only on stderr or through an exit code the server converts into a success result.\n\n- The envelope's root `warnings` array gains plain sentences: the baseline staleness advisory or a tripped stale-baseline gate with the re-save remedy, one entry per `gate_outcomes` entry reporting `fail` or `warn` with its numbers and whether it was enforced, and one aggregated entry for workspace diagnostics that degrade analysis.\n- Applied on the subprocess route, the Code Mode route and the typed API route, with sectioned lookups for `audit`, whose staleness and diagnostics live under `complexity.summary` and `dead_code`.\n- No envelope member moves, the result body stays JSON, a clean run is byte-identical to CLI stdout, and the `isError` policy for exit codes 2 and above is unchanged. `structured_content` is left unpopulated until a tool declares an output schema for it.\n\nFixes #2676\nRefs #2682 #2692 #2693",
          "timestamp": "2026-09-17T04:54:03+02:00",
          "tree_id": "891cc8b71330e699dfd9f65d4ede85eb706051ca",
          "url": "https://github.com/fallow-rs/fallow/commit/b846e626b559c75e021e61df59857b51e09b7f9a"
        },
        "date": 1789614065949,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "adff2c9f20fe769e2e093b2e99ee09723f6310bd",
          "message": "chore: release v3.27.0",
          "timestamp": "2026-09-17T05:49:53+02:00",
          "tree_id": "5f5087ab391bf4b9521abc7c194acfa3c166f709",
          "url": "https://github.com/fallow-rs/fallow/commit/adff2c9f20fe769e2e093b2e99ee09723f6310bd"
        },
        "date": 1789617838237,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "ca7c3f6ed8b87d43033168d979ad6b92f4e22329",
          "message": "ci: soft-fail the cargo-modules install in the coupling workflow (#2696)\n\nThe coupling job is a metric tracker and its collect step already degrades to a warning, but the install step was not covered: an unlocked cargo install resolved unicode-ident 1.0.25 against unicode-properties 0.1.4, which trips a const assertion in ra-ap-rustc_lexer 0.166.0 and turned the check red with exit 101.\n\nKeep the unlocked install first so a cargo_metadata fix can still land ahead of a cargo-modules release, retry with the packaged lockfile (pins unicode-ident 1.0.24), and mark the step continue-on-error. When neither install succeeds, the collect step emits a warning, writes the coupling metrics skipped summary, and sets collected=false so the analyze and store steps skip.",
          "timestamp": "2026-09-17T08:13:19+02:00",
          "tree_id": "95bd3553d77eb4f61c4b9dff41067e2f477a5e3e",
          "url": "https://github.com/fallow-rs/fallow/commit/ca7c3f6ed8b87d43033168d979ad6b92f4e22329"
        },
        "date": 1789626122863,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "85b12cdaf782632837d02a892499d9e91e8b91b3",
          "message": "chore: advance the schema policy baseline to v3.27.0",
          "timestamp": "2026-09-17T22:34:38+02:00",
          "tree_id": "3c30f2b26fd2c496c37119a3d683c3794fbe16db",
          "url": "https://github.com/fallow-rs/fallow/commit/85b12cdaf782632837d02a892499d9e91e8b91b3"
        },
        "date": 1789677673747,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "cec24ab14159ebd56b10fb6f569280d9a845f9b0",
          "message": "ci(release): retry transient VS Code registry failures per pass (#2697)\n\nBoth VSIX publish jobs looped over the seven targets with a single publish call per target and no retry, so one intermittent registry error failed the whole job. The v3.26.0 and v3.27.0 release runs needed repeated manual reruns on Marketplace gallery timeouts and Open VSX 503 responses, including 503s on the duplicate check after every target was already published.\n\n- Both steps now retry per pass: attempt all seven targets, collect the failed set, sleep, and retry only that set, up to six passes on a 20/40/60/90/120 second schedule. Total sleeping is capped at 330 seconds per step however many targets fail, so a sustained outage still ends through the step's own exit instead of the job timeout.\n- One warning per pass names the targets still failing; the error per target is printed only after the last pass. `--skip-duplicate` is what makes every retry safe.\n- Inventory validation, the exact target list, `--no-dependencies`, the pinned tool versions and the tag-last invariants are unchanged. The release security document states the retry budget, and the workflow policy tests assert the pass loop and its schedule.",
          "timestamp": "2026-09-17T22:41:14+02:00",
          "tree_id": "bfb05e10e7f41c7f98c8db934627b202fca15b86",
          "url": "https://github.com/fallow-rs/fallow/commit/cec24ab14159ebd56b10fb6f569280d9a845f9b0"
        },
        "date": 1789678063213,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "aeb92c9a0f54ec18a11b2522459688728abf53e2",
          "message": "chore(docker): pin FALLOW_VERSION 3.27.0 with refreshed checksums",
          "timestamp": "2026-09-17T23:33:41+02:00",
          "tree_id": "c7948bda2b9cec39eb44c080799ce2fede772441",
          "url": "https://github.com/fallow-rs/fallow/commit/aeb92c9a0f54ec18a11b2522459688728abf53e2"
        },
        "date": 1789681202032,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "cc74281756dd722afb169d8310dbb73fbf06ec50",
          "message": "fix(mcp): auto-detect the audit base ref on the typed audit and decision_surface routes (#2703)\n\nThe MCP audit and decision_surface tools failed with\nFALLOW_CHANGED_FILES_FAILED when no base was passed, on any repository\nwith a remote: the engine's git probe returned raw stdout, so the\nauto-detected ref carried git's trailing newline into ref validation.\nThe CLI route trimmed in its own copy of the detection and kept working.\nRegression from 3.1.0.\n\nThe engine probe returns trimmed, non-empty output again, which also\nrepairs the repository root used when a typed run starts in a\nsubdirectory. The API validates an auto-detected ref the same way it\nvalidates an explicit one. The CLI's duplicate detection is removed and\nrouted through the engine, so both routes share one implementation.\n\nThanks @codingthat for the report and the bisect.\n\nCloses #2699",
          "timestamp": "2026-09-19T06:55:22+02:00",
          "tree_id": "a8a79896a2165d3fb493f72eb05741bc740fc219",
          "url": "https://github.com/fallow-rs/fallow/commit/cc74281756dd722afb169d8310dbb73fbf06ec50"
        },
        "date": 1789796682734,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.6,
            "unit": "%"
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
          "id": "61275ff83c557589aac1b36638b95a83dbfc0e2b",
          "message": "fix(output): report what a run was asked to do and whether it did it (#2704)\n\nSeveral requests could be dropped or degraded by a run with only a\nstderr line to say so, which --quiet and the CI integrations never see.\n\nA new optional root object, request_outcomes, records each narrowing or\nartifact request the run received: status (applied or not-applied),\naffects (scope or artifact), requested as the user spelled it, and for an\nunapplied entry a reason token and the sentence the CLI printed. It covers\n--changed-since falling back to the whole project, the diff filter\nstanding down, and a --sarif-file that could not be written. The object\nis absent when nothing was asked for, so no schema version moves, and the\nJSON root-prefix pass leaves it verbatim.\n\nDegraded health inputs (file scores, hotspots without git, shallow\nclones, an unpinned clock, ownership, unreadable trend snapshots) reach\nworkspace_diagnostics with degrades_analysis set. --group-by on a format\nthat cannot carry it now says so on every such format and in the comment\nand review bodies.\n\nThe GitHub Action publishes requests-unapplied and warns when the SARIF\nfile it was about to upload was never written; the GitLab template writes\nFALLOW_REQUESTS_UNAPPLIED. The MCP tools state the same facts.\n\nCloses #2687\nCloses #2688\nCloses #2689\nCloses #2690\nCloses #2691",
          "timestamp": "2026-09-19T09:51:52+02:00",
          "tree_id": "e9ed8aaf16c5a0cde9447e9781ea8054829fefbb",
          "url": "https://github.com/fallow-rs/fallow/commit/61275ff83c557589aac1b36638b95a83dbfc0e2b"
        },
        "date": 1789804948602,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.7,
            "unit": "%"
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
          "id": "40e9dfb659aefb38c8df28a626b222f1bae6e5cb",
          "message": "fix(cli): say on every artefact when a baseline went stale or cannot be judged (#2705)\n\nFollow-ups to the baseline staleness work in 3.27.0.\n\nThe sticky PR comment, the decision sidecar behind the Check Run and the\nGitLab MR note now carry the baseline advisory, so a reviewer sees a\nrotted baseline on the surface they read and not only in the job log.\n\nbaseline_staleness gains scope_reasons, which names the channels that\nnarrowed a run (diff, changed-since, changed-files, workspace,\nchanged-workspaces, scope, file, issue-type-filter, production), and\nunrecognised_format, which is true when the loaded file carries nothing\nthe running command writes into its own baselines. A baseline another\ncommand saved no longer reports zero entries and gates green in silence,\nwhile a baseline this command saved on a clean project is not flagged.\n\nA narrowed run with a non-empty baseline gains a recheck-baseline next\nstep when repeating the command without the narrowing can judge it. The\nstep is withheld for production and workspace scoping, and while\nFALLOW_DIFF_FILE is exported on a diff-scoped run.\n\nfallow audit's three baselines are judged and reported instead of\nrotting behind the changed-file scope. The Action publishes\nbaseline-scope-reasons and baseline-unrecognised; the GitLab template\nand the MCP tools state the same facts.\n\nCloses #2675\nCloses #2677\nCloses #2678\nCloses #2679",
          "timestamp": "2026-09-19T13:35:04+02:00",
          "tree_id": "4392418240b688d6c4cb66a997e74a2ac9387788",
          "url": "https://github.com/fallow-rs/fallow/commit/40e9dfb659aefb38c8df28a626b222f1bae6e5cb"
        },
        "date": 1789818268500,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.7,
            "unit": "%"
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
          "id": "86482cc66fe9a331d8477584be5bcba3f99e8923",
          "message": "docs(changelog): tighten the 3.27.0 and unreleased entries (#2728)\n\nRewrites the [Unreleased] and [3.27.0] sections in the format the earlier\nreleases use: one bold headline and one or two paragraphs per entry,\ngrouped under Added, Changed and Fixed.\n\nEvery fact, flag, JSON member, reason token, output name, contributor\ncredit and issue link is kept. The entries for #2675, #2677, #2678 and\n#2679 gain the closing links they were missing. Everything below\n[3.26.0] is byte-identical.",
          "timestamp": "2026-09-20T22:53:39+02:00",
          "tree_id": "179c61c59632fc4fb182e124d53099fe07a1d1b5",
          "url": "https://github.com/fallow-rs/fallow/commit/86482cc66fe9a331d8477584be5bcba3f99e8923"
        },
        "date": 1789938181924,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "95aa9c480480800bc80779509105ad9e4879e3fa",
          "message": "fix(nuxt): report unused convention files when the auto-import scan is off (#2702)\n\nWith autoImports: true the Nuxt plugin kept its convention entry patterns\nwhenever nuxt.config declared a components or imports key, because custom\nlayouts are not modelled. A config that only switches the scan off\n(components: false, components: [], components: { dirs: [] },\nimports: { scan: false }) was treated the same way, so components,\ncomposables and utils were never reported in a project that requires\nexplicit imports.\n\nThose literal shapes now count as the scan being off and the entry\npatterns are dropped, as with Nuxt's default config. Every other shape\nkeeps its entry patterns, including a lone imports: { autoImport: false },\na config that declares extends, and a non-empty imports.dirs next to\nscan: false. A project that combines autoImports: true with one of the\nrecognised shapes will see new unused-file findings.\n\nThanks @Tsuyoshi84 for the report.\n\nCloses #2695",
          "timestamp": "2026-09-20T22:47:44+02:00",
          "tree_id": "d622111a908439494035b34f665e79f384970a21",
          "url": "https://github.com/fallow-rs/fallow/commit/95aa9c480480800bc80779509105ad9e4879e3fa"
        },
        "date": 1789967031274,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "0ba3bc3644dea4474f37ca8b91d9aea497b41815",
          "message": "chore(deps): stop dependabot from bumping cssparser ahead of lightningcss (#2729)\n\nlightningcss 1.0.0-alpha.72 requires cssparser ^0.37 and types its public\nParse trait against it without re-exporting the crate, so the direct pin\nhas to equal the version lightningcss resolves. Bumping it alone adds a\nsecond cssparser to the graph and CssColor::parse stops compiling.\n\nIgnore cssparser in the root cargo entry until lightningcss releases on a\nnewer version, then move both pins together.\n\nSupersedes #2664.",
          "timestamp": "2026-09-21T07:04:44+02:00",
          "tree_id": "0ed657e5d140a8445e887fa9056bde24042ab604",
          "url": "https://github.com/fallow-rs/fallow/commit/0ba3bc3644dea4474f37ca8b91d9aea497b41815"
        },
        "date": 1789967682461,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "f6265843f78ec62f6168c2dc0d8097b407946f0e",
          "message": "chore(scripts): make the local repository checks pass outside CI (#2742)\n\nThe lint, agent adapter and companion parity checks failed on a developer machine for environment reasons while CI stayed green. The corpus driver imports the converter by name, adapter ownership is judged by tracked content (generate mode no longer deletes untracked local skill directories), and the parity checks resolve companion checkouts from the main checkout and only skip when the guessed default is absent.\n\nCloses #2741",
          "timestamp": "2026-09-21T12:18:02+02:00",
          "tree_id": "90e3a2d9178543efbc9370784997e77ef3c771c6",
          "url": "https://github.com/fallow-rs/fallow/commit/f6265843f78ec62f6168c2dc0d8097b407946f0e"
        },
        "date": 1789986429814,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "57366bbeba411e39536f9fcc106c639516c5eaa7",
          "message": "ci(release): stage the fallow npm root for maintainer approval (#2744)\n\nThe release workflow stages the fallow npm root with npm stage publish instead of publishing it. The fallow trusted publisher grants stage publish only, so the version becomes installable when the maintainer approves the stage with npm 2FA before the signed tag, after comparing the staged bytes with the npm-tarballs artifact of the same run. Every other package keeps publishing over OIDC. The pinned npm moves to 11.19.0.",
          "timestamp": "2026-09-21T13:17:39+02:00",
          "tree_id": "358676d7ed3bc237258ac029ab0990a84c91df99",
          "url": "https://github.com/fallow-rs/fallow/commit/57366bbeba411e39536f9fcc106c639516c5eaa7"
        },
        "date": 1789990019186,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "731a504da976f1a8f4ce08bf08c2defd961e5f44",
          "message": "feat(output): carry the request outcomes and skip causes four commands left on stderr (#2743)\n\nfallow flags and fallow suppressions carry request_outcomes with the changed-since entry. An applied diff-filter request carries scope_size, so a diff with no added lines reads as an empty scope on every consumer: the JSON envelope, the Action and GitLab logs, the MCP warnings, and the comment, review and summary bodies. hotspots-skipped carries a cause for a malformed --since and an unreadable churn file as well as a missing repository. fallow security --sarif-file carries the sarif-file entry. Exit codes are unchanged.\n\nCloses #2734",
          "timestamp": "2026-09-21T13:27:22+02:00",
          "tree_id": "ec39db9de0c07dfd63efca0e42efec8559ece446",
          "url": "https://github.com/fallow-rs/fallow/commit/731a504da976f1a8f4ce08bf08c2defd961e5f44"
        },
        "date": 1789990870830,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "d4264c27859c6b307fbebd269c969c0b107984ee",
          "message": "fix(plugins): credit bundler entry packages and close the Nuxt autoImports precision gaps (#2745)\n\nA package named in a webpack, rspack or rsbuild entry, with or without a resource query, is credited as a dependency instead of becoming an entry pattern that matches nothing; glob-shaped entries stay entry patterns.\n\nNuxt autoImports: components under global/ and islands/ get the name Nuxt gives them, a config with a computed key, accessor or top-level spread keeps the convention entry patterns for that root, each workspace root is classified on its own, components: true, imports: {} and imports: { dirs: [] } count as the defaults they are, and a name imported or re-exported from #components or #imports is credited. #layers/<name>/ resolves as a path alias.\n\nCloses #2739\nCloses #2737",
          "timestamp": "2026-09-21T14:15:43+02:00",
          "tree_id": "38b1e5a0bba36bb6b69c4d828010e925328fe1ba",
          "url": "https://github.com/fallow-rs/fallow/commit/d4264c27859c6b307fbebd269c969c0b107984ee"
        },
        "date": 1789993530458,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "4cec981c7cafc6d6d6f87127011c58b021fc23e0",
          "message": "docs(release): let the preflight name repository secrets instead of blocking on them (#2747)\n\nGitHub never returns a secret value, so a publication secret can move into the release environment only when its value is entered again, in practice at its next rotation. The preflight now passes with a note that names each secret still at repository level as unprotected by the environment, and fails on a secret that exists at both levels or at neither.",
          "timestamp": "2026-09-21T23:45:25+02:00",
          "tree_id": "d4eb00354d3b74eec4d55af981d273adedf965c5",
          "url": "https://github.com/fallow-rs/fallow/commit/4cec981c7cafc6d6d6f87127011c58b021fc23e0"
        },
        "date": 1790028141486,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "7e2e7ad99634f2c7ccd359a6a3ea33b162e3d0e6",
          "message": "feat(plugins): record a plugin config fallow cannot read as a workspace diagnostic (#2748)\n\nA framework plugin that cannot read a config statically records a workspace_diagnostics entry. plugin-config-unreadable covers a Module Federation exposes or remotes key that is not a static object literal; it sets degrades_analysis, so the Action, the GitLab template and the MCP warnings report degraded inputs. plugin-effect-not-modeled covers a Nuxt autoImports surface that kept its convention entry patterns; it does not degrade the analysis and prints nothing. Both carry plugin, key and reason, with the config file in path. The stderr line for an unreadable key gains the fallow: prefix. Exit codes are unchanged.\n\nCloses #2736",
          "timestamp": "2026-09-22T00:28:05+02:00",
          "tree_id": "ac5030a20efa7a218729cb54898dbf043b672274",
          "url": "https://github.com/fallow-rs/fallow/commit/7e2e7ad99634f2c7ccd359a6a3ea33b162e3d0e6"
        },
        "date": 1790030941704,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "234d24c8d1d8cb34bd22f166cddf48fd5dd59191",
          "message": "feat(baseline): give saved baselines a kind and report an unrecognised one on every surface (#2749)\n\nEach saved baseline carries a top-level kind: dead-code, dupes or health. A baseline that another command saved loads as zero entries on all three commands, warns on stderr with both kinds and the path, and carries baseline_staleness.unrecognised_format. fallow dead-code no longer exits 2 on such a file. The stale-baseline gate trips on it, so --fail-on-stale-baseline fails the run, and gate_trips carries that rule. A save over a baseline of another kind is refused with exit 2. Audit reports every baseline it cannot read. The pull-request comment and the merge-request note carry the unrecognised-baseline sentence, the combined run gets the recheck-baseline next step, and the live github-summary render carries the gate outcomes.\n\nCloses #2738\nCloses #2735",
          "timestamp": "2026-09-22T01:11:08+02:00",
          "tree_id": "ce61633c08d0dda3ca253d3c7651587e7a4855e1",
          "url": "https://github.com/fallow-rs/fallow/commit/234d24c8d1d8cb34bd22f166cddf48fd5dd59191"
        },
        "date": 1790032832256,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "c3197ecf8cd63e1a3ea094d88f36d5c0ca9b88ba",
          "message": "feat(plugins): read Module Federation options wherever a config declares them (#2750)\n\nThe Module Federation reader finds a plugin call anywhere in a bundler config: in a nested plugin array, in a plugins variable, under tools.rspack.plugins, in an rsbuild appendPlugins hook, and in a Next.js webpack(config) hook. next.config.* is read with NextFederationPlugin as a recognised callee. The array form of exposes is read; the array form of remotes keeps its array-form diagnostic. Options that a same-file const holds are read when the program has one binding of that name and never writes to it. An array element with glob syntax, a nested array or a non-string element is reported under unreadable-entries.\n\nRefs #2698",
          "timestamp": "2026-09-22T01:50:50+02:00",
          "tree_id": "6ea057c1b237d7a18463f64bb290803a317825c1",
          "url": "https://github.com/fallow-rs/fallow/commit/c3197ecf8cd63e1a3ea094d88f36d5c0ca9b88ba"
        },
        "date": 1790034985966,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "f9fb3f9719b757df756d425302a5898e34fc1258",
          "message": "refactor(cli): resolve the base analysis root and the short HEAD through the engine (#2751)\n\nOne engine helper resolves the base analysis root and canonicalizes both sides. The two CLI copies are removed, and the short HEAD probe has one implementation. The security copy did not canonicalize the root; every entry point resolves the root before the analysis starts, so the widening did not reach a run. fallow security --base warns on stderr when the base root is remapped. The CLI boundary test fails on a reintroduced git probe for ref or root detection. Discarded stderr in the Action and GitLab scripts is replayed as debug lines, and a green run gains no log line. The MCP tests no longer read FALLOW_CHANGED_SINCE from the environment.\n\nCloses #2740",
          "timestamp": "2026-09-22T03:23:11+02:00",
          "tree_id": "3d196158c9a4781ba7c9bec79d6923f5cd38e429",
          "url": "https://github.com/fallow-rs/fallow/commit/f9fb3f9719b757df756d425302a5898e34fc1258"
        },
        "date": 1790040732870,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "b04ec44d560eb2375ef9a675df05d1c05a75c210",
          "message": "docs(changelog): edit the unreleased entries for the 3.28.0 release (#2759)\n\nEach unreleased entry is a headline and two short paragraphs. Three sentences were corrected against the code: dead-code reports an unrecognised baseline instead of exiting 2, the baseline check reads kind before the keys, and the stderr prefix belongs to the plugin-config-unreadable kind. The issue links are unchanged.",
          "timestamp": "2026-09-22T03:42:04+02:00",
          "tree_id": "7e1f801d0547ae375502c1de2568b7c7629deed1",
          "url": "https://github.com/fallow-rs/fallow/commit/b04ec44d560eb2375ef9a675df05d1c05a75c210"
        },
        "date": 1790041668523,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "bd8fca5af5df4ccfd94c7a835d17bd93c31a7cef",
          "message": "chore: release v3.28.0",
          "timestamp": "2026-09-22T04:43:46+02:00",
          "tree_id": "4213b6d9d0adfa274094c8bf54443252987a4ccc",
          "url": "https://github.com/fallow-rs/fallow/commit/bd8fca5af5df4ccfd94c7a835d17bd93c31a7cef"
        },
        "date": 1790045726543,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "6a60a9573d4fa716d1f2d8ec41d52c18b07d4c1c",
          "message": "docs(security): describe the OIDC publication path and the staged fallow root (#2760)\n\nSECURITY.md gains a Publication path subsection: OIDC trusted publishing without long-lived registry tokens, credential-bearing jobs in the main-only release environment, and the fallow npm root that CI can only stage while the maintainer approves it with npm two-factor authentication after a byte comparison. The rotation and compromise sections are aligned with that.",
          "timestamp": "2026-09-22T08:40:07+02:00",
          "tree_id": "f5a8ae1d6f1726cdc7156ae3ef33f7232ecfc38a",
          "url": "https://github.com/fallow-rs/fallow/commit/6a60a9573d4fa716d1f2d8ec41d52c18b07d4c1c"
        },
        "date": 1790059786979,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "cef0e55a9442f7246543189b4903e8cea255346e",
          "message": "docs(changelog): say the 3.28.0 entries in plain English (#2761)\n\nThe 19 entries under 3.28.0 use plain developer English. Every fact, link, code span and heading stays.",
          "timestamp": "2026-09-22T09:51:49+02:00",
          "tree_id": "0f3629ce027cb93b1260b1eaff2e949ca6dbfafc",
          "url": "https://github.com/fallow-rs/fallow/commit/cef0e55a9442f7246543189b4903e8cea255346e"
        },
        "date": 1790063803057,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "9667d803f993216650970d17684ef0da0836f987",
          "message": "ci(release): publish the VSIX only after the approved fallow root is public (#2762)\n\nThe VS Code extension downloads its binary from the GitHub Release of its own version and purges an installed binary of another version, so a VSIX that is public before that release exists breaks auto-updating installs. Both VSIX publishers and release-ready now wait for npm-root-approved, a credential-free job that polls the public registry until the maintainer-approved fallow root is public and requires the public tarball's sha256 to equal the digest npm-publish recorded for the tarball it staged. Crates and the direct npm publishes stay ahead of the approval.",
          "timestamp": "2026-09-22T10:18:36+02:00",
          "tree_id": "46565911f98c7d629c651fb11314ad650b0dc5dc",
          "url": "https://github.com/fallow-rs/fallow/commit/9667d803f993216650970d17684ef0da0836f987"
        },
        "date": 1790065549497,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "d541fa809c6fcf73ab3dda4011c8a2d6763fe11a",
          "message": "fix(vscode): keep the installed binary while the extension's release is not published yet (#2764)\n\nA managed binary that passed signature or digest verification but reports another version stays on disk until a download of the matching version replaces it; only a failed verification purges. While the GitHub Release of the extension's own version answers HTTP 404, the extension keeps serving the installed binary, says so once per session without a modal, and with fallow.autoDownload on retries in the background with a bounded backoff and offers a restart when the new version has landed. Without an installed binary the download prompt still appears and names the missing release.",
          "timestamp": "2026-09-22T11:13:28+02:00",
          "tree_id": "3609d6f64d8a9a9d1413c2d107ca25e17b9998df",
          "url": "https://github.com/fallow-rs/fallow/commit/d541fa809c6fcf73ab3dda4011c8a2d6763fe11a"
        },
        "date": 1790068974283,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "12c5c81929790cf59609323a7170a286fa99ab6b",
          "message": "fix(security): require complete URL authority boundaries (#2763)",
          "timestamp": "2026-09-22T11:30:59+02:00",
          "tree_id": "60f9a70a520fa2ae0ac58ed46ca680dda7535608",
          "url": "https://github.com/fallow-rs/fallow/commit/12c5c81929790cf59609323a7170a286fa99ab6b"
        },
        "date": 1790069901670,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "7c173685891c5f244fd5b31575a4b704b67bd9f0",
          "message": "feat(security): record exact-origin guard observations (#2765)",
          "timestamp": "2026-09-22T10:26:41Z",
          "tree_id": "61650fc87910fbb7f77f5f181cdc3120d45540bc",
          "url": "https://github.com/fallow-rs/fallow/commit/7c173685891c5f244fd5b31575a4b704b67bd9f0"
        },
        "date": 1790073443605,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "d4742dac6e20450de88df7b3d89895d97eaee84c",
          "message": "fix: distinguish security findings on the same line (#2766)",
          "timestamp": "2026-09-22T13:20:07+02:00",
          "tree_id": "11dedd2531ee6aa01f12bd8cdeb50e1afdbcd069",
          "url": "https://github.com/fallow-rs/fallow/commit/d4742dac6e20450de88df7b3d89895d97eaee84c"
        },
        "date": 1790076602097,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "cd91addff1a7060d0e09cd101a25e78c2bd0fd2e",
          "message": "fix(security): ignore constant writeHead header objects",
          "timestamp": "2026-09-22T13:42:25Z",
          "tree_id": "520484eb8abb37d6981edf8e3f8a9b3247d3762e",
          "url": "https://github.com/fallow-rs/fallow/commit/cd91addff1a7060d0e09cd101a25e78c2bd0fd2e"
        },
        "date": 1790085259788,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "ecb35b4f0e2e29cf0f69078ae8af37227eaf6915",
          "message": "docs: add proof levels and test evidence rules to the maintainer lifecycle (#2784)\n\nimplement resolves open decisions before editing: facts are looked up, observable behavior is probed on a public fixture, and only real decisions go to the maintainer in one round. review compares runtime behavior on public projects against the latest release, filters reviewer findings before acting on them, and records dismissed findings. ship treats a second identical CI failure as a defect and checks the base before a retry. quality-gates.md now holds the proof levels, the test evidence rules and the behavior comparison.",
          "timestamp": "2026-09-23T11:07:08+02:00",
          "tree_id": "92ae1a0877a2d0e22a2a744e90b93078cc4cde5a",
          "url": "https://github.com/fallow-rs/fallow/commit/ecb35b4f0e2e29cf0f69078ae8af37227eaf6915"
        },
        "date": 1790155039414,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.8,
            "unit": "%"
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
          "id": "55c5faf0cccfc284531b70d3b67ddedc1c1e0e5b",
          "message": "refactor: remove dead code, shadow tests and duplicate helpers (#2786)\n\n* refactor(extract): remove duplicate helpers and dead consumer collectors\n\nRemove code that repeats other code or that no code path runs:\n- Remove the CssInJs ConsumerCollector and StyleXThemeGroupCollector.\n  The batched collector handles these queries.\n- Remove the four single-query token consumer wrappers from the public\n  fallow-extract API. The engine calls css_in_js_consumer_scan only.\n- Merge the JSDoc bare-tag checks into one helper.\n- Remove the duplicate visitor identifier helpers.\n- Share the Astro and SFC script complexity remap.\n- Inline the semantic-fact wrappers that have one caller.\n\nReplace the bump history on CACHE_VERSION and DUPES_CACHE_VERSION with a\nshort rule. Git history and the CHANGELOG keep the reasons.\n\nTests:\n- Build cache fixtures from ModuleInfo::empty instead of full literals.\n- Put the visibility roundtrip cases in one table test.\n- Replace the regex smoke tests with a test that compiles the literal of\n  every static_regex call.\n- Point the Svelte dollar-ref tests at the production bound-target\n  functions.\n\n* refactor(graph,config,types): remove test-only copies and dead accessors\n\nNamespace narrowing tests now call the production *_at_site markers\nwith a real effective export index. The cfg(test) copies skipped the\ndeclaration-slot guard. A new test pins that a value member access\ndoes not credit the type-only slot of the same name.\n\nThe levenshtein helper moves to fallow-types. fallow-config re-exports\nit, so the suppression parser and the config consumers use one copy.\n\nRemove public methods that have no production caller:\ncodeclimate_enabled, sarif_enabled, suppressed_kind, has_local_export,\nset_entry_point, physical_reference_count, preset_name and the\npublic_export_declaration_bindings wrapper. leading_command_token\nmoves into the task_matrix test module.\n\nRemove tests that only assert derived equality, hash, clone, Debug or\nstruct field readback in fallow-types and graph::types.\n\n* refactor(core): remove dead helpers and test-only shadows\n\nRemove deprecation attributes from detector helpers in private\nmodules. Keep them on the public analyze entry points and on\nfeature_flags.\n\nReduce FallowError to the config case. The E004 display text and the\ncode, help and context getters do not change.\n\nDelete helpers that have no callers: the non-kinded nested alias and\nstring-pair extractors, kind_to_kebab, extend_test,\nbuiltin_plugin_config_candidate_basenames, and the test-only source\nmatcher and import-location copies. Their tests now run the production\nfunctions. Move the test-only script analysis wrappers into the tests.\nUpdate the tainted-sink comment to name the production source matcher.\n\nUse one borrowed path index for public API entry points, and skip the\nsuppression filters when a result list is empty. Replace duplicate\nextension membership tests with one exact-set test. Use tempfile\ndirectories in the supabase and cache tests. Remove a dead document\nlink from the fallow-v8-coverage crate docs.\n\n* refactor(engine,api,output): remove unused entry points and fix trace path lookup\n\nTrace lookups now prefer an exact root-relative match over a nested\nsuffix match. Before this change, a trace of src/a.ts could select\npackages/x/src/a.ts in a monorepo. An ambiguous short path still takes\nthe first suffix match. All trace sites use one lookup built on\nmatching_module_indexes. path_matches is removed.\n\nDuplicate detection has one internal entry point with optional focus\nfiles and an optional cache. It replaces six thin wrappers. The clone\nfinding with_actions constructors and fingerprint_for_fragment are\nremoved. The fragment fingerprint did not match the report fingerprint.\n\nAlso removed:\n- functions with no caller in editor, session, workspace_scope and\n  gate_outcomes\n- the unused ci_output re-exports and four unused type-aware aliases\n  from fallow-api\n- the port scaffolding in the engine trace modules and the test-only\n  shim modules\n\nThe docs URL builders now share issue_contract::rule_docs_url. The\nhealth runner docs now describe the current design.\n\n* refactor(cli): remove duplicate upload plumbing and test-only shadows\n\nMove the shared error type, exit mapping, dirty-tree gate, message\nformat, count format, endpoint display and path helper of the two\nSHA-keyed coverage uploaders into upload_common. Exit codes and\nmessage text do not change.\n\nMake the fix plan root a required path, so fix tests run the same\ncontainment check as real fixes. Delete the test-only JSONC comma\nstripper, the test-only _meta insert helper, the fingerprint forwarder\nmodule and CLI tests that repeat owner-crate tests.\n\nSecurity JSON renderers now return an error. A serialization failure\nprints the documented error envelope and exits 2.\n\nThe shared test runners now remove FALLOW_COVERAGE and\nFALLOW_COVERAGE_ROOT for every command.\n\n* refactor(mcp,lsp,napi): remove unobserved MCP fallback reasons and weak tests\n\nChange the MCP CLI fallback helpers to bool predicates. Delete the\nCliFallbackReason enum, which no caller reads, and update the MCP\ninternals reference to name the new predicates. Add one type-aware\npredicate for analyze, audit and health. Move the duplicated mode,\nmin_occurrences and non-empty helpers into tools/mod.rs.\n\nMake spawn_fallow always tag the tool. Move the subprocess tests to the\ntagged run_tool entry points that production uses.\n\nReplace the substring-based MCP schema tests with one table that checks\ntop-level properties and required fields. Delete the redundant\ncross-tool argument tables, the tautological edge-case tests and the\nloose description tests.\n\nPin the hover and diagnostic text for the eight named-anchor LSP kinds.\nThen build these hovers and diagnostics with shared anchor helpers.\nDelete the LSP issue-type forwarding wrapper and the copied key list.\n\nDelete the napi tests that only exercised fallow-api, and keep the\nInvalidArg status check on the real detect_duplication export.\n\n* refactor(vscode): remove unused tree badge code and duplicate helpers\n\nRemove the no-op badge code from the health, security and runtime\ncoverage tree providers. The dead-code view keeps its count badge.\n\nExport the structured CLI error guard once from cli-args-utils.\nThe coverage gate message now uses describeAnalysisFailure, so an\nempty envelope message shows the fallback text (new test).\n\nMove the first-workspace-folder lookup into workspaceRoot.ts.\nRemove the compareVersions re-export, the BuiltAnalysisArgs alias,\nthe unused setMutedCategories method and a stale suppression.\nBuild the item icon map from the category map with three overrides.\n\nThe commands test now runs the real configured-binary resolver\nagainst the mocked file system. Remove duplicate or tautological\ntests in the security tree, treemap and review adapter suites.\n\n* chore(tooling): remove dead scripts and share bench fixtures\n\nMake the wrapper trap parity checks in ci/tests/run.sh able to fail.\nThe old `grep -c ... || echo 0` pattern gave \"0\\n0\" on no match, so a\nmissing trap read as present. Each trap must now exist in both wrappers.\nThe structured-error check now matches the \"$RESULTS_FILE\" form in the\naction wrapper.\n\nMove the fixture builders that are the same in the programmatic_commands\nand programmatic_stable benches into benches/support/mod.rs. Bench\nnames do not change.\n\nRemove scripts/generate-readme-screenshot.sh, which writes an image that\nnothing uses. Remove the generate:all aliases of generate:contracts.\nRemove export from helpers that only their own module uses.\n\n* refactor(graph): remove test-only resolver reuse predicate and cache version history\n\nThe tests now assert on classify_resolution_mismatch, which is the\npredicate that production runs, instead of a separate copy of it.\nThe GRAPH_CACHE_VERSION doc keeps the bump rules and the rule not to\nreuse a published version number. Git history and the CHANGELOG\nrecord the reason for each bump.\n\n* refactor(output,api): remove the single-variant root envelope mode parameter\n\nRootEnvelopeMode had one variant, Tagged, and every root serializer\nignored it. Remove the enum, the mode parameter on the root serializers\nand apply_root_kind, the envelope_mode fields on the fallow-api output\nstructs, and the helpers that returned the constant. Update the cli,\nmcp, napi and bench call sites.\n\nThe body of apply_root_kind does not change, so the top-level kind\ndiscriminator and the JSON output stay the same. Remove the boundary\ntest that made the CLI route through the removed helper. The guard that\nblocks the legacy mode stays.\n\n* ci: remove the GitLab jq summary renderers\n\nThe GitLab template stopped loading ci/jq when the MR comment and\nreview scripts moved to the typed Rust formats. Only tests and guards\nstill read these files.\n\n- Delete ci/jq/ and the fixtures that only its tests read.\n- Remove the GitLab summary, parity and markdown sections from\n  ci/tests/run.sh. Keep the drift guard helper assertions.\n- Reduce check-ci-summary-rows.mjs to the action/jq table. Keep the\n  check for rows that are not in the registry, and add a unit test\n  for it.\n- Remove the ci/jq cases from the issue_meta.rs table tests.\n- Fix a doc comment that named jq files that do not exist.\n- Update repo-map.md and the CI reviewer file.\n\n* refactor(npm): share the binary list and skip check between install and lazy verification\n\nThe lazy verification sentinel and the signature check each kept their own\nlist of platform binaries. They also kept their own copy of the skip\nenvironment check. The sentinel must bind the same binaries that the\nsignature check verifies. lazy-verify now uses the exported list and the\nexported skip check from verify-binary. The sentinel format does not change.\n\n* refactor(graph): remove the unused path index and query methods from ProjectState\n\nAnalysis setup builds ProjectState once and reads only files() and\nworkspaces(). The path_to_id map and the six query methods had callers\nonly in tests and benches. The map also cloned every discovered path on\neach run.\n\nRemove path_to_id and these methods: id_for_path, stable_key_for_file,\nid_for_stable_key, workspace_for_file, workspace_by_name and\nfiles_in_workspace. Keep new, files, workspaces, file_by_id and the\ndense FileId check. The struct doc now states only what the struct does.\n\nThe workspace integration test now asserts on workspaces(), files() and\nfile_by_id(). The discovery determinism test stays unchanged.\n\nThis removes two CodSpeed bench IDs from the baseline list:\ncomponent_graph_project_state_lookups and\ncomponent_graph_project_state_workspace_queries. Construction is now a\nmove, so component_graph_project_state_build also reads files() and\nworkspaces(). Without these reads, Criterion reports zero time. Its\nbaseline shifts for this reason.\n\n* test(cli): share one isolated git helper across integration tests\n\nAdd git_command, git, git_capture and commit_all to tests/common. The\ncommand clears the repository variables that a git hook sets (GIT_DIR,\nGIT_WORK_TREE, GIT_INDEX_FILE and others), ignores the global and system\ngit config, and uses a fixed author and committer.\n\nRemove the file-local git and commit copies. Some copies removed no\nrepository variables, some kept the global config, and one committed\nwith the global signing config active. Tests that ignored git failures\nnow fail at the git call.\n\n* test(api,lsp): test the editor merge logic in fallow-api\n\nMove the all-fields merge test, the percentage recompute test and the\nzero-lines test from the LSP crate to the EditorAnalysisOutput tests in\ncrates/api/src/editor.rs. The exhaustive fixture moves with them and\nkeeps the #444 compile guard.\n\nRemove five weaker LSP merge tests. The moved tests cover their\ncontract. Remove the two test-only merge helpers in the LSP analysis\nmodule and the unused imports.\n\nThe api tests now fail when merge_duplication does not recompute the\nduplication percentage.\n\n* refactor(engine,api,cli): share one CI check and one opt-out gate\n\nAdd fallow_engine::ci_env::is_ci() and use it in place of the four local\ncopies in the api next steps, telemetry, the update check and the cache\nnotice. The cache notice now reads update_check::env_disabled, so one\nopt-out always silences both notices. The update check cache path now\nbuilds on telemetry::config_dir. Environment variable names and values\ndo not change.\n\n* ci: remove the allocation baseline gate that has no baseline\n\nThe repository never contained alloc-baseline.json. The allocs workflow\nexited before it compared against a baseline, so the gate never ran.\nDelete scripts/alloc-check.sh, the compare block and the path filters\nfor the missing file. The store-benchmark step keeps the regression\nalerts at the 120% threshold.\n\n* test: clear ambient git state in crate test git helpers\n\nTest helpers that run git init, git config and git commit now remove\nGIT_DIR, GIT_WORK_TREE and GIT_INDEX_FILE (and the other ambient\nvariables where the engine helper is available). Without this, a test\nrun inside a git hook or a `git rebase -x` step writes config, index\nentries and commits into the enclosing repository.\n\n- engine tests use the engine git runners\n- api, cli and bench tests use clear_ambient_git_env\n- mcp tool tests share one scrubbed runner in base_root_fixture.rs\n- the lsp test helper removes the three variables directly\n\n* docs(core,engine): fix stale paths in coupling concentration docs\n\nThe render fan-in concentration doc named crates/cli/src/vital_signs.rs,\nwhich does not exist. The engine doc named crate::render_fan_in and\nhealth/mod.rs for code that is in fallow_core analyze::render_fan_in and\nhealth/vital_data.rs. Both docs now name the correct paths and say to\nchange the shared p95 and floor math together.\n\n* refactor(cli): share envelope and provider helpers of the ci post commands\n\nThe review and PR comment post commands had their own copies of the\nenvelope fingerprint helpers and the GitHub and GitLab target lookups.\nThe variable precedence must stay the same for both commands, so the\nlookups now live once in ci.rs. A purpose argument keeps each error\nmessage unchanged. Tests assert the exact messages and the precedence\nof the repository sources.\n\n* refactor(api): remove the private fingerprint_hash forwarder\n\nCall codeclimate_fingerprint_hash directly in the dead-code CodeClimate\noutput, as the health and duplication outputs already do. The hash input\nis the same, so the fingerprints do not change.\n\n* refactor(mcp): build duplication mode error text from VALID_DUPES_MODES\n\nThe dupes, trace_clone and code mode combined validators restated the\nmode list as a literal in their error text. They now join\nVALID_DUPES_MODES, so the list has one definition. The error strings\ndo not change.\n\n* chore: remove private repository references from public code\n\nRewrite the sidecar key comments, one test assertion message and the\nstatic-findings limit comment without private paths or workflow names.\nPoint the test-key comment at TEST_SIDECAR_SEED in\ncrates/cli/tests/common/sign.rs. Remove a private decision-record\ncitation from the static-findings upload module doc. Remove private\nissue citations from the runtime-coverage schema doc comments and\nregenerate the output contracts.\n\n* refactor(types): share one root-relative display path helper\n\nAdd fallow_types::path_util::display_relative and use it in types,\nconfig, core, api and cli in place of the private copies. The\nper-instance and aggregated workspace messages must format paths the\nsame way, and one helper now keeps that rule. The output does not\nchange. The discovery note keeps its \".\" mapping for the project root.\n\n* docs: fix stale source paths in reviewer guidance and analyzer docs\n\nThe rust-reviewer attribution check named files that no longer exist,\nso its grep loop reported a false MISSING for every issue type. Point\nit at crates/engine/src/changed_files.rs and crates/api/src/audit_keys.rs,\nand rename the loop variable so it does not overwrite PATH in zsh.\n\nAlso correct the generated output contract path in vscode-reviewer, the\nRuleDef location in analyzer-authoring, and the CODEOWNERS parser path\nin the codeowners test doc comment.\n\n* docs(changelog): note the monorepo trace path fix\n\n* test(lsp): pin the exact diagnostic code list\n\nEditors save code filters by the diagnostic codes that the LSP\npublishes. Compare the full, ordered list from diagnostic_issue_types()\nagainst a fixed list. A code that disappears then fails the test and\ndoes not silently leave the editor filters.\n\n* fix(engine): drop a test import that only unix tests use\n\nThe repo_refs tests import std::process::Command, but only the\nunix-gated mkfifo test uses it after the git helper change. Windows\nclippy rejects the unused import. Qualify the one call instead.",
          "timestamp": "2026-09-23T13:01:25+02:00",
          "tree_id": "2e6bf881a4e9718e89c78d62c3631e4cfbb31d86",
          "url": "https://github.com/fallow-rs/fallow/commit/55c5faf0cccfc284531b70d3b67ddedc1c1e0e5b"
        },
        "date": 1790162443683,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.9,
            "unit": "%"
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
          "id": "c5ba8022d3a7250ede4b9064d9d9715e148e52e7",
          "message": "docs: add evidence, triage and CI rules to the maintainer skills (#2788)\n\ndebug-false-positive saves one reproducing command before any change, ranks falsifiable hypotheses and stops after three failed fixes. address-pr-comments checks each comment against the code, sorts findings and drafts replies for approval. triage-issue reproduces on current main and gives each cause claim a confidence level. fix-gh-actions classifies failures and treats a second identical failure as a defect. perf-loop proves the benchmark sees the problem first. The review-board and session-audit skills filter findings and lessons, and slop-audit flags surprise-explaining comments instead of deleting them.",
          "timestamp": "2026-09-23T13:26:27+02:00",
          "tree_id": "1117ab88bb0d17e4bf8fb46d549e8fefa27fc012",
          "url": "https://github.com/fallow-rs/fallow/commit/c5ba8022d3a7250ede4b9064d9d9715e148e52e7"
        },
        "date": 1790163216143,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.9,
            "unit": "%"
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
          "id": "2f7c2fbbd2ab52f3c57df2518e28a87cd0119b1e",
          "message": "test: add a drift contract and a cross-surface harness (#2787)\n\nAdd docs/development/drift-contract.md and a proptest harness in crates/cli/tests/drift that compares CLI, MCP and fallow_api results on generated projects. The drift CI job and release validation run it.",
          "timestamp": "2026-09-23T13:55:00+02:00",
          "tree_id": "f705533578aa404edc3355642f366bc5813f8fdc",
          "url": "https://github.com/fallow-rs/fallow/commit/2f7c2fbbd2ab52f3c57df2518e28a87cd0119b1e"
        },
        "date": 1790164874653,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.9,
            "unit": "%"
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
          "id": "a760767376cedb7b6ef3aa15e24177c2e3251b0b",
          "message": "chore: re-include .claude/ so tracked adapters stay visible to git add (#2789)\n\nA global .claude/ ignore hides the whole directory, so git add skips new\ngenerated skills and agents under .claude/ even though .gitignore\nre-includes them. Re-including .claude/ first restores the intended\nrules; local settings and worktrees stay ignored.",
          "timestamp": "2026-09-23T14:03:24+02:00",
          "tree_id": "4361bca0e83178a3728e9423a3ccd43c655ccff3",
          "url": "https://github.com/fallow-rs/fallow/commit/a760767376cedb7b6ef3aa15e24177c2e3251b0b"
        },
        "date": 1790165421724,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.9,
            "unit": "%"
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
          "id": "6cd8d83e04143c28885c81805ec73320872814c9",
          "message": "fix: harden failure handling and remove redundant state (#2726)",
          "timestamp": "2026-09-23T14:11:09+02:00",
          "tree_id": "2bf6d8b65caec54284e07087786a9ae4df326b16",
          "url": "https://github.com/fallow-rs/fallow/commit/6cd8d83e04143c28885c81805ec73320872814c9"
        },
        "date": 1790166103955,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.9,
            "unit": "%"
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
          "id": "4fc827f3e54f6e5703685dff91d396eb83095dd9",
          "message": "fix: run one audit implementation for the CLI, MCP and API (#2792)\n\nThe CLI audit, the MCP audit tool and fallow_api::run_audit now share one implementation in fallow-api. Audit reports dependency findings only when the manifest changed, and it runs a real base analysis when a changed file holds a suppression or visibility marker. The drift harness checks I4 and I5.",
          "timestamp": "2026-09-23T14:59:16+02:00",
          "tree_id": "9252f37c097cdb8f14077ee0bcb60e7dc98f9eaa",
          "url": "https://github.com/fallow-rs/fallow/commit/4fc827f3e54f6e5703685dff91d396eb83095dd9"
        },
        "date": 1790169267714,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.9,
            "unit": "%"
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
          "id": "c69751ce13d0e2c9fc2e76872217bc7cf44b81df",
          "message": "fix(cli): say in runtime coverage help that a single capture is free (#2809)\n\nThe --runtime-coverage help of audit and security called the input paid,\nbut a single local capture runs without a license. Use one note on every\nruntime coverage input: a single local capture is free, and continuous or\nmulti-capture monitoring needs a license. Say that upload-inventory needs\na fallow cloud API key, and remove the path of a file that is not public\nfrom the coverage help.",
          "timestamp": "2026-09-23T16:20:22+02:00",
          "tree_id": "243e3c75ce820a98a99bb82a14cfeb9e05a1ad88",
          "url": "https://github.com/fallow-rs/fallow/commit/c69751ce13d0e2c9fc2e76872217bc7cf44b81df"
        },
        "date": 1790174049961,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.9,
            "unit": "%"
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
          "id": "5862f7ef099a155809269f394d5c1433197f9448",
          "message": "perf: compile guard rule scopes once and drop repeated boundary work (#2811)\n\n- guard: compile the files and exclude globs of each rule-pack rule once\n  per report. Before, guard compiled them again for each target file.\n- rule packs: validate zone references against the expanded zone names.\n  The check no longer compiles the zone globs, and it skips the boundary\n  expansion when no rule has a zones scope.\n- trace: compare module paths through a borrowed forward-slash view. The\n  lookup no longer allocates two strings per module.\n- boundaries: build one glob candidate per path in classify_zone instead\n  of one per glob.\n- re-export cycles: Tarjan SCC no longer allocates a vector for each\n  single-node component. An acyclic barrel chain has one per node.",
          "timestamp": "2026-09-23T17:25:10+02:00",
          "tree_id": "aecc16cd17f5211cbb9417a08a0be6a78501d468",
          "url": "https://github.com/fallow-rs/fallow/commit/5862f7ef099a155809269f394d5c1433197f9448"
        },
        "date": 1790177903678,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.9,
            "unit": "%"
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
          "id": "d6e4fdf5b104402b51e69e8d767e9d78c9ba84b3",
          "message": "fix: apply combined baselines and state the verdict in every JSON envelope (#2810)\n\nBare fallow applies --dupes-baseline and --health-baseline. Every machine envelope with a default exit rule always carries that rule in gate_outcomes, so a JSON reader can see a failing run. Exit codes do not change. The drift harness checks I3 and I7.",
          "timestamp": "2026-09-23T19:53:24+02:00",
          "tree_id": "7de9478ff67ea975a394141ddb9805b386e6c3e1",
          "url": "https://github.com/fallow-rs/fallow/commit/d6e4fdf5b104402b51e69e8d767e9d78c9ba84b3"
        },
        "date": 1790186698941,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.9,
            "unit": "%"
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
          "id": "1df164f7c772200e1da7731f91d7a9edfba06b65",
          "message": "fix: give CI formats the rule severity of each dead-code finding (#2814)\n\nEach dead-code finding now carries an optional effective_severity field (error or warn). The analysis writes it once, after rule resolution, so overrides[].rules apply. SARIF, CodeClimate, github-annotations and the bundled annotation filter read it in the direct run and in report --from, so an annotation or SARIF level now follows the configured rule. A saved report without the field, or with an unknown value, keeps the old level.\n\nEmpty catalog groups and unused or misconfigured dependency overrides now use the rules of the file that declares them, in the exit code, the audit verdict and the annotations.\n\nThanks to @jwenger-notion for the report.\n\nCloses #2782",
          "timestamp": "2026-09-23T21:12:27+02:00",
          "tree_id": "bc4d1db47ac1e5b1a6973c1bc2c8b826584eea59",
          "url": "https://github.com/fallow-rs/fallow/commit/1df164f7c772200e1da7731f91d7a9edfba06b65"
        },
        "date": 1790191932794,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.9,
            "unit": "%"
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
          "id": "43881a70dc7c78a77914d6d279d332402263b3b3",
          "message": "feat: add complexity gate rules so CRAP and complexity findings can warn (#2823)\n\nNew rules complexity-cyclomatic, complexity-cognitive and complexity-crap decide whether a complexity finding blocks. The default is error, so current behavior does not change. The threshold keys still decide whether a finding exists. A finding above several thresholds takes the most severe rule of those kinds, and it is dropped only when all of them are off. overrides[].rules apply.\n\nThe fallow health exit code, gate_outcomes, the bare fallow summary and the audit verdict read the rules, and a warn finding gives verdict warn. Each complexity finding carries effective_severity next to the band severity. GitHub annotations, SARIF and CodeClimate take their level from the rule, so with the default a moderate finding now shows as an error annotation, the same as the failing job.\n\nThanks to @jwenger-notion for the report.\n\nCloses #2783",
          "timestamp": "2026-09-24T00:14:17+02:00",
          "tree_id": "94d356767a719c78105d1790874dcfca1fa66bba",
          "url": "https://github.com/fallow-rs/fallow/commit/43881a70dc7c78a77914d6d279d332402263b3b3"
        },
        "date": 1790203309792,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.9,
            "unit": "%"
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
          "id": "58e3bd25b8d1c8269578f4d01a5ea41a9020319a",
          "message": "refactor: share decision logic across the CLI, MCP, API and editor (#2815)\n\nEach decision that more than one command or surface makes now has one implementation below the CLI: the diff filter, the dead-code scope, clone group workspace scoping, the production mode, one per-finding severity table, baseline loading, the gate outcome builders and the verdict-to-exit-code table. The drift harness now checks all invariants I1 to I8.",
          "timestamp": "2026-09-24T02:19:16+02:00",
          "tree_id": "a7004480dd5562493265a8fc4418a55d19b49773",
          "url": "https://github.com/fallow-rs/fallow/commit/58e3bd25b8d1c8269578f4d01a5ea41a9020319a"
        },
        "date": 1790209786883,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93,
            "unit": "%"
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
          "id": "4e62249b20d452bd84b33c3cba6e2ca34006c508",
          "message": "test: take audit renames and clone identity from git in the drift harness (#2834)\n\nThe I4 oracle reads renames from git diff --find-renames, the same way audit does, and models clone-group size and the documented no-added-line demotion. The full 512-case check found these gaps; the product behavior was correct.",
          "timestamp": "2026-09-24T07:07:33+02:00",
          "tree_id": "2cadc13943bb2c8e7e1d55f68ee2c153e79dc105",
          "url": "https://github.com/fallow-rs/fallow/commit/4e62249b20d452bd84b33c3cba6e2ca34006c508"
        },
        "date": 1790228083434,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93,
            "unit": "%"
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
          "id": "bb13ec8babeb0c1a5ba69da742e67a7203157fac",
          "message": "fix: read an absolute config path under the project root as absolute (#2821)\n\nConfig readers treated a leading slash as relative to the project root before they checked for an absolute path, so a literal absolute path under the root was read wrong. An absolute path under the root is now read as absolute. Webpack, rspack and rsbuild read a leading slash as a filesystem path for context and entries, and webpack also for resolve.alias. Vite and the other readers keep the root-relative reading, including Vite's rule for a root whose own name repeats inside it.\n\nCloses #2806",
          "timestamp": "2026-09-24T08:26:03+02:00",
          "tree_id": "df9e15db823853bffe175022624907f50df946b8",
          "url": "https://github.com/fallow-rs/fallow/commit/bb13ec8babeb0c1a5ba69da742e67a7203157fac"
        },
        "date": 1790232124821,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93,
            "unit": "%"
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
          "id": "8e8fbfe48eec6c78c6105806e8a56f9d3851f4c2",
          "message": "fix: align rendered output and MCP write hints with the envelope (#2829)\n\nThe live combined PR comment and its Check Run summary now show the same status note as report --from. github-summary sorts dead-code rows by path in the renderer, so the live and saved summaries agree, and the parity tests cover github-summary and github-annotations.\n\nThe MCP tools analyze, check_changed, find_dupes and check_health accept a file-writing parameter, so they now declare readOnlyHint false and destructiveHint false. code_execute refuses save_baseline, save_regression_baseline and save_snapshot before dispatch, and a refused call spends no host-call slot; call the standalone tool for the write.\n\nCloses #2755",
          "timestamp": "2026-09-24T09:18:00+02:00",
          "tree_id": "b4885c1869691ac9d64868d2fe5d18505aa0b5ca",
          "url": "https://github.com/fallow-rs/fallow/commit/8e8fbfe48eec6c78c6105806e8a56f9d3851f4c2"
        },
        "date": 1790234817623,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93,
            "unit": "%"
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
          "id": "d9a7e1c2f1b84b502e7227ffef8ad6f4908a7ab6",
          "message": "fix: give a hidden-directory remedy that fixes the current run (#2818)\n\nThe skipped-source-dotdir diagnostic suggested --root, which does not clear the false positive in the main run. The message, the stderr note and the kind description now name the fix per case: an entry for a file, ignoreExports for an export, and ignoreDependencies for a dependency that only the hidden directory imports.\n\nCloses #2797",
          "timestamp": "2026-09-24T09:42:32+02:00",
          "tree_id": "f248a1bf7500b9d62704effca52bd3269b992ca4",
          "url": "https://github.com/fallow-rs/fallow/commit/d9a7e1c2f1b84b502e7227ffef8ad6f4908a7ab6"
        },
        "date": 1790236767193,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93,
            "unit": "%"
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
          "id": "bb1e1fb689bd17e4999857a71816830ac2230b31",
          "message": "fix: share the git helpers and report hotspots on a branch without commits (#2816)\n\nThe CLI now uses one set of git helpers from the engine: the base-worktree cache remap (which now also handles a symlinked root), the full HEAD sha, and the remote default branch probe. fallow init writes the right base branch for a clone that has only origin/master and no origin/HEAD. The loaded-baseline slot resets at the start of each command run.\n\nfallow health --hotspots on a branch without commits now records hotspots-skipped with cause no-commits and prints a note, instead of producing no result and no diagnostic.\n\nCloses #2758\nCloses #2803",
          "timestamp": "2026-09-24T10:16:38+02:00",
          "tree_id": "baaf72baf89e9f74b7f5f2fd6ffd5f5edfec42f4",
          "url": "https://github.com/fallow-rs/fallow/commit/bb1e1fb689bd17e4999857a71816830ac2230b31"
        },
        "date": 1790238874276,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93,
            "unit": "%"
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
          "id": "38e1b05bfc1788d40a6de0b1adb38cd675c7fbd1",
          "message": "perf(trace): index module paths once for semantic reference checks (#2839)\n\nreconcile_semantic_trace_reachability resolved each reference of a\ntype-aware trace with matching_module_indexes. Each call scanned every\nmodule and made two canonicalize calls. The checker returns up to 40\nreferences, and when none is reachable every reference runs a full scan.\n\nA ModulePathLookup now indexes the module paths once, by path and by\nfile name. Each reference then costs a few map lookups and one\ncanonicalize call. On a 20,000-module graph, 40 lookups drop from about\n41 ms to about 7 ms, index build included.\n\nDifferential tests check that the index returns the same modules, in\nthe same order, as matching_module_indexes, including suffix, ambiguous\nand missing paths, and a symlinked temp root on disk.",
          "timestamp": "2026-09-24T12:10:25+02:00",
          "tree_id": "8dd99c6fcccda19ab141039fd0b10a39c80db214",
          "url": "https://github.com/fallow-rs/fallow/commit/38e1b05bfc1788d40a6de0b1adb38cd675c7fbd1"
        },
        "date": 1790246070372,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93,
            "unit": "%"
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
          "id": "b3918d5b491afac714780262ec70197dd6c01d0b",
          "message": "feat: read Module Federation runtime remotes with literal arguments (#2874)\n\nWhen a file imports the Module Federation runtime, registerRemotes([{ name, entry }]) with literal names registers those remote aliases like remotes, and loadRemote('app/Button') with a literal string credits the remote alias, scoped to the workspace that holds the file. A call with a non-literal argument records plugin-config-unreadable with the new reason dynamic-argument. The extraction cache version is bumped.\n\nCloses #2795",
          "timestamp": "2026-09-24T23:20:02+02:00",
          "tree_id": "a6937409933a4a71d6d6aea0a5d826393055728e",
          "url": "https://github.com/fallow-rs/fallow/commit/b3918d5b491afac714780262ec70197dd6c01d0b"
        },
        "date": 1790288086921,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "4dc965c0647c6675ece5be0e37128fd5ce1b6d3f",
          "message": "feat: name the Module Federation source in trace output (#2875)\n\n--trace-file on a file that Module Federation exposes, and --trace-dependency on a remote alias, now list a sources array with each config that exposes the file or declares the remote (kind, plugin, config and key). A remote that only a literal registerRemotes or loadRemote call names points to that source file and call. The field is optional and absent when empty, so other traces do not change. The human trace output adds one Source line per entry, and the MCP trace tools carry the same field.\n\nCloses #2796",
          "timestamp": "2026-09-25T06:22:26+02:00",
          "tree_id": "93265992ab27aff7957aeb15654828e05caede91",
          "url": "https://github.com/fallow-rs/fallow/commit/4dc965c0647c6675ece5be0e37128fd5ce1b6d3f"
        },
        "date": 1790310622177,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "f9a2841a9adf643fc32236db4e4408096cb2b760",
          "message": "fix: scope Module Federation exposes credit and read helper modules and SFC runtime calls (#2879)\n\nA bare package in exposes now gets dependency credit only for the package that owns the config, the same as shared. A Federation plugin call in a helper module that a bundler config imports (one relative import or require, inside the project root) is read, with the importing config as the base for paths. Runtime calls with literal arguments in .vue and .svelte script blocks are read, and init and createInstance register the remotes that their options name. The extraction cache version is bumped.\n\nCloses #2876",
          "timestamp": "2026-09-25T07:33:10+02:00",
          "tree_id": "a775dc45f622ff5dc1286624c72c85c618988884",
          "url": "https://github.com/fallow-rs/fallow/commit/f9a2841a9adf643fc32236db4e4408096cb2b760"
        },
        "date": 1790315059829,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "48fba5a025aa6793823bfcc3857f55dbec00d6a5",
          "message": "fix: discard reports sent to NUL on Windows and reject an unused --sarif-file (#2878)\n\nOn Windows, -o NUL, --sarif-file NUL and the save flags now write to the null device, in any case and as NUL:, as /dev/null does on Unix. A name with an extension such as NUL.txt stays a normal file. A bare run without the dead-code analysis (--skip dead-code, --only dupes, --only health) rejects --sarif-file with exit 2 and points to --format sarif --output-file. The write guard passes clippy on Windows, and the Windows CI job now runs the write guard and null device tests.\n\nCloses #2877",
          "timestamp": "2026-09-25T10:31:11+02:00",
          "tree_id": "a8e8727e7b94265964277f6d86ba013507bd9e0e",
          "url": "https://github.com/fallow-rs/fallow/commit/48fba5a025aa6793823bfcc3857f55dbec00d6a5"
        },
        "date": 1790325807169,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "0f7f81e0b016b923063feda79eb5f437fb1c149d",
          "message": "fix: expect one list entry point per workspace in the benchmark (#2887)\n\nfallow list now reports the same deduplicated entry points as the analysis, so the list inventory benchmark fixture has one entry point per workspace, not two. The benchmark assertion expected the old count and failed the Benchmarks workflow.",
          "timestamp": "2026-09-25T10:50:32+02:00",
          "tree_id": "ae170b01543c64031d9e8882ade4ab3925d4e2f5",
          "url": "https://github.com/fallow-rs/fallow/commit/0f7f81e0b016b923063feda79eb5f437fb1c149d"
        },
        "date": 1790327789159,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "bdc683dd4db9f15c124088737fe9c8bd06e9a9f0",
          "message": "ci: cut pull request jobs and gate releases on a green release commit (#2882)\n\nThe free plan runs 20 jobs at a time. A Rust pull request started about\n30 jobs, so pull requests and main pushes waited for runners.\n\nPull requests:\n- Coverage, Module Coupling, Fuzz Smoke and Cross-Architecture run on\n  main only. Cross-Architecture no longer checks x86_64 linux-gnu, which\n  the Check job already covers.\n- Benchmarks, Binary Size and Allocation Tracking run on main, and on a\n  pull request only with the ci:perf label.\n- The VS Code target host smoke runs on pull requests only when the VSIX\n  inputs change. Main and Release Validation still run it.\n- Ecosystem CI builds the release binary once and shares it with the\n  five project jobs.\n\nCritical path:\n- Tests run with cargo-nextest. Windows runs one nextest command in place\n  of eight cargo test calls. The old call for the Windows Job Object test\n  selected no test; the filter now selects it in fallow-process.\n- Pull requests build the NAPI addon with the dev profile. Main keeps\n  napi-release.\n- The two feature clippy runs are one run.\n- Only main saves the Rust cache.\n\nMain and releases:\n- A newer push to main cancels the older run. Main gets many merges a\n  day, so only the newest commit needs a result.\n- The release commit (\"chore: release v\") gets a concurrency group of its\n  own, so a later merge cannot cancel its runs.\n- release.yml runs scripts/verify-release-ci.mjs before anything builds.\n  It waits for the push runs on the release commit and fails when a\n  required workflow is missing or any run failed.\n\nAlso: cargo doc in pre-push, scripts/ci-metrics.mjs for queue and run\ntimes, and the dead merge_group triggers are gone.",
          "timestamp": "2026-09-25T12:24:29+02:00",
          "tree_id": "1642c34f1b890571d59d76272cb42f2d77e36054",
          "url": "https://github.com/fallow-rs/fallow/commit/bdc683dd4db9f15c124088737fe9c8bd06e9a9f0"
        },
        "date": 1790333539149,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "02f31dcd02b5f61fce3edc8ec3cef636920bd600",
          "message": "ci: require a Windows CI job for every file with Windows-only code (#2894)\n\nA policy test fails when a Rust file with Windows-only cfg code is not in a Windows path filter, or when an exact filter entry names a missing file. The windows-rust filter now lists every flagged file, and two stale entries are gone.",
          "timestamp": "2026-09-25T13:44:31+02:00",
          "tree_id": "33c344025e19642f70e9b59e9a3a4adc2dec75e2",
          "url": "https://github.com/fallow-rs/fallow/commit/02f31dcd02b5f61fce3edc8ec3cef636920bd600"
        },
        "date": 1790337291843,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "8673fb376f8b3a11c394d6447dfc64ed11430f23",
          "message": "ci: run Windows-only graph, api and multicall tests on Windows (#2895)\n\nThe Windows job now builds, lints and runs the Windows-only tests of fallow-graph, fallow-api and fallow-multicall. The time budget goes from 30 to 40 minutes.",
          "timestamp": "2026-09-25T14:06:30+02:00",
          "tree_id": "411fe3333895c0faebc313b405628da3dbba59e3",
          "url": "https://github.com/fallow-rs/fallow/commit/8673fb376f8b3a11c394d6447dfc64ed11430f23"
        },
        "date": 1790338428096,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "cbe3e3e6b2b516f42aca8069c5c11327ebe493ad",
          "message": "refactor: filter component prop suppressions through retain_unsuppressed (#2892)\n\nMove the unused-component-prop, prop-drilling, thin-wrapper and duplicate-prop-shape suppression filters onto the shared helper. The helper now takes an optional location, so a prop-drilling chain without a source hop stays, as before.\n\nAdd next-line and file-wide suppression rows for these four kinds to the stale-suppression tests.",
          "timestamp": "2026-09-25T14:15:42+02:00",
          "tree_id": "f10b0262d09ec8c05286abb25788faa33507c6fe",
          "url": "https://github.com/fallow-rs/fallow/commit/cbe3e3e6b2b516f42aca8069c5c11327ebe493ad"
        },
        "date": 1790338939717,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "bb4e15f86b201c3092a2247a5411d7dbd1a07e26",
          "message": "test: find the fallow binary through CARGO_TARGET_DIR (#2897)\n\nTests and scripts that read the fallow binary from ./target now follow CARGO_TARGET_DIR. Closes #2888",
          "timestamp": "2026-09-25T14:42:05+02:00",
          "tree_id": "f5b35b2208a6c49190a9536171e7ccefd782026d",
          "url": "https://github.com/fallow-rs/fallow/commit/bb4e15f86b201c3092a2247a5411d7dbd1a07e26"
        },
        "date": 1790340701996,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "139dc1662e7e99123b80f4b47549238213c8ef3f",
          "message": "fix: stop Dockerfile parser panic on non-ASCII text (#2898)\n\nstrip_dockerfile_instruction compared the line start to RUN, CMD and ENTRYPOINT through a byte slice. A multi-byte character across the keyword length split a UTF-8 boundary and the run stopped with a panic. The comparison now uses str::get, so such a line counts as an ordinary line.\n\nFixes #2896",
          "timestamp": "2026-09-25T14:52:26+02:00",
          "tree_id": "84aa1c545779f11091c3f950d902bad1067d3454",
          "url": "https://github.com/fallow-rs/fallow/commit/139dc1662e7e99123b80f4b47549238213c8ef3f"
        },
        "date": 1790341214245,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "a5ed1a2ffd273479b07cbe49bcadd8bc2bd2bf7b",
          "message": "test: build the CLI integration tests as one binary (#2899)\n\nEach file in crates/cli/tests was a test binary of its own, and each one\nlinked the whole CLI. The 60 plain test files are now modules of one\nbinary, crates/cli/tests/integration. Three targets stay separate:\nruntime_coverage_tests and audit_brief_runtime_focus_tests need the\ntest-sidecar-key feature, and drift has its own ignored cases.\n\nOn a 10-core machine a clean build of the CLI tests goes from 113 s to\n63 s of CPU time, and a rebuild after a change in lib.rs goes from 18 s\nto 6 s. The gain is larger on the 4-core CI runners and on the Windows\nlinker. The test list is the same 1,428 tests, now under integration::.\n\nSnapshots moved to tests/integration/snapshots with the integration__\nprefix that insta derives from the new module path.\n\nCI now also runs the two gated audit_brief_runtime_focus_tests tests. No\njob ran them before. The Windows nextest filter selects the null device\ntest by its new name.",
          "timestamp": "2026-09-25T15:05:08+02:00",
          "tree_id": "54c666fd3e6def4b7a784b570f2a83dfd660e770",
          "url": "https://github.com/fallow-rs/fallow/commit/a5ed1a2ffd273479b07cbe49bcadd8bc2bd2bf7b"
        },
        "date": 1790341881799,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "3fc30869022f37a58dbce69c5250bc101a0bebd2",
          "message": "test: give the Storybook absolute story glob a drive on Windows (#2903)\n\nA path without a drive is not absolute on Windows, so the fixture dropped the absolute story glob there. The code is correct. The fixture now uses a drive-rooted path on Windows, and the Windows Rust job now runs for the Storybook plugin.",
          "timestamp": "2026-09-25T16:48:25+02:00",
          "tree_id": "e61bcb16391c003ad8aed28cbb2e4bcad8b75547",
          "url": "https://github.com/fallow-rs/fallow/commit/3fc30869022f37a58dbce69c5250bc101a0bebd2"
        },
        "date": 1790348502789,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "50a412d5f6f46f3104f40e31640997d61d8ac459",
          "message": "chore: check local installs before verify gates run (#2904)\n\nverify:fast and verify:full found a missing or stale local install only when the gate that needed it ran. A fresh worktree then needed one full run per missing install. The verify script now checks the installs for the selected mode first and lists every problem with its fix command.",
          "timestamp": "2026-09-25T17:15:35+02:00",
          "tree_id": "169b2929be6e8a022db27d8a93a882a4ca2fd30e",
          "url": "https://github.com/fallow-rs/fallow/commit/50a412d5f6f46f3104f40e31640997d61d8ac459"
        },
        "date": 1790349757811,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "e37801d9fa3e0fcd816975b45a13fec1e4b00502",
          "message": "fix: resolve sibling-workspace entry patterns on Windows (#2905)\n\nThe workspace prefix comes from a native path, so on Windows it uses backslashes. The parent-relative resolver split it on forward slashes only, so a Storybook story or a Module Federation exposes target in a sibling workspace got no entry credit on Windows. The resolver now splits on both separators.\n\nThe drift audit identity of a clone group now compares the clone text, as the audit key does. A head edit inside a surviving clone makes a new group.",
          "timestamp": "2026-09-25T19:20:19+02:00",
          "tree_id": "2a0e976c923e60f2686bc20cbde7c7ecc95e7801",
          "url": "https://github.com/fallow-rs/fallow/commit/e37801d9fa3e0fcd816975b45a13fec1e4b00502"
        },
        "date": 1790357241490,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "b672399c3eefd89f15614361612f039abedc4f3f",
          "message": "test: give each stale suppression its own drift identity (#2907)\n\nThe audit key of a stale suppression holds its directive and the issue kind it suppresses. The drift key had no symbol for it, so two stale suppressions in one file shared an identity. The drift symbol now comes from the suppression origin. A control pins the case.",
          "timestamp": "2026-09-25T22:29:18+02:00",
          "tree_id": "1c3d215405f56d6fa7c1b460f02cef04cb5497ef",
          "url": "https://github.com/fallow-rs/fallow/commit/b672399c3eefd89f15614361612f039abedc4f3f"
        },
        "date": 1790368736421,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "3f6f8358a8bb50f56d65bb309327dad6ab313921",
          "message": "feat: read raw V8 coverage for health --coverage (#2908)\n\nhealth --coverage, health.coverage, FALLOW_COVERAGE, audit and the MCP tools now read a NODE_V8_COVERAGE directory or one V8 coverage JSON file. Statements come from the AST of the file on disk and counts from the V8 block ranges; the dumps of all test processes add up. Transpiled scripts (tsx, esbuild, webpack) map back to their sources through the source map that Node records in the dump. summary.coverage_input_format names the input (istanbul or v8).\n\nCloses #2906",
          "timestamp": "2026-09-25T23:03:09+02:00",
          "tree_id": "4d06dd26ca04a03ce8611c49d0a091d109d09b9f",
          "url": "https://github.com/fallow-rs/fallow/commit/3f6f8358a8bb50f56d65bb309327dad6ab313921"
        },
        "date": 1790370698154,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "08ce24a0f2917e084400c849bc9bae1bb33b3b40",
          "message": "chore: release v3.29.0",
          "timestamp": "2026-09-25T23:19:48+02:00",
          "tree_id": "1963a2e2a4be162410a86ae03cde3a29d48a463d",
          "url": "https://github.com/fallow-rs/fallow/commit/08ce24a0f2917e084400c849bc9bae1bb33b3b40"
        },
        "date": 1790375332953,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "e37828d92a0053c4a01d09627111a7bff07bbe06",
          "message": "chore(docker): pin FALLOW_VERSION 3.29.0 with refreshed checksums",
          "timestamp": "2026-09-26T07:33:27+02:00",
          "tree_id": "0608fa38cc7c3f6fdff695564f3c371543544f4f",
          "url": "https://github.com/fallow-rs/fallow/commit/e37828d92a0053c4a01d09627111a7bff07bbe06"
        },
        "date": 1790401481435,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "def16b17a774ed8771b082ff6ad7d5fcefd38bd2",
          "message": "perf: add deterministic work counters to --performance (#2913)\n\nAdds exact work counters, a process clock and a span tree to `--performance`, so speed work can target counts instead of wall-clock time.",
          "timestamp": "2026-09-26T11:41:17+02:00",
          "tree_id": "31b0062604678093d30e139d40b041aca146502d",
          "url": "https://github.com/fallow-rs/fallow/commit/def16b17a774ed8771b082ff6ad7d5fcefd38bd2"
        },
        "date": 1790416179443,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "e20e97b640fa4ce6211507cf6f3e63e6b8689b01",
          "message": "perf(lsp): publish only diagnostics that changed (#2914)\n\nThe language server publishes only changed diagnostics, converts columns in linear time, reads open files only when needed, and debounces and cancels runs under autosave. Adds a save-to-publish lab bench.",
          "timestamp": "2026-09-26T12:09:55+02:00",
          "tree_id": "69bc6f9989dc3c96d14155c721e5ab236593c8b8",
          "url": "https://github.com/fallow-rs/fallow/commit/e20e97b640fa4ce6211507cf6f3e63e6b8689b01"
        },
        "date": 1790418327095,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "87e6557441b1a23cd2a8dbb8a7709b7bcf684aac",
          "message": "chore(deps): move Oxc to 0.151 (#2910)\n\nMove the Oxc crates from 0.126 to 0.151 and oxc_coverage_instrument to 0.13. Loosen the srcmap-sourcemap pin so the grouped oxc_* Dependabot update can resolve. Port the Oxc AST changes with identical extraction output, give V8 offsets to the instrumenter unchanged, bump the extraction, graph and duplication cache versions, and raise the minimum Rust version to 1.96.",
          "timestamp": "2026-09-26T13:00:06+02:00",
          "tree_id": "a759604e3eb0cba114b61dbf85cffdf312448202",
          "url": "https://github.com/fallow-rs/fallow/commit/87e6557441b1a23cd2a8dbb8a7709b7bcf684aac"
        },
        "date": 1790421079462,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.1,
            "unit": "%"
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
          "id": "cba730f8b7b426dfe417b76f66a91f92125370a3",
          "message": "perf(viz): defer the lens payloads and send lists as tables (#2916)\n\n## Summary",
          "timestamp": "2026-09-26T13:18:45+02:00",
          "tree_id": "c4e40b3b20bd929df852da3d1eaab7271228f065",
          "url": "https://github.com/fallow-rs/fallow/commit/cba730f8b7b426dfe417b76f66a91f92125370a3"
        },
        "date": 1790421849574,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.2,
            "unit": "%"
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
          "id": "e7533d75d1f9f0c17c0ce27bb69bdefb84c086f6",
          "message": "feat(coverage): rank runtime hot paths by per-call cost (#2919)\n\n## Summary",
          "timestamp": "2026-09-26T13:43:42+02:00",
          "tree_id": "30837ce688ac7522863aacaa4525c8bd683c5eb9",
          "url": "https://github.com/fallow-rs/fallow/commit/e7533d75d1f9f0c17c0ce27bb69bdefb84c086f6"
        },
        "date": 1790423508979,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.2,
            "unit": "%"
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
          "id": "38a193bfd38ec78074fd037b2ad42363daed8ae7",
          "message": "perf(lsp): keep one project session between saves (#2920)\n\n## Summary",
          "timestamp": "2026-09-26T14:26:06+02:00",
          "tree_id": "99be9c33713b1e791de56cf4076fd4ac46100227",
          "url": "https://github.com/fallow-rs/fallow/commit/38a193bfd38ec78074fd037b2ad42363daed8ae7"
        },
        "date": 1790426225146,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.2,
            "unit": "%"
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
          "id": "e77cd05343f27953875dfe72e8a3bc019813e5c0",
          "message": "test: run the warm parse sequence test on Unix only (#2924)\n\nA warm parse needs the inode change time of each source file. Windows does not expose it, so the store never reuses a parse there and counts nothing. Gate the API test the same way as the engine tests of the store.",
          "timestamp": "2026-09-26T14:51:20+02:00",
          "tree_id": "44b3f99df25c88cca6a785dcde0626d11ffd3c2d",
          "url": "https://github.com/fallow-rs/fallow/commit/e77cd05343f27953875dfe72e8a3bc019813e5c0"
        },
        "date": 1790427501563,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.2,
            "unit": "%"
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
          "id": "3bbd7f098dcf62ce5a67e08fedabac92d7dbd147",
          "message": "feat(list): report the startup import weight of each runtime entry (#2918)\n\n## Summary",
          "timestamp": "2026-09-26T15:01:25+02:00",
          "tree_id": "5bb1d819946846ef11eb41841a39083ee57b4954",
          "url": "https://github.com/fallow-rs/fallow/commit/3bbd7f098dcf62ce5a67e08fedabac92d7dbd147"
        },
        "date": 1790428086821,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.2,
            "unit": "%"
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
          "id": "dc3742b0ecc77803d06e94f768eebba06d0dd2f6",
          "message": "feat(flags): detect import.meta.env reads, registry keys and JSX guards (#2925)\n\n## Summary",
          "timestamp": "2026-09-26T15:47:52+02:00",
          "tree_id": "a21e5a166b211e6057809703fc606780927ec0ae",
          "url": "https://github.com/fallow-rs/fallow/commit/dc3742b0ecc77803d06e94f768eebba06d0dd2f6"
        },
        "date": 1790431351728,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.2,
            "unit": "%"
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
          "id": "ebf516b11950248342563048f32a85285234e00f",
          "message": "fix(overrides): read every project document of a pnpm lockfile (#2923)\n\npnpm 12 writes pnpm-lock.yaml as two YAML documents when package.json sets packageManager. The override check read only the first document, so every transitive pnpm override was reported as unused. The reader now collects packages from each project document and skips the package manager document.\n\nCloses #2909",
          "timestamp": "2026-09-26T16:18:11+02:00",
          "tree_id": "63ae249463c24942d4305a3e386c723489a46803",
          "url": "https://github.com/fallow-rs/fallow/commit/ebf516b11950248342563048f32a85285234e00f"
        },
        "date": 1790432809921,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.2,
            "unit": "%"
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
          "id": "e0bb6ea7f54357f95723d9572272e8f30a75b581",
          "message": "test: fix two release-validation failures (#2927)\n\nRun the session refresh fingerprint test on Unix only: Windows has no inode change time. Compare only the cloned text span in the drift clone identity, because a clone can start or end inside a line; keep the failing I4 case as a proptest regression.",
          "timestamp": "2026-09-26T16:30:11+02:00",
          "tree_id": "258f2f0ae99178ac451d600182a7dfc90dcd6063",
          "url": "https://github.com/fallow-rs/fallow/commit/e0bb6ea7f54357f95723d9572272e8f30a75b581"
        },
        "date": 1790433472606,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 93.2,
            "unit": "%"
          }
        ]
      }
    ]
  }
}