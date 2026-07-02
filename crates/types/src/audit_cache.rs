//! Typed audit cache-key inputs.

use serde::Serialize;

use crate::source_fingerprint::SourceFingerprint;

/// Fingerprint of the resolved config that can affect audit output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditConfigFingerprint {
    /// Path of the config file that was loaded, or `None` when no config exists.
    pub path: Option<String>,
    /// Stable hash of the resolved config object.
    pub resolved_hash: Option<String>,
}

/// Fingerprint of an optional coverage input that can affect health findings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditCoverageFingerprint {
    /// User-provided coverage path.
    pub path: String,
    /// Actual file path hashed after directory resolution.
    pub resolved_path: String,
    /// Metadata freshness for the resolved coverage file, when it was readable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceFingerprint>,
    /// Stable content hash for the resolved coverage file, when it was readable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// File length in bytes, when it was readable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub len: Option<usize>,
    /// I/O error kind, when the resolved coverage file was not readable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Typed payload hashed to address an audit base-snapshot cache entry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AuditCacheKeyPayload {
    /// Audit base snapshot cache schema version.
    pub cache_version: u8,
    /// Fallow CLI version that produced the key.
    pub cli_version: String,
    /// Resolved git SHA for the base ref.
    pub base_sha: String,
    /// Config fingerprint.
    pub config_file: AuditConfigFingerprint,
    /// Changed files normalized to git-root-relative, forward-slash paths.
    pub changed_files: Vec<String>,
    /// Global production mode.
    pub production: bool,
    /// Dead-code-specific production override.
    pub production_dead_code: Option<bool>,
    /// Health-specific production override.
    pub production_health: Option<bool>,
    /// Duplication-specific production override.
    pub production_dupes: Option<bool>,
    /// Workspace filters.
    pub workspace: Option<Vec<String>>,
    /// Changed-workspaces base ref, when enabled.
    pub changed_workspaces: Option<String>,
    /// Grouping mode.
    pub group_by: Option<String>,
    /// Whether entry exports are analyzed.
    pub include_entry_exports: bool,
    /// CRAP threshold override.
    pub max_crap: Option<f64>,
    /// Coverage input fingerprint.
    pub coverage: Option<AuditCoverageFingerprint>,
    /// Coverage root override.
    pub coverage_root: Option<String>,
    /// Whether audit health computed styling keys for the base snapshot.
    pub css: bool,
    /// Whether audit health used deep CSS analysis for the base snapshot.
    pub css_deep: bool,
    /// Dead-code baseline path.
    pub dead_code_baseline: Option<String>,
    /// Health baseline path.
    pub health_baseline: Option<String>,
    /// Duplication baseline path.
    pub dupes_baseline: Option<String>,
}

/// Builder for audit base-snapshot cache keys.
#[derive(Debug, Clone)]
pub struct AuditCacheKeyBuilder {
    payload: AuditCacheKeyPayload,
}

impl AuditCacheKeyBuilder {
    /// Start a cache-key payload with the invariant identity fields.
    #[must_use]
    pub fn new(
        cache_version: u8,
        cli_version: impl Into<String>,
        base_sha: impl Into<String>,
        config_file: AuditConfigFingerprint,
        changed_files: Vec<String>,
    ) -> Self {
        Self {
            payload: AuditCacheKeyPayload {
                cache_version,
                cli_version: cli_version.into(),
                base_sha: base_sha.into(),
                config_file,
                changed_files,
                production: false,
                production_dead_code: None,
                production_health: None,
                production_dupes: None,
                workspace: None,
                changed_workspaces: None,
                group_by: None,
                include_entry_exports: false,
                max_crap: None,
                coverage: None,
                coverage_root: None,
                css: false,
                css_deep: false,
                dead_code_baseline: None,
                health_baseline: None,
                dupes_baseline: None,
            },
        }
    }

    /// Set production-mode options.
    #[must_use]
    pub const fn production(
        mut self,
        production: bool,
        dead_code: Option<bool>,
        health: Option<bool>,
        dupes: Option<bool>,
    ) -> Self {
        self.payload.production = production;
        self.payload.production_dead_code = dead_code;
        self.payload.production_health = health;
        self.payload.production_dupes = dupes;
        self
    }

