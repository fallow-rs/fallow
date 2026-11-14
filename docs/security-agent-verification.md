# Security Agent Verification

`fallow security` produces deterministic security candidates. It does not call a model, decide exploitability, or emit verified vulnerabilities. Use this workflow to verify the candidates with an agent or a separate verifier harness.

The workflow uses these Fallow commands and contracts:

- `fallow security --format json --surface` for the candidate list and attack-surface inventory.
- The candidate contract: fallow fills `source_kind`, `sink`, `boundary`, `severity`, and reachability context; the verifier owns `impact`.
- `fallow security survivors --candidates fallow-security.json --verdicts verdicts.json --format json` to render verifier-retained survivors without rewriting the raw candidate output.
- `fallow security blind-spots --format json` to group unresolved callee diagnostics into action-oriented blind-spot output.
- The MCP `security_candidates` tool for agent edit loops that need the same JSON shape without shelling out directly.

## CLI Flow

Run the candidate producer with surface inventory enabled:

```bash
fallow security --format json --surface --quiet > fallow-security.json
```

The JSON envelope contains:

- `kind: "security"`
- `security_findings[]`, the raw candidates
- `attack_surface[]`, present only when `--surface` is passed
- `unresolved_edge_files` and `unresolved_callee_sites`, the in-band blind-spot counters
- `unresolved_callee_diagnostics`, when present, a bounded sample plus top files and reason counts for unresolved callee blind spots

See [`docs/output-schema.json`](output-schema.json) for the generated fallow output contract. The verifier packet below is a harness-owned convention, not a field fallow emits. The verdict schema below is a supported CLI input contract for `fallow security survivors`; fallow parses it, validates it, and renders survivors, but still does not produce verifier verdicts.

For each `security_findings[]` item, build a verifier packet from these fields:

- `finding_id`, the location-based correlation id
- `stable_finding_id`, the line-independent correlation id for verdicts that must survive edits
- `kind`, `category`, `cwe`, `path`, `line`, `col`, and `evidence`
- `severity`, the review-priority tier, not a verified vulnerability verdict
- `candidate.source_kind`, the deterministic untrusted-input kind or `null`
- `candidate.sink`, the sink location and catalogue metadata. For URL categories it may include `url_shape` (`fixed-origin-dynamic-path` or `dynamic-origin`) so verifiers can prioritize origin-control cases without parsing evidence prose.
- `candidate.boundary`, the crossed boundary fallow can derive
- `trace`, the structural import-hop trace
- `taint_flow`, if present
- `reachability.taint_confidence`, if present, to distinguish `arg-level` from `module-level` source association
- `reachability.untrusted_source_trace`, if present
- the matching `attack_surface[]` entry, if one has the same sink path and location
- caller-provided source windows for the sink, trace hops, and source endpoint

Fallow does not read source windows into the security JSON. The verifier harness should collect them from disk after the scan, usually with a small fixed radius such as 20 lines around each location. Keep the window outside fallow output so the core stays deterministic, compact, and provider-neutral. Treat source windows and verifier artifacts as local review material; do not publish private project code or verifier transcripts in release notes, README examples, or public issue comments.

## Finding identity migration

Finding IDs include the rule, project-relative path, line and column so separate
sinks on one line stay distinct. JSON and the visualization share this ID with
security SARIF's `partialFingerprints["fallowSecurity/v2"]`. Selecting fewer
files does not change the IDs of the findings that remain.

When upgrading from line-only IDs, regenerate candidate files and verdicts
together, and refresh evaluation labels keyed by `finding_id`. Every security
finding gets a new ID, including findings that did not collide before. Keep
historical candidate/verdict pairs together: they remain valid for replay, but
their verdicts must not be matched to fresh candidates by path and line alone.
Stored review history does not transfer automatically, and SARIF consumers may
close and reopen security alerts once. The legacy `fallowSecurity/v1` key is not
emitted because it could merge distinct sinks again.

An edit above a sink moves its line, so `finding_id` changes. Each finding also
has a `stable_finding_id` of the form `sec1:<16 hex digits>`. It hashes the
rule, the project-relative path and a name of the finding without its line:

- A sink in script code: the exact source text of the full sink expression.
  The extractor records the byte span of each sink in `SinkSite`: the full
  assignment for an assignment sink, and the full call with all arguments for
  a call sink. `fallow_extract::sink_identity_text` lexes the text of that
  span with the Oxc parser and joins the token texts. Whitespace, comments,
  CRLF line ends, a leading byte order mark and a trailing comma before a
  closing bracket do not count. Text inside string, template and regex
  literals counts as it is. For a JSX attribute sink such as
  `dangerouslySetInnerHTML`, the span is the attribute and not the element.
  Two such attributes with the same text share a base id, also on different
  elements or next to different other attributes.
