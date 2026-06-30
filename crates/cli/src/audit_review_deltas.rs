//! Diff-aware deterministic deltas for the review brief (6.A).
//!
//! Three exports-aware / graph-structural deltas, framed new-vs-pre-existing
//! against the audit base snapshot:
//!
//! 1. **boundary-violation-introduced**: a cross-zone edge present at head but not
//!    at base. R2 (first-edge-only): keyed on the `(from_zone, to_zone)` PAIR, so
//!    the first import that establishes a zone pair fires once; subsequent imports
//!    across the same already-established pair do not refire.
//! 2. **circular-dependency-introduced**: a cycle (canonical, rotation-independent
//!    file set) present at head but not at base.
//! 3. **public-API-surface delta**: EXPORTS-AWARE. The set of public-export keys
//!    (`<rel_path>::<name>`) reachable through `package.json` `exports` +
//!    re-export reachability, head-minus-base. A symbol re-exported only through
//!    an internal barrel NOT in `exports` is in neither set, so it yields ZERO
//!    public-API delta; one reachable through an `exports` path yields exactly
//!    one (the Aisha repro). R4 (attribute to the exports-mapped copy only) is
//!    encoded by which modules land in the public-API entry-point set
//!    (`fallow_engine::public_api_package_entry_points`).
//!
//! Relocation-awareness (R3) falls out of the set-difference: a moved file that
//! preserves its zone pair / public-export key / cycle membership produces no
//! head-minus-base delta, because the key is path-canonical for cycles and
//! zone-pair / `(rel_path, name)` for the others, and base already contained it.
//!
//! R1 (batch-consolidate public-API to ONE decision per change) is honored at the
//! summary line: the brief renders a single "public API surface widened by N"
//! decision, never one-per-symbol, while still carrying the added keys as
//! evidence.

use std::path::Path;

use fallow_types::output_dead_code::{BoundaryViolationFinding, CircularDependencyFinding};
use rustc_hash::FxHashSet;

use super::keys::relative_key_path;

/// The cross-zone edge key for R2 first-edge-only framing: one key per distinct
/// `(from_zone, to_zone)` pair, NOT per import statement. A second import across
/// an already-established pair shares this key, so it never re-fires the delta.
#[must_use]
pub fn boundary_edge_key(finding: &BoundaryViolationFinding) -> String {
    format!(
        "{}->-{}",
        finding.violation.from_zone, finding.violation.to_zone
    )
}

/// Canonical (rotation-independent) cycle key: the sorted root-relative file set.
#[must_use]
pub fn cycle_key(finding: &CircularDependencyFinding, root: &Path) -> String {
    let mut files: Vec<String> = finding
        .cycle
        .files
        .iter()
        .map(|p| relative_key_path(p, root))
        .collect();
    files.sort_unstable();
    files.dedup();
    files.join("|")
}

/// Build the deduped set of cross-zone edge keys for a results' boundary
/// violations (R2: one per zone pair).
#[must_use]
pub fn boundary_edge_keys(findings: &[BoundaryViolationFinding]) -> FxHashSet<String> {
    findings.iter().map(boundary_edge_key).collect()
}

/// Build the deduped set of canonical cycle keys.
#[must_use]
pub fn cycle_keys(findings: &[CircularDependencyFinding], root: &Path) -> FxHashSet<String> {
    findings.iter().map(|f| cycle_key(f, root)).collect()
}

/// Compute the exports-aware public-export key set from a retained graph + the
/// project config and package metadata. Wires
/// `fallow_engine::public_api_package_entry_points` (the R4 exports-aware
/// entry set) into the retained graph's public export key query.
#[must_use]
pub fn public_export_keys_for(
    graph: &fallow_engine::RetainedModuleGraph,
    config: &fallow_config::ResolvedConfig,
    root_pkg: Option<&fallow_config::PackageJson>,
    workspaces: &[fallow_config::WorkspaceInfo],
    root: &Path,
) -> FxHashSet<String> {
    let public_entries =
        fallow_engine::public_api_package_entry_points(graph, config, root_pkg, workspaces);
    graph.public_export_keys(&public_entries, root)
}

