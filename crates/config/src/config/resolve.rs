use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Module resolver configuration.
///
/// Controls how fallow resolves import specifiers against package.json
/// `exports` / `imports` fields and tsconfig paths. Configured via the
/// `resolve` section in `.fallowrc.json` or `fallow.toml`.
///
/// # Examples
///
/// ```json
/// {
///   "resolve": {
///     "conditions": ["development", "worker"]
///   }
/// }
/// ```
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveConfig {
    /// Additional export/import condition names to honor during module
    /// resolution. Merged with fallow's built-in conditions (`development`,
    /// `import`, `require`, `default`, `types`, `node`; plus `react-native`
    /// and `browser` when the React Native or Expo plugin is active).
    ///
    /// User conditions are matched with higher priority than the baseline,
    /// so a package.json `exports` entry like:
    ///
    /// ```json
    /// { "./api": { "worker": "./src/api.worker.ts", "import": "./dist/api.js" } }
    /// ```
    ///
    /// resolves to the `worker` branch when `"worker"` is listed here.
    ///
    /// See <https://nodejs.org/api/packages.html#community-conditions-definitions>
    /// for the set of community-defined conditions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<String>,
}
