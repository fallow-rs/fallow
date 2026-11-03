//! Metro resolver settings that a Metro config declares.

use std::path::Path;

use oxc_ast::ast::{
    AssignmentTarget, Expression, ObjectExpression, ObjectPropertyKind, PropertyKey,
};
use oxc_ast_visit::{Visit, walk};

use super::super::{PluginResult, ProvidedDependencyRule, config_parser};

const EXTRA_NODE_MODULES: &str = "extraNodeModules";

/// The files that a Metro config applies to: every file below its folder.
const SCOPE_SUFFIX: &str = "**/*";

/// Credit the package names that `resolver.extraNodeModules` maps to a folder.
///
/// Metro resolves an import of such a name, or of a subpath of it, to the
/// folder that the config names. The folder is often generated and is not a
/// listed dependency, so the names are provided dependencies of the files
/// below the config folder. A computed key or a spread adds no name.
pub(super) fn add_extra_node_modules(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    root: &Path,
) {
    let Some(config_dir) = config_path
        .parent()
        .and_then(|dir| dir.strip_prefix(root).ok())
        .map(config_parser::path_to_config_string)
    else {
        return;
    };
    let names = extra_node_modules_keys(source, config_path);
    if names.is_empty() {
        return;
    }
    let scope = if config_dir.is_empty() {
        SCOPE_SUFFIX.to_string()
    } else {
        format!("{config_dir}/{SCOPE_SUFFIX}")
    };
    for name in names {
        let prefix = format!("{name}/");
        result
            .provided_dependencies
            .push(ProvidedDependencyRule::new(scope.clone(), [name], [prefix]));
    }
}

/// The static keys of each object that the config assigns to
/// `extraNodeModules`, as a property value or through a member assignment.
fn extra_node_modules_keys(source: &str, config_path: &Path) -> Vec<String> {
    config_parser::extract_from_source(source, config_path, |program| {
        let mut collector = ExtraNodeModulesCollector::default();
        collector.visit_program(program);
        Some(collector.names)
    })
    .unwrap_or_default()
}

#[derive(Default)]
struct ExtraNodeModulesCollector {
    names: Vec<String>,
}

impl ExtraNodeModulesCollector {
    fn add_keys(&mut self, value: &Expression<'_>) {
        let Some(object) = config_parser::object_expression(value) else {
            return;
        };
        for name in object_keys(object) {
            if config_parser::is_package_specifier(&name) && !self.names.contains(&name) {
                self.names.push(name);
            }
        }
    }
}

impl<'a> Visit<'a> for ExtraNodeModulesCollector {
    fn visit_object_property(&mut self, property: &oxc_ast::ast::ObjectProperty<'a>) {
        if !property.computed && is_extra_node_modules_key(&property.key) {
            self.add_keys(&property.value);
        }
        walk::walk_object_property(self, property);
    }

    fn visit_assignment_expression(&mut self, assignment: &oxc_ast::ast::AssignmentExpression<'a>) {
        if let AssignmentTarget::StaticMemberExpression(member) = &assignment.left
            && member.property.name == EXTRA_NODE_MODULES
        {
            self.add_keys(&assignment.right);
        }
        walk::walk_assignment_expression(self, assignment);
    }
}

fn is_extra_node_modules_key(key: &PropertyKey<'_>) -> bool {
    config_parser::property_key_to_string(key).is_some_and(|name| name == EXTRA_NODE_MODULES)
}

fn object_keys(object: &ObjectExpression<'_>) -> Vec<String> {
    object
        .properties
        .iter()
        .filter_map(|property| match property {
            ObjectPropertyKind::ObjectProperty(property) if !property.computed => {
                config_parser::property_key_to_string(&property.key)
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(source: &str) -> Vec<String> {
        extra_node_modules_keys(source, Path::new("metro.config.js"))
    }

    #[test]
    fn reads_keys_of_a_resolver_object() {
        let source = r#"module.exports = {
          resolver: { extraNodeModules: { "@scope/a": "./a", b: "./b", ["c"]: "./c" } },
        };"#;
        assert_eq!(keys(source), ["@scope/a", "b"]);
    }

    #[test]
    fn reads_keys_of_a_member_assignment() {
        let source = r#"const config = {};
        config.resolver.extraNodeModules = { ...other, "@scope/a": dir };"#;
        assert_eq!(keys(source), ["@scope/a"]);
    }

    #[test]
    fn ignores_relative_and_dynamic_values() {
        let source = r#"config.resolver.extraNodeModules = new Proxy({}, handler);
        module.exports = { resolver: { extraNodeModules: { "./local": "./x" } } };"#;
        assert!(keys(source).is_empty());
    }
}
