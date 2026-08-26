# Semantic Similar-Code Workflow

Similar-code is an opt-in, local semantic discovery workflow. It complements deterministic `fallow dupes`; it does not replace clone detection, tests, or human judgment.

1. Check readiness with `fallow similar-code status --format json --quiet`. If the pinned model is missing, ask the user to run `fallow similar-code setup --local`. Agents must not authorize setup or run it on the user's behalf.
2. Run discovery once. Preserve its independent JSON envelope so inspection and review use the exact candidate set:

   ```bash
   fallow similar-code --file src/services/api.ts --format json --quiet > similar-code.json
   ```

   In Node, call `detectSimilarCode({ files: ["src/services/api.ts"] })`. Over MCP, call standalone `find_similar_code` with `paths: ["src/services/api.ts"]`. Do not use Code Mode for similar-code because its 30-second window cannot accommodate documented cold inference. The standalone MCP tools have a dedicated 15-minute timeout.
3. Inspect a candidate before judging it. Hand off the original raw discovery document so inspect selects that exact candidate without rerunning global retrieval or ranking:

   ```bash
   fallow similar-code inspect sc_example --candidates similar-code.json \
     --format json --quiet
   ```

   Over MCP, call standalone `inspect_similar_code` with `candidate_id` and a typed `snapshot` containing the unchanged discovery `schema_version`, `generation`, selected `candidate`, `completion`, and `diagnostics`. Treat `generation.scope.paths` as provenance, not as an argument list. Inspect re-extracts only the two snapshot endpoints, validates both current source hashes, and fails closed on stale source. Review source, callers, callees, tests, side effects, ownership, and missing evidence.
4. Author a separate verdict document. Keep the three axes independent. `refactor_safe: true` requires `behaviorally_equivalent: true`, which requires `candidate_worthy: true`. Use `null` for an undecided axis, use `needs-human-review`, and abstain when evidence is incomplete.

   ```json
   {
     "schema_version": "1",
     "verdicts": [
       {
         "candidate_id": "sc_example",
         "review_key": "scr_example",
         "candidate_worthy": true,
         "behaviorally_equivalent": false,
         "refactor_safe": false,
         "outcome": "related-but-distinct",
         "rationale": "Both normalize input, but only one preserves empty values."
       }
     ]
   }
   ```

5. Join raw candidates and verdicts without changing either input:

   ```bash
   fallow similar-code review --candidates similar-code.json --verdicts verdicts.json \
     --require-verdict-for-each-candidate --format json --quiet
   ```

Only `completion.status: "complete"` makes an empty candidate list conclusive for the admitted scope. Candidates remain advisory and unverified until the separate verdict flow supplies source-grounded judgment.