- A hard-coded secret: the binding name and the detector signal, which is the
  provider family (for example `Stripe key`) or `entropy`. The secret value
  never enters the hash. A new value with the same signal keeps the id and
  its verdict: one entropy value in place of another, or one key in place of
  another key of the same provider. Only a new signal gives a new id, for
  example a known provider key in place of an `entropy` test value.
- A client-server leak: the project-relative paths of the first and the last
  trace hop, and the names of the env secrets that the server module reads
  (for example `process.env.DATABASE_PASSWORD`). A read of a different secret
  gives a new id.

Identical names in one file get the suffix `~1`, `~2` and so on, in source
order. A sink that an inline suppression comment hides keeps its place in
this order, so a suppression does not give its id to the next identical sink.
An edit above the sink keeps the stable id. A reformat keeps it when the
formatter changes only whitespace, comments, line breaks or trailing commas.
A change to the sink text or a file rename gives a new stable id. A new quote
style also changes the text of a string literal, so it gives a new id. A
verdict for the old id then fails with exit code 2 and does not apply to
another candidate.

`stable_finding_id` is absent when fallow does not know the exact sink text.
This is the case for a sink in Vue, Svelte or Angular template markup, because
its span covers the tag or the attribute and not the sink expression. It is
also the case for a span whose text does not lex as one complete expression,
object property or JSX attribute without a syntax error, and for an anchor
file that fallow cannot read. Key the verdict of such a finding by
`finding_id`.

The hash does not include the enclosing function. This is a deliberate
trade-off: a finding has no scope information, and the sink text already
separates most sinks. Two sinks with identical text in different functions of
one file share a base id, and the second one gets the suffix `~1`. When an edit
deletes the first sink, the second sink gets the base id, and the stored
verdict of the deleted sink applies to it. A new identical sink above an
existing one moves the suffixes in the same way. After an edit that adds or
removes such a sink, review the remaining sinks with that text again.

## MCP Flow

Ask the MCP server for the same scan:

```json
{
  "root": "/path/to/repo",
  "surface": true,
  "paths": ["src/routes/login.ts"]
}
```

`surface: true` forwards `--surface` and includes the top-level `attack_surface` array. `paths` forwards repeated `--file` flags and is intended for agent edit loops, where only just-edited anchors, trace hops, or source-trace hops should be returned.

The `security_candidates` tool returns unverified candidates. Treat it as evidence for a verification loop, not as permission to edit code. If the repository is large, raise `FALLOW_TIMEOUT_SECS` in the MCP server environment before widening scope.

MCP intentionally does not run `security survivors`: that renderer joins two local files supplied by the caller and does not need repository access. Use the CLI for survivor rendering, and use MCP `security_candidates` for the read-only candidate scan inside agent edit loops.

## Verifier Packet

Normalize each candidate into one packet before prompting a verifier:

```json
{
  "schema_version": "fallow-security-verifier-input/v1",
  "finding_id": "security:...",
  "severity": "high",
  "candidate": {
    "source_kind": "http-request-input",
    "sink": {},
    "boundary": {}
  },
  "trace": [],
  "taint_flow": null,
  "taint_confidence": "arg-level",
  "reachability_trace": [],
  "attack_surface": null,
  "source_windows": [
    {
      "path": "src/routes/login.ts",
      "start_line": 12,
      "end_line": 52,
      "text": "..."
    }
  ],
  "blind_spots": {
    "unresolved_edge_files": 0,
    "unresolved_callee_sites": 0,
    "unresolved_callee_diagnostics": null
  }
}
```

Use the packet to preserve the separation of duties:

- `fallow-security-verifier-input/v1` is a recommended harness convention, not a fallow output schema.
- Fallow-provided fields are deterministic candidate evidence.
- `source_windows` are caller-collected context.
- `blind_spots.unresolved_callee_diagnostics` can be copied from the top-level fallow output when a verifier queue wants sample locations for follow-up review. It is bounded metadata, not proof of a vulnerability.
- The verifier verdict is downstream state and must not be written back into fallow JSON.

## Prompt Contract

Use a prompt that asks the verifier to dismiss candidates aggressively unless the provided evidence supports a real exploit path:

```text
You are verifying one fallow security candidate.

Fallow is a deterministic candidate producer, not a vulnerability oracle.
Use only the supplied candidate, trace, attack-surface entry, and source windows.
Do not assume data flow beyond the provided code.

Check:
1. Is the input attacker-controlled?
2. Does the value reach the reported sink?
3. Is the reported boundary relevant to exploitability?
4. What concrete impact would remain if the candidate is real?
5. Is there an existing defensive control that dismisses it?

Return only JSON matching fallow-security-verdict/v1.
```

Pass the packet after the prompt. If `attack_surface.defensive_boundary.verification_prompt` is present, include it as an additional question, not as a verdict.

Defensive controls are observations from files on the import trace. They do not establish that a control runs before this sink or protects the same input. The `origin-equality-guard` observation records a strict `.origin` comparison against a known string with a directly exiting mismatch branch. It does not prove that the object is a built-in `URL`, that the guarded value reaches the sink unchanged, or that redirects and network policy are safe. Verify those properties from source. These observations do not suppress candidates or reduce their taint or severity.

