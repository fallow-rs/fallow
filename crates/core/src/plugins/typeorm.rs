//! `TypeORM` plugin.
//!
//! Detects `TypeORM` projects and marks entity, migration, and config files as entry points.
//! Reads the `migrations` globs of a `DataSource` config, because `TypeORM`
//! loads the matching files at runtime without an import.

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["typeorm"];

const ENTRY_PATTERNS: &[&str] = &[
    "src/entity/**/*.{ts,js}",
    "src/entities/**/*.{ts,js}",
    "src/migration/**/*.{ts,js}",
    "src/migrations/**/*.{ts,js}",
    "src/subscriber/**/*.{ts,js}",
    "src/subscribers/**/*.{ts,js}",
    "entity/**/*.{ts,js}",
    "entities/**/*.{ts,js}",
    "migration/**/*.{ts,js}",
    "migrations/**/*.{ts,js}",
    "subscriber/**/*.{ts,js}",
    "subscribers/**/*.{ts,js}",
];

const ALWAYS_USED: &[&str] = &[
    "ormconfig.{json,js,ts,yml,yaml}",
    "data-source.{ts,js}",
    "src/data-source.{ts,js}",
];

/// File names that the `TypeORM` docs and the `NestJS` convention use for a
/// `DataSource` config. The list stays narrow, because each match is parsed.
const CONFIG_PATTERNS: &[&str] = &[
    "data-source.{ts,js}",
    "*.data-source.{ts,js}",
    "*.datasource.{ts,js}",
    "ormconfig.{ts,js,json}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["typeorm"];

/// The `DataSource` options whose globs become entry points. `TypeORM` runs
/// every exported class of a migration file, so each export is used. The
/// `entities` and `subscribers` globs stay out: `TypeORM` uses only the
/// decorated classes of those files, and an entry point would hide the other
/// unused exports and members of the file.
const GLOB_OPTION_KEYS: &[&str] = &["migrations"];

/// The class name of the `TypeORM` connection.
const DATA_SOURCE_CLASS: &str = "DataSource";

/// The highest number of globs that one option can contribute.
const MAX_GLOBS_PER_OPTION: usize = 64;

/// The deepest nesting of arrays and conditionals that one option value can have.
const MAX_VALUE_DEPTH: usize = 8;

/// The directory of the compiled output and of the source.
const DIST_DIR: &str = "dist";
const SOURCE_DIR: &str = "src";

define_plugin! {
    struct TypeormPlugin => "typeorm",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();
        // `TypeORM` resolves a relative glob against the process directory,
        // which is the package root. The globs therefore stay package-relative
        // and do not move to the directory of the config file.
        let globs = config_parser::extract_from_source(source, config_path, |program| {
            Some(data_source_globs(program))
        })
        .unwrap_or_default();
        for glob in globs {
            result.push_entry_pattern(glob);
        }
        result
    }
}

/// The package-relative globs of every `DataSource` options object in the program.
fn data_source_globs(program: &Program<'_>) -> Vec<String> {
    let mut globs = Vec::new();
    for object in data_source_objects(program) {
        for key in GLOB_OPTION_KEYS {
            let Some(property) = config_parser::find_property(object, key) else {
                continue;
            };
            let mut values = Vec::new();
            collect_glob_values(&property.value, MAX_VALUE_DEPTH, &mut values);
            for value in values {
                if let Some(glob) = package_relative_glob(&value)
                    && !globs.contains(&glob)
                {
                    globs.push(glob);
                }
            }
        }
    }
    globs
}

/// The options objects of the config: the exported config object, and the
/// argument of each top-level `new DataSource(...)`.
fn data_source_objects<'a>(program: &'a Program<'a>) -> Vec<&'a ObjectExpression<'a>> {
    let mut objects: Vec<&'a ObjectExpression<'a>> = Vec::new();
    let candidates = config_parser::find_config_object(program)
        .into_iter()
        .chain(
            top_level_data_source_arguments(program)
                .filter_map(|argument| options_object(program, argument)),
        );
    for object in candidates {
        if !objects.iter().any(|known| std::ptr::eq(*known, object)) {
            objects.push(object);
        }
    }
    objects
}

/// The first argument of each `new DataSource(...)` that a top-level variable
/// or the default export holds.
fn top_level_data_source_arguments<'a>(
    program: &'a Program<'a>,
) -> impl Iterator<Item = &'a Expression<'a>> {
    program
        .body
        .iter()
        .flat_map(top_level_initializers)
        .filter_map(data_source_argument)
}

