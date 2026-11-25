//! Check the file paths that the type-aware companion returns.
//!
//! The companion reports each path relative to the analysis root. The
//! companion can read files above the root: the files of the read scope base
//! (the Git work tree of the root), also a hoisted
//! `../../node_modules/@angular/core/index.d.ts` of that work tree. The
//! companion also reads the library files of its compiler package, which can
//! be installed outside the project (`npx`, `npm -g`). A returned path, with
//! its `..` segments applied to the root, must stay inside one of those
//! directories. The caller drops each entry with a path that does not.

use std::path::{Component, Path, PathBuf};

/// The directories that a returned path may name for one analysis root.
pub(super) struct ResponseScope {
    root: PathBuf,
    allowed: Vec<PathBuf>,
}

impl ResponseScope {
    /// The scope of `root`: its read scope base and the compiler package
    /// directories of the companion.
    pub(super) fn new(root: &Path) -> Self {
        Self::with_compiler_dirs(
            root,
            crate::type_aware::transport::companion_compiler_dirs(root),
        )
    }

    /// The scope of `root` with the given compiler package directories.
    pub(super) fn with_compiler_dirs(root: &Path, compiler_dirs: Vec<PathBuf>) -> Self {
        let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let mut allowed = vec![fallow_config::project_scope_base(&root)];
        allowed.extend(compiler_dirs);
        Self { root, allowed }
    }

    /// True when the root-relative `path`, with each `..` and `.` applied to
    /// the root by text, stays inside an allowed directory. An absolute path
    /// is never allowed, because the protocol names root-relative paths.
    pub(super) fn allows(&self, path: &Path) -> bool {
        let mut joined = self.root.clone();
        for component in path.components() {
            match component {
                Component::Normal(part) => joined.push(part),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !joined.pop() {
                        return false;
                    }
                }
                Component::RootDir | Component::Prefix(_) => return false,
            }
        }
        self.allowed.iter().any(|dir| joined.starts_with(dir))
    }
}

/// True when each contract declaration of `data` stays inside `scope`.
pub(super) fn contract_paths_in_scope(data: &super::SymbolUseData, scope: &ResponseScope) -> bool {
    data.contract_relations
        .iter()
        .map(|contract| &contract.declaration.path)
        .chain(
            data.framework_contract_relations
                .iter()
                .map(|contract| &contract.declaration.path),
        )
        .all(|path| scope.allows(path))
}

/// True when the path and each alias hop of `evidence` stay inside `scope`.
pub(super) fn reference_paths_in_scope(
    evidence: &super::SemanticReferenceData,
    scope: &ResponseScope,
) -> bool {
    scope.allows(&evidence.path) && evidence.via.iter().all(|hop| scope.allows(&hop.path))
}

