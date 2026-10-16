#![no_main]
//! Fallow config files and external plugin files, in each supported format.
//!
//! An analyzed repository can ship `.fallowrc.json`, `fallow.toml`,
//! `.fallow/plugins/*` and `fallow-plugin-*` files. The target parses them the
//! way the loader does and runs the glob validation. When the validation
//! accepts a plugin, every glob that it validated must satisfy the glob
//! invariant. Detection expands `fileExists` patterns on disk.

use fallow_config::{ExternalPluginDef, FallowConfig, PluginDetection};
use libfuzzer_sys::fuzz_target;

#[path = "common/glob_invariant.rs"]
mod glob_invariant;

fn assert_contained(pattern: &str, field: &str) {
    if let Some(reason) = glob_invariant::escape_reason(pattern) {
        panic!("accepted plugin with {field} pattern {pattern:?} ({reason})");
    }
}

fn assert_detection_contained(detection: &PluginDetection) {
    match detection {
        PluginDetection::Dependency { .. } => {}
        PluginDetection::FileExists { pattern } => assert_contained(pattern, "fileExists"),
        PluginDetection::All { conditions } | PluginDetection::Any { conditions } => {
            conditions.iter().for_each(assert_detection_contained);
        }
    }
}

/// When validation accepts a plugin, every glob field that it validates must
/// satisfy the invariant.
fn check_plugin(plugin: &ExternalPluginDef) {
    if plugin.validate_user_globs().is_err() {
        return;
    }
    for (field, patterns) in [
        ("entryPoints", &plugin.entry_points),
        ("alwaysUsed", &plugin.always_used),
        ("configPatterns", &plugin.config_patterns),
    ] {
        patterns.iter().for_each(|p| assert_contained(p, field));
    }
    for used in &plugin.used_exports {
        assert_contained(&used.pattern, "usedExports[].pattern");
    }
    for rule in &plugin.manifest_entries {
        assert_contained(&rule.manifests, "manifestEntries[].manifests");
        for seed in &rule.entries {
            assert_contained(
                &seed.path.validation_pattern(),
                "manifestEntries[].entries[].path",
            );
        }
    }
    if let Some(detection) = &plugin.detection {
        assert_detection_contained(detection);
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    let Ok(source) = std::str::from_utf8(rest) else {
        return;
    };
    match selector % 3 {
        0 => {
            if let Ok(plugin) = serde_json::from_str::<ExternalPluginDef>(source) {
                check_plugin(&plugin);
            }
            if let Ok(config) = fallow_config::jsonc::parse_to_value::<FallowConfig>(source) {
                let _ = config.validate_user_globs();
            }
        }
        1 => {
            if let Ok(plugin) = fallow_config::jsonc::parse_to_value::<ExternalPluginDef>(source) {
                check_plugin(&plugin);
            }
        }
        _ => {
            if let Ok(plugin) = toml::from_str::<ExternalPluginDef>(source) {
                check_plugin(&plugin);
            }
            if let Ok(config) = toml::from_str::<FallowConfig>(source) {
                let _ = config.validate_user_globs();
            }
        }
    }
});
