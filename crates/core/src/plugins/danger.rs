//! Danger JS plugin.
//!
//! Danger evaluates a project Dangerfile from CI instead of importing it from
//! application code, so active Danger projects need those files treated as used.

use super::Plugin;

const ENABLERS: &[&str] = &["danger"];

const ALWAYS_USED: &[&str] = &[
    "dangerfile.{js,ts,mjs,cjs}",
    "**/dangerfile.{js,ts,mjs,cjs}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["danger"];

define_plugin! {
    struct DangerPlugin => "danger",
    enablers: ENABLERS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_used_includes_dangerfile_variants() {
        let plugin = DangerPlugin;
        assert!(plugin.always_used().contains(&"dangerfile.{js,ts,mjs,cjs}"));
        assert!(
            plugin
                .always_used()
                .contains(&"**/dangerfile.{js,ts,mjs,cjs}")
        );
    }

    #[test]
    fn tooling_dependencies_include_danger() {
        let plugin = DangerPlugin;
        assert!(plugin.tooling_dependencies().contains(&"danger"));
    }
}