fn top_level_initializers<'a>(statement: &'a Statement<'a>) -> Vec<&'a Expression<'a>> {
    let declaration = match statement {
        Statement::VariableDeclaration(declaration) => Some(&**declaration),
        Statement::ExportDeclaration(export) => match &export.declaration {
            Declaration::VariableDeclaration(declaration) => Some(&**declaration),
            _ => None,
        },
        Statement::ExportDefaultDeclaration(export) => {
            return export.declaration.as_expression().into_iter().collect();
        }
        _ => None,
    };
    declaration
        .map(|declaration| {
            declaration
                .declarations
                .iter()
                .filter_map(|declarator| declarator.init.as_ref())
                .collect()
        })
        .unwrap_or_default()
}

fn data_source_argument<'a>(expression: &'a Expression<'a>) -> Option<&'a Expression<'a>> {
    let Expression::NewExpression(new_expression) = unwrap_type_wrappers(expression) else {
        return None;
    };
    let Expression::Identifier(callee) = &new_expression.callee else {
        return None;
    };
    if callee.name != DATA_SOURCE_CLASS {
        return None;
    }
    new_expression.arguments.first()?.as_expression()
}

/// The options object that `argument` names: an inline object, or a top-level
/// binding that the program does not reassign.
fn options_object<'a>(
    program: &'a Program<'a>,
    argument: &'a Expression<'a>,
) -> Option<&'a ObjectExpression<'a>> {
    match unwrap_type_wrappers(argument) {
        Expression::ObjectExpression(object) => Some(object),
        Expression::Identifier(identifier) => {
            let init = config_parser::find_stable_binding_init(program, &identifier.name)?;
            match unwrap_type_wrappers(init) {
                Expression::ObjectExpression(object) => Some(object),
                _ => None,
            }
        }
        _ => None,
    }
}

fn unwrap_type_wrappers<'a>(expression: &'a Expression<'a>) -> &'a Expression<'a> {
    match expression {
        Expression::ParenthesizedExpression(inner) => unwrap_type_wrappers(&inner.expression),
        Expression::TSAsExpression(inner) => unwrap_type_wrappers(&inner.expression),
        Expression::TSSatisfiesExpression(inner) => unwrap_type_wrappers(&inner.expression),
        _ => expression,
    }
}

/// Collect the static strings of an option value. A conditional value adds
/// both branches, because each branch is a possible runtime config.
fn collect_glob_values(expression: &Expression<'_>, depth: usize, values: &mut Vec<String>) {
    if depth == 0 || values.len() >= MAX_GLOBS_PER_OPTION {
        return;
    }
    match unwrap_type_wrappers(expression) {
        Expression::StringLiteral(literal) => values.push(literal.value.to_string()),
        Expression::TemplateLiteral(template) => {
            if let Some(value) = template_glob(template) {
                values.push(value);
            }
        }
        Expression::ArrayExpression(array) => {
            for element in &array.elements {
                if let Some(element) = element.as_expression() {
                    collect_glob_values(element, depth - 1, values);
                }
            }
        }
        Expression::ConditionalExpression(conditional) => {
            collect_glob_values(&conditional.consequent, depth - 1, values);
            collect_glob_values(&conditional.alternate, depth - 1, values);
        }
        _ => {}
    }
}

/// The glob of a template literal. Each interpolation must be a conditional
/// that selects the source or the compiled directory. Any other
/// interpolation makes the value unknown.
fn template_glob(template: &TemplateLiteral<'_>) -> Option<String> {
    let mut glob = String::new();
    for (index, quasi) in template.quasis.iter().enumerate() {
        glob.push_str(quasi.value.raw.as_str());
        if let Some(expression) = template.expressions.get(index) {
            glob.push_str(&source_directory_branch(expression)?);
        }
    }
    Some(glob)
}

/// The branch of `cond ? 'src/' : 'dist/'` that points to the source. When no
/// branch is `src`, a `dist` branch maps to `src`, because the compiled files
/// mirror the source tree.
fn source_directory_branch(expression: &Expression<'_>) -> Option<String> {
    let Expression::ConditionalExpression(conditional) = unwrap_type_wrappers(expression) else {
        return None;
    };
    let branches = [
        config_parser::expression_to_string(&conditional.consequent)?,
        config_parser::expression_to_string(&conditional.alternate)?,
    ];
    if let Some(source) = branches
        .iter()
        .find(|branch| top_directory(branch) == Some(SOURCE_DIR))
    {
        return Some(source.clone());
    }
    branches
        .iter()
        .find(|branch| top_directory(branch) == Some(DIST_DIR))
        .and_then(|branch| {
            let stripped = branch.strip_prefix("./").unwrap_or(branch);
            let rest = stripped.strip_prefix(DIST_DIR)?;
            Some(format!("{SOURCE_DIR}{rest}"))
        })
}

/// The first path segment of a relative directory value such as `./src/`.
fn top_directory(value: &str) -> Option<&str> {
    let value = value.strip_prefix("./").unwrap_or(value);
    value
        .split('/')
        .next()
        .filter(|segment| !segment.is_empty())
}

