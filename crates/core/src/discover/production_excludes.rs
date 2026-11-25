use fallow_config::ResolvedConfig;

/// Glob patterns for test/dev/story files excluded in production mode.
pub const PRODUCTION_EXCLUDE_PATTERNS: &[&str] = &[
    "**/*.test.*",
    "**/*.spec.*",
    "**/*.e2e.*",
    "**/*.e2e-spec.*",
    "**/*.bench.*",
    "**/*.fixture.*",
    "**/*.stories.*",
    "**/*.story.*",
    "**/__tests__/**",
    "**/__mocks__/**",
    "**/__snapshots__/**",
    "**/__fixtures__/**",
    "**/test-d/**",
    "**/test/**",
    "**/tests/**",
    "*.config.*",
    "**/.*.js",
    "**/.*.ts",
    "**/.*.mjs",
    "**/.*.cjs",
];

/// Compile the production-mode exclude glob set, or `None` outside production
/// mode. The set holds the built-in patterns plus the `production.ignore` globs.
pub(super) fn build_production_excludes(config: &ResolvedConfig) -> Option<globset::GlobSet> {
    if !config.production {
        return None;
    }
    production_exclude_globs(config)
}

/// The built-in production exclude patterns plus the `production.ignore`
/// globs, in any mode. The set matches root-relative paths. A pattern that
/// does not compile is left out.
pub fn production_exclude_globs(config: &ResolvedConfig) -> Option<globset::GlobSet> {
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

#[cfg(test)]
mod tests {
    use super::PRODUCTION_EXCLUDE_PATTERNS;

    /// `build_production_excludes` drops a pattern that does not compile, so a
    /// broken pattern would silently stop excluding its files.
    #[test]
    fn production_exclude_patterns_all_compile() {
        for pattern in PRODUCTION_EXCLUDE_PATTERNS {
            assert!(
                globset::GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .build()
                    .is_ok(),
                "production exclude pattern does not compile: {pattern}"
            );
        }
    }
}
