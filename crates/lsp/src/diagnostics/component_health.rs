//! The opt-in component health signals: prop drilling chains, thin wrappers
//! and duplicate prop shapes.
//!
//! Each rule defaults to `off`, and the analysis fills the result arrays only
//! when the rule is on, so these diagnostics show only in a project that
//! turned the signal on. They are `HINT` diagnostics: a signal suggests a
//! refactor, it is never a correctness error.

use std::path::Path;

use rustc_hash::FxHashMap;

use ls_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString, Uri,
};

use fallow_api::EditorAnalysisResults as AnalysisResults;

use super::doc_link_for_code;
use crate::position::{PositionMapper, line_range_from_byte_col};

/// Push the diagnostics of the three component health signals.
pub fn push_component_health_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    push_prop_drilling_diagnostics(map, results, mapper);
    push_thin_wrapper_diagnostics(map, results, mapper);
    push_duplicate_prop_shape_diagnostics(map, results, mapper);
}

struct SignalDiagnostic<'a> {
    code: &'static str,
    path: &'a Path,
    /// 1-based line of the component, as the results report it.
    line: u32,
    message: String,
    related: Vec<DiagnosticRelatedInformation>,
}

fn push_signal(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    mapper: &mut PositionMapper,
    signal: SignalDiagnostic<'_>,
) {
    let Some(uri) = Uri::from_file_path(signal.path) else {
        return;
    };
    let range = line_range_from_byte_col(mapper, signal.path, signal.line.saturating_sub(1), 0);
    map.entry(uri).or_default().push(Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::HINT),
        source: Some("fallow".to_string()),
        code: Some(NumberOrString::String(signal.code.to_string())),
        code_description: doc_link_for_code(signal.code),
        message: signal.message,
        related_information: (!signal.related.is_empty()).then_some(signal.related),
        ..Default::default()
    });
}

fn related_at(
    mapper: &mut PositionMapper,
    path: &Path,
    line: u32,
    message: String,
) -> Option<DiagnosticRelatedInformation> {
    let uri = Uri::from_file_path(path)?;
    let range = line_range_from_byte_col(mapper, path, line.saturating_sub(1), 0);
    Some(DiagnosticRelatedInformation {
        location: Location { uri, range },
        message,
    })
}

/// One diagnostic per chain, at the source hop that owns the prop. The other
/// hops are related information, in chain order.
fn push_prop_drilling_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.prop_drilling_chains {
        let chain = &finding.chain;
        let Some(source) = chain.hops.first() else {
            continue;
        };
        let trail = chain
            .hops
            .iter()
            .map(|hop| hop.component.as_str())
            .collect::<Vec<_>>()
            .join(" \u{2192} ");
        let last = chain.hops.len() - 1;
        let related = chain
            .hops
            .iter()
            .enumerate()
            .skip(1)
            .filter_map(|(index, hop)| {
                let role = if index == last {
                    "consumes"
                } else {
                    "forwards"
                };
                related_at(
                    mapper,
                    &hop.file,
                    hop.line,
                    format!("{} {role} {}", hop.component, chain.prop),
                )
            })
            .collect();
        push_signal(
            map,
            mapper,
            SignalDiagnostic {
                code: "prop-drilling",
                path: &source.file,
                line: source.line,
                message: format!(
                    "Prop '{}' is forwarded unused through {trail} (depth {}); colocate the \
                     consumer or lift it to a context at a mid-chain hop",
                    chain.prop, chain.depth
                ),
                related,
            },
        );
    }
}

fn push_thin_wrapper_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.thin_wrappers {
        let wrapper = &finding.wrapper;
        push_signal(
            map,
            mapper,
            SignalDiagnostic {
                code: "thin-wrapper",
                path: &wrapper.file,
                line: wrapper.line,
                message: format!(
                    "{} is a thin wrapper around {} (candidate for inlining at call sites or \
                     deleting)",
                    wrapper.component, wrapper.child_component
                ),
                related: Vec::new(),
            },
        );
    }
}