/// A glob relative to the package root. A glob that leaves the package or
/// that is absolute is ignored, because the package root is the only base
/// that the config defines.
fn package_relative_glob(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let glob = trimmed.strip_prefix("./").unwrap_or(trimmed);
    let leaves_package = glob.split('/').any(|segment| segment == "..");
    if glob.is_empty() || glob.starts_with('/') || glob.contains('\\') || leaves_package {
        return None;
    }
    Some(glob.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::plugins::PluginResult;

    fn patterns(file: &str, source: &str) -> Vec<String> {
        let result: PluginResult =
            TypeormPlugin.resolve_config(Path::new(file), source, Path::new("/project"));
        result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    #[test]
    fn conditional_template_takes_the_source_branch() {
        let source = r"
            import { DataSource } from 'typeorm';
            const isJest = process.argv.some((arg) => arg.includes('jest'));
            export const options = {
                type: 'postgres',
                migrations:
                    process.env.BILLING === 'true'
                        ? [
                              `${isJest ? 'src/' : 'dist/'}database/common/*{.ts,.js}`,
                              `${isJest ? 'src/' : 'dist/'}database/billing/*{.ts,.js}`,
                          ]
                        : [`${isJest ? 'src/' : 'dist/'}database/common/*{.ts,.js}`],
            };
            export const connectionSource = new DataSource(options as DataSourceOptions);
        ";
        let found = patterns("src/database/core.datasource.ts", source);
        assert!(found.contains(&"src/database/common/*{.ts,.js}".to_string()));
        assert!(found.contains(&"src/database/billing/*{.ts,.js}".to_string()));
        assert!(!found.iter().any(|pattern| pattern.starts_with("dist/")));
    }

    #[test]
    fn conditional_template_without_source_branch_maps_dist_to_src() {
        let source = r"
            import { DataSource } from 'typeorm';
            export default new DataSource({
                migrations: [`${prod ? 'dist/' : 'build/'}db/changes/*.js`],
            });
        ";
        assert_eq!(
            patterns("data-source.ts", source),
            vec!["src/db/changes/*.js".to_string()]
        );
    }

    #[test]
    fn plain_string_value_becomes_an_entry_pattern() {
        let source = r"
            import { DataSource } from 'typeorm';
            export const AppDataSource = new DataSource({
                type: 'sqlite',
                migrations: './src/db/changes/*.ts',
            });
        ";
        assert_eq!(
            patterns("src/app.data-source.ts", source),
            vec!["src/db/changes/*.ts".to_string()]
        );
    }

    #[test]
    fn entity_and_subscriber_globs_are_not_credited() {
        let source = r"
            import { DataSource } from 'typeorm';
            export const AppDataSource = new DataSource({
                entities: ['src/schema/*.entity.ts'],
                subscribers: 'src/events/*.subscriber.ts',
            });
        ";
        assert!(patterns("data-source.ts", source).is_empty());
    }

    #[test]
    fn array_values_in_module_exports_become_entry_patterns() {
        let source = r"
            module.exports = {
                type: 'postgres',
                migrations: ['db/changes/*.js', 'db/seeds/*.js'],
            };
        ";
        assert_eq!(
            patterns("ormconfig.js", source),
            vec!["db/changes/*.js".to_string(), "db/seeds/*.js".to_string()]
        );
    }

    #[test]
    fn json_config_values_become_entry_patterns() {
        let source = r#"{ "type": "mysql", "migrations": ["lib/changes/**/*.ts"] }"#;
        assert_eq!(
            patterns("ormconfig.json", source),
            vec!["lib/changes/**/*.ts".to_string()]
        );
    }

    #[test]
    fn unsupported_interpolation_is_ignored() {
        let source = r"
            import { DataSource } from 'typeorm';
            export const AppDataSource = new DataSource({
                migrations: [
                    `${__dirname}/changes/*.ts`,
                    `${root}/changes/*.ts`,
                    `${isJest ? 'src/' : 'dist/'}${folder}/*.ts`,
                    `${a ? 'lib/' : 'out/'}changes/*.ts`,
                ],
            });
        ";
        assert!(patterns("data-source.ts", source).is_empty());
    }

    #[test]
    fn paths_outside_the_package_are_ignored() {
        let source = r"
            import { DataSource } from 'typeorm';
            export const AppDataSource = new DataSource({
                migrations: ['../shared/changes/*.ts', '/abs/changes/*.ts'],
            });
        ";
        assert!(patterns("data-source.ts", source).is_empty());
    }

    #[test]
    fn other_options_are_not_credited() {
        let source = r"
            import { DataSource } from 'typeorm';
            export const AppDataSource = new DataSource({
                type: 'postgres',
                url: 'src/not-a-glob.ts',
                migrationsTableName: 'src/migrations.ts',
            });
        ";
        assert!(patterns("data-source.ts", source).is_empty());
    }
}
