use std::path::PathBuf;

use super::PathRule;

/// What a Module Federation config declares, and where, for the trace output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationSource {
    /// What the config names.
    pub target: FederationSourceTarget,
    /// The absolute path of the config file.
    pub config_path: PathBuf,
    /// The plugin that read the config, as it labels itself.
    pub plugin: String,
    /// The config key that names the target.
    pub key: &'static str,
}

/// The thing a [`FederationSource`] names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FederationSourceTarget {
    /// An `exposes` target, as the entry-point rule it became.
    Exposed(PathRule),
    /// A `remotes` alias.
    Remote(String),
}

impl FederationSource {
    #[must_use]
    pub(super) fn prefixed(&self, ws_prefix: &str) -> Self {
        let target = match &self.target {
            FederationSourceTarget::Exposed(rule) => {
                FederationSourceTarget::Exposed(rule.prefixed(ws_prefix))
            }
            FederationSourceTarget::Remote(alias) => FederationSourceTarget::Remote(alias.clone()),
        };
        Self {
            target,
            config_path: self.config_path.clone(),
            plugin: self.plugin.clone(),
            key: self.key,
        }
    }
}
