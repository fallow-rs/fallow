//! `unused-component-prop` (React arm): a React component prop destructured in
//! the signature but read NOWHERE in the component body. Reuses the SAME finding
//! kind and rule key as the Vue arm. Asserts the genuine true positive fires and
//! every abstain case (exported public contract, forwardRef imported interface,
//! rest-spread, nested destructure) produces NO finding (zero-FP doctrine).

use super::common::{create_config, fixture_path};

#[test]
fn flags_unused_react_prop_and_abstains_on_every_ladder_case() {
    let root = fixture_path("unused-react-prop");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let flagged: Vec<(&str, &str)> = results
        .unused_component_props
        .iter()
        .map(|p| (p.prop.component_name.as_str(), p.prop.prop_name.as_str()))
        .collect();

    // (a) The genuine true positive: a non-exported component's inline prop read
    // nowhere in its body is flagged, attributed to the right component.
    assert!(
        flagged.contains(&("LocalInner", "deadProp")),
        "a genuinely-unused React prop should be flagged: {flagged:?}"
    );

    // (a) A prop READ in the component body is credited (not flagged).
    assert!(
        !flagged.iter().any(|(_, prop)| *prop == "kept"),
        "a prop read in the body must not be flagged: {flagged:?}"
    );

    // (b) Public-API abstain: a prop on an EXPORTED component's contract never
    // flags even when read nowhere in that component.
    assert!(
        !flagged.iter().any(|(_, prop)| *prop == "label"),
        "an exported component's contract prop must abstain: {flagged:?}"
    );

    // (b) forwardRef + imported-interface abstain: the bare `props` signature
    // (props from an unresolvable imported interface) abstains the component.
    assert!(
        !flagged.iter().any(|(_, prop)| *prop == "unread"),
        "a forwardRef imported-interface component must abstain: {flagged:?}"
    );

    // (b) rest-spread abstain: `{ a, ...rest }` makes the prop set incomplete.
    assert!(
        !flagged.iter().any(|(_, prop)| *prop == "deadInSpread"),
        "a rest-spread destructure must abstain the component: {flagged:?}"
    );

    // (b) nested-destructure abstain: `{ user: { name } }` cannot be flattened.
    assert!(
        !flagged.iter().any(|(_, prop)| *prop == "dead"),
        "a nested destructure must abstain the component: {flagged:?}"
    );

    // (a) JSX-read usage credit: a prop read only inside `{...}` JSX is credited.
    assert!(
        !flagged.iter().any(|(_, prop)| *prop == "shownInner"),
        "a prop read in a JSX expression must not be flagged: {flagged:?}"
    );

    // The ONLY finding is the single true positive (zero false positives).
    assert_eq!(
        flagged.len(),
        1,
        "exactly one React prop finding expected (zero FP): {flagged:?}"
    );
}

/// Suppress-token round-trip: `// fallow-ignore-next-line unused-component-prop`
/// above a React prop drops the finding AND is not reported stale (the
/// framework detector consumes the suppression before stale detection runs).
#[test]
fn inline_suppression_drops_react_prop_and_is_not_stale() {
    let root = fixture_path("unused-react-prop-suppress");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_component_props.is_empty(),
        "the inline suppression must drop the React prop finding: {:?}",
        results
            .unused_component_props
            .iter()
            .map(|p| (p.prop.component_name.as_str(), p.prop.prop_name.as_str()))
            .collect::<Vec<_>>()
    );

    let stale: Vec<&str> = results
        .stale_suppressions
        .iter()
        .filter_map(|s| match &s.origin {
            fallow_types::results::SuppressionOrigin::Comment { issue_kind, .. } => {
                issue_kind.as_deref()
            }
            fallow_types::results::SuppressionOrigin::JsdocTag { .. } => None,
        })
        .collect();
    assert!(
        !stale.contains(&"unused-component-prop"),
        "a consumed unused-component-prop suppression must not be reported stale: {stale:?}"
    );
}

/// The dep gate: a project without a React/Preact dependency emits no React prop
/// findings even if a `.tsx` file destructures an unused prop.
#[test]
fn react_arm_is_dep_gated() {
    // The Vue fixture declares only `vue` (no react/react-dom/next/preact), so the
    // React producer returns empty there. Re-run the Vue fixture and assert no
    // React-shaped (multi-component) finding leaks in: all findings are Vue SFC
    // component-name = file-stem shaped.
    let root = fixture_path("unused-component-prop");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    // Every finding's path is a `.vue` file (the React arm only emits `.jsx`/`.tsx`).
    for finding in &results.unused_component_props {
        let ext = finding
            .prop
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        assert_eq!(
            ext, "vue",
            "the React arm must not fire on a vue-only project: {:?}",
            finding.prop.path
        );
    }
}
