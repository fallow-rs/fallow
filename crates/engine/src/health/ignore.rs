/// Build a glob set from health ignore patterns.
///
/// User patterns were validated at config load time
/// (see `FallowConfig::validate_user_globs`).
#[expect(
    clippy::expect_used,
    reason = "health ignore globs are validated before health analysis"
)]
pub fn build_ignore_set(patterns: &[String]) -> globset::GlobSet {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            fallow_config::compile_path_glob(pattern)
                .expect("health.ignore pattern was validated at config load time"),
        );
    }
    builder
        .build()
        .unwrap_or_else(|_| globset::GlobSet::empty())
}

#[cfg(test)]
mod tests {
    use super::build_ignore_set;

    #[test]
    fn single_star_stays_within_one_segment() {
        let set = build_ignore_set(&["src/gen/*.ts".to_string()]);
        assert!(set.is_match("src/gen/a.ts"));
        assert!(!set.is_match("src/gen/deep/a.ts"));
    }
}
