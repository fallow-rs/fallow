use fallow_config::ResolvedConfig;

use super::walk::PRODUCTION_EXCLUDE_PATTERNS;

/// Compile the production-mode exclude glob set, or `None` outside production
/// mode. The set holds the built-in patterns plus the `production.ignore` globs.
pub(super) fn build_production_excludes(config: &ResolvedConfig) -> Option<globset::GlobSet> {
    if !config.production {
        return None;
    }
    let mut builder = globset::GlobSetBuilder::new();
    let user_patterns = config.production_ignore.iter().map(String::as_str);
    for pattern in PRODUCTION_EXCLUDE_PATTERNS
        .iter()
        .copied()
        .chain(user_patterns)
    {
        if let Ok(glob) = globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
        {
            builder.add(glob);
        }
    }
    builder.build().ok()
}