/// The impact paths whose path and provenance stay inside `scope`. The
/// other paths are dropped.
pub(super) fn impact_paths(
    paths: Vec<super::ImpactPathData>,
    relation: &str,
    scope: &ResponseScope,
) -> Vec<super::SemanticImpactPath> {
    paths
        .into_iter()
        .filter(|path| {
            scope.allows(&path.path) && path.provenance.iter().all(|hop| scope.allows(hop))
        })
        .map(|path| super::SemanticImpactPath {
            distance: path.provenance.len().max(1),
            via: path.provenance,
            path: path.path,
            relation: relation.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::tests::symbol;
    use super::super::*;
    use super::*;
    use serde_json::json;

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_package_root_accepts_paths_into_the_work_tree_and_hoisted_node_modules() {
        let dir = tempfile::tempdir().expect("temp dir");
        let outer = dunce::canonicalize(dir.path()).expect("canonical");
        let top = outer.join("repo");
        let root = top.join("packages/app");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::create_dir(top.join(".git")).expect("mkdir");
        std::fs::create_dir_all(top.join("node_modules/@angular/core")).expect("mkdir");
        let scope = ResponseScope::new(&root);

        for path in [
            "src/a.ts",
            "./src/a.ts",
            "src/../lib/a.ts",
            "../lib/src/a.ts",
            "../../node_modules/@angular/core/index.d.ts",
        ] {
            assert!(scope.allows(Path::new(path)), "{path}");
        }
        for path in ["../../../outside.ts", "src/../../../../a.ts", "/abs/a.ts"] {
            assert!(!scope.allows(Path::new(path)), "{path}");
        }
    }

    /// A companion installed outside the project returns the library files
    /// of its compiler package with a path above the root.
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn library_files_of_a_compiler_outside_the_root_are_accepted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let outer = dunce::canonicalize(dir.path()).expect("canonical");
        let root = outer.join("project");
        std::fs::create_dir_all(&root).expect("mkdir");
        let modules = outer.join("global/lib/node_modules");
        let companion = modules.join("fallow-type-aware/bin/fallow-type-aware");
        std::fs::create_dir_all(companion.parent().expect("parent")).expect("mkdir");
        std::fs::write(&companion, "").expect("write");
        std::fs::create_dir_all(modules.join("typescript")).expect("mkdir");
        std::fs::write(modules.join("typescript/package.json"), "{}").expect("write");
        std::fs::create_dir_all(modules.join("@typescript/typescript-darwin-arm64/lib"))
            .expect("mkdir");
        let dirs = crate::type_aware::transport::compiler_dirs_of(&companion, "typescript");
        assert_eq!(dirs, vec![modules.join("typescript"), modules.clone()]);

        let lib = "../global/lib/node_modules/@typescript/typescript-darwin-arm64/lib/lib.es2015.collection.d.ts";
        assert!(!ResponseScope::with_compiler_dirs(&root, Vec::new()).allows(Path::new(lib)));
        assert!(ResponseScope::with_compiler_dirs(&root, dirs).allows(Path::new(lib)));
        assert!(
            !ResponseScope::with_compiler_dirs(&root, vec![modules.join("typescript")])
                .allows(Path::new("../global/lib/other.ts"))
        );
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_root_without_a_work_tree_accepts_only_its_own_paths() {
        let dir = tempfile::tempdir().expect("temp dir");
        let outer = dunce::canonicalize(dir.path()).expect("canonical");
        let root = outer.join("app");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::create_dir_all(outer.join("node_modules/pkg")).expect("mkdir");
        let scope = ResponseScope::new(&root);

        assert!(scope.allows(Path::new("src/a.ts")));
        assert!(!scope.allows(Path::new("../node_modules/pkg/index.d.ts")));
        assert!(!scope.allows(Path::new("../shared/consumer.ts")));
    }

    /// A package root below a work tree with hoisted `node_modules` gets
    /// framework contract and reference paths above the root. The decode
    /// keeps such a path, and drops only the entry with a path outside the
    /// work tree.
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn hoisted_node_modules_paths_decode_for_a_package_root() {
        let dir = tempfile::tempdir().expect("temp dir");
        let top = dunce::canonicalize(dir.path())
            .expect("canonical")
            .join("repo");
        let root = top.join("packages/app");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::create_dir(top.join(".git")).expect("mkdir");
        std::fs::create_dir_all(top.join("node_modules/@angular/core")).expect("mkdir");
        let requested = SemanticFrameworkContract {
            framework: "angular".to_string(),
            package: "@angular/core".to_string(),
            heritage_symbol: "OnInit".to_string(),
            heritage_names: vec!["OnInit".to_string()],
            relation: SemanticFrameworkRelation::Implements,
            members: vec!["ngOnInit".to_string()],
        };
        let query = SemanticQuery::SymbolUse {
            id: 0,
            symbol: symbol(),
            framework_contracts: vec![requested],
        };
        let mut declaration = symbol();
        declaration.path = PathBuf::from("../../node_modules/@angular/core/index.d.ts");
        declaration.exported_name = "OnInit".to_string();
        declaration.local_name = "OnInit".to_string();
        let reference = |path: &str| {
            json!({
                "path": path,
                "line": 1,
                "col": 0,
                "role": "import",
                "source": "checker",
                "namespace": "value",
                "via": []
            })
        };
        let result = SemanticQueryResponse {
            query_id: 0,
            operation: SemanticOperation::SymbolUse,
            assertion: "confirmed-used".to_string(),
            status: SemanticCompleteness::Complete,
            reason_code: None,
            actions: Vec::new(),
            evidence: vec![
                reference("../../node_modules/@angular/core/use.d.ts"),
                reference("../../../outside.ts"),
            ],
            total_evidence_count: 2,
            truncated: false,
            omissions: Vec::new(),
            data: json!({
                "symbol": symbol(),
                "selected_project": "tsconfig.json",
                "owning_projects": ["tsconfig.json"],
                "total_reference_count": 2,
                "contract_relations": [],
                "framework_contract_relations": [{
                    "framework": "angular",
                    "package": "@angular/core",
                    "relation": "implements",
                    "declaration": declaration
                }],
                "closed_world_eligible": false,
                "edit_guard": { "start": 0, "end": 1, "declaration_sha256": "unused" }
            }),
        };
        let scope = ResponseScope::new(&root);

        let decision = decode_candidate_decision(
            &root,
            &scope,
            &query,
            &result,
            false,
            &mut FxHashMap::default(),
        )
        .expect("hoisted paths decode");

        assert!(decision.framework_contract.is_some());
        assert_eq!(decision.evidence.len(), 1);
        assert_eq!(
            decision.evidence[0].path,
            PathBuf::from("../../node_modules/@angular/core/use.d.ts")
        );

        let mut outside = result;
        outside.data["framework_contract_relations"][0]["declaration"]["path"] =
            json!("../../../node_modules/@angular/core/index.d.ts");
        let decision = decode_candidate_decision(
            &root,
            &scope,
            &query,
            &outside,
            false,
            &mut FxHashMap::default(),
        )
        .expect("an outside contract path keeps the decode");
        assert_eq!(
            decision.decision,
            SemanticCandidateDecisionKind::RetainedUnresolved
        );
    }

    #[test]
    fn partial_symbol_impact_preserves_valid_consumers_and_targeted_tests() {
        let target = symbol();
        let scope = ResponseScope::new(Path::new("."));
        let request = SemanticRequest {
            protocol_version: PROTOCOL_VERSION,
            operation: OPERATION,
            root: ".".to_string(),
            projects: vec!["tsconfig.json".to_string()],
            evidence_limit: EVIDENCE_LIMIT,
            queries: vec![SemanticQuery::SymbolImpact {
                id: 0,
                symbol: target.clone(),
            }],
        };
        let result = SemanticQueryResponse {
            query_id: 0,
            operation: SemanticOperation::SymbolImpact,
            assertion: "consumers-found".to_string(),
            status: SemanticCompleteness::Partial,
            reason_code: Some(SemanticGapReason::DynamicBehavior),
            actions: vec!["Review dynamic consumers.".to_string()],
            evidence: Vec::new(),
            total_evidence_count: 2,
            truncated: false,
            omissions: vec![SemanticOmission {
                reason_code: SemanticGapReason::DynamicBehavior,
                count: 1,
            }],
            data: json!({
                "symbol": target,
                "selected_project": "tsconfig.json",
                "direct_consumers": [{
                    "path": "src/consumer.ts",
                    "namespace": "value"
                }],
                "total_direct_consumer_count": 1,
                "transitive_affected_files": [{
                    "path": "src/transitive.ts",
                    "provenance": ["src/consumer.ts", "src/transitive.ts"]
                }],
                "total_transitive_affected_file_count": 1,
                "targeted_tests": [{
                    "path": "test/config.test.ts",
                    "provenance": ["src/consumer.ts", "test/config.test.ts"]
                }],
                "total_targeted_test_count": 1,
                "confidence": "bounded"
            }),
        };
        let response = SemanticResponse {
            protocol_version: PROTOCOL_VERSION,
            operation: OPERATION.to_string(),
            sidecar_version: env!("CARGO_PKG_VERSION").to_string(),
            backend: BACKEND_FAMILY.to_string(),
            backend_version: BACKEND_VERSION.to_string(),
            selected_tsconfigs: vec!["tsconfig.json".to_string()],
            projects: Vec::new(),
            results: vec![result.clone()],
            phase_timings_ms: SemanticPhaseTimings {
                project_setup: 1,
                diagnostics: 1,
                semantic_queries: 1,
            },
            warnings: Vec::new(),
            elapsed_ms: 3,
        };

        let impact = decode_symbol_impact(&response, &request, &result, symbol(), &scope)
            .expect("valid impact");

        assert_eq!(impact.status, SemanticCompleteness::Partial);
        assert_eq!(impact.total_direct_consumer_count, 1);
        assert_eq!(
            impact.direct_consumers[0].path,
            PathBuf::from("src/consumer.ts")
        );
        assert_eq!(impact.total_affected_file_count, 1);
        assert_eq!(impact.total_targeted_test_count, 1);
        assert_eq!(
            impact.targeted_tests[0].path,
            PathBuf::from("test/config.test.ts")
        );
        assert_eq!(impact.confidence, SemanticImpactConfidence::Bounded);

        let mut invalid_confidence = result.clone();
        invalid_confidence.data["confidence"] = json!("certain");
        assert!(
            decode_symbol_impact(&response, &request, &invalid_confidence, symbol(), &scope)
                .is_err()
        );

        let mut invalid_path = result;
        invalid_path.data["direct_consumers"][0]["path"] = json!("/outside.ts");
        let impact = decode_symbol_impact(&response, &request, &invalid_path, symbol(), &scope)
            .expect("an outside path drops only its own entry");
        assert!(impact.direct_consumers.is_empty());
        assert_eq!(impact.targeted_tests.len(), 1);
    }
}
