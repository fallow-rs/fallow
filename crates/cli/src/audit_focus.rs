//! Weighted focus map (stage 4): a COMPOSITE attention score per review unit
//! that ranks where scarce reviewer attention goes.
//!
//! A 40-file diff becomes a handful of `review-here` pieces plus an enumerable
//! `not-prioritized` remainder. The free tier RANKS but NEVER says "skip" (safe
//! explicit-skip is paid, runtime-backed only); each unit carries a human
//! reason; a per-unit confidence flag protects dynamically-wired / re-export-heavy
//! code from a silent static-reachability de-prioritization; and the
//! `deprioritized` escape-hatch list makes EVERY de-prioritized piece reachable.
//!
//! ## The composite score (deterministic, no runtime input)
//!
//! `score = fan_io + security_taint + risk_zone + change_shape`, an integer sum
//! (no floats, matching the partition + order engine's determinism posture) of four deterministic signals,
//! each derived from data the brief already retains:
//!
//! 1. **fan-in / fan-out** (graph blast): from engine-owned focus fact helpers.
//! 2. **security taint touch**: a source -> sink taint trace touches the unit
//!    (reuse `SecurityFinding.trace`). Built as a pure function of a security-
//!    finding slice; the brief path carries an EMPTY slice today (security is the
//!    opt-in `fallow security` command, not the bare dead-code analysis), so this
//!    contributes 0 until a future epic threads a security pass. The seam is wired
//!    and tested; no taint engine runs here.
//! 3. **risk zone**: boundary / public-API / security-sensitive.
//! 4. **change shape**: new export / widened visibility / signature change (the
//!    coordination-gap proxy, ADR-001 syntactic).
//!
//! ## The runtime layer (paid, built)
//!
//! When `FocusInputs::runtime` is `Some` (the paid `--runtime-coverage` path),
//! a hot file adds an invocation-bucketed `runtime` component to its score so it
//! amplifies the blast and outranks an otherwise-equal cold unit, and a unit the
//! runtime proves cold AND that carries no deterministic signal earns the SAFE
//! explicit-skip label (`FocusLabel::Skip`). When `runtime` is `None` (free
//! mode) the layer contributes nothing: the `runtime` component is `0`, no unit
//! can reach the `Skip` arm, and the output is byte-identical to the no-runtime baseline. The free
//! tier ranks but never skips; safe-skip is runtime-backed only.

use fallow_engine::FocusFileFactsPaths;
pub use fallow_output::{ConfidenceFlag, FocusLabel, FocusMap, FocusScore, FocusUnit};

/// A unit's score at or above this threshold is labeled [`FocusLabel::ReviewHere`];
/// below it, [`FocusLabel::NotPrioritized`]. Tuned so a unit with any non-trivial
/// blast or a single risk-zone / change-shape signal lands above the line, while a
/// fully isolated change (no fan-in, no zone, no change-shape) lands below it.
const REVIEW_HERE_THRESHOLD: u32 = 3;

/// Fan-in (blast radius) is the stage-4 priority signal; weight it higher than
/// fan-out. Each is capped at [`FAN_CAP`] so one extreme-fan-in file does not
/// swamp the bounded zone / change-shape signals.
const FAN_IN_WEIGHT: u32 = 2;
/// Fan-out weight (forward-dependency breadth), lower than fan-in.
const FAN_OUT_WEIGHT: u32 = 1;
/// Cap on the raw fan-in / fan-out count before weighting, so the blast signal
/// stays bounded relative to the other three.
const FAN_CAP: u32 = 5;
/// Points added per present risk zone (boundary / public-API / security-sensitive).
const RISK_ZONE_WEIGHT: u32 = 2;
/// Points added per present change-shape signal (new/widened export, sig change).
const CHANGE_SHAPE_WEIGHT: u32 = 2;
/// Points added when a unit sits on a security source -> sink taint trace.
const SECURITY_TAINT_WEIGHT: u32 = 3;

/// Runtime-weight floor: any hot path adds at least this, so a hot unit
/// always outranks an otherwise-equal cold one. At/above [`REVIEW_HERE_THRESHOLD`]
/// so a hot path alone pulls a unit into `review-here`.
const RUNTIME_HOT_FLOOR: u32 = 3;
/// Runtime weight for a warm path (>= [`RUNTIME_WARM_INVOCATIONS`]).
const RUNTIME_HOT_WARM: u32 = 4;
/// Runtime weight for a blazing path (>= [`RUNTIME_BLAZING_INVOCATIONS`]). Capped
/// (bucketed, not the raw count) so one extreme-traffic file cannot swamp the
/// bounded deterministic signals and the score stays deterministic.
const RUNTIME_HOT_BLAZING: u32 = 6;
/// Invocation count at/above which a hot path is "warm".
const RUNTIME_WARM_INVOCATIONS: u64 = 100;
/// Invocation count at/above which a hot path is "blazing".
const RUNTIME_BLAZING_INVOCATIONS: u64 = 1_000;

