/// Build a glob set from health ignore patterns.
///
/// User patterns were validated at config load time
/// (see `FallowConfig::validate_user_globs`).
#[expect(
    clippy::expect_used,
    reason = "health ignore globs are validated before health analysis"
)]
pub(super) fn build_ignore_set(patterns: &[String]) -> globset::GlobSet {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            globset::Glob::new(pattern)
                .expect("health.ignore pattern was validated at config load time"),
        );
    }
    builder
        .build()
        .unwrap_or_else(|_| globset::GlobSet::empty())
}
