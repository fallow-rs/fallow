//! The names that address Nuxt layers in `#layers/<name>/` imports.

use std::path::Path;

use super::{LAYER_CONFIG_FILES, PluginResult, config_parser};

/// Nuxt treats the project root as a layer too: when the root `nuxt.config`
/// sets `$meta.name`, `#layers/<name>/` resolves to the project root.
pub(super) fn add_root_layer_alias(result: &mut PluginResult, source: &str, config_path: &Path) {
    let Some(name) = config_parser::extract_config_string(source, config_path, &["$meta", "name"])
        .filter(|name| is_valid_layer_name(name))
    else {
        return;
    };
    result
        .path_aliases
        .push((format!("#layers/{name}/"), String::new()));
}

/// The name a local layer is addressed by in `#layers/<name>/`: its
/// `$meta.name`. Nuxt names an auto-registered `layers/<name>` directory by its
/// directory name when `$meta.name` is absent, and gives an `extends` layer no
/// name then.
pub(super) fn layer_name(root: &Path, layer: &str, auto_registered: bool) -> Option<String> {
    let dir = root.join(layer);
    let meta_name = LAYER_CONFIG_FILES.iter().find_map(|file| {
        let path = dir.join(file);
        let source = std::fs::read_to_string(&path).ok()?;
        config_parser::extract_config_string(&source, &path, &["$meta", "name"])
    });
    meta_name
        .or_else(|| {
            if !auto_registered {
                return None;
            }
            Path::new(layer)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .filter(|name| is_valid_layer_name(name))
}

fn is_valid_layer_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\'])
}