## Verdict Schema

The verifier should return a compact verdict object:

```json
{
  "schema_version": "fallow-security-verdict/v1",
  "finding_id": "security:...",
  "verdict": "survivor",
  "reason": "The request query value reaches execSync without validation.",
  "impact": "Command injection through the id query parameter.",
  "evidence_checked": {
    "source": true,
    "sink": true,
    "boundary": true,
    "trace": true,
    "source_window": true
  },
  "dismissal_reason": null,
  "fix_direction": "avoid-shell"
}
```

`fallow-security-verdict/v1` is a supported input contract for `fallow security survivors`. Reject extra prose around the JSON object so the survivor renderer can parse the verdict without model-specific cleanup. Fallow persists `reason`, `rationale`, `confidence`, `impact`, and `fix_direction` in survivor output. `evidence_checked` and `dismissal_reason` are harness-owned audit fields today; fallow accepts verdict objects that include them, but does not render those fields.

The survivor renderer accepts either an array of verdict objects or this wrapper shape:

```json
{
  "schema_version": "fallow-security-verdicts/v1",
  "verdicts": [
    {
      "schema_version": "fallow-security-verdict/v1",
      "finding_id": "security:...",
      "verdict": "survivor",
      "rationale": "The request query value reaches execSync without validation.",
      "impact": "Command injection through the id query parameter."
    }
  ]
}
```

A verdict `finding_id` can name the candidate `finding_id` or its `stable_finding_id`. Store the stable id when a verdict file must stay valid across edits and rebases. The renderer rejects unknown verdict values, two verdicts for one candidate (also when one uses each id), unsupported schema versions, a wrapper `verdicts` field that is not an array, and verdicts that do not match any candidate in the `--candidates` file.

Allowed `verdict` values:

- `survivor`: the verifier could not dismiss the candidate from the supplied evidence.
- `dismissed`: the candidate is not exploitable from the supplied evidence.
- `needs-human-review`: the evidence is incomplete, contradictory, or blocked by missing context.

Allowed `fix_direction` values are harness-owned. Common values are:

- `delete-dead-code`
- `validate-input`
- `escape-output`
- `avoid-shell`
- `restrict-url`
- `add-authz-check`
- `harden-config`
- `needs-design-review`

For `dismissed`, set `impact` to `null` and fill `dismissal_reason`. For `survivor`, set `dismissal_reason` to `null` and explain the concrete impact. For `needs-human-review`, keep both short and point at the missing evidence.

## Rendering Survivors

After verification, run the survivor renderer:

```bash
fallow security survivors \
  --candidates fallow-security.json \
  --verdicts verdicts.json \
  --format json
```

The renderer emits `kind: "security-survivors"` with `survivors` and `needs_human_review` objects keyed by the candidate `finding_id`, plus `summary.unverdicted` for candidates with no matching verdict. Human output says "verifier-retained candidate" and keeps the boundary clear: fallow did not prove a vulnerability. In CI, add `--require-verdict-for-each-candidate` so an incomplete verdict file exits 2 instead of silently rendering a partial review.

Carry through:

- `finding_id`
- `path`, `line`, and `col`
- `category` and `cwe`
- `candidate`
- `taint_flow` or `reachability_trace`
- verifier `impact`, `reason`, and `fix_direction`

Do not rewrite fallow's original JSON with verdict fields. Store verifier output beside it, keyed by `stable_finding_id`, so reruns can correlate after a rebase without changing the fallow contract. When a finding has no `stable_finding_id` (a template markup sink, or an anchor file that fallow could not read), key its verdict by `finding_id`.

## Rendering Blind Spots

Use blind-spot output when a verifier queue needs to understand where fallow may have missed candidates behind dynamic callees:

```bash
fallow security blind-spots --format json
```

The renderer emits `kind: "security-blind-spots"` with aggregate counts and grouped samples by unresolved reason, expression kind, file, and suggested next action. It accepts `--file <PATH>` either before or after `blind-spots`, matching the parent `fallow security --file <PATH>` scope. It is derived from existing bounded diagnostics. A non-zero result is not a finding by itself, but it is not a clean bill either.

## Quality Caveats

Candidate quality depends on the source and trace fidelity in the current fallow version:

- HTTP-input source patterns are receiver-gated to avoid broad `*.query` collisions with unrelated APIs, but framework-specific request aliases can still need verifier judgment.
- `reachability.taint_confidence` distinguishes `arg-level` from `module-level` source association, and arg-level traces anchor the source read when available. Module-level traces remain ranking evidence, not proof of value flow.

These caveats do not change the recipe. They mean the verifier should use `severity` and `taint_confidence` for triage order, then verify source control, value flow, sink behavior, and defensive controls from source windows before reporting a survivor.
