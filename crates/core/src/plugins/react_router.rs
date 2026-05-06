//! React Router (v7+) framework plugin.
//!
//! Detects React Router projects and marks route files, root layout, and entry points.
//! Recognizes conventional route exports (loader, action, meta, etc.).

use std::{
    fs,
    path::{Path, PathBuf},
};

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    CallExpression, ExportDefaultDeclarationKind, Expression, ObjectExpression, Program, Statement,
};
use oxc_parser::Parser;
use oxc_span::SourceType;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["@react-router/dev"];
const CONFIG_PATTERNS: &[&str] = &[
    "react-router.config.{ts,js,mjs,cjs}",
    "app/routes.{ts,js,mts,mjs}",
    "src/routes.{ts,js,mts,mjs}",
];

const ENTRY_PATTERNS: &[&str] = &[
    "app/routes/**/*.{ts,tsx,js,jsx}",
    "app/root.{ts,tsx,js,jsx}",
    "app/entry.client.{ts,tsx,js,jsx}",
    "app/entry.server.{ts,tsx,js,jsx}",
];

const ALWAYS_USED: &[&str] = &[
    "react-router.config.{ts,js,mjs,cjs}",
    "app/routes.{ts,js,mts,mjs}",
    "src/routes.{ts,js,mts,mjs}",
];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "@react-router/dev",
    "@react-router/serve",
    "@react-router/node",
];

const BUNDLE_BOUNDARY_DIRS: &[&str] = &[".client", ".server"];

macro_rules! route_module_exports {
    ($($export:literal),+ $(,)?) => {
        const ROUTE_EXPORTS: &[&str] = &[$($export),+];
        const ROOT_EXPORTS: &[&str] = &[$($export,)+ "Layout"];
    };
}

route_module_exports!(
    "default",
    "loader",
    "clientLoader",
    "action",
    "clientAction",
    "meta",
    "links",
    "headers",
    "handle",
    "ErrorBoundary",
    "HydrateFallback",
    "shouldRevalidate",
    "middleware",
    "clientMiddleware",
);

const ROUTE_CONFIG_EXPORTS: &[&str] = &["default"];

define_plugin! {
    struct ReactRouterPlugin => "react-router",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    discovery_hidden_dirs: BUNDLE_BOUNDARY_DIRS,
    used_exports: [
        ("app/routes/**/*.{ts,tsx,js,jsx}", ROUTE_EXPORTS),
        ("app/root.{ts,tsx,js,jsx}", ROOT_EXPORTS),
        ("app/routes.{ts,js,mts,mjs}", ROUTE_CONFIG_EXPORTS),
    ],
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();
        add_referenced_dependencies(&mut result, source, config_path);

        if is_react_router_project_config(config_path) {
            let app_dir = extract_app_directory(source, config_path, root);
            add_app_dir_patterns(&mut result, &app_dir);
            if !matches!(app_dir.as_str(), "app" | "src") {
                collect_route_config_from_disk(&mut result, root, &app_dir);
            }
            return result;
        }

        let Some(app_dir) = route_config_app_dir(config_path, root) else {
            return result;
        };
        add_app_dir_patterns(&mut result, &app_dir);
        collect_route_config_from_source(&mut result, source, config_path, root);
        result
    },
}

fn add_referenced_dependencies(result: &mut PluginResult, source: &str, config_path: &Path) {
    for import in config_parser::extract_imports(source, config_path) {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(&import));
    }
}

fn is_react_router_project_config(config_path: &Path) -> bool {
    config_path.file_name().is_some_and(|name| {
        let file_name = name.to_string_lossy();
        file_name.starts_with("react-router.config.")
    })
}

fn extract_app_directory(source: &str, config_path: &Path, root: &Path) -> String {
    config_parser::extract_config_string(source, config_path, &["appDirectory"])
        .as_deref()
        .and_then(|raw| config_parser::normalize_config_path(raw, config_path, root))
        .unwrap_or_else(|| "app".to_string())
}

fn route_config_app_dir(config_path: &Path, root: &Path) -> Option<String> {
    let file_name = config_path.file_name()?.to_string_lossy();
    if !file_name.starts_with("routes.") {
        return None;
    }

    let parent = config_path.parent().unwrap_or(root);
    let relative = parent.strip_prefix(root).ok()?;
    let normalized = relative.to_string_lossy().replace('\\', "/");
    (!normalized.is_empty()).then_some(normalized)
}