/// One diagnostic per component of a group. The other components of the
/// group are related information.
fn push_duplicate_prop_shape_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.duplicate_prop_shapes {
        let shape = &finding.shape;
        let related = shape
            .sharing_components
            .iter()
            .filter_map(|member| {
                related_at(
                    mapper,
                    &member.file,
                    member.line,
                    format!("{} has the same prop shape", member.component),
                )
            })
            .collect();
        push_signal(
            map,
            mapper,
            SignalDiagnostic {
                code: "duplicate-prop-shape",
                path: &shape.file,
                line: shape.line,
                message: format!(
                    "{} shares an identical prop shape {{{}}} with {} other component(s) \
                     (extract a shared Props type)",
                    shape.component,
                    shape.shape.join(", "),
                    shape.group_size.saturating_sub(1)
                ),
                related,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_api::editor_results::{
        DuplicatePropShape, DuplicatePropShapeFinding, DuplicatePropShapeMember, PropDrillHop,
        PropDrillingChain, PropDrillingChainFinding,
    };

    use super::*;

    fn root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\project")
        } else {
            PathBuf::from("/project")
        }
    }

    fn build(results: &AnalysisResults) -> FxHashMap<Uri, Vec<Diagnostic>> {
        let mut map = FxHashMap::default();
        push_component_health_diagnostics(&mut map, results, &mut PositionMapper::default());
        map
    }

    #[test]
    fn prop_drilling_sits_on_the_source_and_lists_the_other_hops() {
        let root = root();
        let hop = |file: &str, line: u32, component: &str| PropDrillHop {
            file: root.join(file),
            line,
            component: component.to_string(),
        };
        let results = AnalysisResults {
            prop_drilling_chains: vec![PropDrillingChainFinding::with_actions(PropDrillingChain {
                prop: "user".to_string(),
                depth: 3,
                hops: vec![
                    hop("App.tsx", 4, "App"),
                    hop("Page.tsx", 2, "Page"),
                    hop("Avatar.tsx", 7, "Avatar"),
                ],
            })],
            ..AnalysisResults::default()
        };

        let map = build(&results);

        let uri = Uri::from_file_path(root.join("App.tsx")).expect("uri");
        let diagnostic = &map[&uri][0];
        assert_eq!(diagnostic.range.start.line, 3);
        assert_eq!(
            diagnostic.message,
            "Prop 'user' is forwarded unused through App \u{2192} Page \u{2192} Avatar \
             (depth 3); colocate the consumer or lift it to a context at a mid-chain hop"
        );
        let related = diagnostic
            .related_information
            .as_ref()
            .expect("related hops")
            .iter()
            .map(|info| (info.location.range.start.line, info.message.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            related,
            vec![(1, "Page forwards user"), (6, "Avatar consumes user")]
        );
        assert_eq!(map.len(), 1, "one diagnostic per chain, at the source");
    }

    #[test]
    fn duplicate_prop_shape_lists_the_sibling_components() {
        let root = root();
        let results = AnalysisResults {
            duplicate_prop_shapes: vec![DuplicatePropShapeFinding::with_actions(
                DuplicatePropShape {
                    file: root.join("Card.tsx"),
                    line: 3,
                    component: "Card".to_string(),
                    shape: vec!["body".to_string(), "title".to_string()],
                    group_size: 2,
                    sharing_components: vec![DuplicatePropShapeMember {
                        file: root.join("Panel.tsx"),
                        line: 5,
                        component: "Panel".to_string(),
                    }],
                },
            )],
            ..AnalysisResults::default()
        };

        let map = build(&results);

        let uri = Uri::from_file_path(root.join("Card.tsx")).expect("uri");
        let diagnostic = &map[&uri][0];
        assert_eq!(
            diagnostic.message,
            "Card shares an identical prop shape {body, title} with 1 other component(s) \
             (extract a shared Props type)"
        );
        let related = diagnostic.related_information.as_ref().expect("siblings");
        assert_eq!(related[0].message, "Panel has the same prop shape");
        assert_eq!(related[0].location.range.start.line, 4);
    }
}
