# Security Agent Verification

`fallow security` is a deterministic candidate producer. It does not call a model, decide exploitability, or emit verified vulnerabilities. Use this recipe when an agent or out-of-core harness should turn raw candidates into verifier-filtered survivors.

The workflow uses three fallow surfaces:

- `fallow security --format json --surface` for the candidate list and attack-surface inventory.
- The candidate contract: fallow fills `source_kind`, `sink`, `boundary`, `severity`, and reachability context; the verifier owns `impact`.
- `fallow security survivors --candidates fallow-security.json --verdicts verdicts.json --format json` to render verifier-filtered survivors without rewriting the raw candidate output.
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

- `finding_id`, the stable correlation id
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

`fallow-security-verdict/v1` is a supported input contract for `fallow security survivors`. Reject extra prose around the JSON object so the survivor renderer can parse the verdict without model-specific cleanup.

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

The renderer rejects unknown verdict values, duplicate `finding_id` values, unsupported schema versions, and verdicts that do not match any candidate in the `--candidates` file.

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

The renderer emits `kind: "security-survivors"` with `survivors` and `needs_human_review` objects keyed by `finding_id`. Human output says "externally verified candidate" and keeps the boundary clear: fallow did not prove a vulnerability.

Carry through:

- `finding_id`
- `path`, `line`, and `col`
- `category` and `cwe`
- `candidate`
- `taint_flow` or `reachability_trace`
- verifier `impact`, `reason`, and `fix_direction`

Do not rewrite fallow's original JSON with verdict fields. Store verifier output beside it, keyed by `finding_id`, so reruns can correlate after a rebase without changing the fallow contract.

## Rendering Blind Spots

Use blind-spot output when a verifier queue needs to understand where fallow may have missed candidates behind dynamic callees:

```bash
fallow security blind-spots --format json
```

The renderer emits `kind: "security-blind-spots"` with aggregate counts and grouped samples by unresolved reason, expression kind, file, and suggested next action. It is derived from existing bounded diagnostics. A non-zero result is not a finding by itself, but it is not a clean bill either.

## Quality Caveats

Candidate quality depends on the source and trace fidelity in the current fallow version:

- HTTP-input source patterns are receiver-gated to avoid broad `*.query` collisions with unrelated APIs, but framework-specific request aliases can still need verifier judgment.
- `reachability.taint_confidence` distinguishes `arg-level` from `module-level` source association, and arg-level traces anchor the source read when available. Module-level traces remain ranking evidence, not proof of value flow.

These caveats do not change the recipe. They mean the verifier should use `severity` and `taint_confidence` for triage order, then verify source control, value flow, sink behavior, and defensive controls from source windows before reporting a survivor.