fn add_app_dir_patterns(result: &mut PluginResult, app_dir: &str) {
    let route_dir_pattern = route_dir_pattern(app_dir);
    let route_config_pattern = route_config_pattern(app_dir);
    let root_pattern = format!("{app_dir}/root.{{ts,tsx,js,jsx}}");

    result.push_entry_pattern(route_dir_pattern.clone());
    result.push_entry_pattern(root_pattern.clone());
    result.push_entry_pattern(format!("{app_dir}/entry.client.{{ts,tsx,js,jsx}}"));
    result.push_entry_pattern(format!("{app_dir}/entry.server.{{ts,tsx,js,jsx}}"));
    result.push_entry_pattern(route_config_pattern.clone());

    result.push_used_export_rule(route_dir_pattern, ROUTE_EXPORTS.iter().copied());
    result.push_used_export_rule(root_pattern, ROOT_EXPORTS.iter().copied());
    result.push_used_export_rule(route_config_pattern, ROUTE_CONFIG_EXPORTS.iter().copied());
}

fn route_dir_pattern(app_dir: &str) -> String {
    format!("{app_dir}/routes/**/*.{{ts,tsx,js,jsx}}")
}

fn route_config_pattern(app_dir: &str) -> String {
    format!("{app_dir}/routes.{{ts,js,mts,mjs}}")
}

fn collect_route_config_from_disk(result: &mut PluginResult, root: &Path, app_dir: &str) {
    for candidate in route_config_candidates(root, app_dir) {
        if !candidate.is_file() {
            continue;
        }
        let Ok(source) = fs::read_to_string(&candidate) else {
            continue;
        };
        add_referenced_dependencies(result, &source, &candidate);
        collect_route_config_from_source(result, &source, &candidate, root);
        break;
    }
}

fn route_config_candidates(root: &Path, app_dir: &str) -> [PathBuf; 4] {
    [
        root.join(app_dir).join("routes.ts"),
        root.join(app_dir).join("routes.js"),
        root.join(app_dir).join("routes.mts"),
        root.join(app_dir).join("routes.mjs"),
    ]
}

fn collect_route_config_from_source(
    result: &mut PluginResult,
    source: &str,
    route_config_path: &Path,
    root: &Path,
) {
    let source_type = SourceType::from_path(route_config_path).unwrap_or_default();
    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, source, source_type).parse();
    let Some(expr) = find_default_export_expression(&parsed.program) else {
        return;
    };

    let base_dir = route_config_path.parent().unwrap_or(root);
    collect_route_entries(expr, &parsed.program, base_dir, root, result);
}

fn find_default_export_expression<'a>(program: &'a Program<'a>) -> Option<&'a Expression<'a>> {
    for stmt in &program.body {
        let Statement::ExportDefaultDeclaration(decl) = stmt else {
            continue;
        };

        let expr = match &decl.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(_)
            | ExportDefaultDeclarationKind::ClassDeclaration(_) => return None,
            _ => decl.declaration.as_expression()?,
        };
        let expr = strip_wrappers(expr);

        if let Some(name) = unwrap_identifier_name(expr) {
            return find_variable_init_expression(program, name);
        }
        return Some(expr);
    }

    None
}

fn find_variable_init_expression<'a>(
    program: &'a Program<'a>,
    name: &str,
) -> Option<&'a Expression<'a>> {
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };

        for declarator in &decl.declarations {
            let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &declarator.id else {
                continue;
            };
            if id.name == name {
                return declarator.init.as_ref().map(strip_wrappers);
            }
        }
    }

    None
}

fn strip_wrappers<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(paren) => strip_wrappers(&paren.expression),
        Expression::TSSatisfiesExpression(ts_sat) => strip_wrappers(&ts_sat.expression),
        Expression::TSAsExpression(ts_as) => strip_wrappers(&ts_as.expression),
        _ => expr,
    }
}

