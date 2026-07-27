//! Construction of bounded semantic requests from syntactic candidates.

use super::*;

pub(super) fn build_dead_code_queries(
    root: &Path,
    results: &AnalysisResults,
    entry_points: &[PathBuf],
    include_symbol_use: bool,
    include_private_type_leaks: bool,
    include_type_coupling: bool,
) -> Result<DeadCodeQueryBatch, TypeAwareError> {
    let mut queries = Vec::new();
    let mut targets = BTreeMap::new();
    let graph_query_count = usize::from(include_private_type_leaks && !entry_points.is_empty())
        + usize::from(include_type_coupling);
    let unrequested_symbol_count = if include_symbol_use {
        append_symbol_use_queries(
            root,
            results,
            MAX_SEMANTIC_QUERIES.saturating_sub(graph_query_count),
            &mut queries,
            &mut targets,
        )?
    } else {
        0
    };
    if include_private_type_leaks && !entry_points.is_empty() {
        let id = queries.len();
        let unrequested_candidate_count = results
            .private_type_leaks
            .len()
            .saturating_sub(MAX_PRIVATE_LEAK_CANDIDATES);
        queries.push(SemanticQuery::ApiSurface {
            id,
            entry_points: entry_points
                .iter()
                .map(|path| protocol_path(root, path))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .collect(),
            include_cycles: false,
            private_leak_candidates: results
                .private_type_leaks
                .iter()
                .enumerate()
                .take(MAX_PRIVATE_LEAK_CANDIDATES)
                .map(|(candidate_id, finding)| {
                    Ok(PrivateLeakCandidate {
                        id: candidate_id,
                        path: protocol_path(root, &finding.leak.path)?
                            .to_string_lossy()
                            .replace('\\', "/"),
                        export_name: finding.leak.export_name.clone(),
                        type_name: finding.leak.type_name.clone(),
                    })
                })
                .collect::<Result<Vec<_>, TypeAwareError>>()?,
        });
        targets.insert(
            id,
            QueryTarget::ApiSurface {
                unrequested_candidate_count,
            },
        );
    }
    if include_type_coupling {
        let id = queries.len();
        queries.push(SemanticQuery::TypeCoupling {
            id,
            entry_points: entry_points
                .iter()
                .map(|path| protocol_path(root, path))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .collect(),
            include_cycles: true,
        });
        targets.insert(id, QueryTarget::TypeCoupling);
    }
    Ok(DeadCodeQueryBatch {
        queries,
        targets,
        capacity: LocalCapacity {
            unrequested_symbol_count,
        },
    })
}

fn append_symbol_use_queries(
    root: &Path,
    results: &AnalysisResults,
    capacity: usize,
    queries: &mut Vec<SemanticQuery>,
    targets: &mut BTreeMap<usize, QueryTarget>,
) -> Result<usize, TypeAwareError> {
    let class_count = results.unused_class_members.len().min(capacity);
    for (index, finding) in results
        .unused_class_members
        .iter()
        .take(class_count)
        .enumerate()
    {
        let id = queries.len();
        queries.push(SemanticQuery::SymbolUse {
            id,
            symbol: SemanticSymbol {
                path: protocol_path(root, &finding.member.path)?,
                namespace: SemanticNamespace::Value,
                declaration_kind: member_kind_name(finding.member.kind).to_string(),
                exported_name: finding.member.member_name.clone(),
                local_name: finding.member.member_name.clone(),
                owner: Some(finding.member.parent_name.clone()),
                line: finding.member.line,
                col: finding.member.col,
            },
            framework_contracts: results
                .semantic_framework_contracts
                .iter()
                .filter(|contract| contract.members.contains(&finding.member.member_name))
                .cloned()
                .collect(),
        });
        targets.insert(id, QueryTarget::ClassMember(index));
    }

    let remaining = capacity - class_count;
    let export_count = results.unused_exports.len().min(remaining);
    for (index, finding) in results.unused_exports.iter().take(export_count).enumerate() {
        let id = queries.len();
        queries.push(SemanticQuery::SymbolUse {
            id,
            symbol: export_symbol(root, &finding.export, SemanticNamespace::Value)?,
            framework_contracts: Vec::new(),
        });
        targets.insert(id, QueryTarget::UnusedExport(index));
    }

    let remaining = remaining - export_count;
    let type_count = results.unused_types.len().min(remaining);
    for (index, finding) in results.unused_types.iter().take(type_count).enumerate() {
        let id = queries.len();
        queries.push(SemanticQuery::SymbolUse {
            id,
            symbol: export_symbol(root, &finding.export, SemanticNamespace::Type)?,
            framework_contracts: Vec::new(),
        });
        targets.insert(id, QueryTarget::UnusedType(index));
    }

    Ok(
        results.unused_class_members.len() - class_count + results.unused_exports.len()
            - export_count
            + results.unused_types.len()
            - type_count,
    )
}

fn export_symbol(
    root: &Path,
    export: &fallow_types::results::UnusedExport,
    namespace: SemanticNamespace,
) -> Result<SemanticSymbol, TypeAwareError> {
    Ok(SemanticSymbol {
        path: protocol_path(root, &export.path)?,
        namespace,
        declaration_kind: "export".to_string(),
        exported_name: export.export_name.clone(),
        local_name: export.export_name.clone(),
        owner: None,
        line: export.line,
        col: export.col,
    })
}

const fn member_kind_name(kind: MemberKind) -> &'static str {
    match kind {
        MemberKind::ClassMethod => "class_method",
        MemberKind::ClassProperty => "class_property",
        MemberKind::EnumMember => "enum_member",
        MemberKind::NamespaceMember => "namespace_member",
        MemberKind::StoreMember => "store_member",
    }
}

pub(super) fn query_supports_guarded_fix(query: &SemanticQuery) -> bool {
    matches!(
        query,
        SemanticQuery::SymbolUse { symbol, .. } if symbol.declaration_kind == "class_method"
    )
}
