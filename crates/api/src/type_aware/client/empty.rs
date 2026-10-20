//! Outcome for a type-aware run that has no semantic queries to send.

use fallow_types::envelope::TypeAwareMeta;
use fallow_types::semantic::{
    DEFERRED_PROJECT_CONFIG_HASH, SemanticAnalysisIdentity, SemanticAnalysisMode,
    SemanticCapability, SemanticCompleteness,
};

use super::super::manifest::{SEMANTIC_SCHEMA_VERSION, WIRE_PROTOCOL_VERSION as PROTOCOL_VERSION};
use super::super::transport::TypeAwareOutcome;
use super::SemanticDeadCodeOutcome;

pub(super) fn empty_semantic_outcome(
    mut capabilities: Vec<SemanticCapability>,
    backend_family: &str,
) -> SemanticDeadCodeOutcome {
    capabilities.sort_unstable();
    capabilities.dedup();
    let identity = SemanticAnalysisIdentity {
        mode: SemanticAnalysisMode::TypeAware,
        semantic_schema_version: SEMANTIC_SCHEMA_VERSION,
        capabilities,
        project_config_hash: DEFERRED_PROJECT_CONFIG_HASH.to_string(),
        backend_family: backend_family.to_string(),
        completeness: SemanticCompleteness::Complete,
    };
    SemanticDeadCodeOutcome {
        type_aware: TypeAwareOutcome {
            meta: TypeAwareMeta {
                identity: Some(identity),
                executed: false,
                protocol_version: PROTOCOL_VERSION,
                sidecar_version: None,
                backend: backend_family.to_string(),
                backend_version: None,
                ..TypeAwareMeta::default()
            },
            warnings: Vec::new(),
        },
        type_coupling: None,
    }
}