    /// Set scope and grouping options.
    #[must_use]
    pub fn scope(
        mut self,
        workspace: Option<Vec<String>>,
        changed_workspaces: Option<String>,
        group_by: Option<String>,
        include_entry_exports: bool,
    ) -> Self {
        self.payload.workspace = workspace;
        self.payload.changed_workspaces = changed_workspaces;
        self.payload.group_by = group_by;
        self.payload.include_entry_exports = include_entry_exports;
        self
    }

    /// Set health and coverage options.
    #[must_use]
    pub fn health(
        mut self,
        max_crap: Option<f64>,
        coverage: Option<AuditCoverageFingerprint>,
        coverage_root: Option<String>,
    ) -> Self {
        self.payload.max_crap = max_crap;
        self.payload.coverage = coverage;
        self.payload.coverage_root = coverage_root;
        self
    }

    /// Set styling-analysis options that affect base health snapshot keys.
    #[must_use]
    pub const fn styling(mut self, css: bool, css_deep: bool) -> Self {
        self.payload.css = css;
        self.payload.css_deep = css_deep;
        self
    }

    /// Set baseline paths.
    #[must_use]
    pub fn baselines(
        mut self,
        dead_code: Option<String>,
        health: Option<String>,
        dupes: Option<String>,
    ) -> Self {
        self.payload.dead_code_baseline = dead_code;
        self.payload.health_baseline = health;
        self.payload.dupes_baseline = dupes;
        self
    }

    /// Borrow the completed payload.
    #[must_use]
    pub const fn payload(&self) -> &AuditCacheKeyPayload {
        &self.payload
    }

    /// Serialize the completed payload into stable JSON bytes for hashing.
    ///
    /// # Errors
    ///
    /// Returns a serde error when a payload field cannot be serialized.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&self.payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AuditConfigFingerprint {
        AuditConfigFingerprint {
            path: Some("fallow.toml".to_string()),
            resolved_hash: Some("abc".to_string()),
        }
    }

    #[test]
    fn audit_cache_key_builder_preserves_typed_fields() {
        let coverage = AuditCoverageFingerprint {
            path: "coverage".to_string(),
            resolved_path: "coverage/coverage-final.json".to_string(),
            source: Some(SourceFingerprint::new(12, 34)),
            content_hash: Some("hash".to_string()),
            len: Some(34),
            error: None,
        };

        let builder =
            AuditCacheKeyBuilder::new(3, "1.2.3", "abc123", config(), vec!["src/a.ts".to_string()])
                .production(true, Some(false), Some(true), None)
                .scope(
                    Some(vec!["web".to_string()]),
                    Some("main".to_string()),
                    Some("Package".to_string()),
                    true,
                )
                .health(Some(42.0), Some(coverage), Some("/workspace".to_string()))
                .styling(true, true)
                .baselines(
                    Some("dead.json".to_string()),
                    Some("health.json".to_string()),
                    Some("dupes.json".to_string()),
                );

        let payload = builder.payload();
        assert_eq!(payload.cache_version, 3);
        assert_eq!(payload.base_sha, "abc123");
        assert_eq!(payload.workspace.as_deref(), Some(&["web".to_string()][..]));
        assert!(payload.include_entry_exports);
        assert!(payload.css);
        assert!(payload.css_deep);
        assert_eq!(
            payload.coverage.as_ref().and_then(|c| c.source),
            Some(SourceFingerprint::new(12, 34))
        );
    }

    #[test]
    fn audit_cache_key_bytes_reflect_changed_file_order() {
        let first = AuditCacheKeyBuilder::new(
            1,
            "1.0.0",
            "base",
            config(),
            vec!["src/a.ts".to_string(), "src/b.ts".to_string()],
        )
        .to_json_bytes()
        .expect("payload should serialize");
        let second = AuditCacheKeyBuilder::new(
            1,
            "1.0.0",
            "base",
            config(),
            vec!["src/b.ts".to_string(), "src/a.ts".to_string()],
        )
        .to_json_bytes()
        .expect("payload should serialize");

        assert_ne!(first, second);
    }
}