fn unwrap_identifier_name<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match strip_wrappers(expr) {
        Expression::Identifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn collect_route_entries(
    expr: &Expression<'_>,
    program: &Program<'_>,
    base_dir: &Path,
    root: &Path,
    result: &mut PluginResult,
) {
    match strip_wrappers(expr) {
        Expression::ArrayExpression(array) => {
            for element in &array.elements {
                let Some(element_expr) = element.as_expression() else {
                    continue;
                };
                collect_route_entries(element_expr, program, base_dir, root, result);
            }
        }
        Expression::ObjectExpression(obj) => {
            collect_route_object(obj, program, base_dir, root, result);
        }
        Expression::CallExpression(call) => {
            collect_route_call(call, program, base_dir, root, result);
        }
        Expression::Identifier(id) => {
            if let Some(init) = find_variable_init_expression(program, id.name.as_str()) {
                collect_route_entries(init, program, base_dir, root, result);
            }
        }
        _ => {}
    }
}

fn collect_route_object(
    obj: &ObjectExpression<'_>,
    program: &Program<'_>,
    base_dir: &Path,
    root: &Path,
    result: &mut PluginResult,
) {
    if let Some(file) = config_parser::property_string(obj, "file") {
        add_route_module_file(result, &file, base_dir, root);
    }

    if let Some(children) = config_parser::property_expr(obj, "children") {
        collect_route_entries(children, program, base_dir, root, result);
    }
}

fn collect_route_call(
    call: &CallExpression<'_>,
    program: &Program<'_>,
    base_dir: &Path,
    root: &Path,
    result: &mut PluginResult,
) {
    let Some(name) = unwrap_identifier_name(&call.callee) else {
        return;
    };

    match name {
        "route" => {
            if let Some(file) = nth_argument_string(call, 1) {
                add_route_module_file(result, &file, base_dir, root);
            }
            if let Some(children) = nth_argument_expression(call, 2) {
                collect_route_entries(children, program, base_dir, root, result);
            }
        }
        "index" => {
            if let Some(file) = nth_argument_string(call, 0) {
                add_route_module_file(result, &file, base_dir, root);
            }
        }
        "layout" => {
            if let Some(file) = nth_argument_string(call, 0) {
                add_route_module_file(result, &file, base_dir, root);
            }
            if let Some(children) = nth_argument_expression(call, 1) {
                collect_route_entries(children, program, base_dir, root, result);
            }
        }
        "prefix" => {
            if let Some(children) = nth_argument_expression(call, 1) {
                collect_route_entries(children, program, base_dir, root, result);
            }
        }
        "relative" => {
            let next_base = nth_argument_string(call, 0)
                .and_then(|raw| normalize_route_base_dir(&raw, base_dir, root))
                .unwrap_or_else(|| base_dir.to_path_buf());
            if let Some(children) = nth_argument_expression(call, 1) {
                collect_route_entries(children, program, &next_base, root, result);
            }
        }
        "flatRoutes" => {
            if let Some(route_dir) = extract_flat_routes_root_dir(call, base_dir, root) {
                add_route_dir_override(result, &route_dir);
            }
        }
        _ => {
            for argument in &call.arguments {
                let Some(argument_expr) = argument.as_expression() else {
                    continue;
                };
                collect_route_entries(argument_expr, program, base_dir, root, result);
            }
        }
    }
}

fn nth_argument_expression<'a>(
    call: &'a CallExpression<'a>,
    index: usize,
) -> Option<&'a Expression<'a>> {
    call.arguments
        .get(index)?
        .as_expression()
        .map(strip_wrappers)
}

fn nth_argument_string(call: &CallExpression<'_>, index: usize) -> Option<String> {
    expression_string(nth_argument_expression(call, index)?)
}

fn expression_string(expr: &Expression<'_>) -> Option<String> {
    match strip_wrappers(expr) {
        Expression::StringLiteral(lit) => Some(lit.value.to_string()),
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
            tpl.quasis.first().map(|quasi| quasi.value.raw.to_string())
        }
        _ => None,
    }
}

fn extract_flat_routes_root_dir(
    call: &CallExpression<'_>,
    base_dir: &Path,
    root: &Path,
) -> Option<String> {
    let options_expr = nth_argument_expression(call, 0)?;
    let options = config_parser::object_expression(options_expr)?;
    let raw = config_parser::property_string(options, "rootDirectory")?;
    normalize_route_path(&raw, base_dir, root)
}

fn normalize_route_path(raw: &str, base_dir: &Path, root: &Path) -> Option<String> {
    let synthetic_path = base_dir.join("__fallow__.ts");
    config_parser::normalize_config_path(raw, &synthetic_path, root)
}

fn normalize_route_base_dir(raw: &str, base_dir: &Path, root: &Path) -> Option<PathBuf> {
    normalize_route_path(raw, base_dir, root).map(|relative| root.join(relative))
}