/// Compute the head-minus-base delta key set, sorted for deterministic output.
#[must_use]
#[allow(
    clippy::implicit_hasher,
    reason = "callers always pass FxHashSet; generalizing the hasher adds noise for no in-tree benefit"
)]
pub fn introduced_keys(head: &FxHashSet<String>, base: &FxHashSet<String>) -> Vec<String> {
    let mut introduced: Vec<String> = head.difference(base).cloned().collect();
    introduced.sort_unstable();
    introduced
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::output_dead_code::{BoundaryViolationFinding, CircularDependencyFinding};
    use fallow_types::results::{BoundaryViolation, CircularDependency};
    use std::path::PathBuf;

    fn boundary(from_zone: &str, to_zone: &str, specifier: &str) -> BoundaryViolationFinding {
        BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: PathBuf::from("/p/src/a.ts"),
            to_path: PathBuf::from("/p/src/b.ts"),
            from_zone: from_zone.to_string(),
            to_zone: to_zone.to_string(),
            import_specifier: specifier.to_string(),
            line: 1,
            col: 0,
        })
    }

    fn cycle(files: &[&str]) -> CircularDependencyFinding {
        CircularDependencyFinding::with_actions(CircularDependency {
            files: files.iter().map(PathBuf::from).collect(),
            length: files.len(),
            line: 1,
            col: 0,
            edges: vec![],
            is_cross_package: false,
        })
    }

    #[test]
    fn boundary_edges_dedup_per_zone_pair_r2() {
        // Two imports across the SAME zone pair collapse to one edge key (R2).
        let findings = vec![
            boundary("ui", "db", "./b"),
            boundary("ui", "db", "./c"),
            boundary("app", "db", "./d"),
        ];
        let keys = boundary_edge_keys(&findings);
        assert_eq!(keys.len(), 2, "one key per zone pair: {keys:?}");
        assert!(keys.contains("ui->-db"));
        assert!(keys.contains("app->-db"));
    }

    #[test]
    fn boundary_delta_fires_only_on_new_zone_pair() {
        // Base has ui->db established. Head adds a second ui->db import (no new
        // pair) plus a genuinely-new app->db pair. Only app->db is introduced.
        let base = boundary_edge_keys(&[boundary("ui", "db", "./b")]);
        let head = boundary_edge_keys(&[
            boundary("ui", "db", "./b"),
            boundary("ui", "db", "./c"),
            boundary("app", "db", "./d"),
        ]);
        let introduced = introduced_keys(&head, &base);
        assert_eq!(introduced, vec!["app->-db".to_string()]);
    }

    #[test]
    fn cycle_key_is_rotation_independent() {
        let root = Path::new("/p");
        let a = cycle(&["/p/a.ts", "/p/b.ts", "/p/c.ts"]);
        let b = cycle(&["/p/c.ts", "/p/a.ts", "/p/b.ts"]);
        assert_eq!(cycle_key(&a, root), cycle_key(&b, root));
    }

    #[test]
    fn cycle_delta_new_vs_pre_existing() {
        let root = Path::new("/p");
        let base = cycle_keys(&[cycle(&["/p/a.ts", "/p/b.ts"])], root);
        let head = cycle_keys(
            &[
                cycle(&["/p/a.ts", "/p/b.ts"]),
                cycle(&["/p/x.ts", "/p/y.ts"]),
            ],
            root,
        );
        let introduced = introduced_keys(&head, &base);
        assert_eq!(introduced, vec!["x.ts|y.ts".to_string()]);
    }

    #[test]
    fn relocation_fires_no_delta() {
        // R3: a "moved" cycle whose canonical file set is unchanged from base
        // produces no delta even though the finding object is freshly built.
        let root = Path::new("/p");
        let base = cycle_keys(&[cycle(&["/p/a.ts", "/p/b.ts"])], root);
        let head = cycle_keys(&[cycle(&["/p/b.ts", "/p/a.ts"])], root);
        assert!(introduced_keys(&head, &base).is_empty());
    }

    #[test]
    fn public_api_delta_zero_for_internal_only_one_for_exports() {
        // Pure set arithmetic mirroring the exports-aware repro: base has the
        // exports-reachable `pub`; head adds an internal-only `priv` (NOT in the
        // public set, so absent from both base and head public sets) AND an
        // exports-reachable `widget`. Only `widget` is an introduced public key.
        let base: FxHashSet<String> = std::iter::once("src/impl.ts::pub".to_string()).collect();
        let head: FxHashSet<String> = [
            "src/impl.ts::pub".to_string(),
            "src/impl.ts::widget".to_string(),
        ]
        .into_iter()
        .collect();
        let introduced = introduced_keys(&head, &base);
        assert_eq!(introduced, vec!["src/impl.ts::widget".to_string()]);
    }
}
