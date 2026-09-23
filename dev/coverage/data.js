window.BENCHMARK_DATA = {
  "lastUpdate": 1790191936519,
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
        "date": 1788775857774,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.3,
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
          "id": "cdc5c0ee05031c77cb420b635de403164733d958",
          "message": "chore(docker): pin FALLOW_VERSION 3.23.0 with refreshed checksums",
          "timestamp": "2026-09-07T14:06:24+02:00",
          "tree_id": "0cdd881d3c9e5ed647639962932bae7ed312e739",
          "url": "https://github.com/fallow-rs/fallow/commit/cdc5c0ee05031c77cb420b635de403164733d958"
        },
        "date": 1788783328768,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.3,
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
          "id": "d5cb80f405d611772a2da634ac851a71c796ca1b",
          "message": "fix(brand): round the icon badge to the logo.svg radius\n\nThe standalone icon drew its badge as a hard square while logo.svg rounds the\nsame badge at rx 10 of 64, so the two brand assets disagreed wherever the icon\nappears unmasked, such as the VS Code marketplace.\n\nApply the same 15.6% radius and re-render the rasters. Consumers that mask the\navatar themselves are unaffected.",
          "timestamp": "2026-09-07T14:23:04+02:00",
          "tree_id": "dd22fd4201d32c80fa2c5af7739af505742b9679",
          "url": "https://github.com/fallow-rs/fallow/commit/d5cb80f405d611772a2da634ac851a71c796ca1b"
        },
        "date": 1788784211251,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.3,
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
          "id": "a6afda2f0136f645cea233b546a150ac79011115",
          "message": "feat(review): replace the duplicated blast-radius list with a count and a rollup (#2563)\n\nThe review-brief envelope carried the impact closure's affected-but-not-in-diff paths twice, in full: graph_facts.reachable_from was a verbatim clone of impact_closure.affected_not_shown, and neither was capped. On a one-file change to colinhacks/zod the two lists were 28,372 of 52,676 bytes while the focus map and decision surface were 1,276.\n\ngraph_facts.reachable_from is removed; it had no reader. impact_closure now reports affected_count (exact, computed before capping), a ten-path sorted-prefix sample, and affected_by_dir: {dir, count} rows heaviest first, capped at 25 with affected_by_dir_omitted counting the rest. A prefix sample alone would mislead: on a 20-file zod diff a 25-path prefix covers one of 24 directories while the weight sits in two others.\n\nBoth human renderers read affected_count, so their totals are unchanged. Decisions, ranks, verdicts and exit codes are untouched; the decision surface takes its blast metric from the uncapped engine closure. Brief schema_version moves to 9.\n\nEnvelope on the same reproduction: 52,676 -> 25,818 bytes.",
          "timestamp": "2026-09-07T15:01:24+02:00",
          "tree_id": "9caf7344e24e187a754859e296c4a9f8888f158e",
          "url": "https://github.com/fallow-rs/fallow/commit/a6afda2f0136f645cea233b546a150ac79011115"
        },
        "date": 1788787143809,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.3,
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
          "id": "4503f09c0f324f4c55e2cff6c581f7fd92c36d6f",
          "message": "fix(guard): state the coverage requirement for unzoned files\n\nAn unzoned file under boundaries.coverage.requireAllFiles was told it was\nunrestricted while an analysis run would report it as a boundary-coverage\nviolation. guard_notes never read coverage_required, which the JSON already\ncarried, so the human output contradicted the JSON for the same file.\n\nThe unrestricted note now names what it actually covers, import and call\nchecks, and a second note states the coverage requirement when it applies.\nPaths exempted by allowUnmatched keep the old single note.",
          "timestamp": "2026-09-07T15:19:35+02:00",
          "tree_id": "10fefeb404697cc57b5fa643833ee776d735b09b",
          "url": "https://github.com/fallow-rs/fallow/commit/4503f09c0f324f4c55e2cff6c581f7fd92c36d6f"
        },
        "date": 1788787585760,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.3,
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
          "id": "25c58ed04d7977d07711a6be4bdff34d8cfb6d28",
          "message": "fix(review-electron): say the brief schema version is a floor, not the version (#2564)\n\nhasHeader accepts schema_version >= REVIEW_BRIEF_SCHEMA_VERSION, and a test pins that + 1 is accepted, so the constant has always been a minimum. The name and the parse error both read as an exact version: 'expected audit-brief schema version 8'. That was harmless while 8 was the only version fallow emitted, and became wrong when the brief moved to 9, because the app accepts 9 and would still tell a user it expected 8.\n\nRenamed to MIN_REVIEW_BRIEF_SCHEMA_VERSION, message now says '8 or newer', and the constant carries the reason not to raise it: the floor exists so a brief schema bump keeps working while the read fields stay present. The value stays 8, since none of the fields the app reads changed.",
          "timestamp": "2026-09-07T15:56:07+02:00",
          "tree_id": "1dfff14abd79e4336991afcba1662ee0ade43053",
          "url": "https://github.com/fallow-rs/fallow/commit/25c58ed04d7977d07711a6be4bdff34d8cfb6d28"
        },
        "date": 1788789917995,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.3,
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
          "id": "dc8656e74ff5e58df08ccd03bc8aabac4966c594",
          "message": "test: cover guard multi-file batches and candidate loader errors\n\nTwo gaps salvaged from unlanded optimization branches, without the\noptimizations themselves.\n\nEvery existing guard test passes exactly one file, so nothing caught state\nleaking across files in a batch. The new test asserts a four-file report\nserializes identically to the four single-file reports concatenated, and pins\nthe per-file rule-id ordering, including that an unparsable files glob applies\nto every file.\n\nload_candidate_map had no coverage of its three rejection paths. The verdict\nside was tested, the candidate side was not, so a missing security_findings\narray, a malformed finding, and a duplicate finding_id were all unguarded.",
          "timestamp": "2026-09-07T17:03:32+02:00",
          "tree_id": "63b88877e3eddebf22d21f468e060de7c6727e75",
          "url": "https://github.com/fallow-rs/fallow/commit/dc8656e74ff5e58df08ccd03bc8aabac4966c594"
        },
        "date": 1788793853596,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.3,
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
          "id": "9e690609e1f918887937d3ff5d890adba4f2feba",
          "message": "fix(cli): bound the coordination-gap lines on the human review brief (#2565)\n\nThe brief printed one line per gap, joining every consumed symbol and both full paths. On a zod change to a module a barrel re-exports, one line rendered at 955 columns; a project with thirty out-of-diff consumers produced sixty lines at ~96 columns. They sat directly under an impact-closure summary that holds to eighty.\n\nThe section now states how many consumers sit outside the diff, walks the three that take the most symbols (the consumer on its own line, the contract it consumes on the next, as a branching split renders), and closes with the remainder and where to read it. Paths shorten from the left through elide_path; the symbol list fills a budget and cuts with a +N more suffix, shortened from the right because a symbol's leading characters identify it.\n\nOrdering by symbols taken rather than by path matters: the JSON gap list is path-sorted with no ranking, so an alphabetical prefix collapsed the 26-symbol barrel consumer behind the remainder. The header says 'use exports of changed files' because collect_coordination_gaps never verifies the export itself changed.\n\nRendering only. impact_closure.coordination_gap still carries every gap with every symbol, so the JSON contract and schema_version are untouched, and a new test pins that invariant next to the sibling fields that are capped.\n\nzod: 955 columns -> 77. Thirty consumers: 60 lines -> 9, none over 78.",
          "timestamp": "2026-09-07T17:26:51+02:00",
          "tree_id": "4a4fb606e3ddc28d76caf0267d08b4b45f27b3f6",
          "url": "https://github.com/fallow-rs/fallow/commit/9e690609e1f918887937d3ff5d890adba4f2feba"
        },
        "date": 1788795521851,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.3,
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
          "id": "ca508ed0aaca90e0ac3d23bb61d8f60f943ed935",
          "message": "refactor: replace copied helpers with one definition each\n\nFour report surfaces in the graph crate each carried a byte-identical\nrelativize, and their doc comments named the other copies as the thing to\nkeep in sync. A cross-platform key invariant shared by four outputs was\nenforced by prose. One pub(super) helper now owns it.\n\nranges_to_gaps and push_region were copied whole from sfc into astro, which\nalready imports SourceRegion from sfc. The sorted-input precondition the\nfunction depends on was implied only by the sort call sitting above it, so it\nis now written down.\n\nline_range_from_byte_col was defined twice, and hover wrapped\nutf16_col_span in a forwarder that added nothing. Both move to position.rs,\nwhich already owns the byte-column to UTF-16 boundary.",
          "timestamp": "2026-09-07T18:46:01+02:00",
          "tree_id": "4e1407a84e73c484101ba6e0d89ec4709332933c",
          "url": "https://github.com/fallow-rs/fallow/commit/ca508ed0aaca90e0ac3d23bb61d8f60f943ed935"
        },
        "date": 1788800130893,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.3,
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
          "id": "f63c4dbff717c1e9037d3290174d425d282340e7",
          "message": "fix(cli): wrap the decision-surface and focus lines to eighty columns (#2566)\n\nThe two sections the brief leads with were the two that wrapped unpredictably in a terminal. The decision surface printed its question, trade-off and expert list on single unbounded lines: on zod, a question naming every widened export ran to 159 columns. The focus map put an un-elided path and an unbounded reason on one row, reaching 103.\n\nBoth now wrap under a hanging indent. Prose is re-flowed rather than cut: a decision question ends in the actual ask, so truncating it would drop the question. Only a single word that alone overruns its line is shortened, from whichever end identifies it: a path keeps its tail, an owner identity keeps its head. The continuation indents give the block a 2 / 5 / 7 hierarchy so column 5 stays the key column.\n\nTwo defects found in review and fixed: the trailing blank line after a populated decision block was dropped, so the apex ran straight into the drill-down header; and the ask line double-counted its bus-factor reservation and elided owner identities from the wrong end, rendering a 52-character email as '.../ame.lastname@engineering.example.com' with fifteen columns free.\n\nRendering only; no field, format or schema moves. Zero lines over eighty across three fixtures, in both focus branches.",
          "timestamp": "2026-09-07T19:17:01+02:00",
          "tree_id": "9821252e4f154e68aaaf5b19df1d0b7dfdd9b64d",
          "url": "https://github.com/fallow-rs/fallow/commit/f63c4dbff717c1e9037d3290174d425d282340e7"
        },
        "date": 1788802147472,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.3,
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
          "id": "cd9b2e46a8a4f19ceab9c5d6e060abe133c16dcd",
          "message": "refactor: remove test-only shadows and copied catalogues\n\nAn audit across eight domains, each challenged by an independent reviewer,\nfound the same shape repeatedly: logic copied into a second place, then\nasserted against itself.\n\nSeveral test modules re-implemented the production function they claimed to\ncover, so the tests passed against their own copy while the real code was\nnever exercised. report/ci/severity.rs was an entirely cfg(test) shadow of\nmappings owned by fallow-output and fallow-config; sarif.rs, codeclimate.rs\nand serde_path.rs each kept a second copy of a function and asserted it\nagainst that copy. Each removal names the executed assertion elsewhere that\nstill covers the contract.\n\nThe React runtime dependency gate existed in six detectors under four names,\nthe security binding-trace catalogue in two, and the jsonc dialect catalogue\nin two crates. Forwarding wrappers that added nothing to their callee are\ngone, as are the three is_config_fixable tests left behind when the function\nmoved to fallow-config.\n\nBehavior is unchanged throughout. The whole workspace suite, the JS suites,\nthe contract-drift check and the agent-adapter check all pass.",
          "timestamp": "2026-09-07T20:08:45+02:00",
          "tree_id": "9e1227668769b378099f057f2fe83efd032c26f3",
          "url": "https://github.com/fallow-rs/fallow/commit/cd9b2e46a8a4f19ceab9c5d6e060abe133c16dcd"
        },
        "date": 1788805658439,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "Code Coverage",
            "value": 92.3,
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
          "id": "0cc81b8761da239b721b3d70c3a26cc4db48d412",
          "message": "fix: report what fallow does not know (#2568)\n\nfix: report what fallow does not know",
          "timestamp": "2026-09-08T13:02:05+02:00",
          "tree_id": "ca9780cfd6bea5be4a4375ad44e5957fae7a1082",
          "url": "https://github.com/fallow-rs/fallow/commit/0cc81b8761da239b721b3d70c3a26cc4db48d412"
        },
        "date": 1788866200880,
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
        "date": 1788867359705,
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
        "date": 1788868428174,
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
        "date": 1788881183940,
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
        "date": 1788884584741,
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
          "id": "e5e92c787465f81e6fcd8f84a33be5b7f6ee93ac",
          "message": "fix: make the caveat reach every surface, and the gates that guard it fail\n\n* fix: disclose incomplete evidence on unused store members\n\nEight of the nine dead-code finding types tell a reader when their verdict\nrests on a file the run never fully read. Store members were the ninth, left\nout on the argument that they expose no mutation on any surface.\n\nThat argument is correct and still holds: a store member has no `Fix` action,\nno LSP code action, and `store_members_never_offer_unverified_line_deletions`\npins the absent review suggestion. There is nothing here to withhold.\n\nBut the caveat is a disclosure before it is a gate. A reader deciding by hand\nwhether to delete a store member deserves the same hint as the eight arrays\nbeside it, and a run that reports eight caveated findings and one bare one\nreads as though the bare one were better evidenced.\n\nStore members take the member rule unchanged, the same one class and enum\nmembers take: member usage is collected by one walk over the accesses of every\nmodule the run parsed, reachable or not, so any module analyzed incompletely\ncan hold the access that credits it.\n\nThe added test asserts the disclosure and the absence of a mutation together,\nso if a store-member mutation is ever introduced the gate has to be reasoned\nabout rather than inherited silently.\n\nThe field is additive-optional and absent on a clean run, so no envelope moves.\n\n* fix: make the caveat reach every surface, and the gates that guard it fail\n\nA slop audit of the output-honesty work, four reviewers on disjoint domains,\neach building their own binaries and reproducing before reporting.\n\nThe store-member caveat reached the JSON wire and almost nothing else. Eight\nrender sites hardcoded an empty caveat slice in the store-member branch, left\nfrom before store members were registered, and because the four pr-comment and\nreview formats render from the CodeClimate description, that one site cascaded\ninto all four. Nine of eleven surfaces. The eighth site was not in the reported\nlist: the human summary rollup chained eight arrays and omitted the ninth, so\n`--summary` undercounted.\n\nIt shipped green because no fixture on any of those surfaces built a member\nfinding carrying a caveat. The fixtures now do, everywhere, so the class cannot\nreturn silently.\n\nTwo gates could not fail. `every_machine_consumed_format_carries_the_caveat`\nomitted `sarif` from its list, the format GitHub code scanning ingests, so\ndeleting the caveat from every SARIF message left it green. The summary rollup\ntest gave each finding exactly one caveat, making per-token over-counting\nundetectable, though a finding whose own file is degraded really carries two.\nBoth now go red under the mutation that previously passed.\n\nThe MCP byte gate was measuring a number no commit ever had: the recorded mark\nwas arrived at by summing deltas by hand rather than running the gate, which\nleft the stale-headroom guard passing vacuously with a 602-byte dead zone. It\nis re-pinned from a real measurement, and the comment says how.\n\n`max_output_bytes` told agents that exceeding the cap returns a preview. It\nreturns a refusal. An agent lowering the cap to bound its context was planning\non a bounded result and getting a run that returns no data.\n\nNine claims in the compatibility document and changelog were falsified by\nrunning what they described, including the combined envelope having no\n`dead_code` block, a counter documented as counting removals that counts files,\nand a changelog that still said store members stay out twenty lines from its\nown sentence saying they do not. The three entries are rewritten from about\n1790 words to about 875, structured as what changed, which findings, what you\nmust do, what stays the same, with the archaeology moved out.\n\nAlso: two dependency findings in one package.json shared a SARIF fingerprint,\nso GitHub collapsed two alerts into one; the human directory rollup labelled a\nfile as a directory; and four test names promised more than their bodies\nchecked.",
          "timestamp": "2026-09-08T22:39:13+02:00",
          "tree_id": "57ebe14e0632c954e383a18e842ee6a390cdcf3a",
          "url": "https://github.com/fallow-rs/fallow/commit/e5e92c787465f81e6fcd8f84a33be5b7f6ee93ac"
        },
        "date": 1788900532376,
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
        "date": 1788937516892,
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
        "date": 1788949448323,
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
          "id": "9dc043deca19489fdbc7dfd382e53d95c5c8e114",
          "message": "fix(coverage): point the post-upload dashboard link at the repository route\n\nCloses #2597",
          "timestamp": "2026-09-09T12:49:28+02:00",
          "tree_id": "c1144f3de6279ebaf6a926203038832f530859fa",
          "url": "https://github.com/fallow-rs/fallow/commit/9dc043deca19489fdbc7dfd382e53d95c5c8e114"
        },
        "date": 1788951545138,
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
          "id": "0b2922747231a43c269b2e555e0f366f6025e294",
          "message": "fix(coverage): match cloud runtime functions through path prefixes and runtime names\n\nCloses #2593.\n\nThe cloud join compared the full runtime file path against the repo-relative\nstatic index and required both sides to spell the function name the same way.\nThe busiest functions failed both tests. A containerized service reports\n/app/src/a.ts, which never equals src/a.ts, so whole files were dropped; and\nruntime instrumentation names a function from its surroundings, so an anonymous\ncallback arrives under the name of the callee it was passed to and an accessor\nkeeps its get prefix, neither of which the static index spells that way. Most of\nthe payload landed in the cloud_functions_unmatched warning instead of in\nfindings, and the hot-path list was led by whatever incidental helper survived.\n\nRuntime paths are now rebased onto the local tree by file name plus a\nsegment-wise suffix comparison, and a function whose name disagrees is matched\non position within the resolved file. Both tiers refuse an ambiguous answer\nrather than guess: two local files equally entitled to one runtime path, or two\ndefinitions opening on one line with no end line to separate them, stay\nunmatched. Stable-id matching is unchanged and still runs first.\n\n--debug-unmatched lists what remains on stderr, highest traffic first, so the\nresidue can be read without a debugger; stdout stays machine-readable.\n\nThe fixture test builds a real static index from a project holding a top-level\narrow, an object-literal method, an accessor and two callee-named callbacks,\nthen merges a snapshot whose paths carry a container prefix. Without the path\nrebase all six functions are dropped; with the rebase alone the three\nruntime-named ones are still dropped.",
          "timestamp": "2026-09-09T13:42:19+02:00",
          "tree_id": "3aa835b93bf50be2c00205eaf4792c758f62b7fe",
          "url": "https://github.com/fallow-rs/fallow/commit/0b2922747231a43c269b2e555e0f366f6025e294"
        },
        "date": 1788954674644,
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
          "id": "adeca23ebe17e714a27e7a4f57300cdefd3d283b",
          "message": "fix: let license refresh fall back to a full-access API key\n\nCloses #2595",
          "timestamp": "2026-09-09T14:43:18+02:00",
          "tree_id": "888f0016c2a13aeacb9a60ab716f4e87e075cbe7",
          "url": "https://github.com/fallow-rs/fallow/commit/adeca23ebe17e714a27e7a4f57300cdefd3d283b"
        },
        "date": 1788958244678,
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
        "date": 1788958777341,
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
        "date": 1788963497848,
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
          "id": "5cd805c63be517cecff3e0ddcc7c52e7422555b7",
          "message": "feat(mcp): read runtime coverage from fallow cloud\n\nAdds get_cloud_runtime_context, an MCP tool that pulls a repository's runtime facts from fallow cloud and returns the same runtime_coverage block the local runtime-coverage tools return, backed by fallow coverage analyze --cloud --format json. The API key is read from FALLOW_API_KEY in the server environment and is never a tool parameter; a call without one is refused before any subprocess starts with code cloud_api_key_missing and the CLI's own remediation sentence, now shared through fallow-types.\n\nCloses #2596",
          "timestamp": "2026-09-09T17:16:46+02:00",
          "tree_id": "4050e0734283a649176268b4df2849375a2863d8",
          "url": "https://github.com/fallow-rs/fallow/commit/5cd805c63be517cecff3e0ddcc7c52e7422555b7"
        },
        "date": 1788967635934,
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
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "3b7cb01a1e4cdff6f3a9d700c7cddce7178bebad",
          "message": "ci: narrow the apt freshness exemption to the frozen security suite",
          "timestamp": "2026-09-09T17:36:51+02:00",
          "tree_id": "c7a13975adfb045d61873173f0cf95438448383a",
          "url": "https://github.com/fallow-rs/fallow/commit/3b7cb01a1e4cdff6f3a9d700c7cddce7178bebad"
        },
        "date": 1788968807539,
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
          "id": "4163a2a7664eee3bb37b371fa97bc1ad39156496",
          "message": "fix(coverage): stop calling a test-only export safe to delete under --production\n\nCloses #2594",
          "timestamp": "2026-09-09T18:36:40+02:00",
          "tree_id": "4f0a6315d5dcd9ed69a1d13d618115155f6bb47f",
          "url": "https://github.com/fallow-rs/fallow/commit/4163a2a7664eee3bb37b371fa97bc1ad39156496"
        },
        "date": 1788972341894,
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
        "date": 1788975789429,
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
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "3628395fb2a16a1fd8edf83eb459ac6f45d17723",
          "message": "ci: stop a third-party apt source failing the cross-compile jobs",
          "timestamp": "2026-09-09T20:25:31+02:00",
          "tree_id": "cee49837dca8619aff2c26878e6b9614f934d58a",
          "url": "https://github.com/fallow-rs/fallow/commit/3628395fb2a16a1fd8edf83eb459ac6f45d17723"
        },
        "date": 1788978889536,
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
        "date": 1788991671065,
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
            "email": "bart@waardenburg.dev",
            "name": "Bart Waardenburg",
            "username": "BartWaardenburg"
          },
          "distinct": true,
          "id": "4b8d6915926a8e0b3d39b8d21995423a1d6705f2",
          "message": "chore: advance the schema policy baseline to v3.24.1",
          "timestamp": "2026-09-10T02:13:08+02:00",
          "tree_id": "afa911f33fc2506ab7b5d0e7fc2d3428c334c970",
          "url": "https://github.com/fallow-rs/fallow/commit/4b8d6915926a8e0b3d39b8d21995423a1d6705f2"
        },
        "date": 1788999761448,
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
          "id": "69d8d818a9d6b094545dae2f5c476cb0870feaa3",
          "message": "feat: support positional PATH scope across file commands\n\nAdds an optional positional [PATH] to bare fallow, check, dupes, health, audit, security, fix, list, and similar-code. The scope narrows reported findings to the file or directory while the full project graph is still built, so cross-file facts stay sound.\n\nResolution is root-first for bare relative paths, honors ./ and ../ as current-directory claims, and rejects missing or outside-root paths with an actionable exit-2 error. Scope composes with --workspace as one more workspace root and intersects with --changed-since and --diff-file. Audit narrows its changed-file universe so verdict and base attribution stay coherent; its base pass stays unscoped because it runs in another worktree.",
          "timestamp": "2026-09-10T09:51:56+02:00",
          "tree_id": "19d0ed01da89ec323b163b9b7d303bc03dfbc6d9",
          "url": "https://github.com/fallow-rs/fallow/commit/69d8d818a9d6b094545dae2f5c476cb0870feaa3"
        },
        "date": 1789027295872,
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
          "id": "b7722aff930fdad053d4d811b8e5fb055ff1ef44",
          "message": "feat(coverage): report the caller-edge size guard in the inventory blob\n\nCloses #2607",
          "timestamp": "2026-09-10T10:31:42+02:00",
          "tree_id": "11ef3a131bf22aca2845d8069ed0526ea2b43240",
          "url": "https://github.com/fallow-rs/fallow/commit/b7722aff930fdad053d4d811b8e5fb055ff1ef44"
        },
        "date": 1789029644463,
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
          "id": "423ce06892b814c76c22d9e745df44aafa25d732",
          "message": "fix(coverage): index instrumenter-named callbacks and object members for the cloud join\n\nCloses #2606",
          "timestamp": "2026-09-10T09:47:57Z",
          "tree_id": "ca11dae5f453b957d1c02ac10cf747052331515c",
          "url": "https://github.com/fallow-rs/fallow/commit/423ce06892b814c76c22d9e745df44aafa25d732"
        },
        "date": 1789034194720,
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
          "id": "03aad8caad3c563481e2b00b5c0d7c67f2a371d8",
          "message": "fix(cli): print forward slashes in the check and health human output\n\nThe two human renderers rendered the platform path separator, so a Windows\nuser was told `src\\a.ts` while `dupes`, `list`, `fix`, and every JSON\nsurface said `src/a.ts`. The damage was not only cosmetic: the dimmed-directory\n/ bold-filename split keys on `/`, so a native-separator path also lost its\nemphasis and rendered as one bold blob. Every path the check and health\nrenderers put on screen now goes through the existing display helper, the way\nthe rest of the CLI already did. On-disk path handling is untouched; only the\nrendered text changes, and it is byte-identical on POSIX.\n\n`scope_path_tests` caught this as three Windows-only failures, and its\nnegative assertions were the reason it caught no more: `!contains(\"other/c.ts\")`\npassed vacuously while the file was on screen as `other\\c.ts`. The captured\noutput is now normalised once before every assertion, so a leaked out-of-scope\nfile trips the suite under either separator. Two renderer unit tests pin the\nforward-slash convention itself, since the integration tests no longer can.\n\nCloses #2611",
          "timestamp": "2026-09-10T14:35:15+02:00",
          "tree_id": "8c55301281961a9e4bd58b54ef7b236a3b08eb9a",
          "url": "https://github.com/fallow-rs/fallow/commit/03aad8caad3c563481e2b00b5c0d7c67f2a371d8"
        },
        "date": 1789044110982,
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
        "date": 1789111124561,
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
        "date": 1789117382868,
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
          "id": "5c1add6b22533b9d580932edbd728dded915643e",
          "message": "fix(audit): scope base snapshot to sparse cone and analysis subdir\n\nfallow audit hung to the CI timeout on GitHub Actions runners for sparse checkouts of large monorepos. The raw object materialization introduced in 3.4.2 reads every blob in the base commit, so on a blobless partial clone each out-of-cone blob triggers a lazy promisor fetch.\n\nmaterialize_committed_tree now filters the committed tree through a MaterializationScope before touching blobs. Verified against a local blobless partial clone: the old binary fetched all 300 out-of-cone blobs, the fixed binary only in-cone blobs, with identical audit output apart from the telemetry id. verify:fast green, rust-review APPROVE.\n\nFixes #2615.",
          "timestamp": "2026-09-11T21:03:09+02:00",
          "tree_id": "47abbcb18e5c6ecfe0b2ca98a45a1f1b60441e8d",
          "url": "https://github.com/fallow-rs/fallow/commit/5c1add6b22533b9d580932edbd728dded915643e"
        },
        "date": 1789153977572,
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
      }
    ]
  }
}