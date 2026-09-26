window.BENCHMARK_DATA = {
  "lastUpdate": 1790452998963,
  "repoUrl": "https://github.com/fallow-rs/fallow",
  "entries": {
    "Fallow CLI Work Counters": [
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
          "id": "fd7dcc674f6f774e96e7414fcc25e14b3bb835f6",
          "message": "fix: start no child process in the CLI instruction count benchmarks\n\nThe CodSpeed CPU simulation rejects a measured process that starts a\nchild process. The scheduled run failed on the first benchmark, because\n`fallow dead-code` started `git` to decide on the `audit-changed` and\n`scope-workspaces` next steps.\n\n- `FALLOW_SUGGESTIONS=off` now skips these git probes. The next steps\n  were already hidden in that case, so only the wasted probes go.\n- The workflow and `benchmarks/cli-instructions.sh` set\n  `FALLOW_SUGGESTIONS=off`. GitHub Actions sets `CI`, which already\n  skips the impact identity probe.\n- `audit` leaves the benchmark list, because it reads the changeset\n  with git and cannot run under the simulation.\n- A new integration test runs the benchmark flags with a git shim on\n  `PATH` and fails on any git call. A control run shows that the shim\n  records the probes when suggestions are on.",
          "timestamp": "2026-09-26T19:51:18Z",
          "url": "https://github.com/fallow-rs/fallow/commit/fd7dcc674f6f774e96e7414fcc25e14b3bb835f6"
        },
        "date": 1790452993866,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cli preact dead-code (cold): files_read",
            "value": 253,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (cold): source_bytes_read",
            "value": 1314639,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (cold): parse_cache_bytes_read",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (cold): css_masked_bytes",
            "value": 11968,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (cold): resolve_specifier_calls",
            "value": 1524,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (cold): unique_specifiers",
            "value": 655,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (cold): oxc_resolve_calls",
            "value": 833,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (cold): canonicalize_calls",
            "value": 263,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (warm): files_read",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (warm): source_bytes_read",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (warm): parse_cache_bytes_read",
            "value": 1232424,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (warm): css_masked_bytes",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (warm): resolve_specifier_calls",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (warm): unique_specifiers",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (warm): oxc_resolve_calls",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli preact dead-code (warm): canonicalize_calls",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (cold): files_read",
            "value": 175,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (cold): source_bytes_read",
            "value": 1011824,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (cold): parse_cache_bytes_read",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (cold): css_masked_bytes",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (cold): resolve_specifier_calls",
            "value": 578,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (cold): unique_specifiers",
            "value": 416,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (cold): oxc_resolve_calls",
            "value": 359,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (cold): canonicalize_calls",
            "value": 1,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (warm): files_read",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (warm): source_bytes_read",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (warm): parse_cache_bytes_read",
            "value": 1131648,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (warm): css_masked_bytes",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (warm): resolve_specifier_calls",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (warm): unique_specifiers",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (warm): oxc_resolve_calls",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli zod dead-code (warm): canonicalize_calls",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (cold): files_read",
            "value": 563,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (cold): source_bytes_read",
            "value": 4283038,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (cold): parse_cache_bytes_read",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (cold): css_masked_bytes",
            "value": 4977,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (cold): resolve_specifier_calls",
            "value": 6047,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (cold): unique_specifiers",
            "value": 2086,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (cold): oxc_resolve_calls",
            "value": 2119,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (cold): canonicalize_calls",
            "value": 978,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (warm): files_read",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (warm): source_bytes_read",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (warm): parse_cache_bytes_read",
            "value": 5601131,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (warm): css_masked_bytes",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (warm): resolve_specifier_calls",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (warm): unique_specifiers",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (warm): oxc_resolve_calls",
            "value": 0,
            "unit": "count"
          },
          {
            "name": "cli vue-core dead-code (warm): canonicalize_calls",
            "value": 0,
            "unit": "count"
          }
        ]
      }
    ]
  }
}