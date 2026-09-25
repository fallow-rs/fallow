window.BENCHMARK_DATA = {
  "lastUpdate": 1790314718665,
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
          "id": "17c82aa7c56b2e946b6d0905355e532d4c7b5d77",
          "message": "fix(cli): isolate reconcile failures per fingerprint so every stale thread resolves (#2562)\n\nA staged provider lifecycle is a flat list across every stale fingerprint, and both apply loops returned on the first error, so one failed mutation discarded every remaining operation, including the resolution replies of unrelated fingerprints. A failure now blocks only the remaining operations of the same fingerprint, on GitHub and on GitLab alike, and a failed thread resolve still blocks that fingerprint's own marker reply so a later run can tell a failed resolve from a reopened lifecycle.\n\npost-review now reports failed_fingerprints and unapplied_fingerprints, both omitted when empty, so a dropped resolution is visible instead of silent, and both review.sh wrappers name the unapplied fingerprints in their warning. The GitHub wrapper's warning gate was dead: jq binds | looser than or, so the unparenthesised condition always raised 'boolean has no length' and the error was swallowed by the redirect. It now matches the GitLab form.\n\nNo review-mutating endpoint is added: the only PATCH targets the sticky issue comment. The content-free 'reviewed' row that follows a resolution reply is GitHub's own wrapper around a standalone review-comment reply, documented in the code and in cli-internals. Retry policy and the all-or-nothing preflight gate are deliberately unchanged.",
          "timestamp": "2026-09-07T15:51:35+02:00",
          "tree_id": "c274ceeb7e4bbe5dc5e81b32c8bdc91f6f43d649",
          "url": "https://github.com/fallow-rs/fallow/commit/17c82aa7c56b2e946b6d0905355e532d4c7b5d77"
        },
        "date": 1788789390463,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9832956,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50628,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1180056,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8406,
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
          "id": "dc8656e74ff5e58df08ccd03bc8aabac4966c594",
          "message": "test: cover guard multi-file batches and candidate loader errors\n\nTwo gaps salvaged from unlanded optimization branches, without the\noptimizations themselves.\n\nEvery existing guard test passes exactly one file, so nothing caught state\nleaking across files in a batch. The new test asserts a four-file report\nserializes identically to the four single-file reports concatenated, and pins\nthe per-file rule-id ordering, including that an unparsable files glob applies\nto every file.\n\nload_candidate_map had no coverage of its three rejection paths. The verdict\nside was tested, the candidate side was not, so a missing security_findings\narray, a malformed finding, and a duplicate finding_id were all unguarded.",
          "timestamp": "2026-09-07T17:03:32+02:00",
          "tree_id": "63b88877e3eddebf22d21f468e060de7c6727e75",
          "url": "https://github.com/fallow-rs/fallow/commit/dc8656e74ff5e58df08ccd03bc8aabac4966c594"
        },
        "date": 1788793811879,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9864621,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50675,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1187362,
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
          "id": "9e690609e1f918887937d3ff5d890adba4f2feba",
          "message": "fix(cli): bound the coordination-gap lines on the human review brief (#2565)\n\nThe brief printed one line per gap, joining every consumed symbol and both full paths. On a zod change to a module a barrel re-exports, one line rendered at 955 columns; a project with thirty out-of-diff consumers produced sixty lines at ~96 columns. They sat directly under an impact-closure summary that holds to eighty.\n\nThe section now states how many consumers sit outside the diff, walks the three that take the most symbols (the consumer on its own line, the contract it consumes on the next, as a branching split renders), and closes with the remainder and where to read it. Paths shorten from the left through elide_path; the symbol list fills a budget and cuts with a +N more suffix, shortened from the right because a symbol's leading characters identify it.\n\nOrdering by symbols taken rather than by path matters: the JSON gap list is path-sorted with no ranking, so an alphabetical prefix collapsed the 26-symbol barrel consumer behind the remainder. The header says 'use exports of changed files' because collect_coordination_gaps never verifies the export itself changed.\n\nRendering only. impact_closure.coordination_gap still carries every gap with every symbol, so the JSON contract and schema_version are untouched, and a new test pins that invariant next to the sibling fields that are capped.\n\nzod: 955 columns -> 77. Thirty consumers: 60 lines -> 9, none over 78.",
          "timestamp": "2026-09-07T17:26:51+02:00",
          "tree_id": "4a4fb606e3ddc28d76caf0267d08b4b45f27b3f6",
          "url": "https://github.com/fallow-rs/fallow/commit/9e690609e1f918887937d3ff5d890adba4f2feba"
        },
        "date": 1788795193371,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9821035,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50605,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1169391,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8369,
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
          "id": "ca508ed0aaca90e0ac3d23bb61d8f60f943ed935",
          "message": "refactor: replace copied helpers with one definition each\n\nFour report surfaces in the graph crate each carried a byte-identical\nrelativize, and their doc comments named the other copies as the thing to\nkeep in sync. A cross-platform key invariant shared by four outputs was\nenforced by prose. One pub(super) helper now owns it.\n\nranges_to_gaps and push_region were copied whole from sfc into astro, which\nalready imports SourceRegion from sfc. The sorted-input precondition the\nfunction depends on was implied only by the sort call sitting above it, so it\nis now written down.\n\nline_range_from_byte_col was defined twice, and hover wrapped\nutf16_col_span in a forwarder that added nothing. Both move to position.rs,\nwhich already owns the byte-column to UTF-16 boundary.",
          "timestamp": "2026-09-07T18:46:01+02:00",
          "tree_id": "4e1407a84e73c484101ba6e0d89ec4709332933c",
          "url": "https://github.com/fallow-rs/fallow/commit/ca508ed0aaca90e0ac3d23bb61d8f60f943ed935"
        },
        "date": 1788799847275,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9871165,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50687,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1196299,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8452,
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
          "id": "f63c4dbff717c1e9037d3290174d425d282340e7",
          "message": "fix(cli): wrap the decision-surface and focus lines to eighty columns (#2566)\n\nThe two sections the brief leads with were the two that wrapped unpredictably in a terminal. The decision surface printed its question, trade-off and expert list on single unbounded lines: on zod, a question naming every widened export ran to 159 columns. The focus map put an un-elided path and an unbounded reason on one row, reaching 103.\n\nBoth now wrap under a hanging indent. Prose is re-flowed rather than cut: a decision question ends in the actual ask, so truncating it would drop the question. Only a single word that alone overruns its line is shortened, from whichever end identifies it: a path keeps its tail, an owner identity keeps its head. The continuation indents give the block a 2 / 5 / 7 hierarchy so column 5 stays the key column.\n\nTwo defects found in review and fixed: the trailing blank line after a populated decision block was dropped, so the apex ran straight into the drill-down header; and the ask line double-counted its bus-factor reservation and elided owner identities from the wrong end, rendering a 52-character email as '.../ame.lastname@engineering.example.com' with fifteen columns free.\n\nRendering only; no field, format or schema moves. Zero lines over eighty across three fixtures, in both focus branches.",
          "timestamp": "2026-09-07T19:17:01+02:00",
          "tree_id": "9821252e4f154e68aaaf5b19df1d0b7dfdd9b64d",
          "url": "https://github.com/fallow-rs/fallow/commit/f63c4dbff717c1e9037d3290174d425d282340e7"
        },
        "date": 1788801970070,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9861933,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50674,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1185376,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8406,
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
          "id": "cd9b2e46a8a4f19ceab9c5d6e060abe133c16dcd",
          "message": "refactor: remove test-only shadows and copied catalogues\n\nAn audit across eight domains, each challenged by an independent reviewer,\nfound the same shape repeatedly: logic copied into a second place, then\nasserted against itself.\n\nSeveral test modules re-implemented the production function they claimed to\ncover, so the tests passed against their own copy while the real code was\nnever exercised. report/ci/severity.rs was an entirely cfg(test) shadow of\nmappings owned by fallow-output and fallow-config; sarif.rs, codeclimate.rs\nand serde_path.rs each kept a second copy of a function and asserted it\nagainst that copy. Each removal names the executed assertion elsewhere that\nstill covers the contract.\n\nThe React runtime dependency gate existed in six detectors under four names,\nthe security binding-trace catalogue in two, and the jsonc dialect catalogue\nin two crates. Forwarding wrappers that added nothing to their callee are\ngone, as are the three is_config_fixable tests left behind when the function\nmoved to fallow-config.\n\nBehavior is unchanged throughout. The whole workspace suite, the JS suites,\nthe contract-drift check and the agent-adapter check all pass.",
          "timestamp": "2026-09-07T20:08:45+02:00",
          "tree_id": "9e1227668769b378099f057f2fe83efd032c26f3",
          "url": "https://github.com/fallow-rs/fallow/commit/cd9b2e46a8a4f19ceab9c5d6e060abe133c16dcd"
        },
        "date": 1788805608697,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9853645,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50665,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1191930,
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
        "date": 1788865605565,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9819693,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50679,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1187497,
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
        "date": 1788867232560,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9867870,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50725,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1186394,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8416,
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
          "id": "c178aafaa0545cf499c9b9cc9a2f32cdd0b33843",
          "message": "fix: expose cyclomatic metric populations (#2569)\n\nfix: explain cyclomatic complexity populations",
          "timestamp": "2026-09-08T13:47:23+02:00",
          "tree_id": "981d36c6070bf96456e3867ca8577b10993f4ea6",
          "url": "https://github.com/fallow-rs/fallow/commit/c178aafaa0545cf499c9b9cc9a2f32cdd0b33843"
        },
        "date": 1788868329162,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9868986,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50729,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1195540,
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
        "date": 1788881013693,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9893558,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50758,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1185333,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8409,
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
          "id": "147004e79ecb8cbe0b8059036021c09596942fae",
          "message": "Merge pull request #2590 from fallow-rs/feat/dependabot-batch\n\nchore: batch Dependabot updates",
          "timestamp": "2026-09-08T18:13:16+02:00",
          "tree_id": "8548dc61538a4e6f585ecbbc514d4ac74197ed5f",
          "url": "https://github.com/fallow-rs/fallow/commit/147004e79ecb8cbe0b8059036021c09596942fae"
        },
        "date": 1788884332984,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9870926,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50738,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1204074,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8489,
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
          "id": "e5e92c787465f81e6fcd8f84a33be5b7f6ee93ac",
          "message": "fix: make the caveat reach every surface, and the gates that guard it fail\n\n* fix: disclose incomplete evidence on unused store members\n\nEight of the nine dead-code finding types tell a reader when their verdict\nrests on a file the run never fully read. Store members were the ninth, left\nout on the argument that they expose no mutation on any surface.\n\nThat argument is correct and still holds: a store member has no `Fix` action,\nno LSP code action, and `store_members_never_offer_unverified_line_deletions`\npins the absent review suggestion. There is nothing here to withhold.\n\nBut the caveat is a disclosure before it is a gate. A reader deciding by hand\nwhether to delete a store member deserves the same hint as the eight arrays\nbeside it, and a run that reports eight caveated findings and one bare one\nreads as though the bare one were better evidenced.\n\nStore members take the member rule unchanged, the same one class and enum\nmembers take: member usage is collected by one walk over the accesses of every\nmodule the run parsed, reachable or not, so any module analyzed incompletely\ncan hold the access that credits it.\n\nThe added test asserts the disclosure and the absence of a mutation together,\nso if a store-member mutation is ever introduced the gate has to be reasoned\nabout rather than inherited silently.\n\nThe field is additive-optional and absent on a clean run, so no envelope moves.\n\n* fix: make the caveat reach every surface, and the gates that guard it fail\n\nA slop audit of the output-honesty work, four reviewers on disjoint domains,\neach building their own binaries and reproducing before reporting.\n\nThe store-member caveat reached the JSON wire and almost nothing else. Eight\nrender sites hardcoded an empty caveat slice in the store-member branch, left\nfrom before store members were registered, and because the four pr-comment and\nreview formats render from the CodeClimate description, that one site cascaded\ninto all four. Nine of eleven surfaces. The eighth site was not in the reported\nlist: the human summary rollup chained eight arrays and omitted the ninth, so\n`--summary` undercounted.\n\nIt shipped green because no fixture on any of those surfaces built a member\nfinding carrying a caveat. The fixtures now do, everywhere, so the class cannot\nreturn silently.\n\nTwo gates could not fail. `every_machine_consumed_format_carries_the_caveat`\nomitted `sarif` from its list, the format GitHub code scanning ingests, so\ndeleting the caveat from every SARIF message left it green. The summary rollup\ntest gave each finding exactly one caveat, making per-token over-counting\nundetectable, though a finding whose own file is degraded really carries two.\nBoth now go red under the mutation that previously passed.\n\nThe MCP byte gate was measuring a number no commit ever had: the recorded mark\nwas arrived at by summing deltas by hand rather than running the gate, which\nleft the stale-headroom guard passing vacuously with a 602-byte dead zone. It\nis re-pinned from a real measurement, and the comment says how.\n\n`max_output_bytes` told agents that exceeding the cap returns a preview. It\nreturns a refusal. An agent lowering the cap to bound its context was planning\non a bounded result and getting a run that returns no data.\n\nNine claims in the compatibility document and changelog were falsified by\nrunning what they described, including the combined envelope having no\n`dead_code` block, a counter documented as counting removals that counts files,\nand a changelog that still said store members stay out twenty lines from its\nown sentence saying they do not. The three entries are rewritten from about\n1790 words to about 875, structured as what changed, which findings, what you\nmust do, what stays the same, with the archaeology moved out.\n\nAlso: two dependency findings in one package.json shared a SARIF fingerprint,\nso GitHub collapsed two alerts into one; the human directory rollup labelled a\nfile as a directory; and four test names promised more than their bodies\nchecked.",
          "timestamp": "2026-09-08T22:39:13+02:00",
          "tree_id": "57ebe14e0632c954e383a18e842ee6a390cdcf3a",
          "url": "https://github.com/fallow-rs/fallow/commit/e5e92c787465f81e6fcd8f84a33be5b7f6ee93ac"
        },
        "date": 1788900277803,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9866502,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50723,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1202018,
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
        "date": 1788937261935,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9880038,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50741,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1187872,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8363,
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
          "id": "85615b646a1adb45a30bf24a45ffa55dfa088737",
          "message": "fix(dupes): fall back to content when the fingerprint cannot be trusted\n\nAdding ctime to the extract fingerprint closed a real staleness bug, but the\nduplication token cache reacted to a missing ctime by returning nothing rather\nthan by falling back. ctime is always absent on Windows, so that cache never\nhit there: correct results, and a permanently cold cache on an entire platform.\nThe release validation caught it; a reviewer had noted the behaviour earlier as\na clean miss, which is true and was too light a reading.\n\nIt now takes the same fast-path and slow-path split the extract cache already\nhad. When the timestamps can be trusted, an exact match or a miss, unchanged.\nWhen they cannot, read the file and compare content hashes against the source\nthe entry already stores, so no cache version moves.\n\nThe invariant the ctime work exists to protect survives, because the fallback\ntrusts bytes rather than metadata: a size-preserving edit with a restored mtime\nstill misses, on every platform. The new tests build a fingerprint without a\nctime directly, so a macOS run proves the Windows path rather than skipping it.\n\nThe graph cache was checked and is unaffected: it keys on content hashes and\ndeliberately avoids ctime, since cp -Rp and CI cache restores preserve mtime\nbut reset it.",
          "timestamp": "2026-09-09T12:16:58+02:00",
          "tree_id": "58042834076bade476e9312b54f764a1dd71f3f8",
          "url": "https://github.com/fallow-rs/fallow/commit/85615b646a1adb45a30bf24a45ffa55dfa088737"
        },
        "date": 1788949277852,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9877086,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50737,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1184962,
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
          "id": "9dc043deca19489fdbc7dfd382e53d95c5c8e114",
          "message": "fix(coverage): point the post-upload dashboard link at the repository route\n\nCloses #2597",
          "timestamp": "2026-09-09T12:49:28+02:00",
          "tree_id": "c1144f3de6279ebaf6a926203038832f530859fa",
          "url": "https://github.com/fallow-rs/fallow/commit/9dc043deca19489fdbc7dfd382e53d95c5c8e114"
        },
        "date": 1788951333020,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9865486,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50734,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1203527,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8458,
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
          "id": "0b2922747231a43c269b2e555e0f366f6025e294",
          "message": "fix(coverage): match cloud runtime functions through path prefixes and runtime names\n\nCloses #2593.\n\nThe cloud join compared the full runtime file path against the repo-relative\nstatic index and required both sides to spell the function name the same way.\nThe busiest functions failed both tests. A containerized service reports\n/app/src/a.ts, which never equals src/a.ts, so whole files were dropped; and\nruntime instrumentation names a function from its surroundings, so an anonymous\ncallback arrives under the name of the callee it was passed to and an accessor\nkeeps its get prefix, neither of which the static index spells that way. Most of\nthe payload landed in the cloud_functions_unmatched warning instead of in\nfindings, and the hot-path list was led by whatever incidental helper survived.\n\nRuntime paths are now rebased onto the local tree by file name plus a\nsegment-wise suffix comparison, and a function whose name disagrees is matched\non position within the resolved file. Both tiers refuse an ambiguous answer\nrather than guess: two local files equally entitled to one runtime path, or two\ndefinitions opening on one line with no end line to separate them, stay\nunmatched. Stable-id matching is unchanged and still runs first.\n\n--debug-unmatched lists what remains on stderr, highest traffic first, so the\nresidue can be read without a debugger; stdout stays machine-readable.\n\nThe fixture test builds a real static index from a project holding a top-level\narrow, an object-literal method, an accessor and two callee-named callbacks,\nthen merges a snapshot whose paths carry a container prefix. Without the path\nrebase all six functions are dropped; with the rebase alone the three\nruntime-named ones are still dropped.",
          "timestamp": "2026-09-09T13:42:19+02:00",
          "tree_id": "3aa835b93bf50be2c00205eaf4792c758f62b7fe",
          "url": "https://github.com/fallow-rs/fallow/commit/0b2922747231a43c269b2e555e0f366f6025e294"
        },
        "date": 1788954428330,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9833598,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50718,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1191058,
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
          "id": "adeca23ebe17e714a27e7a4f57300cdefd3d283b",
          "message": "fix: let license refresh fall back to a full-access API key\n\nCloses #2595",
          "timestamp": "2026-09-09T14:43:18+02:00",
          "tree_id": "888f0016c2a13aeacb9a60ab716f4e87e075cbe7",
          "url": "https://github.com/fallow-rs/fallow/commit/adeca23ebe17e714a27e7a4f57300cdefd3d283b"
        },
        "date": 1788958045765,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9839796,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50675,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1182019,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8365,
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
          "id": "4e9a9c39b74edaacd7945addb493ec35f9287d28",
          "message": "fix(dupes): let content settle a cache lookup the timestamps cannot\n\nThe first pass gave the token cache a content fallback only when the\nfingerprint was untrustworthy, which fixed Windows and left the two platforms\nbehaving differently: a touch re-tokenized on Unix and hit on Windows.\n\nMetadata is the fast path, not the verdict. A match settles the lookup without\ntouching the disk; a mismatch only means the timestamps cannot settle it, so\ncontent decides, on every platform. A file whose bytes never moved now survives\na touch or a checkout that rewrites timestamps.\n\nThe staleness invariant is unchanged because content is the authority in both\nbranches: a size-preserving edit with a restored mtime still misses.\n\nOne test asserted the old rule, that a metadata mismatch means a miss. It is\nreplaced by the two properties that are actually worth holding: timestamps\nmoving alone hits, timestamps and content moving together misses.",
          "timestamp": "2026-09-09T14:51:53+02:00",
          "tree_id": "c79a453c179bbba0c3304652645d9a19f31bec2b",
          "url": "https://github.com/fallow-rs/fallow/commit/4e9a9c39b74edaacd7945addb493ec35f9287d28"
        },
        "date": 1788958714899,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9864670,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50719,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1198387,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8464,
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
          "id": "8b875bdf06105317ab1b94c4493cf639c9e4839b",
          "message": "chore: release v3.24.0",
          "timestamp": "2026-09-09T15:58:39+02:00",
          "tree_id": "634fecbf4bb35acf7ebd8db7434cea2872ccce3b",
          "url": "https://github.com/fallow-rs/fallow/commit/8b875bdf06105317ab1b94c4493cf639c9e4839b"
        },
        "date": 1788962741050,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9879470,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50745,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1198134,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8453,
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
          "id": "5cd805c63be517cecff3e0ddcc7c52e7422555b7",
          "message": "feat(mcp): read runtime coverage from fallow cloud\n\nAdds get_cloud_runtime_context, an MCP tool that pulls a repository's runtime facts from fallow cloud and returns the same runtime_coverage block the local runtime-coverage tools return, backed by fallow coverage analyze --cloud --format json. The API key is read from FALLOW_API_KEY in the server environment and is never a tool parameter; a call without one is refused before any subprocess starts with code cloud_api_key_missing and the CLI's own remediation sentence, now shared through fallow-types.\n\nCloses #2596",
          "timestamp": "2026-09-09T17:16:46+02:00",
          "tree_id": "4050e0734283a649176268b4df2849375a2863d8",
          "url": "https://github.com/fallow-rs/fallow/commit/5cd805c63be517cecff3e0ddcc7c52e7422555b7"
        },
        "date": 1788967278476,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9837670,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50727,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1197348,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8411,
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
          "id": "4163a2a7664eee3bb37b371fa97bc1ad39156496",
          "message": "fix(coverage): stop calling a test-only export safe to delete under --production\n\nCloses #2594",
          "timestamp": "2026-09-09T18:36:40+02:00",
          "tree_id": "4f0a6315d5dcd9ed69a1d13d618115155f6bb47f",
          "url": "https://github.com/fallow-rs/fallow/commit/4163a2a7664eee3bb37b371fa97bc1ad39156496"
        },
        "date": 1788972120848,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9877974,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50740,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1193715,
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
        "date": 1788975582651,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9835942,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50723,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1181374,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8409,
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
          "id": "c2da9fcae5388226a87e986b1812f9c872d288c6",
          "message": "chore: release v3.24.1",
          "timestamp": "2026-09-09T21:36:33+02:00",
          "tree_id": "082d4b12b54247a074954b50c7c31d0af8936e9b",
          "url": "https://github.com/fallow-rs/fallow/commit/c2da9fcae5388226a87e986b1812f9c872d288c6"
        },
        "date": 1788991270439,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9885998,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50750,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1198586,
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
        "date": 1788999424121,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9882974,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50744,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1211454,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8482,
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
          "id": "69d8d818a9d6b094545dae2f5c476cb0870feaa3",
          "message": "feat: support positional PATH scope across file commands\n\nAdds an optional positional [PATH] to bare fallow, check, dupes, health, audit, security, fix, list, and similar-code. The scope narrows reported findings to the file or directory while the full project graph is still built, so cross-file facts stay sound.\n\nResolution is root-first for bare relative paths, honors ./ and ../ as current-directory claims, and rejects missing or outside-root paths with an actionable exit-2 error. Scope composes with --workspace as one more workspace root and intersects with --changed-since and --diff-file. Audit narrows its changed-file universe so verdict and base attribution stay coherent; its base pass stays unscoped because it runs in another worktree.",
          "timestamp": "2026-09-10T09:51:56+02:00",
          "tree_id": "19d0ed01da89ec323b163b9b7d303bc03dfbc6d9",
          "url": "https://github.com/fallow-rs/fallow/commit/69d8d818a9d6b094545dae2f5c476cb0870feaa3"
        },
        "date": 1789026961042,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9873902,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50732,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1197125,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8459,
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
          "id": "b7722aff930fdad053d4d811b8e5fb055ff1ef44",
          "message": "feat(coverage): report the caller-edge size guard in the inventory blob\n\nCloses #2607",
          "timestamp": "2026-09-10T10:31:42+02:00",
          "tree_id": "11ef3a131bf22aca2845d8069ed0526ea2b43240",
          "url": "https://github.com/fallow-rs/fallow/commit/b7722aff930fdad053d4d811b8e5fb055ff1ef44"
        },
        "date": 1789029414144,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9893422,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50765,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1182029,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8098,
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
          "id": "423ce06892b814c76c22d9e745df44aafa25d732",
          "message": "fix(coverage): index instrumenter-named callbacks and object members for the cloud join\n\nCloses #2606",
          "timestamp": "2026-09-10T09:47:57Z",
          "tree_id": "ca11dae5f453b957d1c02ac10cf747052331515c",
          "url": "https://github.com/fallow-rs/fallow/commit/423ce06892b814c76c22d9e745df44aafa25d732"
        },
        "date": 1789034020492,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9886174,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50750,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1182029,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8098,
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
          "id": "03aad8caad3c563481e2b00b5c0d7c67f2a371d8",
          "message": "fix(cli): print forward slashes in the check and health human output\n\nThe two human renderers rendered the platform path separator, so a Windows\nuser was told `src\\a.ts` while `dupes`, `list`, `fix`, and every JSON\nsurface said `src/a.ts`. The damage was not only cosmetic: the dimmed-directory\n/ bold-filename split keys on `/`, so a native-separator path also lost its\nemphasis and rendered as one bold blob. Every path the check and health\nrenderers put on screen now goes through the existing display helper, the way\nthe rest of the CLI already did. On-disk path handling is untouched; only the\nrendered text changes, and it is byte-identical on POSIX.\n\n`scope_path_tests` caught this as three Windows-only failures, and its\nnegative assertions were the reason it caught no more: `!contains(\"other/c.ts\")`\npassed vacuously while the file was on screen as `other\\c.ts`. The captured\noutput is now normalised once before every assertion, so a leaked out-of-scope\nfile trips the suite under either separator. Two renderer unit tests pin the\nforward-slash convention itself, since the integration tests no longer can.\n\nCloses #2611",
          "timestamp": "2026-09-10T14:35:15+02:00",
          "tree_id": "8c55301281961a9e4bd58b54ef7b236a3b08eb9a",
          "url": "https://github.com/fallow-rs/fallow/commit/03aad8caad3c563481e2b00b5c0d7c67f2a371d8"
        },
        "date": 1789044040769,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9816612,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50644,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1173718,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8327,
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
          "id": "30167b42257ded3bef683f6c91b4aa4017a1ff59",
          "message": "chore: release v3.25.0",
          "timestamp": "2026-09-11T09:05:59+02:00",
          "tree_id": "beca44a718db4073802dc0e467c40169e0df45a3",
          "url": "https://github.com/fallow-rs/fallow/commit/30167b42257ded3bef683f6c91b4aa4017a1ff59"
        },
        "date": 1789110714603,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9877014,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50737,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1202104,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8438,
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
          "id": "bb3893aa62cc51a0be60037ea7ab5ccf9ade3d5a",
          "message": "chore: advance the schema policy baseline to v3.25.0",
          "timestamp": "2026-09-11T10:55:09+02:00",
          "tree_id": "e72ba4e1cece94196473dee09cb72bb686e081b5",
          "url": "https://github.com/fallow-rs/fallow/commit/bb3893aa62cc51a0be60037ea7ab5ccf9ade3d5a"
        },
        "date": 1789117292153,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9823476,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50651,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1179112,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8384,
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
          "id": "5c1add6b22533b9d580932edbd728dded915643e",
          "message": "fix(audit): scope base snapshot to sparse cone and analysis subdir\n\nfallow audit hung to the CI timeout on GitHub Actions runners for sparse checkouts of large monorepos. The raw object materialization introduced in 3.4.2 reads every blob in the base commit, so on a blobless partial clone each out-of-cone blob triggers a lazy promisor fetch.\n\nmaterialize_committed_tree now filters the committed tree through a MaterializationScope before touching blobs. Verified against a local blobless partial clone: the old binary fetched all 300 out-of-cone blobs, the fixed binary only in-cone blobs, with identical audit output apart from the telemetry id. verify:fast green, rust-review APPROVE.\n\nFixes #2615.",
          "timestamp": "2026-09-11T21:03:09+02:00",
          "tree_id": "47abbcb18e5c6ecfe0b2ca98a45a1f1b60441e8d",
          "url": "https://github.com/fallow-rs/fallow/commit/5c1add6b22533b9d580932edbd728dded915643e"
        },
        "date": 1789153699610,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9857358,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 50711,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1196740,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8438,
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
          "id": "1ee9062b92e2b4a002844bf4f778917433901e1b",
          "message": "feat(plugins): add built-in Oxfmt plugin\n\nBuilt-in Oxfmt plugin mirroring the Oxlint plugin: oxfmt.config.ts (and siblings) are marked always-used and static imports from TS configs are credited. Knip migration reports oxfmt sections as auto-detected.\n\nFixes #2614.",
          "timestamp": "2026-09-11T22:03:57+02:00",
          "tree_id": "b098fc0afebc4b7f9dabc7650fde84b911c42308",
          "url": "https://github.com/fallow-rs/fallow/commit/1ee9062b92e2b4a002844bf4f778917433901e1b"
        },
        "date": 1789157381765,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9920492,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51232,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1200402,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8476,
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
          "id": "8622017e6effcad51ddff304032914e08366de27",
          "message": "chore(audit): drop unused derives on MaterializationScope\n\nSlop-audit follow-up to the sparse base-snapshot change: the private scope struct is only borrowed, so Debug, Clone, and Default have no users.",
          "timestamp": "2026-09-12T11:02:14+02:00",
          "tree_id": "d54e94b9a600e1b58a89a66a6514e25c5da7be53",
          "url": "https://github.com/fallow-rs/fallow/commit/8622017e6effcad51ddff304032914e08366de27"
        },
        "date": 1789203957700,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9884204,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51219,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1216460,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8545,
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
          "id": "a013bc4285df07f9e706b1357de421222e9ebb4e",
          "message": "chore(deps): update rustls to 0.23.45 for RUSTSEC-2026-0285 (#2630)\n\nrustls 0.23.37 is affected by RUSTSEC-2026-0285: TLS 1.3 handshake messages are accepted across encryption level boundaries. Lockfile-only bump to 0.23.45, pulling rustls-webpki 0.103.13 to 0.103.15; both arrive transitively through ureq and neither is pinned.\n\nCargo Deny and Security Audit are required checks that only run on Rust paths, so this failed every open pull request touching a Rust file while main stayed green.",
          "timestamp": "2026-09-14T21:47:13+02:00",
          "tree_id": "7257b3005d74f4838d53fbaca6906f003d2f1c3a",
          "url": "https://github.com/fallow-rs/fallow/commit/a013bc4285df07f9e706b1357de421222e9ebb4e"
        },
        "date": 1789415509302,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9883220,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51226,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1205478,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8496,
            "unit": "allocations"
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
        "date": 1789428017815,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9935012,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51260,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1192120,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8398,
            "unit": "allocations"
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
        "date": 1789429072465,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9924700,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51235,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1195361,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8423,
            "unit": "allocations"
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
        "date": 1789429797893,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9924284,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51238,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1205069,
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
          "id": "f89a4d571e1a092e2b4b08dcb01489a1540e5f37",
          "message": "feat(plugins): recognize the Expo getNavOptions and generateMetadata exports (#2629)\n\nCompletes the Expo Router route-export list against the framework's own LoadedRoute type, following #2618 which added SuspenseFallback.\n\nExpo Router declares eight members on LoadedRoute. Six were recognized; getNavOptions and generateMetadata were not, so a route file exporting either was reported as an unused export. The author of #2618 flagged both and kept that change to a single export.\n\nRemoving the two plugin entries makes the fixture integration test fail on src/app/_layout.tsx:getNavOptions, so the added assertions are load-bearing. No interaction with the Next.js invalid-client-export rule, which also knows generateMetadata: that detector is gated on the project declaring next as a dependency, and the Expo fixture declares only expo and expo-router.",
          "timestamp": "2026-09-15T02:11:23+02:00",
          "tree_id": "f500de20b0ee63b6404728bbf56bfc04800b9411",
          "url": "https://github.com/fallow-rs/fallow/commit/f89a4d571e1a092e2b4b08dcb01489a1540e5f37"
        },
        "date": 1789431352658,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9894226,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51177,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1178550,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8379,
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
          "id": "c00331886f81edf7fe4dbf8e5ab4b784d9f363e8",
          "message": "fix(lsp): resolve per-path rule overrides on the editor analysis path (#2635)\n\nRule severity resolution, including overrides[].rules, lived in the check\ncommand, so the language server published diagnostics for rules a project had\nturned off for the matching path while fallow dead-code filtered them out.\nThe pass now lives in fallow-engine next to the sibling dead-code result\nfilters and runs inside EditorAnalysisSession, once per analyzed project slice\nand again after type-aware refinement because reconciliation can add\nfindings. The check command delegates to the same code, so CLI output is\nunchanged. The programmatic runtime behind MCP does not run the pass yet.\n\nFixes #2621",
          "timestamp": "2026-09-15T08:29:22+02:00",
          "tree_id": "7a08998b33bfa08ce5fbac050fa82773e1a0cfe7",
          "url": "https://github.com/fallow-rs/fallow/commit/c00331886f81edf7fe4dbf8e5ab4b784d9f363e8"
        },
        "date": 1789454079162,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9926052,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51236,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1208067,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8491,
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
          "id": "9dabdbd75f746848fdbe891e7217077774f62c06",
          "message": "fix(config): exclude build directories at any depth by default (#2632)\n\nThe built-in discovery ignore list anchored `build/**` at the project root\nwhile every neighboring generated-output default is recursive, so a monorepo\nthat keeps per-package output under `projects/*/apps/web/build/` had that\noutput walked, graphed and reported as unused files, unused exports,\nduplication and health findings. The default is now `**/build/**`, which also\nmatches how workspace discovery has always classified the directory name.\n\nEverything under a `build` segment now leaves analysis and `ignorePatterns`\ncannot bring it back, since the field has no negation. Hand-written source in\na nested `build/` directory is skipped; a workspace package literally named\n`build` keeps its workspace entry but none of its files are analyzed; a\nframework config under a nested `build/` directory is no longer read, so the\npath aliases it declares are lost; and an entry point that resolves into a\nnested `build/` directory stops seeding reachability. Tests pin each of these\nconsequences so they are recorded choices rather than surprises.\n\nFixes #2622",
          "timestamp": "2026-09-15T08:40:20+02:00",
          "tree_id": "ed405b4eb67ad91ceb0c87619cbe3a299ec1f42c",
          "url": "https://github.com/fallow-rs/fallow/commit/9dabdbd75f746848fdbe891e7217077774f62c06"
        },
        "date": 1789454676918,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9909459,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51184,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1197237,
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
          "id": "ea2e2e6bb27228b3465c2c18edeb756c89ee0549",
          "message": "fix(engine): resolve rule severity on every surface and watch every config file name (#2640)\n\nRule severity, including per-path overrides[].rules, is now resolved on every surface that reports findings: the programmatic runtime behind the MCP analyze and check_changed tools, the audit sub-analyses, Code Mode's combined run, the Node bindings and the decision surface run the same engine pass the CLI runs, once over the analyzed set and once after type-aware reconciliation. The CLI applies the second pass on --type-aware runs as the editor already did. The language server derives its watched-file registration and its semantic invalidation classifier from the config file names the loader accepts, so editing .fallowrc.json, .fallowrc.jsonc or .fallow.toml refreshes diagnostics. The MCP decision_surface tool judges its base snapshot by the head configuration, as the CLI does.\n\nFixes #2636",
          "timestamp": "2026-09-15T20:10:08+02:00",
          "tree_id": "c8031b0412f80a36f028f2dcbf58b4e3ce3e063f",
          "url": "https://github.com/fallow-rs/fallow/commit/ea2e2e6bb27228b3465c2c18edeb756c89ee0549"
        },
        "date": 1789496071396,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9891748,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51234,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1200521,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8496,
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
          "id": "39e13ca491388e3e67a372af0fd86d97319cdf15",
          "message": "feat(cli): add --fail-on-stale-baseline and align baseline staleness across commands (#2642)\n\nAdds the global --fail-on-stale-baseline flag: when a loaded baseline has any entry that matched nothing this run, the run exits 1 with one stderr line naming the stale count, the baseline path and the re-save command, in every output format and on dead-code, check, the bare run, dupes and health. A run that cannot judge the baseline (a narrowed scope, health --report-only, audit's changed-code slice, decision-surface's brief) stands down and says so on stderr. dupes --baseline gets the same partial-staleness warning and change-scope guard as dead-code and health through the shared predicate, no baseline warning fires on a project with no findings left to compare, and health's scope guard includes production mode so a fresh baseline is never called stale on an unchanged production run. JSON envelopes are unchanged.\n\nFixes #2637",
          "timestamp": "2026-09-15T20:16:22+02:00",
          "tree_id": "51819a833be59c4e7029f65ba08d67b73b6d99ca",
          "url": "https://github.com/fallow-rs/fallow/commit/39e13ca491388e3e67a372af0fd86d97319cdf15"
        },
        "date": 1789496485676,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9893588,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51208,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1185514,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8114,
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
          "id": "80ea4e5198b68bd42ef527d2e638dad44081374a",
          "message": "test(lsp): canonicalize the watched-config test root through the Windows-safe helper\n\nstd::fs::canonicalize yields a verbatim path on Windows, so the file URI the test waited on never matched the URI the server publishes and the Windows job failed on the first diagnostic. The test now uses the same helper as the other LSP tests.",
          "timestamp": "2026-09-15T20:43:15+02:00",
          "tree_id": "f56f96766246835fd2bb79ec57975d23410251d5",
          "url": "https://github.com/fallow-rs/fallow/commit/80ea4e5198b68bd42ef527d2e638dad44081374a"
        },
        "date": 1789498165074,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9924380,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51229,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1193193,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8435,
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
          "id": "8102e22e0afcc70fb518cd26ea1a2f18f5d710ff",
          "message": "chore: release v3.26.0",
          "timestamp": "2026-09-15T21:40:27+02:00",
          "tree_id": "6e3085f55cd4967cfb8184193c170c651b79f792",
          "url": "https://github.com/fallow-rs/fallow/commit/8102e22e0afcc70fb518cd26ea1a2f18f5d710ff"
        },
        "date": 1789501536808,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9932100,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51240,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1197803,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8396,
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
          "id": "5e509d0ce7a9c3ec63c07f73e7d00cf7ef17f8ba",
          "message": "chore(napi): sync package.json / package-lock / index.js to v3.26.0",
          "timestamp": "2026-09-16T02:08:38+02:00",
          "tree_id": "8159bceec3d91f5b09e92b3d0bf220c9fae7de69",
          "url": "https://github.com/fallow-rs/fallow/commit/5e509d0ce7a9c3ec63c07f73e7d00cf7ef17f8ba"
        },
        "date": 1789517648790,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9921347,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51196,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1193138,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8384,
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
          "id": "1080fe084b5a0fbd61e05f808664c82290427cd2",
          "message": "chore(deps-dev): bump rolldown to 1.2.7 in viz-frontend (#2671)\n\nThe viz bundle is checked in and byte-compared in CI, so the bundler bump\ncarries the rebuilt crates/cli/viz-assets/viz.js with it.",
          "timestamp": "2026-09-16T07:30:04+02:00",
          "tree_id": "99601523abb138f4109ab7329fb19cd109438a60",
          "url": "https://github.com/fallow-rs/fallow/commit/1080fe084b5a0fbd61e05f808664c82290427cd2"
        },
        "date": 1789536897200,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9915644,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51220,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1194903,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8396,
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
          "id": "de90d3109e5eee2bb7729f487c65476a3d0a2df2",
          "message": "fix(action): surface baseline staleness and propagate the stale-baseline gate in CI (#2674)\n\nThe dead-code baseline staleness warning and the opt-in stale-baseline gate shipped in 3.26.0 were unreachable through the GitHub Action and the GitLab template: both run with `--quiet --format json`, the warning is quiet-suppressed, stderr went to `::debug::`, and the CLI exit code was dropped whenever stdout parsed as JSON.\n\n- dead-code, dupes and health envelopes publish one shared `baseline_staleness` object (health's existing member names plus `current_findings`, `warning` and `gate_trips`); no schema version moves and the `HealthBaselineStaleness` TypeScript name stays as a deprecated alias.\n- The action and the GitLab template read the verdict from that object, never from the exit code, so `fail-on-issues: false` keeps its meaning; a new `fail-on-stale-baseline` input (`FALLOW_FAIL_ON_STALE_BASELINE` on GitLab) fails the job on a tripped gate.\n- On pull requests the primary run is change-scoped, so a second unscoped, report-discarding run judges the baseline whenever the action itself narrowed the run; the advisory lands as `::warning::`, a tripped gate as `::error::`, a stand-down as `::warning::`, and a line in the step summary.\n- Fail open on an older binary or a command without staleness; invalid input combinations are rejected at validation.\n\nFixes #2673",
          "timestamp": "2026-09-16T18:44:44+02:00",
          "tree_id": "04ca6836a569a821d7453c67930f0a5ce3c3bdc1",
          "url": "https://github.com/fallow-rs/fallow/commit/de90d3109e5eee2bb7729f487c65476a3d0a2df2"
        },
        "date": 1789577333105,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9923564,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51234,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1208428,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8465,
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
          "id": "5cebdde102efc436f89b7fc9878990c1b3647e0e",
          "message": "feat(output): publish gate_outcomes on every analysis envelope (#2692)\n\nEvery gate the CLI arms now publishes its outcome in the JSON envelope, computed once by the rule that decides the exit code, so a consumer that runs `--quiet --format json` and drops the exit code can still read the verdict.\n\n- Root optional `gate_outcomes` object keyed by gate name on dead-code, dupes, health, audit, security, combined and grouped envelopes, absent when no gate was armed. Each entry carries `status` (`pass`, `warn`, `fail`, `skipped`), `enforced` (whether the outcome affects the exit code on this run, false under `--report-only` and on combined machine output), and `observed`, `threshold` and `threshold_label` where a message needs numbers. The default exit rule of the command is always included when the object exists, so the object explains the exit code.\n- `workspace_diagnostics[].degrades_analysis` marks the diagnostics that mean the run analyzed less than the project, and a run that found no source files says so with its own diagnostic.\n- `fallow report --from` renders a neutral \"Gate outcomes\" line on the GitHub summary, annotations, PR comment and review targets, and the GitLab MR note, live and saved paths identical; the check-run conclusion and the exit code are unchanged.\n- No schema version moves; the gate name set is open on the wire. The 3.26.0 sentence that `--fail-on-stale-baseline` changes nothing but the exit code is superseded: it now also sets `enforced` on the stale-baseline entry.\n\nFixes #2682\nRefs #2680 #2681 #2683 #2684 #2685 #2686",
          "timestamp": "2026-09-17T02:33:28+02:00",
          "tree_id": "8433d161d981a823ae88dfb066c62a83aa25d36e",
          "url": "https://github.com/fallow-rs/fallow/commit/5cebdde102efc436f89b7fc9878990c1b3647e0e"
        },
        "date": 1789605548349,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9858866,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51133,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1179682,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8362,
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
          "id": "1645a5ee564500e38a37e774748c0f4d66c93bac",
          "message": "fix(action): read every gate verdict from the envelope and fail the job the inputs asked for (#2693)\n\nThe GitHub Action and the GitLab template read every gate verdict from the envelope's `gate_outcomes` object instead of the CLI exit code, which both dropped whenever stdout parsed as JSON. Each gate fails the job only when the input that owns it asked for it and the CLI reports the gate as failed and enforced; a gate that tripped through `args` alone warns instead, so `fail-on-issues: false` stays authoritative.\n\n- `fail-on-regression`, `threshold`, `min-severity` and the security gate inputs now deliver the verdict they promised; a new `min-score` input runs health with `--complexity` so findings, annotations and the comment are not emptied; `--dupes-threshold` reaches the bare command.\n- One failure accumulator prints every failing gate and exits once, after outputs, artifacts and the summary are written; the inline `Check threshold` step and the early return before the security check are gone, so both paths are covered by tests.\n- Behaviour change: a repository with a security gate configured and `fail-on-issues: false` now fails on a tripped gate; not configuring the gate is the opt-out. `command: audit` with `gate: all` and `fail-on-issues: false` stays a reporting configuration.\n- A binary without `gate_outcomes` falls back to `regression.exceeded`, `gate.verdict`, `baseline_staleness.gate_trips` and the type-aware meta; gates with no older field fail open with one warning, only when their input asked for them.\n- Workspace diagnostics that degrade analysis land as one aggregated warning, an empty analysis warns by default and fails behind `fail-on-empty-analysis` (`FALLOW_FAIL_ON_EMPTY_ANALYSIS` on GitLab), and gate outcomes are exposed as step outputs and a GitLab dotenv report.\n\nFixes #2680 #2681 #2683 #2684 #2685 #2686\nRefs #2682 #2692",
          "timestamp": "2026-09-17T03:57:56+02:00",
          "tree_id": "adc91d7a23a127af177427766993f17e444c98cd",
          "url": "https://github.com/fallow-rs/fallow/commit/1645a5ee564500e38a37e774748c0f4d66c93bac"
        },
        "date": 1789610543010,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9888388,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51226,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1193316,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8427,
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
          "id": "b846e626b559c75e021e61df59857b51e09b7f9a",
          "message": "feat(mcp): state the run's gate and baseline verdicts in the tool result (#2694)\n\nMCP tool results now state the run's verdicts that the CLI expressed only on stderr or through an exit code the server converts into a success result.\n\n- The envelope's root `warnings` array gains plain sentences: the baseline staleness advisory or a tripped stale-baseline gate with the re-save remedy, one entry per `gate_outcomes` entry reporting `fail` or `warn` with its numbers and whether it was enforced, and one aggregated entry for workspace diagnostics that degrade analysis.\n- Applied on the subprocess route, the Code Mode route and the typed API route, with sectioned lookups for `audit`, whose staleness and diagnostics live under `complexity.summary` and `dead_code`.\n- No envelope member moves, the result body stays JSON, a clean run is byte-identical to CLI stdout, and the `isError` policy for exit codes 2 and above is unchanged. `structured_content` is left unpopulated until a tool declares an output schema for it.\n\nFixes #2676\nRefs #2682 #2692 #2693",
          "timestamp": "2026-09-17T04:54:03+02:00",
          "tree_id": "891cc8b71330e699dfd9f65d4ede85eb706051ca",
          "url": "https://github.com/fallow-rs/fallow/commit/b846e626b559c75e021e61df59857b51e09b7f9a"
        },
        "date": 1789613987829,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9914532,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51220,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1200408,
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
        "date": 1789617307343,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9872554,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51151,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1183098,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8412,
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
          "id": "85b12cdaf782632837d02a892499d9e91e8b91b3",
          "message": "chore: advance the schema policy baseline to v3.27.0",
          "timestamp": "2026-09-17T22:34:38+02:00",
          "tree_id": "3c30f2b26fd2c496c37119a3d683c3794fbe16db",
          "url": "https://github.com/fallow-rs/fallow/commit/85b12cdaf782632837d02a892499d9e91e8b91b3"
        },
        "date": 1789677614893,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9935140,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51244,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1198364,
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
          "id": "cc74281756dd722afb169d8310dbb73fbf06ec50",
          "message": "fix(mcp): auto-detect the audit base ref on the typed audit and decision_surface routes (#2703)\n\nThe MCP audit and decision_surface tools failed with\nFALLOW_CHANGED_FILES_FAILED when no base was passed, on any repository\nwith a remote: the engine's git probe returned raw stdout, so the\nauto-detected ref carried git's trailing newline into ref validation.\nThe CLI route trimmed in its own copy of the detection and kept working.\nRegression from 3.1.0.\n\nThe engine probe returns trimmed, non-empty output again, which also\nrepairs the repository root used when a typed run starts in a\nsubdirectory. The API validates an auto-detected ref the same way it\nvalidates an explicit one. The CLI's duplicate detection is removed and\nrouted through the engine, so both routes share one implementation.\n\nThanks @codingthat for the report and the bisect.\n\nCloses #2699",
          "timestamp": "2026-09-19T06:55:22+02:00",
          "tree_id": "a8a79896a2165d3fb493f72eb05741bc740fc219",
          "url": "https://github.com/fallow-rs/fallow/commit/cc74281756dd722afb169d8310dbb73fbf06ec50"
        },
        "date": 1789794024947,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9897884,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51234,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1199437,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8469,
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
          "id": "61275ff83c557589aac1b36638b95a83dbfc0e2b",
          "message": "fix(output): report what a run was asked to do and whether it did it (#2704)\n\nSeveral requests could be dropped or degraded by a run with only a\nstderr line to say so, which --quiet and the CI integrations never see.\n\nA new optional root object, request_outcomes, records each narrowing or\nartifact request the run received: status (applied or not-applied),\naffects (scope or artifact), requested as the user spelled it, and for an\nunapplied entry a reason token and the sentence the CLI printed. It covers\n--changed-since falling back to the whole project, the diff filter\nstanding down, and a --sarif-file that could not be written. The object\nis absent when nothing was asked for, so no schema version moves, and the\nJSON root-prefix pass leaves it verbatim.\n\nDegraded health inputs (file scores, hotspots without git, shallow\nclones, an unpinned clock, ownership, unreadable trend snapshots) reach\nworkspace_diagnostics with degrades_analysis set. --group-by on a format\nthat cannot carry it now says so on every such format and in the comment\nand review bodies.\n\nThe GitHub Action publishes requests-unapplied and warns when the SARIF\nfile it was about to upload was never written; the GitLab template writes\nFALLOW_REQUESTS_UNAPPLIED. The MCP tools state the same facts.\n\nCloses #2687\nCloses #2688\nCloses #2689\nCloses #2690\nCloses #2691",
          "timestamp": "2026-09-19T09:51:52+02:00",
          "tree_id": "e9ed8aaf16c5a0cde9447e9781ea8054829fefbb",
          "url": "https://github.com/fallow-rs/fallow/commit/61275ff83c557589aac1b36638b95a83dbfc0e2b"
        },
        "date": 1789804641287,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9912875,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51187,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1187634,
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
        "date": 1789817975546,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 9894148,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51236,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1206451,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8491,
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
          "id": "3f82ed029b182456d17d49c0a8f062054c18fd4a",
          "message": "feat(plugins): read Module Federation exposes and remotes from config (#2706)\n\nA new built-in module-federation plugin reads exposes and remotes from a\nstandalone module-federation.config file and from the inline plugin\noptions of webpack, rspack, rsbuild and vite configs.\n\nexposes targets become runtime entry points, so an exposed module is no\nlonger reported as unused when nothing local imports it. remotes aliases\nbecome provided dependencies, matched on the alias and its subpaths, so\nimports from a remote are not reported as unresolved or unlisted.\n\nThis first slice covers static object-literal exposes and remotes. The\narray form and plugin calls built elsewhere in the config are not read\nyet.\n\nPart of #2698",
          "timestamp": "2026-09-20T22:40:25+02:00",
          "tree_id": "4fda472047d86fe1615bfaa46a3c19707e919ab7",
          "url": "https://github.com/fallow-rs/fallow/commit/3f82ed029b182456d17d49c0a8f062054c18fd4a"
        },
        "date": 1789937183280,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10050415,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51528,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1198293,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8438,
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
          "id": "95aa9c480480800bc80779509105ad9e4879e3fa",
          "message": "fix(nuxt): report unused convention files when the auto-import scan is off (#2702)\n\nWith autoImports: true the Nuxt plugin kept its convention entry patterns\nwhenever nuxt.config declared a components or imports key, because custom\nlayouts are not modelled. A config that only switches the scan off\n(components: false, components: [], components: { dirs: [] },\nimports: { scan: false }) was treated the same way, so components,\ncomposables and utils were never reported in a project that requires\nexplicit imports.\n\nThose literal shapes now count as the scan being off and the entry\npatterns are dropped, as with Nuxt's default config. Every other shape\nkeeps its entry patterns, including a lone imports: { autoImport: false },\na config that declares extends, and a non-empty imports.dirs next to\nscan: false. A project that combines autoImports: true with one of the\nrecognised shapes will see new unused-file findings.\n\nThanks @Tsuyoshi84 for the report.\n\nCloses #2695",
          "timestamp": "2026-09-20T22:47:44+02:00",
          "tree_id": "d622111a908439494035b34f665e79f384970a21",
          "url": "https://github.com/fallow-rs/fallow/commit/95aa9c480480800bc80779509105ad9e4879e3fa"
        },
        "date": 1789937534997,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10088976,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51561,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1206643,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8480,
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
          "id": "731a504da976f1a8f4ce08bf08c2defd961e5f44",
          "message": "feat(output): carry the request outcomes and skip causes four commands left on stderr (#2743)\n\nfallow flags and fallow suppressions carry request_outcomes with the changed-since entry. An applied diff-filter request carries scope_size, so a diff with no added lines reads as an empty scope on every consumer: the JSON envelope, the Action and GitLab logs, the MCP warnings, and the comment, review and summary bodies. hotspots-skipped carries a cause for a malformed --since and an unreadable churn file as well as a missing repository. fallow security --sarif-file carries the sarif-file entry. Exit codes are unchanged.\n\nCloses #2734",
          "timestamp": "2026-09-21T13:27:22+02:00",
          "tree_id": "ec39db9de0c07dfd63efca0e42efec8559ece446",
          "url": "https://github.com/fallow-rs/fallow/commit/731a504da976f1a8f4ce08bf08c2defd961e5f44"
        },
        "date": 1789990537599,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10078392,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51547,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1193875,
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
          "id": "d4264c27859c6b307fbebd269c969c0b107984ee",
          "message": "fix(plugins): credit bundler entry packages and close the Nuxt autoImports precision gaps (#2745)\n\nA package named in a webpack, rspack or rsbuild entry, with or without a resource query, is credited as a dependency instead of becoming an entry pattern that matches nothing; glob-shaped entries stay entry patterns.\n\nNuxt autoImports: components under global/ and islands/ get the name Nuxt gives them, a config with a computed key, accessor or top-level spread keeps the convention entry patterns for that root, each workspace root is classified on its own, components: true, imports: {} and imports: { dirs: [] } count as the defaults they are, and a name imported or re-exported from #components or #imports is credited. #layers/<name>/ resolves as a path alias.\n\nCloses #2739\nCloses #2737",
          "timestamp": "2026-09-21T14:15:43+02:00",
          "tree_id": "38b1e5a0bba36bb6b69c4d828010e925328fe1ba",
          "url": "https://github.com/fallow-rs/fallow/commit/d4264c27859c6b307fbebd269c969c0b107984ee"
        },
        "date": 1789993270603,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10093352,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51566,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1212778,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8501,
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
          "id": "7e2e7ad99634f2c7ccd359a6a3ea33b162e3d0e6",
          "message": "feat(plugins): record a plugin config fallow cannot read as a workspace diagnostic (#2748)\n\nA framework plugin that cannot read a config statically records a workspace_diagnostics entry. plugin-config-unreadable covers a Module Federation exposes or remotes key that is not a static object literal; it sets degrades_analysis, so the Action, the GitLab template and the MCP warnings report degraded inputs. plugin-effect-not-modeled covers a Nuxt autoImports surface that kept its convention entry patterns; it does not degrade the analysis and prints nothing. Both carry plugin, key and reason, with the config file in path. The stderr line for an unreadable key gains the fallow: prefix. Exit codes are unchanged.\n\nCloses #2736",
          "timestamp": "2026-09-22T00:28:05+02:00",
          "tree_id": "ac5030a20efa7a218729cb54898dbf043b672274",
          "url": "https://github.com/fallow-rs/fallow/commit/7e2e7ad99634f2c7ccd359a6a3ea33b162e3d0e6"
        },
        "date": 1790030230296,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10044640,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51545,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1208911,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8496,
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
          "id": "234d24c8d1d8cb34bd22f166cddf48fd5dd59191",
          "message": "feat(baseline): give saved baselines a kind and report an unrecognised one on every surface (#2749)\n\nEach saved baseline carries a top-level kind: dead-code, dupes or health. A baseline that another command saved loads as zero entries on all three commands, warns on stderr with both kinds and the path, and carries baseline_staleness.unrecognised_format. fallow dead-code no longer exits 2 on such a file. The stale-baseline gate trips on it, so --fail-on-stale-baseline fails the run, and gate_trips carries that rule. A save over a baseline of another kind is refused with exit 2. Audit reports every baseline it cannot read. The pull-request comment and the merge-request note carry the unrecognised-baseline sentence, the combined run gets the recheck-baseline next step, and the live github-summary render carries the gate outcomes.\n\nCloses #2738\nCloses #2735",
          "timestamp": "2026-09-22T01:11:08+02:00",
          "tree_id": "ce61633c08d0dda3ca253d3c7651587e7a4855e1",
          "url": "https://github.com/fallow-rs/fallow/commit/234d24c8d1d8cb34bd22f166cddf48fd5dd59191"
        },
        "date": 1790032604565,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10036422,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51481,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1185345,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8420,
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
          "id": "c3197ecf8cd63e1a3ea094d88f36d5c0ca9b88ba",
          "message": "feat(plugins): read Module Federation options wherever a config declares them (#2750)\n\nThe Module Federation reader finds a plugin call anywhere in a bundler config: in a nested plugin array, in a plugins variable, under tools.rspack.plugins, in an rsbuild appendPlugins hook, and in a Next.js webpack(config) hook. next.config.* is read with NextFederationPlugin as a recognised callee. The array form of exposes is read; the array form of remotes keeps its array-form diagnostic. Options that a same-file const holds are read when the program has one binding of that name and never writes to it. An array element with glob syntax, a nested array or a non-string element is reported under unreadable-entries.\n\nRefs #2698",
          "timestamp": "2026-09-22T01:50:50+02:00",
          "tree_id": "6ea057c1b237d7a18463f64bb290803a317825c1",
          "url": "https://github.com/fallow-rs/fallow/commit/c3197ecf8cd63e1a3ea094d88f36d5c0ca9b88ba"
        },
        "date": 1790034898950,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10105896,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51596,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1198431,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8448,
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
          "id": "f9fb3f9719b757df756d425302a5898e34fc1258",
          "message": "refactor(cli): resolve the base analysis root and the short HEAD through the engine (#2751)\n\nOne engine helper resolves the base analysis root and canonicalizes both sides. The two CLI copies are removed, and the short HEAD probe has one implementation. The security copy did not canonicalize the root; every entry point resolves the root before the analysis starts, so the widening did not reach a run. fallow security --base warns on stderr when the base root is remapped. The CLI boundary test fails on a reintroduced git probe for ref or root detection. Discarded stderr in the Action and GitLab scripts is replayed as debug lines, and a green run gains no log line. The MCP tests no longer read FALLOW_CHANGED_SINCE from the environment.\n\nCloses #2740",
          "timestamp": "2026-09-22T03:23:11+02:00",
          "tree_id": "3d196158c9a4781ba7c9bec79d6923f5cd38e429",
          "url": "https://github.com/fallow-rs/fallow/commit/f9fb3f9719b757df756d425302a5898e34fc1258"
        },
        "date": 1790040460023,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10051816,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51562,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1204290,
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
        "date": 1790045504505,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10084792,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51557,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1164884,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7102,
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
          "id": "3d45a1407ea0b947f8b0b1f9a63ba81b8cfd7002",
          "message": "chore(docker): pin FALLOW_VERSION 3.28.0 with refreshed checksums",
          "timestamp": "2026-09-22T08:35:22+02:00",
          "tree_id": "2a3bca83d36680b46633c8d0bee35947661c2a54",
          "url": "https://github.com/fallow-rs/fallow/commit/3d45a1407ea0b947f8b0b1f9a63ba81b8cfd7002"
        },
        "date": 1790059253533,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10083359,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51527,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1199367,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8454,
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
          "id": "12c5c81929790cf59609323a7170a286fa99ab6b",
          "message": "fix(security): require complete URL authority boundaries (#2763)",
          "timestamp": "2026-09-22T11:30:59+02:00",
          "tree_id": "60f9a70a520fa2ae0ac58ed46ca680dda7535608",
          "url": "https://github.com/fallow-rs/fallow/commit/12c5c81929790cf59609323a7170a286fa99ab6b"
        },
        "date": 1790069801654,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10089400,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51563,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1206455,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8485,
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
          "id": "7c173685891c5f244fd5b31575a4b704b67bd9f0",
          "message": "feat(security): record exact-origin guard observations (#2765)",
          "timestamp": "2026-09-22T10:26:41Z",
          "tree_id": "61650fc87910fbb7f77f5f181cdc3120d45540bc",
          "url": "https://github.com/fallow-rs/fallow/commit/7c173685891c5f244fd5b31575a4b704b67bd9f0"
        },
        "date": 1790073134492,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10080328,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51551,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1195930,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8454,
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
          "id": "d4742dac6e20450de88df7b3d89895d97eaee84c",
          "message": "fix: distinguish security findings on the same line (#2766)",
          "timestamp": "2026-09-22T13:20:07+02:00",
          "tree_id": "11dedd2531ee6aa01f12bd8cdeb50e1afdbcd069",
          "url": "https://github.com/fallow-rs/fallow/commit/d4742dac6e20450de88df7b3d89895d97eaee84c"
        },
        "date": 1790076352331,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10048085,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51430,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1142672,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7024,
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
          "id": "cd91addff1a7060d0e09cd101a25e78c2bd0fd2e",
          "message": "fix(security): ignore constant writeHead header objects",
          "timestamp": "2026-09-22T13:42:25Z",
          "tree_id": "520484eb8abb37d6981edf8e3f8a9b3247d3762e",
          "url": "https://github.com/fallow-rs/fallow/commit/cd91addff1a7060d0e09cd101a25e78c2bd0fd2e"
        },
        "date": 1790084978012,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10039398,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51485,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1181636,
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
          "id": "55c5faf0cccfc284531b70d3b67ddedc1c1e0e5b",
          "message": "refactor: remove dead code, shadow tests and duplicate helpers (#2786)\n\n* refactor(extract): remove duplicate helpers and dead consumer collectors\n\nRemove code that repeats other code or that no code path runs:\n- Remove the CssInJs ConsumerCollector and StyleXThemeGroupCollector.\n  The batched collector handles these queries.\n- Remove the four single-query token consumer wrappers from the public\n  fallow-extract API. The engine calls css_in_js_consumer_scan only.\n- Merge the JSDoc bare-tag checks into one helper.\n- Remove the duplicate visitor identifier helpers.\n- Share the Astro and SFC script complexity remap.\n- Inline the semantic-fact wrappers that have one caller.\n\nReplace the bump history on CACHE_VERSION and DUPES_CACHE_VERSION with a\nshort rule. Git history and the CHANGELOG keep the reasons.\n\nTests:\n- Build cache fixtures from ModuleInfo::empty instead of full literals.\n- Put the visibility roundtrip cases in one table test.\n- Replace the regex smoke tests with a test that compiles the literal of\n  every static_regex call.\n- Point the Svelte dollar-ref tests at the production bound-target\n  functions.\n\n* refactor(graph,config,types): remove test-only copies and dead accessors\n\nNamespace narrowing tests now call the production *_at_site markers\nwith a real effective export index. The cfg(test) copies skipped the\ndeclaration-slot guard. A new test pins that a value member access\ndoes not credit the type-only slot of the same name.\n\nThe levenshtein helper moves to fallow-types. fallow-config re-exports\nit, so the suppression parser and the config consumers use one copy.\n\nRemove public methods that have no production caller:\ncodeclimate_enabled, sarif_enabled, suppressed_kind, has_local_export,\nset_entry_point, physical_reference_count, preset_name and the\npublic_export_declaration_bindings wrapper. leading_command_token\nmoves into the task_matrix test module.\n\nRemove tests that only assert derived equality, hash, clone, Debug or\nstruct field readback in fallow-types and graph::types.\n\n* refactor(core): remove dead helpers and test-only shadows\n\nRemove deprecation attributes from detector helpers in private\nmodules. Keep them on the public analyze entry points and on\nfeature_flags.\n\nReduce FallowError to the config case. The E004 display text and the\ncode, help and context getters do not change.\n\nDelete helpers that have no callers: the non-kinded nested alias and\nstring-pair extractors, kind_to_kebab, extend_test,\nbuiltin_plugin_config_candidate_basenames, and the test-only source\nmatcher and import-location copies. Their tests now run the production\nfunctions. Move the test-only script analysis wrappers into the tests.\nUpdate the tainted-sink comment to name the production source matcher.\n\nUse one borrowed path index for public API entry points, and skip the\nsuppression filters when a result list is empty. Replace duplicate\nextension membership tests with one exact-set test. Use tempfile\ndirectories in the supabase and cache tests. Remove a dead document\nlink from the fallow-v8-coverage crate docs.\n\n* refactor(engine,api,output): remove unused entry points and fix trace path lookup\n\nTrace lookups now prefer an exact root-relative match over a nested\nsuffix match. Before this change, a trace of src/a.ts could select\npackages/x/src/a.ts in a monorepo. An ambiguous short path still takes\nthe first suffix match. All trace sites use one lookup built on\nmatching_module_indexes. path_matches is removed.\n\nDuplicate detection has one internal entry point with optional focus\nfiles and an optional cache. It replaces six thin wrappers. The clone\nfinding with_actions constructors and fingerprint_for_fragment are\nremoved. The fragment fingerprint did not match the report fingerprint.\n\nAlso removed:\n- functions with no caller in editor, session, workspace_scope and\n  gate_outcomes\n- the unused ci_output re-exports and four unused type-aware aliases\n  from fallow-api\n- the port scaffolding in the engine trace modules and the test-only\n  shim modules\n\nThe docs URL builders now share issue_contract::rule_docs_url. The\nhealth runner docs now describe the current design.\n\n* refactor(cli): remove duplicate upload plumbing and test-only shadows\n\nMove the shared error type, exit mapping, dirty-tree gate, message\nformat, count format, endpoint display and path helper of the two\nSHA-keyed coverage uploaders into upload_common. Exit codes and\nmessage text do not change.\n\nMake the fix plan root a required path, so fix tests run the same\ncontainment check as real fixes. Delete the test-only JSONC comma\nstripper, the test-only _meta insert helper, the fingerprint forwarder\nmodule and CLI tests that repeat owner-crate tests.\n\nSecurity JSON renderers now return an error. A serialization failure\nprints the documented error envelope and exits 2.\n\nThe shared test runners now remove FALLOW_COVERAGE and\nFALLOW_COVERAGE_ROOT for every command.\n\n* refactor(mcp,lsp,napi): remove unobserved MCP fallback reasons and weak tests\n\nChange the MCP CLI fallback helpers to bool predicates. Delete the\nCliFallbackReason enum, which no caller reads, and update the MCP\ninternals reference to name the new predicates. Add one type-aware\npredicate for analyze, audit and health. Move the duplicated mode,\nmin_occurrences and non-empty helpers into tools/mod.rs.\n\nMake spawn_fallow always tag the tool. Move the subprocess tests to the\ntagged run_tool entry points that production uses.\n\nReplace the substring-based MCP schema tests with one table that checks\ntop-level properties and required fields. Delete the redundant\ncross-tool argument tables, the tautological edge-case tests and the\nloose description tests.\n\nPin the hover and diagnostic text for the eight named-anchor LSP kinds.\nThen build these hovers and diagnostics with shared anchor helpers.\nDelete the LSP issue-type forwarding wrapper and the copied key list.\n\nDelete the napi tests that only exercised fallow-api, and keep the\nInvalidArg status check on the real detect_duplication export.\n\n* refactor(vscode): remove unused tree badge code and duplicate helpers\n\nRemove the no-op badge code from the health, security and runtime\ncoverage tree providers. The dead-code view keeps its count badge.\n\nExport the structured CLI error guard once from cli-args-utils.\nThe coverage gate message now uses describeAnalysisFailure, so an\nempty envelope message shows the fallback text (new test).\n\nMove the first-workspace-folder lookup into workspaceRoot.ts.\nRemove the compareVersions re-export, the BuiltAnalysisArgs alias,\nthe unused setMutedCategories method and a stale suppression.\nBuild the item icon map from the category map with three overrides.\n\nThe commands test now runs the real configured-binary resolver\nagainst the mocked file system. Remove duplicate or tautological\ntests in the security tree, treemap and review adapter suites.\n\n* chore(tooling): remove dead scripts and share bench fixtures\n\nMake the wrapper trap parity checks in ci/tests/run.sh able to fail.\nThe old `grep -c ... || echo 0` pattern gave \"0\\n0\" on no match, so a\nmissing trap read as present. Each trap must now exist in both wrappers.\nThe structured-error check now matches the \"$RESULTS_FILE\" form in the\naction wrapper.\n\nMove the fixture builders that are the same in the programmatic_commands\nand programmatic_stable benches into benches/support/mod.rs. Bench\nnames do not change.\n\nRemove scripts/generate-readme-screenshot.sh, which writes an image that\nnothing uses. Remove the generate:all aliases of generate:contracts.\nRemove export from helpers that only their own module uses.\n\n* refactor(graph): remove test-only resolver reuse predicate and cache version history\n\nThe tests now assert on classify_resolution_mismatch, which is the\npredicate that production runs, instead of a separate copy of it.\nThe GRAPH_CACHE_VERSION doc keeps the bump rules and the rule not to\nreuse a published version number. Git history and the CHANGELOG\nrecord the reason for each bump.\n\n* refactor(output,api): remove the single-variant root envelope mode parameter\n\nRootEnvelopeMode had one variant, Tagged, and every root serializer\nignored it. Remove the enum, the mode parameter on the root serializers\nand apply_root_kind, the envelope_mode fields on the fallow-api output\nstructs, and the helpers that returned the constant. Update the cli,\nmcp, napi and bench call sites.\n\nThe body of apply_root_kind does not change, so the top-level kind\ndiscriminator and the JSON output stay the same. Remove the boundary\ntest that made the CLI route through the removed helper. The guard that\nblocks the legacy mode stays.\n\n* ci: remove the GitLab jq summary renderers\n\nThe GitLab template stopped loading ci/jq when the MR comment and\nreview scripts moved to the typed Rust formats. Only tests and guards\nstill read these files.\n\n- Delete ci/jq/ and the fixtures that only its tests read.\n- Remove the GitLab summary, parity and markdown sections from\n  ci/tests/run.sh. Keep the drift guard helper assertions.\n- Reduce check-ci-summary-rows.mjs to the action/jq table. Keep the\n  check for rows that are not in the registry, and add a unit test\n  for it.\n- Remove the ci/jq cases from the issue_meta.rs table tests.\n- Fix a doc comment that named jq files that do not exist.\n- Update repo-map.md and the CI reviewer file.\n\n* refactor(npm): share the binary list and skip check between install and lazy verification\n\nThe lazy verification sentinel and the signature check each kept their own\nlist of platform binaries. They also kept their own copy of the skip\nenvironment check. The sentinel must bind the same binaries that the\nsignature check verifies. lazy-verify now uses the exported list and the\nexported skip check from verify-binary. The sentinel format does not change.\n\n* refactor(graph): remove the unused path index and query methods from ProjectState\n\nAnalysis setup builds ProjectState once and reads only files() and\nworkspaces(). The path_to_id map and the six query methods had callers\nonly in tests and benches. The map also cloned every discovered path on\neach run.\n\nRemove path_to_id and these methods: id_for_path, stable_key_for_file,\nid_for_stable_key, workspace_for_file, workspace_by_name and\nfiles_in_workspace. Keep new, files, workspaces, file_by_id and the\ndense FileId check. The struct doc now states only what the struct does.\n\nThe workspace integration test now asserts on workspaces(), files() and\nfile_by_id(). The discovery determinism test stays unchanged.\n\nThis removes two CodSpeed bench IDs from the baseline list:\ncomponent_graph_project_state_lookups and\ncomponent_graph_project_state_workspace_queries. Construction is now a\nmove, so component_graph_project_state_build also reads files() and\nworkspaces(). Without these reads, Criterion reports zero time. Its\nbaseline shifts for this reason.\n\n* test(cli): share one isolated git helper across integration tests\n\nAdd git_command, git, git_capture and commit_all to tests/common. The\ncommand clears the repository variables that a git hook sets (GIT_DIR,\nGIT_WORK_TREE, GIT_INDEX_FILE and others), ignores the global and system\ngit config, and uses a fixed author and committer.\n\nRemove the file-local git and commit copies. Some copies removed no\nrepository variables, some kept the global config, and one committed\nwith the global signing config active. Tests that ignored git failures\nnow fail at the git call.\n\n* test(api,lsp): test the editor merge logic in fallow-api\n\nMove the all-fields merge test, the percentage recompute test and the\nzero-lines test from the LSP crate to the EditorAnalysisOutput tests in\ncrates/api/src/editor.rs. The exhaustive fixture moves with them and\nkeeps the #444 compile guard.\n\nRemove five weaker LSP merge tests. The moved tests cover their\ncontract. Remove the two test-only merge helpers in the LSP analysis\nmodule and the unused imports.\n\nThe api tests now fail when merge_duplication does not recompute the\nduplication percentage.\n\n* refactor(engine,api,cli): share one CI check and one opt-out gate\n\nAdd fallow_engine::ci_env::is_ci() and use it in place of the four local\ncopies in the api next steps, telemetry, the update check and the cache\nnotice. The cache notice now reads update_check::env_disabled, so one\nopt-out always silences both notices. The update check cache path now\nbuilds on telemetry::config_dir. Environment variable names and values\ndo not change.\n\n* ci: remove the allocation baseline gate that has no baseline\n\nThe repository never contained alloc-baseline.json. The allocs workflow\nexited before it compared against a baseline, so the gate never ran.\nDelete scripts/alloc-check.sh, the compare block and the path filters\nfor the missing file. The store-benchmark step keeps the regression\nalerts at the 120% threshold.\n\n* test: clear ambient git state in crate test git helpers\n\nTest helpers that run git init, git config and git commit now remove\nGIT_DIR, GIT_WORK_TREE and GIT_INDEX_FILE (and the other ambient\nvariables where the engine helper is available). Without this, a test\nrun inside a git hook or a `git rebase -x` step writes config, index\nentries and commits into the enclosing repository.\n\n- engine tests use the engine git runners\n- api, cli and bench tests use clear_ambient_git_env\n- mcp tool tests share one scrubbed runner in base_root_fixture.rs\n- the lsp test helper removes the three variables directly\n\n* docs(core,engine): fix stale paths in coupling concentration docs\n\nThe render fan-in concentration doc named crates/cli/src/vital_signs.rs,\nwhich does not exist. The engine doc named crate::render_fan_in and\nhealth/mod.rs for code that is in fallow_core analyze::render_fan_in and\nhealth/vital_data.rs. Both docs now name the correct paths and say to\nchange the shared p95 and floor math together.\n\n* refactor(cli): share envelope and provider helpers of the ci post commands\n\nThe review and PR comment post commands had their own copies of the\nenvelope fingerprint helpers and the GitHub and GitLab target lookups.\nThe variable precedence must stay the same for both commands, so the\nlookups now live once in ci.rs. A purpose argument keeps each error\nmessage unchanged. Tests assert the exact messages and the precedence\nof the repository sources.\n\n* refactor(api): remove the private fingerprint_hash forwarder\n\nCall codeclimate_fingerprint_hash directly in the dead-code CodeClimate\noutput, as the health and duplication outputs already do. The hash input\nis the same, so the fingerprints do not change.\n\n* refactor(mcp): build duplication mode error text from VALID_DUPES_MODES\n\nThe dupes, trace_clone and code mode combined validators restated the\nmode list as a literal in their error text. They now join\nVALID_DUPES_MODES, so the list has one definition. The error strings\ndo not change.\n\n* chore: remove private repository references from public code\n\nRewrite the sidecar key comments, one test assertion message and the\nstatic-findings limit comment without private paths or workflow names.\nPoint the test-key comment at TEST_SIDECAR_SEED in\ncrates/cli/tests/common/sign.rs. Remove a private decision-record\ncitation from the static-findings upload module doc. Remove private\nissue citations from the runtime-coverage schema doc comments and\nregenerate the output contracts.\n\n* refactor(types): share one root-relative display path helper\n\nAdd fallow_types::path_util::display_relative and use it in types,\nconfig, core, api and cli in place of the private copies. The\nper-instance and aggregated workspace messages must format paths the\nsame way, and one helper now keeps that rule. The output does not\nchange. The discovery note keeps its \".\" mapping for the project root.\n\n* docs: fix stale source paths in reviewer guidance and analyzer docs\n\nThe rust-reviewer attribution check named files that no longer exist,\nso its grep loop reported a false MISSING for every issue type. Point\nit at crates/engine/src/changed_files.rs and crates/api/src/audit_keys.rs,\nand rename the loop variable so it does not overwrite PATH in zsh.\n\nAlso correct the generated output contract path in vscode-reviewer, the\nRuleDef location in analyzer-authoring, and the CODEOWNERS parser path\nin the codeowners test doc comment.\n\n* docs(changelog): note the monorepo trace path fix\n\n* test(lsp): pin the exact diagnostic code list\n\nEditors save code filters by the diagnostic codes that the LSP\npublishes. Compare the full, ordered list from diagnostic_issue_types()\nagainst a fixed list. A code that disappears then fails the test and\ndoes not silently leave the editor filters.\n\n* fix(engine): drop a test import that only unix tests use\n\nThe repo_refs tests import std::process::Command, but only the\nunix-gated mkfifo test uses it after the git helper change. Windows\nclippy rejects the unused import. Qualify the one call instead.",
          "timestamp": "2026-09-23T13:01:25+02:00",
          "tree_id": "2e6bf881a4e9718e89c78d62c3631e4cfbb31d86",
          "url": "https://github.com/fallow-rs/fallow/commit/55c5faf0cccfc284531b70d3b67ddedc1c1e0e5b"
        },
        "date": 1790161862785,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10064562,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51353,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1201158,
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
          "id": "2f7c2fbbd2ab52f3c57df2518e28a87cd0119b1e",
          "message": "test: add a drift contract and a cross-surface harness (#2787)\n\nAdd docs/development/drift-contract.md and a proptest harness in crates/cli/tests/drift that compares CLI, MCP and fallow_api results on generated projects. The drift CI job and release validation run it.",
          "timestamp": "2026-09-23T13:55:00+02:00",
          "tree_id": "f705533578aa404edc3355642f366bc5813f8fdc",
          "url": "https://github.com/fallow-rs/fallow/commit/2f7c2fbbd2ab52f3c57df2518e28a87cd0119b1e"
        },
        "date": 1790164828696,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10065913,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51325,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1173200,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 7998,
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
          "id": "6cd8d83e04143c28885c81805ec73320872814c9",
          "message": "fix: harden failure handling and remove redundant state (#2726)",
          "timestamp": "2026-09-23T14:11:09+02:00",
          "tree_id": "2bf6d8b65caec54284e07087786a9ae4df326b16",
          "url": "https://github.com/fallow-rs/fallow/commit/6cd8d83e04143c28885c81805ec73320872814c9"
        },
        "date": 1790165735037,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10062682,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51350,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1194711,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8372,
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
          "id": "4fc827f3e54f6e5703685dff91d396eb83095dd9",
          "message": "fix: run one audit implementation for the CLI, MCP and API (#2792)\n\nThe CLI audit, the MCP audit tool and fallow_api::run_audit now share one implementation in fallow-api. Audit reports dependency findings only when the manifest changed, and it runs a real base analysis when a changed file holds a suppression or visibility marker. The drift harness checks I4 and I5.",
          "timestamp": "2026-09-23T14:59:16+02:00",
          "tree_id": "9252f37c097cdb8f14077ee0bcb60e7dc98f9eaa",
          "url": "https://github.com/fallow-rs/fallow/commit/4fc827f3e54f6e5703685dff91d396eb83095dd9"
        },
        "date": 1790168693330,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10065890,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51352,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1197158,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8334,
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
          "id": "c69751ce13d0e2c9fc2e76872217bc7cf44b81df",
          "message": "fix(cli): say in runtime coverage help that a single capture is free (#2809)\n\nThe --runtime-coverage help of audit and security called the input paid,\nbut a single local capture runs without a license. Use one note on every\nruntime coverage input: a single local capture is free, and continuous or\nmulti-capture monitoring needs a license. Say that upload-inventory needs\na fallow cloud API key, and remove the path of a file that is not public\nfrom the coverage help.",
          "timestamp": "2026-09-23T16:20:22+02:00",
          "tree_id": "243e3c75ce820a98a99bb82a14cfeb9e05a1ad88",
          "url": "https://github.com/fallow-rs/fallow/commit/c69751ce13d0e2c9fc2e76872217bc7cf44b81df"
        },
        "date": 1790173566996,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10041530,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51363,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1200577,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8414,
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
          "id": "5a64644dedca3c7769ead8c7f7e6e6078ed4bcdf",
          "message": "fix: read bundler and Module Federation config options through indirection (#2793)\n\nThe Module Federation reader now reads plugin options through export const, a non-null assertion, a computed member, a relative ESM import or require of a sibling config, an object spread and Object.assign.\n\nWebpack configs under config/, build/ and webpack/ are read when they export a webpack configuration. Rspack context and rsbuild root apply to relative entries. An entry without an extension matches the source file or the directory index.\n\nRefs #2757, Refs #2753",
          "timestamp": "2026-09-23T17:14:27+02:00",
          "tree_id": "e889c770f2936a506449c15bc5692df6ceb61369",
          "url": "https://github.com/fallow-rs/fallow/commit/5a64644dedca3c7769ead8c7f7e6e6078ed4bcdf"
        },
        "date": 1790177008493,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10203639,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51628,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1190327,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8357,
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
          "id": "5862f7ef099a155809269f394d5c1433197f9448",
          "message": "perf: compile guard rule scopes once and drop repeated boundary work (#2811)\n\n- guard: compile the files and exclude globs of each rule-pack rule once\n  per report. Before, guard compiled them again for each target file.\n- rule packs: validate zone references against the expanded zone names.\n  The check no longer compiles the zone globs, and it skips the boundary\n  expansion when no rule has a zones scope.\n- trace: compare module paths through a borrowed forward-slash view. The\n  lookup no longer allocates two strings per module.\n- boundaries: build one glob candidate per path in classify_zone instead\n  of one per glob.\n- re-export cycles: Tarjan SCC no longer allocates a vector for each\n  single-node component. An acyclic barrel chain has one per node.",
          "timestamp": "2026-09-23T17:25:10+02:00",
          "tree_id": "aecc16cd17f5211cbb9417a08a0be6a78501d468",
          "url": "https://github.com/fallow-rs/fallow/commit/5862f7ef099a155809269f394d5c1433197f9448"
        },
        "date": 1790177371326,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10161255,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51609,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1221860,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8620,
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
          "id": "d6e4fdf5b104402b51e69e8d767e9d78c9ba84b3",
          "message": "fix: apply combined baselines and state the verdict in every JSON envelope (#2810)\n\nBare fallow applies --dupes-baseline and --health-baseline. Every machine envelope with a default exit rule always carries that rule in gate_outcomes, so a JSON reader can see a failing run. Exit codes do not change. The drift harness checks I3 and I7.",
          "timestamp": "2026-09-23T19:53:24+02:00",
          "tree_id": "7de9478ff67ea975a394141ddb9805b386e6c3e1",
          "url": "https://github.com/fallow-rs/fallow/commit/d6e4fdf5b104402b51e69e8d767e9d78c9ba84b3"
        },
        "date": 1790186347482,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10173223,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51592,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1197421,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8393,
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
          "id": "1df164f7c772200e1da7731f91d7a9edfba06b65",
          "message": "fix: give CI formats the rule severity of each dead-code finding (#2814)\n\nEach dead-code finding now carries an optional effective_severity field (error or warn). The analysis writes it once, after rule resolution, so overrides[].rules apply. SARIF, CodeClimate, github-annotations and the bundled annotation filter read it in the direct run and in report --from, so an annotation or SARIF level now follows the configured rule. A saved report without the field, or with an unknown value, keeps the old level.\n\nEmpty catalog groups and unused or misconfigured dependency overrides now use the rules of the file that declares them, in the exit code, the audit verdict and the annotations.\n\nThanks to @jwenger-notion for the report.\n\nCloses #2782",
          "timestamp": "2026-09-23T21:12:27+02:00",
          "tree_id": "bc4d1db47ac1e5b1a6973c1bc2c8b826584eea59",
          "url": "https://github.com/fallow-rs/fallow/commit/1df164f7c772200e1da7731f91d7a9edfba06b65"
        },
        "date": 1790192271148,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10139807,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51582,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1196620,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8365,
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
          "id": "4b428bbd14308964b120e1818e0dd30bb468748f",
          "message": "refactor(core): remove the test-only rules_applying_to_path helper (#2822)\n\n`fallow_core::analyze::rules_applying_to_path` had no production caller.\nThe engine guard owns policy scope matching, and a CLI architecture test\nalready forbids the core backend from calling the core copy. The helper\nalso compiled the scope globs of every rule again on each call.\n\nThe two scope tests now run the production path: `compile_rules` and\n`CompiledRule::applies_to`.",
          "timestamp": "2026-09-23T23:24:05+02:00",
          "tree_id": "dfd0eba4581ff46f6947b7412f25da156958f5e5",
          "url": "https://github.com/fallow-rs/fallow/commit/4b428bbd14308964b120e1818e0dd30bb468748f"
        },
        "date": 1790200292407,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10196919,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51614,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1193044,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8351,
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
          "id": "967c8040ef2ba03e1292f06c256dd3c944f097b4",
          "message": "perf(trace): compare module paths against joined root targets (#2820)\n\nThe exact-match check in matching_module_indexes ran Path::strip_prefix\nfor every module, which walks the path components each time. The lookup\nnow joins the root-relative request with the root and the canonical root\nonce, and compares each module path with those targets.\n\nBefore #2786 the lookup stopped at the first match. It now scans every\nmodule to keep ambiguous matches, so the per-module cost matters.",
          "timestamp": "2026-09-23T23:51:56+02:00",
          "tree_id": "2a782ad4d24ea5d1234e289db848adca1c9cc1fb",
          "url": "https://github.com/fallow-rs/fallow/commit/967c8040ef2ba03e1292f06c256dd3c944f097b4"
        },
        "date": 1790200788392,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10146719,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51588,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1194792,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8389,
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
          "id": "43881a70dc7c78a77914d6d279d332402263b3b3",
          "message": "feat: add complexity gate rules so CRAP and complexity findings can warn (#2823)\n\nNew rules complexity-cyclomatic, complexity-cognitive and complexity-crap decide whether a complexity finding blocks. The default is error, so current behavior does not change. The threshold keys still decide whether a finding exists. A finding above several thresholds takes the most severe rule of those kinds, and it is dropped only when all of them are off. overrides[].rules apply.\n\nThe fallow health exit code, gate_outcomes, the bare fallow summary and the audit verdict read the rules, and a warn finding gives verdict warn. Each complexity finding carries effective_severity next to the band severity. GitHub annotations, SARIF and CodeClimate take their level from the rule, so with the default a moderate finding now shows as an error annotation, the same as the failing job.\n\nThanks to @jwenger-notion for the report.\n\nCloses #2783",
          "timestamp": "2026-09-24T00:14:17+02:00",
          "tree_id": "94d356767a719c78105d1790874dcfca1fa66bba",
          "url": "https://github.com/fallow-rs/fallow/commit/43881a70dc7c78a77914d6d279d332402263b3b3"
        },
        "date": 1790204383834,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10147493,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51536,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1169475,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8308,
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
          "id": "58e3bd25b8d1c8269578f4d01a5ea41a9020319a",
          "message": "refactor: share decision logic across the CLI, MCP, API and editor (#2815)\n\nEach decision that more than one command or surface makes now has one implementation below the CLI: the diff filter, the dead-code scope, clone group workspace scoping, the production mode, one per-finding severity table, baseline loading, the gate outcome builders and the verdict-to-exit-code table. The drift harness now checks all invariants I1 to I8.",
          "timestamp": "2026-09-24T02:19:16+02:00",
          "tree_id": "a7004480dd5562493265a8fc4418a55d19b49773",
          "url": "https://github.com/fallow-rs/fallow/commit/58e3bd25b8d1c8269578f4d01a5ea41a9020319a"
        },
        "date": 1790209409747,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10187647,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51614,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1203832,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8488,
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
          "id": "e4a6b75a0f1e6a30c22bd271f84739399f05fc00",
          "message": "fix: use the singular in health headers for one file (#2817)\n\nThe human and markdown health output printed \"(1 files)\" in the File health scores and Hotspots headers. Both now use the singular for one file.\n\nCloses #2808",
          "timestamp": "2026-09-24T06:42:13+02:00",
          "tree_id": "3b9a526de30a88638fd53a04346ccefb021dcc31",
          "url": "https://github.com/fallow-rs/fallow/commit/e4a6b75a0f1e6a30c22bd271f84739399f05fc00"
        },
        "date": 1790225753773,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10126093,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51507,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1176174,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8325,
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
          "id": "4e62249b20d452bd84b33c3cba6e2ca34006c508",
          "message": "test: take audit renames and clone identity from git in the drift harness (#2834)\n\nThe I4 oracle reads renames from git diff --find-renames, the same way audit does, and models clone-group size and the documented no-added-line demotion. The full 512-case check found these gaps; the product behavior was correct.",
          "timestamp": "2026-09-24T07:07:33+02:00",
          "tree_id": "2cadc13943bb2c8e7e1d55f68ee2c153e79dc105",
          "url": "https://github.com/fallow-rs/fallow/commit/4e62249b20d452bd84b33c3cba6e2ca34006c508"
        },
        "date": 1790226994757,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10203879,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51627,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1204379,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8408,
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
          "id": "bb13ec8babeb0c1a5ba69da742e67a7203157fac",
          "message": "fix: read an absolute config path under the project root as absolute (#2821)\n\nConfig readers treated a leading slash as relative to the project root before they checked for an absolute path, so a literal absolute path under the root was read wrong. An absolute path under the root is now read as absolute. Webpack, rspack and rsbuild read a leading slash as a filesystem path for context and entries, and webpack also for resolve.alias. Vite and the other readers keep the root-relative reading, including Vite's rule for a root whose own name repeats inside it.\n\nCloses #2806",
          "timestamp": "2026-09-24T08:26:03+02:00",
          "tree_id": "df9e15db823853bffe175022624907f50df946b8",
          "url": "https://github.com/fallow-rs/fallow/commit/bb13ec8babeb0c1a5ba69da742e67a7203157fac"
        },
        "date": 1790231665906,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10137869,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51521,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1177075,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8331,
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
          "id": "8e8fbfe48eec6c78c6105806e8a56f9d3851f4c2",
          "message": "fix: align rendered output and MCP write hints with the envelope (#2829)\n\nThe live combined PR comment and its Check Run summary now show the same status note as report --from. github-summary sorts dead-code rows by path in the renderer, so the live and saved summaries agree, and the parity tests cover github-summary and github-annotations.\n\nThe MCP tools analyze, check_changed, find_dupes and check_health accept a file-writing parameter, so they now declare readOnlyHint false and destructiveHint false. code_execute refuses save_baseline, save_regression_baseline and save_snapshot before dispatch, and a refused call spends no host-call slot; call the standalone tool for the write.\n\nCloses #2755",
          "timestamp": "2026-09-24T09:18:00+02:00",
          "tree_id": "b4885c1869691ac9d64868d2fe5d18505aa0b5ca",
          "url": "https://github.com/fallow-rs/fallow/commit/8e8fbfe48eec6c78c6105806e8a56f9d3851f4c2"
        },
        "date": 1790234617572,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10189111,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51602,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1191548,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8334,
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
          "id": "d9a7e1c2f1b84b502e7227ffef8ad6f4908a7ab6",
          "message": "fix: give a hidden-directory remedy that fixes the current run (#2818)\n\nThe skipped-source-dotdir diagnostic suggested --root, which does not clear the false positive in the main run. The message, the stderr note and the kind description now name the fix per case: an entry for a file, ignoreExports for an export, and ignoreDependencies for a dependency that only the hidden directory imports.\n\nCloses #2797",
          "timestamp": "2026-09-24T09:42:32+02:00",
          "tree_id": "f248a1bf7500b9d62704effca52bd3269b992ca4",
          "url": "https://github.com/fallow-rs/fallow/commit/d9a7e1c2f1b84b502e7227ffef8ad6f4908a7ab6"
        },
        "date": 1790236209006,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10186055,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51609,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1216745,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8454,
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
          "id": "bb1e1fb689bd17e4999857a71816830ac2230b31",
          "message": "fix: share the git helpers and report hotspots on a branch without commits (#2816)\n\nThe CLI now uses one set of git helpers from the engine: the base-worktree cache remap (which now also handles a symlinked root), the full HEAD sha, and the remote default branch probe. fallow init writes the right base branch for a clone that has only origin/master and no origin/HEAD. The loaded-baseline slot resets at the start of each command run.\n\nfallow health --hotspots on a branch without commits now records hotspots-skipped with cause no-commits and prints a note, instead of producing no result and no diagnostic.\n\nCloses #2758\nCloses #2803",
          "timestamp": "2026-09-24T10:16:38+02:00",
          "tree_id": "baaf72baf89e9f74b7f5f2fd6ffd5f5edfec42f4",
          "url": "https://github.com/fallow-rs/fallow/commit/bb1e1fb689bd17e4999857a71816830ac2230b31"
        },
        "date": 1790238498930,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10179303,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51599,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1198936,
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
          "id": "d4e694d6721c2b7c61b263b328b878bce288840e",
          "message": "fix: stop warning that a circular-dependency override has no effect (#2836)\n\nA cycle takes the highest severity of its files, and a cycle whose files all resolve to off is dropped, so a per-file circular-dependency override does change the result. The duplicate-exports and re-export-cycle warnings stay.",
          "timestamp": "2026-09-24T11:12:02+02:00",
          "tree_id": "3861867d644b6a86b3cb73c1ff82b1391bfc4e48",
          "url": "https://github.com/fallow-rs/fallow/commit/d4e694d6721c2b7c61b263b328b878bce288840e"
        },
        "date": 1790241991982,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10191527,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51610,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1201923,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8372,
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
          "id": "38e1b05bfc1788d40a6de0b1adb38cd675c7fbd1",
          "message": "perf(trace): index module paths once for semantic reference checks (#2839)\n\nreconcile_semantic_trace_reachability resolved each reference of a\ntype-aware trace with matching_module_indexes. Each call scanned every\nmodule and made two canonicalize calls. The checker returns up to 40\nreferences, and when none is reachable every reference runs a full scan.\n\nA ModulePathLookup now indexes the module paths once, by path and by\nfile name. Each reference then costs a few map lookups and one\ncanonicalize call. On a 20,000-module graph, 40 lookups drop from about\n41 ms to about 7 ms, index build included.\n\nDifferential tests check that the index returns the same modules, in\nthe same order, as matching_module_indexes, including suffix, ambiguous\nand missing paths, and a symlinked temp root on disk.",
          "timestamp": "2026-09-24T12:10:25+02:00",
          "tree_id": "8dd99c6fcccda19ab141039fd0b10a39c80db214",
          "url": "https://github.com/fallow-rs/fallow/commit/38e1b05bfc1788d40a6de0b1adb38cd675c7fbd1"
        },
        "date": 1790245829575,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10192367,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51607,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1191729,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8317,
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
          "id": "1b50587d56a8f29ee8d536c8d44b4a56f8502558",
          "message": "test: compare NAPI findings with the CLI (#2843)\n\ncrates/napi/test.mjs compares detectDeadCode, detectDuplication and computeHealth with CLI dead-code, dupes and health on a two-workspace project, with and without a workspace scope.",
          "timestamp": "2026-09-24T13:15:11+02:00",
          "tree_id": "b7c3a1e64ad464a5e9f58472d6a81ca9d06b1504",
          "url": "https://github.com/fallow-rs/fallow/commit/1b50587d56a8f29ee8d536c8d44b4a56f8502558"
        },
        "date": 1790250071317,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10137101,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51523,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1183394,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8332,
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
          "id": "00a5eac32934a1bd4f733c8e7dee09568a78a258",
          "message": "fix: render a clean grouped dead-code envelope in report --from (#2837)\n\nA --group-by dead-code envelope with no findings has an empty groups list. report --from stopped on it with exit 2 and the error missing field unused_files, for every output format. The flat envelope now gets every required category array before the grouped findings are flattened, so a clean grouped run renders zero findings and exits 0. The GitHub Action renders through report --from, so a clean --group-by run no longer fails at the render step.\n\nCloses #2830",
          "timestamp": "2026-09-24T14:10:43+02:00",
          "tree_id": "922f564297715500c5ece8c4d08f8f9199bd5d2e",
          "url": "https://github.com/fallow-rs/fallow/commit/00a5eac32934a1bd4f733c8e7dee09568a78a258"
        },
        "date": 1790254322811,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10197022,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51603,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1192364,
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
          "id": "6881a70a2d9921de8f939badba98cd64330de6a4",
          "message": "feat: report deprecated exports that still have consumers (#2833)\n\nA new dead-code finding, deprecated-export-in-use, reports an export tagged @deprecated in its JSDoc that reachable code still uses. It carries the deprecation message (plain text, capped), an exact consumer_count, a sorted sample of up to 10 consumers, and a public_api flag. It is off by default; turn it on with the deprecated-exports-in-use rule, the --deprecated-exports-in-use flag or the MCP issue_types selector. A deprecated export with no consumers stays an unused-export and gets deprecated and deprecated_reason.\n\nJSDoc tags now attach only to the export statement they sit on. Before, in a file without semicolons, a @public, @internal or @deprecated tag could apply to the next exports too and hide unused exports.\n\nCloses #2598",
          "timestamp": "2026-09-24T15:26:48+02:00",
          "tree_id": "4ab3457b8356c9f0697178d700a0829066aca7ad",
          "url": "https://github.com/fallow-rs/fallow/commit/6881a70a2d9921de8f939badba98cd64330de6a4"
        },
        "date": 1790259846300,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10223359,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51697,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1211454,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8349,
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
          "id": "cef7570dcdef1e7b57218a0f845d569547b2daa5",
          "message": "perf(report): sort github-summary rows on keys read once per row (#2844)\n\nThe github-summary sort from #2829 serialized every row and read path and line from the JSON map on each comparison, over all rows, while a section renders 25. Path and line are now read once per row, the sort runs on those keys, and the serialized row breaks a tie only inside the tie groups that reach the rendered rows. The rendered rows are the same. CodSpeed instruction time for component_output_dead_code_github_summary_render drops from 2.10 ms to 0.44 ms.",
          "timestamp": "2026-09-24T17:25:12+02:00",
          "tree_id": "e05b0e5cd6339b36d2dcc9a0727d9c876775935e",
          "url": "https://github.com/fallow-rs/fallow/commit/cef7570dcdef1e7b57218a0f845d569547b2daa5"
        },
        "date": 1790266892965,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10247775,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51716,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1210733,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8351,
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
          "id": "fc3aa486bd7e3d4c9fd1f32f17df07d3f67dc908",
          "message": "feat: name the command that wrote a foreign baseline (#2857)\n\nWhen a baseline file comes from another command, baseline_staleness now carries saved_by with that command (dead-code, dupes or health). The field is absent when the file names no known writer, never null. The stderr note, the gate line and the JSON output use one value, and the GitHub Action (a new baseline-saved-by output and the job summary), the GitLab template, PR and MR comments and the MCP warning show it.\n\nCloses #2801",
          "timestamp": "2026-09-24T18:33:42+02:00",
          "tree_id": "82cd28f612626bf549be5d0d62711b16b09b0d78",
          "url": "https://github.com/fallow-rs/fallow/commit/fc3aa486bd7e3d4c9fd1f32f17df07d3f67dc908"
        },
        "date": 1790269895189,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10238935,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51705,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1211553,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8379,
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
          "id": "d10918b39bca583b582278d4ef5740da31a73f60",
          "message": "fix: publish request outcomes on the typed MCP route (#2864)\n\nThe typed route that MCP uses by default did not publish request_outcomes, and a bad FALLOW_DIFF_FILE failed the call while the CLI stands down. The typed dead-code, dupes, health, flags and combined outputs now carry the same request_outcomes object as the CLI, with changed-since and diff-filter entries and scope_size. A diff or ref from the ambient FALLOW_DIFF_FILE or FALLOW_CHANGED_SINCE variable that cannot be used stands down to full scope with a not-applied entry. An explicit diff_file or since argument that cannot be used still fails the call. audit, decision_surface, project_info and list_boundaries keep reading FALLOW_CHANGED_SINCE as an explicit ref.\n\nCloses #2799",
          "timestamp": "2026-09-24T21:26:22+02:00",
          "tree_id": "9c6902f914c36d822dde3825b32dab03f9e65cf9",
          "url": "https://github.com/fallow-rs/fallow/commit/d10918b39bca583b582278d4ef5740da31a73f60"
        },
        "date": 1790279749307,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10229735,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51738,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1207739,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8379,
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
          "id": "dcf13c9b62458a1ffd1d3058234db384a21a50e6",
          "message": "fix: credit names re-exported from an unresolved import (#2871)\n\nA named re-export from an import that fallow cannot resolve (for example a workspace package whose build output is not in the checkout, or export { a } from './missing') was reported as an unused export, even when another file used the name. Such a re-export is now handled like a re-export from an npm package, so the name that consumers import is credited. The unresolved-import finding stays and names the hop, and a re-exported name that no file imports is still reported. The graph cache version is bumped.\n\nCloses #2870",
          "timestamp": "2026-09-24T22:08:26+02:00",
          "tree_id": "74f5fc6b0949699103028e8d83bc0c1cd7b79a42",
          "url": "https://github.com/fallow-rs/fallow/commit/dcf13c9b62458a1ffd1d3058234db384a21a50e6"
        },
        "date": 1790283924370,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10182613,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51622,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1189779,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8290,
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
          "id": "b3918d5b491afac714780262ec70197dd6c01d0b",
          "message": "feat: read Module Federation runtime remotes with literal arguments (#2874)\n\nWhen a file imports the Module Federation runtime, registerRemotes([{ name, entry }]) with literal names registers those remote aliases like remotes, and loadRemote('app/Button') with a literal string credits the remote alias, scoped to the workspace that holds the file. A call with a non-literal argument records plugin-config-unreadable with the new reason dynamic-argument. The extraction cache version is bumped.\n\nCloses #2795",
          "timestamp": "2026-09-24T23:20:02+02:00",
          "tree_id": "a6937409933a4a71d6d6aea0a5d826393055728e",
          "url": "https://github.com/fallow-rs/fallow/commit/b3918d5b491afac714780262ec70197dd6c01d0b"
        },
        "date": 1790288073826,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10258672,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51736,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1222808,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8502,
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
          "id": "4dc965c0647c6675ece5be0e37128fd5ce1b6d3f",
          "message": "feat: name the Module Federation source in trace output (#2875)\n\n--trace-file on a file that Module Federation exposes, and --trace-dependency on a remote alias, now list a sources array with each config that exposes the file or declares the remote (kind, plugin, config and key). A remote that only a literal registerRemotes or loadRemote call names points to that source file and call. The field is optional and absent when empty, so other traces do not change. The human trace output adds one Source line per entry, and the MCP trace tools carry the same field.\n\nCloses #2796",
          "timestamp": "2026-09-25T06:22:26+02:00",
          "tree_id": "93265992ab27aff7957aeb15654828e05caede91",
          "url": "https://github.com/fallow-rs/fallow/commit/4dc965c0647c6675ece5be0e37128fd5ce1b6d3f"
        },
        "date": 1790310404829,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10250616,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51735,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1204853,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8311,
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
          "id": "f9a2841a9adf643fc32236db4e4408096cb2b760",
          "message": "fix: scope Module Federation exposes credit and read helper modules and SFC runtime calls (#2879)\n\nA bare package in exposes now gets dependency credit only for the package that owns the config, the same as shared. A Federation plugin call in a helper module that a bundler config imports (one relative import or require, inside the project root) is read, with the importing config as the base for paths. Runtime calls with literal arguments in .vue and .svelte script blocks are read, and init and createInstance register the remotes that their options name. The extraction cache version is bumped.\n\nCloses #2876",
          "timestamp": "2026-09-25T07:33:10+02:00",
          "tree_id": "a775dc45f622ff5dc1286624c72c85c618988884",
          "url": "https://github.com/fallow-rs/fallow/commit/f9a2841a9adf643fc32236db4e4408096cb2b760"
        },
        "date": 1790314714021,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "Total Bytes Allocated",
            "value": 10237520,
            "unit": "bytes"
          },
          {
            "name": "Total Allocations",
            "value": 51706,
            "unit": "allocations"
          },
          {
            "name": "Peak Memory",
            "value": 1210746,
            "unit": "bytes"
          },
          {
            "name": "Peak Allocations",
            "value": 8376,
            "unit": "allocations"
          }
        ]
      }
    ]
  }
}