/// The bucketed runtime weight for a hot file's peak invocation count. Bucketed
/// (three bands) rather than raw so the weight stays bounded and deterministic.
fn runtime_weight(invocations: u64) -> u32 {
    if invocations >= RUNTIME_BLAZING_INVOCATIONS {
        RUNTIME_HOT_BLAZING
    } else if invocations >= RUNTIME_WARM_INVOCATIONS {
        RUNTIME_HOT_WARM
    } else {
        RUNTIME_HOT_FLOOR
    }
}

/// A boundary-zone signal for a unit: the unit's file introduced a new cross-zone
/// edge (it is the `from_file` of an introduced boundary edge).
#[derive(Debug, Clone)]
pub struct BoundaryZoneFile {
    /// Root-relative path of the importing file that introduced the edge.
    pub from_file: String,
}

/// A runtime-hot file: a changed file with a runtime hot path, paired with the
/// file's peak invocation count (the max over the file's hot functions).
#[derive(Debug, Clone)]
pub struct RuntimeHotFile {
    /// Root-relative path of the hot file (the brief's canonical path space).
    pub file: String,
    /// Peak invocation count across the file's hot functions; drives the band.
    pub invocations: u64,
}

/// Per-file runtime evidence for the paid weighting layer, built from the
/// runtime-coverage health report at the brief path. `None` in free mode; when
/// present it adds the runtime score component and enables the safe explicit-skip
/// label. The two lists are disjoint by construction (a hot file is never cold).
#[derive(Debug, Clone, Default)]
pub struct RuntimeFocus {
    /// Files with a runtime hot path; each adds a bucketed runtime weight.
    pub hot_files: Vec<RuntimeHotFile>,
    /// Files the runtime proves cold (every retained finding is `safe_to_delete`
    /// and the file has no hot path). Eligible for safe-skip when also carrying no
    /// deterministic signal.
    pub cold_files: Vec<String>,
}

/// Everything the focus extractor needs, gathered from the assembled brief data.
/// All path-spaces are root-relative + forward-slashed (the brief's canonical
/// space), so signal joins are byte-exact.
pub struct FocusInputs<'a> {
    /// Per-file graph facts (fan-in/out + confidence-flag signals) from
    /// Engine focus facts, path-resolved. The unit spine.
    pub graph_facts: &'a [FocusFileFactsPaths],
    /// Root-relative `from_file`s of introduced boundary edges. A unit file
    /// in this set carries the boundary risk-zone signal.
    pub boundary_files: &'a [BoundaryZoneFile],
    /// The exports-aware public-API surface delta keys (`<rel_path>::<name>`).
    /// A unit file that is the `<rel_path>` prefix of any key carries the
    /// public-API risk-zone AND new/widened-export change-shape signals.
    pub public_api_added: &'a [String],
    /// Root-relative changed-file paths that changed a contract consumed outside
    /// the diff (coordination-gap `changed_file`s). A unit file here carries
    /// the signature-change change-shape signal (syntactic proxy, ADR-001).
    pub coordination_changed_files: &'a [String],
    /// Root-relative file paths a security source -> sink taint trace touches
    /// (reuse `SecurityFinding.trace`). EMPTY on the brief path today (the taint
    /// engine is the opt-in `fallow security` command); the seam lights up the
    /// moment a security pass is threaded, with no focus-map code change.
    pub taint_touched_files: &'a [String],
    /// Per-file runtime evidence (paid). `None` in free mode, where the focus
    /// map degrades to the deterministic no-runtime baseline byte-for-byte; `Some` on the
    /// `--runtime-coverage` path, where it weights hot files and enables safe-skip.
    pub runtime: Option<&'a RuntimeFocus>,
}

/// Whether a unit `file` is the `<rel_path>` prefix of any public-API delta key
/// (`<rel_path>::<name>`).
fn file_in_public_api(file: &str, public_api_added: &[String]) -> bool {
    public_api_added
        .iter()
        .any(|key| key.split("::").next() == Some(file))
}