fn add_route_module_file(result: &mut PluginResult, raw_path: &str, base_dir: &Path, root: &Path) {
    let Some(path) = normalize_route_path(raw_path, base_dir, root) else {
        return;
    };
    result.push_entry_pattern(path.clone());
    result.push_used_export_rule(path, ROUTE_EXPORTS.iter().copied());
}

fn add_route_dir_override(result: &mut PluginResult, route_dir: &str) {
    let pattern = format!("{route_dir}/**/*.{{ts,tsx,js,jsx}}");
    result.push_entry_pattern(pattern.clone());
    result.push_used_export_rule(pattern, ROUTE_EXPORTS.iter().copied());
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn used_exports_cover_root_and_route_config() {
        let plugin = ReactRouterPlugin;
        let exports = plugin.used_exports();

        assert!(exports.iter().any(|(pattern, names)| {
            pattern == &"app/root.{ts,tsx,js,jsx}" && names.contains(&"Layout")
        }));
        assert!(exports.iter().any(|(pattern, names)| {
            pattern == &"app/routes/**/*.{ts,tsx,js,jsx}"
                && names.contains(&"clientMiddleware")
                && names.contains(&"middleware")
        }));
        assert!(exports.iter().any(|(pattern, names)| {
            pattern == &"app/routes.{ts,js,mts,mjs}" && names == &["default"]
        }));
    }

    #[test]
    fn discovery_hidden_dirs_include_bundle_boundaries() {
        let plugin = ReactRouterPlugin;
        assert_eq!(plugin.discovery_hidden_dirs(), [".client", ".server"]);
    }

    fn has_entry_pattern(result: &PluginResult, pattern: &str) -> bool {
        result
            .entry_patterns
            .iter()
            .any(|entry_pattern| entry_pattern.pattern == pattern)
    }

    #[test]
    fn resolve_config_honors_custom_app_directory() {
        let plugin = ReactRouterPlugin;
        let source = r#"export default { appDirectory: "src" };"#;

        let result = plugin.resolve_config(
            Path::new("/project/react-router.config.ts"),
            source,
            Path::new("/project"),
        );

        assert!(has_entry_pattern(&result, "src/root.{ts,tsx,js,jsx}"));
        assert!(has_entry_pattern(&result, "src/routes.{ts,js,mts,mjs}"));
        assert!(result.used_exports.iter().any(|rule| {
            rule.path.pattern == "src/routes/**/*.{ts,tsx,js,jsx}"
                && rule.exports.iter().any(|export| export == "loader")
        }));
    }

    #[test]
    fn resolve_config_discovers_route_modules_and_flat_routes() {
        let plugin = ReactRouterPlugin;
        let source = r#"
            import { flatRoutes } from "@react-router/fs-routes";
            import { index, layout, route } from "@react-router/dev/routes";

            export default [
                index("./marketing/home.tsx"),
                layout("./account/layout.tsx", [route("login", "./account/login.tsx")]),
                flatRoutes({ rootDirectory: "file-routes" }),
            ];
        "#;

        let result = plugin.resolve_config(
            Path::new("/project/app/routes.ts"),
            source,
            Path::new("/project"),
        );

        for pattern in [
            "app/marketing/home.tsx",
            "app/account/layout.tsx",
            "app/account/login.tsx",
            "app/file-routes/**/*.{ts,tsx,js,jsx}",
        ] {
            assert!(
                has_entry_pattern(&result, pattern),
                "missing {pattern}: {:?}",
                result.entry_patterns
            );
        }
        assert!(result.used_exports.iter().any(|rule| {
            rule.path.pattern == "app/account/login.tsx"
                && rule.exports.iter().any(|export| export == "clientAction")
        }));
    }

    #[test]
    fn resolve_config_reads_custom_route_file_from_disk() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("web")).unwrap();
        fs::write(
            temp.path().join("web/routes.ts"),
            r#"
                import { index } from "@react-router/dev/routes";
                export default [index("./marketing/home.tsx")];
            "#,
        )
        .unwrap();

        let plugin = ReactRouterPlugin;
        let result = plugin.resolve_config(
            temp.path().join("react-router.config.ts").as_path(),
            r#"export default { appDirectory: "web" };"#,
            temp.path(),
        );

        assert!(has_entry_pattern(&result, "web/marketing/home.tsx"));
    }
}
