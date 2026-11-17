//! Trace of the import path from an entry point to a file.

use super::*;

/// Trace the shortest import path from one module to another.
///
/// Exact paths take priority over abbreviated suffixes. Returns the unresolved
/// endpoint when either side names no module or an ambiguous suffix, so the
/// caller can name which half of the request was wrong.
///
/// # Errors
///
/// Returns the endpoint that did not resolve to exactly one module in the graph.
pub fn trace_import_path(
    graph: &ModuleGraph,
    root: &Path,
    from_path: &str,
    to_path: &str,
    eager_only: bool,
) -> Result<ImportPathTrace, ImportPathEndpoint> {
    let from_index = import_path_endpoint_index(
        graph,
        root,
        from_path,
        ImportPathEndpoint::From,
        ImportPathEndpoint::AmbiguousFrom,
    )?;
    let to_index = import_path_endpoint_index(
        graph,
        root,
        to_path,
        ImportPathEndpoint::To,
        ImportPathEndpoint::AmbiguousTo,
    )?;
    let from = &graph.modules[from_index];
    let to = &graph.modules[to_index];

    let from_rel = relativize(&from.path, root);
    let to_rel = relativize(&to.path, root);

    let route = if eager_only {
        graph.shortest_eager_import_path(from.file_id, to.file_id)
    } else {
        graph.shortest_import_path(from.file_id, to.file_id)
    };
    let Some(hops) = route else {
        let kind = if eager_only {
            "eager import path"
        } else {
            "import path"
        };
        return Ok(ImportPathTrace {
            schema_version: ImportPathTraceSchemaVersion::V1,
            reason: format!("no {kind} from {from_rel} to {to_rel}"),
            from: from_rel,
            to: to_rel,
            reachable: false,
            hops: 0,
            path: Vec::new(),
        });
    };

    if hops.is_empty() {
        return Ok(ImportPathTrace {
            schema_version: ImportPathTraceSchemaVersion::V1,
            reason: format!("{from_rel} is the same module as {to_rel}"),
            from: from_rel,
            to: to_rel,
            reachable: true,
            hops: 0,
            path: Vec::new(),
        });
    }

    let path = resolve_import_path_hops(graph, root, &hops);
    let hop_count = path.len();
    let plural = if hop_count == 1 { "hop" } else { "hops" };
    // A route whose every hop is type-only is erased at build time. Saying so is
    // the difference between "these modules ship coupled" and "the coupling only
    // exists for the type checker".
    let reason = if path.iter().all(|hop| hop.type_only) {
        format!(
            "{from_rel} reaches {to_rel} in {hop_count} type-only {plural}, erased at build time"
        )
    } else {
        format!("{from_rel} reaches {to_rel} in {hop_count} {plural}")
    };

    Ok(ImportPathTrace {
        schema_version: ImportPathTraceSchemaVersion::V1,
        from: from_rel,
        to: to_rel,
        reachable: true,
        hops: hop_count,
        path,
        reason,
    })
}