/// Compute one unit's composite score from the present signals.
fn score_unit(facts: &FocusFileFactsPaths, inputs: &FocusInputs<'_>) -> FocusScore {
    let fan_io =
        facts.fan_in.min(FAN_CAP) * FAN_IN_WEIGHT + facts.fan_out.min(FAN_CAP) * FAN_OUT_WEIGHT;

    let taint_touched = inputs.taint_touched_files.iter().any(|f| f == &facts.file);
    let security_taint = if taint_touched {
        SECURITY_TAINT_WEIGHT
    } else {
        0
    };

    let in_boundary = inputs
        .boundary_files
        .iter()
        .any(|b| b.from_file == facts.file);
    let in_public_api = file_in_public_api(&facts.file, inputs.public_api_added);
    // SECURITY-SENSITIVE risk zone reuses the taint-touch signal.
    let zones = u32::from(in_boundary) + u32::from(in_public_api) + u32::from(taint_touched);
    let risk_zone = zones * RISK_ZONE_WEIGHT;

    // NEW/WIDENED EXPORT (public-API delta) + SIGNATURE CHANGE (coordination-gap
    // proxy). DELETED SYMBOL is deferred (no per-symbol deletion delta on the
    // brief path); it is a future change-shape multiply-in, scores 0 today.
    let new_export = in_public_api;
    let sig_change = inputs
        .coordination_changed_files
        .iter()
        .any(|f| f == &facts.file);
    let shapes = u32::from(new_export) + u32::from(sig_change);
    let change_shape = shapes * CHANGE_SHAPE_WEIGHT;

    // Runtime layer (paid): a hot file adds an invocation-bucketed weight so a
    // hot path amplifies the blast and outranks an otherwise-equal cold unit. The
    // deterministic components stay on the wire, so this ADDS without recomputing
    // them. `0` when no runtime input (free mode), keeping `total` the four
    // deterministic components and the output byte-identical to the no-runtime baseline.
    let runtime = inputs.runtime.map_or(0, |rt| {
        rt.hot_files
            .iter()
            .find(|hot| hot.file == facts.file)
            .map_or(0, |hot| runtime_weight(hot.invocations))
    });

    let total = fan_io + security_taint + risk_zone + change_shape + runtime;

    FocusScore {
        fan_io,
        security_taint,
        risk_zone,
        change_shape,
        runtime,
        total,
    }
}

/// Whether a unit is the SAFE explicit-skip case: runtime-backed ONLY. All
/// of: the runtime proves the file cold (in [`RuntimeFocus::cold_files`]); it
/// carries no deterministic signal (`total` is the runtime component alone, i.e.
/// `0`, so no fan-in/out, risk-zone, change-shape, or taint); and it carries NO
/// confidence flag (a dynamically-wired / re-export-heavy unit has uncertain
/// reachability, so a single runtime capture is not enough to call it safe to
/// skip). Any of those keeps the unit visible. Returns `false` whenever `runtime`
/// is `None`, so free mode can never label a unit `skip`.
fn is_safe_skip(facts: &FocusFileFactsPaths, score: &FocusScore, inputs: &FocusInputs<'_>) -> bool {
    inputs.runtime.is_some_and(|rt| {
        score.total == 0
            && !facts.dynamic_dispatch
            && !facts.re_export_indirection
            && rt.cold_files.iter().any(|file| file == &facts.file)
    })
}

/// Build the human reason for a unit from the present signals.
fn build_reason(
    facts: &FocusFileFactsPaths,
    score: &FocusScore,
    inputs: &FocusInputs<'_>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    // Runtime evidence first (it amplifies / disarms the static signals). On the
    // free path `inputs.runtime` is `None`, so no runtime clause is ever added and
    // the reason string stays byte-identical to the no-runtime baseline.
    if let Some(rt) = inputs.runtime {
        if let Some(hot) = rt.hot_files.iter().find(|hot| hot.file == facts.file) {
            parts.push(format!(
                "hot path ({} invocation{})",
                hot.invocations,
                if hot.invocations == 1 { "" } else { "s" }
            ));
        } else if rt.cold_files.iter().any(|file| file == &facts.file) {
            // "no hot path" is the honest, file-level-true claim: the cold signal
            // means no hot path here and the file's tracked findings are all
            // safe-to-delete. It deliberately does NOT say "0 invocations" -- a
            // sub-hot-threshold active function is invisible to this signal.
            parts.push("runtime-cold (no hot path)".to_string());
        }
    }
    if facts.fan_in > 0 {
        parts.push(format!(
            "high fan-in ({} importer{})",
            facts.fan_in,
            if facts.fan_in == 1 { "" } else { "s" }
        ));
    }
    if facts.fan_out > 0 {
        parts.push(format!("fan-out {}", facts.fan_out));
    }
    if score.security_taint > 0 {
        parts.push("on a security taint path".to_string());
    }
    if inputs
        .boundary_files
        .iter()
        .any(|b| b.from_file == facts.file)
    {
        parts.push("introduces a cross-zone edge".to_string());
    }
    if file_in_public_api(&facts.file, inputs.public_api_added) {
        parts.push("widens the public API".to_string());
    }
    if inputs
        .coordination_changed_files
        .iter()
        .any(|f| f == &facts.file)
    {
        parts.push("changes a contract consumed outside the diff".to_string());
    }
    if parts.is_empty() {
        "isolated change, no blast beyond the diff".to_string()
    } else {
        parts.join(", ")
    }
}

