//! Electron Forge config.
//!
//! `@electron-forge/plugin-vite` builds each `build` entry and each `renderer`
//! with the Vite config that the entry names. The plugin reads these paths
//! from `new VitePlugin({ build, renderer })`, or from the plugin object form
//! `{ name: "@electron-forge/plugin-vite", config: { build, renderer } }`.

use std::path::Path;

use fallow_extract::JsText;
use oxc_ast::ast::{
    Argument, BindingPattern, Expression, ImportDeclaration, ImportDeclarationSpecifier,
    NewExpression, ObjectExpression, Program, Statement, StringLiteral, VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};

use super::config_parser;

/// The Forge plugin package that builds the app with Vite.
const VITE_PLUGIN_PACKAGE: &str = "@electron-forge/plugin-vite";

/// The class that `@electron-forge/plugin-vite` exports.
const VITE_PLUGIN_EXPORT: &str = "VitePlugin";

/// What a Forge config tells about its Vite build.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct ForgeVite {
    /// The config names `@electron-forge/plugin-vite`.
    pub(super) names_plugin: bool,
    /// The `entry` paths of the `build` items.
    pub(super) entries: Vec<String>,
    /// The `config` paths of the `build` and `renderer` items.
    pub(super) vite_configs: Vec<String>,
}

/// Read the Vite plugin options of a Forge config.
pub(super) fn read_forge_vite(source: &str, path: &Path) -> ForgeVite {
    config_parser::extract_from_source(source, path, |program| {
        let mut visitor = ForgeViteVisitor {
            bindings: vite_plugin_bindings(program),
            result: ForgeVite::default(),
        };
        visitor.visit_program(program);
        Some(visitor.result)
    })
    .unwrap_or_default()
}

/// The local names that the config gives to the `VitePlugin` class: a bare
/// name for a named import or a destructured `require`, or a namespace for
/// `import * as forgeVite`.
#[derive(Default)]
struct VitePluginBindings {
    classes: Vec<String>,
    namespaces: Vec<String>,
}

fn vite_plugin_bindings(program: &Program<'_>) -> VitePluginBindings {
    let mut bindings = VitePluginBindings::default();
    for stmt in &program.body {
        match stmt {
            Statement::ImportDeclaration(decl) if decl.source.value == VITE_PLUGIN_PACKAGE => {
                add_import_bindings(decl, &mut bindings);
            }
            Statement::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    add_require_bindings(declarator, &mut bindings);
                }
            }
            _ => {}
        }
    }
    bindings
}

/// Record `import { VitePlugin }` and `import * as ns` of the plugin package.
fn add_import_bindings(decl: &ImportDeclaration<'_>, bindings: &mut VitePluginBindings) {
    for specifier in decl.specifiers.iter().flatten() {
        match specifier {
            ImportDeclarationSpecifier::ImportSpecifier(specifier)
                if specifier.imported.name().as_ref() == VITE_PLUGIN_EXPORT =>
            {
                bindings.classes.push(specifier.local.name.to_string());
            }
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                bindings.namespaces.push(specifier.local.name.to_string());
            }
            _ => {}
        }
    }
}

/// Record `const ns = require(...)` and `const { VitePlugin } = require(...)`
/// of the plugin package.
fn add_require_bindings(declarator: &VariableDeclarator<'_>, bindings: &mut VitePluginBindings) {
    let is_plugin_require = declarator.init.as_ref().is_some_and(|init| {
        matches!(init, Expression::CallExpression(call)
            if config_parser::get_require_source(call).as_deref() == Some(VITE_PLUGIN_PACKAGE))
    });
    if !is_plugin_require {
        return;
    }
    match &declarator.id {
        BindingPattern::BindingIdentifier(identifier) => {
            bindings.namespaces.push(identifier.name.to_string());
        }
        BindingPattern::ObjectPattern(object) => {
            for prop in &object.properties {
                if prop
                    .key
                    .static_name()
                    .is_some_and(|name| name.text() == VITE_PLUGIN_EXPORT)
                    && let BindingPattern::BindingIdentifier(identifier) = &prop.value
                {
                    bindings.classes.push(identifier.name.to_string());
                }
            }
        }
        _ => {}
    }
}

struct ForgeViteVisitor {
    bindings: VitePluginBindings,
    result: ForgeVite,
}

impl ForgeViteVisitor {
    fn is_vite_plugin_class(&self, callee: &Expression<'_>) -> bool {
        match callee {
            Expression::Identifier(identifier) => self
                .bindings
                .classes
                .iter()
                .any(|name| name == identifier.name.as_str()),
            Expression::StaticMemberExpression(member) => {
                member.property.name == VITE_PLUGIN_EXPORT
                    && matches!(&member.object, Expression::Identifier(object)
                        if self.bindings.namespaces.iter().any(|name| name == object.name.as_str()))
            }
            _ => false,
        }
    }

    fn read_options(&mut self, options: &ObjectExpression<'_>) {
        for key in ["build", "renderer"] {
            let Some(items) = config_parser::property_expr(options, key)
                .and_then(config_parser::array_expression)
            else {
                continue;
            };
            for item in items.elements.iter().filter_map(|el| el.as_expression()) {
                let Some(item) = config_parser::object_expression(item) else {
                    continue;
                };
                if let Some(config) = config_parser::property_expr(item, "config")
                    .and_then(config_parser::expression_to_path_string)
                {
                    self.result.vite_configs.push(config);
                }
                if key == "build"
                    && let Some(entry) = config_parser::property_expr(item, "entry")
                {
                    match config_parser::expression_to_path_string(entry) {
                        Some(path) => self.result.entries.push(path),
                        None => self
                            .result
                            .entries
                            .extend(config_parser::expression_to_string_or_array(entry)),
                    }
                }
            }
        }
    }
}

impl<'a> Visit<'a> for ForgeViteVisitor {
    fn visit_string_literal(&mut self, literal: &StringLiteral<'a>) {
        if literal.value == VITE_PLUGIN_PACKAGE {
            self.result.names_plugin = true;
        }
    }

    fn visit_new_expression(&mut self, new_expression: &NewExpression<'a>) {
        if self.is_vite_plugin_class(&new_expression.callee)
            && let Some(Expression::ObjectExpression(options)) = new_expression
                .arguments
                .first()
                .and_then(Argument::as_expression)
        {
            self.read_options(options);
        }
        walk::walk_new_expression(self, new_expression);
    }

    fn visit_object_expression(&mut self, object: &ObjectExpression<'a>) {
        if config_parser::property_string(object, "name").as_deref() == Some(VITE_PLUGIN_PACKAGE)
            && let Some(options) = config_parser::property_object(object, "config")
        {
            self.read_options(options);
        }
        walk::walk_object_expression(self, object);
    }
}