/// Collect a unit's confidence flags from its graph facts (sorted, deduped).
fn confidence_flags(facts: &FocusFileFactsPaths) -> Vec<ConfidenceFlag> {
    let mut flags: Vec<ConfidenceFlag> = Vec::new();
    if facts.dynamic_dispatch {
        flags.push(ConfidenceFlag::DynamicDispatch);
    }
    if facts.re_export_indirection {
        flags.push(ConfidenceFlag::ReExportIndirection);
    }
    flags
}

/// Build the weighted focus map from the assembled brief inputs: score each unit,
/// label it (`review-here` / `not-prioritized`, NEVER `skip`), attach the reason
/// and confidence flags, then partition into the ranked `review_here` list and
/// the FULL `deprioritized` escape-hatch list.
///
/// Pure + deterministic: no timestamps, no randomness, integer arithmetic only,
/// so two runs over the same tree produce a byte-identical focus map. The two
/// output lists partition the unit set, so the escape-hatch completeness invariant
/// (`review_here.len() + deprioritized.len() == graph_facts.len()`) holds by
/// construction.
#[must_use]
pub fn build_focus_map(inputs: &FocusInputs<'_>) -> FocusMap {
    let mut units: Vec<FocusUnit> = inputs
        .graph_facts
        .iter()
        .map(|facts| {
            let score = score_unit(facts, inputs);
            // Safe explicit-skip wins first, but is runtime-backed only:
            // `is_safe_skip` is always false without runtime input, so free mode
            // falls through to the no-runtime review-here / not-prioritized split.
            let label = if is_safe_skip(facts, &score, inputs) {
                FocusLabel::Skip
            } else if score.total >= REVIEW_HERE_THRESHOLD {
                FocusLabel::ReviewHere
            } else {
                FocusLabel::NotPrioritized
            };
            let reason = build_reason(facts, &score, inputs);
            FocusUnit {
                file: facts.file.clone(),
                score,
                label,
                reason,
                confidence: confidence_flags(facts),
            }
        })
        .collect();

    // Rank by score descending, ties broken by path for determinism.
    units.sort_by(|a, b| {
        b.score
            .total
            .cmp(&a.score.total)
            .then_with(|| a.file.cmp(&b.file))
    });

    let mut review_here: Vec<FocusUnit> = Vec::new();
    let mut deprioritized: Vec<FocusUnit> = Vec::new();
    for unit in units {
        match unit.label {
            FocusLabel::ReviewHere => review_here.push(unit),
            // Both not-prioritized and the runtime-backed safe-skip land in the
            // escape hatch: nothing is hidden, a skip is just labelled safe.
            FocusLabel::NotPrioritized | FocusLabel::Skip => deprioritized.push(unit),
        }
    }
    // The deprioritized escape hatch is path-sorted (stable enumeration order).
    deprioritized.sort_by(|a, b| a.file.cmp(&b.file));

    FocusMap {
        review_here,
        deprioritized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(
        file: &str,
        fan_in: u32,
        fan_out: u32,
        dynamic: bool,
        re_export: bool,
    ) -> FocusFileFactsPaths {
        FocusFileFactsPaths {
            file: file.to_string(),
            fan_in,
            fan_out,
            dynamic_dispatch: dynamic,
            re_export_indirection: re_export,
        }
    }

    fn inputs<'a>(
        graph_facts: &'a [FocusFileFactsPaths],
        boundary_files: &'a [BoundaryZoneFile],
        public_api_added: &'a [String],
        coordination_changed_files: &'a [String],
        taint_touched_files: &'a [String],
    ) -> FocusInputs<'a> {
        FocusInputs {
            graph_facts,
            boundary_files,
            public_api_added,
            coordination_changed_files,
            taint_touched_files,
            runtime: None,
        }
    }

    /// Build a `RuntimeFocus` from `(file, invocations)` hot pairs and cold files.
    fn runtime(hot: &[(&str, u64)], cold: &[&str]) -> RuntimeFocus {
        RuntimeFocus {
            hot_files: hot
                .iter()
                .map(|(file, invocations)| RuntimeHotFile {
                    file: (*file).to_string(),
                    invocations: *invocations,
                })
                .collect(),
            cold_files: cold.iter().map(|file| (*file).to_string()).collect(),
        }
    }

    /// `inputs` with a runtime layer attached (the paid `--runtime-coverage` path).
    fn inputs_rt<'a>(
        graph_facts: &'a [FocusFileFactsPaths],
        public_api_added: &'a [String],
        taint_touched_files: &'a [String],
        runtime: &'a RuntimeFocus,
    ) -> FocusInputs<'a> {
        FocusInputs {
            graph_facts,
            boundary_files: &[],
            public_api_added,
            coordination_changed_files: &[],
            taint_touched_files,
            runtime: Some(runtime),
        }
    }

    // (a) NO `skip` label is ever emitted in free mode. The enum has no Skip
    // variant; the test pins the serialized strings of every produced label.
    #[test]
    fn no_skip_label_ever_emitted_in_free_mode() {
        let gf = vec![
            facts("src/hot.ts", 12, 3, false, false), // review-here
            facts("src/iso.ts", 0, 0, false, false),  // not-prioritized
        ];
        let map = build_focus_map(&inputs(&gf, &[], &[], &[], &[]));
        let all_units: Vec<&FocusUnit> = map
            .review_here
            .iter()
            .chain(map.deprioritized.iter())
            .collect();
        assert!(!all_units.is_empty());
        for unit in all_units {
            let token = unit.label.token();
            assert_ne!(token, "skip", "free mode must never emit a skip label");
            assert!(
                token == "review-here" || token == "not-prioritized",
                "unexpected label token {token}"
            );
        }
        // Serialized JSON must not carry the token "skip" anywhere either.
        let json = serde_json::to_string(&map).expect("serialize");
        assert!(
            !json.contains("\"skip\""),
            "serialized focus map leaked a skip label: {json}"
        );
    }

    // (b) Every de-prioritized unit is enumerable via the escape hatch:
    // count(review_here) + count(deprioritized) == count(all).
    #[test]
    fn escape_hatch_enumerates_every_deprioritized_unit() {
        let gf = vec![
            facts("src/a.ts", 12, 4, false, false), // review-here
            facts("src/b.ts", 0, 0, false, false),  // not-prioritized
            facts("src/c.ts", 1, 0, false, false),  // not-prioritized (score 2 < 3)
            facts("src/d.ts", 8, 0, false, false),  // review-here
        ];
        let map = build_focus_map(&inputs(&gf, &[], &[], &[], &[]));
        assert_eq!(
            map.total_units(),
            gf.len(),
            "every unit must be reachable via review-here OR deprioritized"
        );
        // The deprioritized list is the escape hatch: nothing is hidden.
        assert!(!map.deprioritized.is_empty());
        // No file appears in both lists (a strict partition).
        for d in &map.deprioritized {
            assert!(
                !map.review_here.iter().any(|r| r.file == d.file),
                "{} is in both lists",
                d.file
            );
        }
    }

    // (c) A dynamically-wired unit carries the `low: dynamic dispatch detected`
    // flag; a re-export-indirection unit carries `low: re-export indirection`.
    #[test]
    fn dynamic_and_re_export_units_carry_low_confidence_flags() {
        let gf = vec![
            facts("src/dyn.ts", 0, 0, true, false),
            facts("src/barrel.ts", 0, 0, false, true),
            facts("src/both.ts", 0, 0, true, true),
        ];
        let map = build_focus_map(&inputs(&gf, &[], &[], &[], &[]));
        let all: Vec<&FocusUnit> = map
            .review_here
            .iter()
            .chain(map.deprioritized.iter())
            .collect();
        let find = |file: &str| all.iter().find(|u| u.file == file).expect("unit present");

        let dyn_unit = find("src/dyn.ts");
        assert!(
            dyn_unit
                .confidence
                .contains(&ConfidenceFlag::DynamicDispatch),
            "dynamic unit must carry the dynamic-dispatch flag"
        );
        assert_eq!(
            ConfidenceFlag::DynamicDispatch.message(),
            "low: dynamic dispatch detected"
        );

        let barrel = find("src/barrel.ts");
        assert!(
            barrel
                .confidence
                .contains(&ConfidenceFlag::ReExportIndirection),
            "barrel unit must carry the re-export-indirection flag"
        );
        assert_eq!(
            ConfidenceFlag::ReExportIndirection.message(),
            "low: re-export indirection"
        );

        let both = find("src/both.ts");
        assert_eq!(both.confidence.len(), 2, "both flags present");
    }

    #[test]
    fn confidence_flag_never_lowers_the_score() {
        // Two identical-signal units, one with confidence flags: same total.
        let plain = facts("src/plain.ts", 5, 0, false, false);
        let flagged = facts("src/flagged.ts", 5, 0, true, true);
        let plain_map = build_focus_map(&inputs(&[plain], &[], &[], &[], &[]));
        let flagged_map = build_focus_map(&inputs(&[flagged], &[], &[], &[], &[]));
        let plain_total = plain_map
            .review_here
            .iter()
            .chain(plain_map.deprioritized.iter())
            .next()
            .unwrap()
            .score
            .total;
        let flagged_total = flagged_map
            .review_here
            .iter()
            .chain(flagged_map.deprioritized.iter())
            .next()
            .unwrap()
            .score
            .total;
        assert_eq!(
            plain_total, flagged_total,
            "flags are advisory, not a penalty"
        );
    }

    #[test]
    fn risk_zone_and_change_shape_signals_raise_the_score() {
        let gf = vec![facts("src/api.ts", 0, 0, false, false)];
        let public_api = vec!["src/api.ts::Widget".to_string()];
        let map = build_focus_map(&inputs(&gf, &[], &public_api, &[], &[]));
        let unit = map
            .review_here
            .iter()
            .chain(map.deprioritized.iter())
            .next()
            .unwrap();
        // public-API delta -> risk_zone (+2) AND change_shape new-export (+2) = 4.
        assert_eq!(unit.score.risk_zone, RISK_ZONE_WEIGHT);
        assert_eq!(unit.score.change_shape, CHANGE_SHAPE_WEIGHT);
        assert_eq!(unit.label, FocusLabel::ReviewHere);
        assert!(unit.reason.contains("public API"));
    }

    #[test]
    fn security_taint_seam_is_zero_with_empty_findings_and_lights_up_with_a_touch() {
        let gf = vec![facts("src/sink.ts", 0, 0, false, false)];
        // Empty taint slice (the brief-path reality today): seam contributes 0.
        let no_taint = build_focus_map(&inputs(&gf, &[], &[], &[], &[]));
        let no_taint_unit = no_taint
            .review_here
            .iter()
            .chain(no_taint.deprioritized.iter())
            .next()
            .unwrap();
        assert_eq!(no_taint_unit.score.security_taint, 0);
        assert_eq!(no_taint_unit.label, FocusLabel::NotPrioritized);

        // A future security pass threads the touched file: the seam lights up.
        let touched = vec!["src/sink.ts".to_string()];
        let with_taint = build_focus_map(&inputs(&gf, &[], &[], &[], &touched));
        let taint_unit = with_taint
            .review_here
            .iter()
            .chain(with_taint.deprioritized.iter())
            .next()
            .unwrap();
        assert_eq!(taint_unit.score.security_taint, SECURITY_TAINT_WEIGHT);
        // taint -> also a security-sensitive risk zone (+2).
        assert_eq!(taint_unit.score.risk_zone, RISK_ZONE_WEIGHT);
        assert_eq!(taint_unit.label, FocusLabel::ReviewHere);
    }

    #[test]
    fn coordination_gap_drives_signature_change_shape() {
        let gf = vec![facts("src/core.ts", 0, 0, false, false)];
        let coordination = vec!["src/core.ts".to_string()];
        let map = build_focus_map(&inputs(&gf, &[], &[], &coordination, &[]));
        let unit = map
            .review_here
            .iter()
            .chain(map.deprioritized.iter())
            .next()
            .unwrap();
        assert_eq!(unit.score.change_shape, CHANGE_SHAPE_WEIGHT);
        assert!(unit.reason.contains("contract consumed outside the diff"));
    }

    #[test]
    fn focus_map_is_byte_identical_across_runs() {
        let gf = vec![
            facts("src/a.ts", 5, 2, true, false),
            facts("src/b.ts", 0, 0, false, true),
            facts("src/c.ts", 3, 1, false, false),
        ];
        let boundary = vec![BoundaryZoneFile {
            from_file: "src/a.ts".to_string(),
        }];
        let public_api = vec!["src/c.ts::Thing".to_string()];
        let first = build_focus_map(&inputs(&gf, &boundary, &public_api, &[], &[]));
        let second = build_focus_map(&inputs(&gf, &boundary, &public_api, &[], &[]));
        let s1 = serde_json::to_string_pretty(&first).unwrap();
        let s2 = serde_json::to_string_pretty(&second).unwrap();
        assert_eq!(s1, s2);
    }

    #[test]
    fn review_here_is_ranked_by_score_descending() {
        let gf = vec![
            facts("src/low.ts", 2, 0, false, false),   // score 4
            facts("src/high.ts", 12, 5, false, false), // score capped high
        ];
        let public_api = vec!["src/low.ts::X".to_string()];
        let map = build_focus_map(&inputs(&gf, &[], &public_api, &[], &[]));
        // Both should be review-here; high.ts ranks first.
        assert_eq!(map.review_here.len(), 2);
        assert!(map.review_here[0].score.total >= map.review_here[1].score.total);
        assert_eq!(map.review_here[0].file, "src/high.ts");
    }

    // done-condition (c): the symbol-level call chain (`fallow trace`) is
    // EXPLICITLY OFF the ranked path. The focus-map ranking inputs
    // (`FocusInputs`) carry NO trace / symbol-chain field, and the composite
    // `FocusScore.total` is the sum of EXACTLY the four documented components
    // (no symbol-chain term). This pins the trace as never feeding
    // de-prioritization.
    #[test]
    fn focus_map_inputs_have_no_symbol_chain_or_trace_field() {
        // FocusInputs is the complete input surface to the focus map. Naming
        // every field here is exhaustive (the struct is `pub` with no `..`), so
        // adding a trace/symbol-chain field would force this destructure to be
        // updated -- a compile-time guard that the trace stays out of the ranking
        // inputs.
        let empty_facts: &[FocusFileFactsPaths] = &[];
        let empty_boundary: &[BoundaryZoneFile] = &[];
        let empty_strings: &[String] = &[];
        let FocusInputs {
            graph_facts: _,
            boundary_files: _,
            public_api_added: _,
            coordination_changed_files: _,
            taint_touched_files: _,
            // `runtime` is the legitimate paid weighting seam, not a ranking
            // input the free tier reads. NOTE: no `symbol_chain` / `trace` field
            // exists. If the trace ever wired one in, this destructure would fail
            // to compile -- the guard that the trace stays OUT of the focus inputs.
            runtime: _,
        } = inputs(
            empty_facts,
            empty_boundary,
            empty_strings,
            empty_strings,
            empty_strings,
        );

        // The composite total is the sum of exactly the four documented
        // components. A symbol-chain term would break this invariant.
        let gf = vec![facts("src/x.ts", 4, 2, false, false)];
        let map = build_focus_map(&inputs(&gf, &[], &[], &[], &[]));
        let unit = map
            .review_here
            .iter()
            .chain(map.deprioritized.iter())
            .next()
            .unwrap();
        let score = &unit.score;
        assert_eq!(
            score.total,
            score.fan_io
                + score.security_taint
                + score.risk_zone
                + score.change_shape
                + score.runtime,
            "the focus total must be the documented components only -- no symbol-chain term"
        );
        // Free-mode (no runtime input): the runtime component is 0, so the total
        // is the four deterministic components, byte-identical to the no-runtime baseline.
        assert_eq!(score.runtime, 0, "free mode adds no runtime weight");
    }

    // With runtime data, a HOT unit outranks a COLD unit
    // that is otherwise identical. The hot path's bucketed runtime weight lifts it.
    #[test]
    fn hot_unit_outranks_cold_with_runtime_data() {
        let gf = vec![
            facts("src/hot.ts", 2, 0, false, false),
            facts("src/cold.ts", 2, 0, false, false),
        ];
        let rt = runtime(&[("src/hot.ts", 500)], &["src/cold.ts"]);
        let map = build_focus_map(&inputs_rt(&gf, &[], &[], &rt));
        let find = |file: &str| {
            map.review_here
                .iter()
                .chain(map.deprioritized.iter())
                .find(|unit| unit.file == file)
                .unwrap_or_else(|| panic!("{file} present"))
        };
        let hot = find("src/hot.ts");
        let cold = find("src/cold.ts");
        assert!(hot.score.runtime > 0, "hot unit carries a runtime weight");
        assert_eq!(cold.score.runtime, 0, "cold unit carries no runtime weight");
        assert!(
            hot.score.total > cold.score.total,
            "hot ({}) must outrank cold ({})",
            hot.score.total,
            cold.score.total
        );
        assert!(hot.reason.contains("hot path (500 invocations)"));
    }

    // A blazing path outweighs a merely-warm one (bands).
    #[test]
    fn runtime_weight_is_invocation_bucketed() {
        assert_eq!(runtime_weight(0), RUNTIME_HOT_FLOOR);
        assert_eq!(runtime_weight(50), RUNTIME_HOT_FLOOR);
        assert_eq!(runtime_weight(100), RUNTIME_HOT_WARM);
        assert_eq!(runtime_weight(1_000), RUNTIME_HOT_BLAZING);
    }

    // The `skip` label is emitted ONLY with runtime
    // evidence, and ONLY for a runtime-cold unit that carries no deterministic
    // signal. A cold unit WITH a risk signal stays visible (never skipped).
    #[test]
    fn safe_skip_only_with_runtime_evidence_and_zero_risk() {
        // Fully isolated + runtime-cold -> skip.
        let isolated = vec![facts("src/dead.ts", 0, 0, false, false)];
        let rt = runtime(&[], &["src/dead.ts"]);
        let map = build_focus_map(&inputs_rt(&isolated, &[], &[], &rt));
        let unit = map
            .review_here
            .iter()
            .chain(map.deprioritized.iter())
            .next()
            .expect("unit present");
        assert_eq!(unit.label, FocusLabel::Skip);
        assert_eq!(unit.label.token(), "skip");
        assert!(unit.reason.contains("runtime-cold"));
        // The skip unit is in the escape hatch (nothing hidden).
        assert!(map.deprioritized.iter().any(|u| u.file == "src/dead.ts"));

        // Same file but with a risk signal (public-API delta) -> NOT skipped, even
        // though runtime says cold. A deterministic signal keeps it visible.
        let public_api = vec!["src/dead.ts::Widget".to_string()];
        let with_risk = build_focus_map(&inputs_rt(&isolated, &public_api, &[], &rt));
        let risky = with_risk
            .review_here
            .iter()
            .chain(with_risk.deprioritized.iter())
            .next()
            .expect("unit present");
        assert_ne!(
            risky.label,
            FocusLabel::Skip,
            "a risk signal blocks safe-skip"
        );
    }

    // Safety: a confidence-flagged unit (dynamic dispatch / re-export
    // indirection) is NOT auto-skipped even when the runtime proves it cold and it
    // carries no other signal -- its reachability is uncertain, so one runtime
    // capture is not proof it is safe to skip.
    #[test]
    fn confidence_flag_blocks_safe_skip_even_when_runtime_cold() {
        let dyn_cold = vec![facts("src/dyn.ts", 0, 0, true, false)];
        let rt = runtime(&[], &["src/dyn.ts"]);
        let map = build_focus_map(&inputs_rt(&dyn_cold, &[], &[], &rt));
        let unit = map
            .review_here
            .iter()
            .chain(map.deprioritized.iter())
            .next()
            .expect("unit present");
        assert_ne!(
            unit.label,
            FocusLabel::Skip,
            "dynamic-dispatch reachability uncertainty blocks safe-skip"
        );
        assert!(unit.confidence.contains(&ConfidenceFlag::DynamicDispatch));
    }

    // With NO runtime input, the map is byte-identical to
    // the no-runtime baseline -- no skip label, no runtime component, same JSON.
    #[test]
    fn no_runtime_data_is_byte_identical_to_e7() {
        let gf = vec![
            facts("src/a.ts", 12, 3, false, false),
            facts("src/b.ts", 0, 0, false, false),
            facts("src/c.ts", 2, 0, false, true),
        ];
        let public_api = vec!["src/c.ts::Thing".to_string()];
        let e7 = build_focus_map(&inputs(&gf, &[], &public_api, &[], &[]));
        let json = serde_json::to_string_pretty(&e7).expect("serialize");
        assert!(!json.contains("\"skip\""), "free mode emits no skip label");
        assert!(
            !json.contains("\"runtime\""),
            "free mode omits the runtime component from the wire"
        );
        for unit in e7.review_here.iter().chain(e7.deprioritized.iter()) {
            assert_eq!(unit.score.runtime, 0);
            assert_ne!(unit.label, FocusLabel::Skip);
        }
    }

    // Runtime weighting + safe-skip is deterministic (byte-identical re-runs).
    #[test]
    fn runtime_focus_map_is_byte_identical_across_runs() {
        let gf = vec![
            facts("src/hot.ts", 4, 2, false, false),
            facts("src/cold.ts", 0, 0, false, false),
            facts("src/warm.ts", 1, 0, false, false),
        ];
        let rt = runtime(
            &[("src/hot.ts", 2_000), ("src/warm.ts", 120)],
            &["src/cold.ts"],
        );
        let first = build_focus_map(&inputs_rt(&gf, &[], &[], &rt));
        let second = build_focus_map(&inputs_rt(&gf, &[], &[], &rt));
        assert_eq!(
            serde_json::to_string_pretty(&first).unwrap(),
            serde_json::to_string_pretty(&second).unwrap()
        );
    }
}
