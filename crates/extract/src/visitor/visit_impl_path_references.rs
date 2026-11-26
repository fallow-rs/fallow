//! Paths that code builds from the directory of the current module.
//!
//! `path.join(__dirname, "scripts", "build.mjs")` names a project file. Code
//! hands that path to a consumer that static analysis cannot follow: a child
//! process, a file read, a dynamic `import()` of a file URL. The extractor
//! records the path as a speculative path reference. The reference keeps the
//! file in use and credits no export, because a process or a file read uses
//! none. Only the `import()` form credits the whole module.

use oxc_ast::ast::*;
use oxc_span::Span;

use crate::js_text::JsText;
use crate::visitor::helpers::escape_glob_literal;
use crate::{DynamicImportInfo, ImportedName};
use fallow_types::extract::{DynamicImportPattern, ImportLoadKind, ModuleLoadMechanism};

use super::super::super::ModuleInfoExtractor;
use super::super::super::declarations::ModuleRelativePath;
use super::visit_commands::CommandApi;

/// The extensions of a file that Node or a bundler runs as a script. A path
/// with another extension, or with none, can name a directory, and a
/// directory specifier resolves to its index file.
const SCRIPT_EXTENSIONS: &[&str] = &["js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx"];

/// The functions that join path segments, in `node:path` and in the
/// libraries with the same interface.
const JOIN_FUNCTIONS: &[&str] = &["join", "resolve"];

impl ModuleRelativePath {
    /// Append a relative path. Returns false for an absolute path, which
    /// discards the base in `path.resolve`.
    fn push(&mut self, text: &str) -> bool {
        if text.starts_with('/') {
            return false;
        }
        for part in text.split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." => {
                    if self.parts.pop().is_none() {
                        self.parents += 1;
                    }
                }
                _ => self.parts.push(part.to_string()),
            }
        }
        true
    }

    /// The glob prefix of the files directly inside the directory that the
    /// path names, such as `../schema/migrations/`.
    fn folder_glob_prefix(&self) -> String {
        let mut prefix = if self.parents == 0 {
            "./".to_string()
        } else {
            "../".repeat(self.parents)
        };
        for part in &self.parts {
            prefix.push_str(&escape_glob_literal(part));
            prefix.push('/');
        }
        prefix
    }

    /// The import specifier of the path, when the path names a script file.
    fn script_specifier(&self) -> Option<String> {
        let (_, extension) = self.parts.last()?.rsplit_once('.')?;
        if !SCRIPT_EXTENSIONS.contains(&extension) {
            return None;
        }
        let prefix = if self.parents == 0 {
            "./".to_string()
        } else {
            "../".repeat(self.parents)
        };
        Some(format!("{prefix}{}", self.parts.join("/")))
    }
}

/// The constructor of Kysely that imports each migration file of a folder.
const FILE_MIGRATION_PROVIDER: &str = "FileMigrationProvider";

/// The Kysely entry points that export `FileMigrationProvider`.
const FILE_MIGRATION_PROVIDER_SOURCES: &[&str] = &["kysely", "kysely/migration"];

/// The option of `FileMigrationProvider` that names the migration folder.
const MIGRATION_FOLDER_OPTION: &str = "migrationFolder";

/// The glob suffix of the files that `FileMigrationProvider` imports. Kysely
/// reads the folder without recursion and skips other extensions.
const MIGRATION_FILE_SUFFIX: &str = ".{js,ts,mjs,mts,cjs,cts}";

/// What reads the path of a reference.
#[derive(Clone, Copy)]
enum PathConsumer {
    /// A consumer that static analysis cannot follow. It uses no export.
    Unknown,
    /// A dynamic `import()` or a `require()`, which loads the module.
    Import,
}

/// The join call in `pathToFileURL(<join>).href`, `pathToFileURL(<join>)` or
/// `<join>`.
fn file_url_path_argument<'a, 'b>(source: &'b Expression<'a>) -> Option<&'b CallExpression<'a>> {
    let source = match source.get_inner_expression() {
        Expression::StaticMemberExpression(member) if member.property.name == "href" => {
            member.object.get_inner_expression()
        }
        other => other,
    };
    let Expression::CallExpression(call) = source else {
        return None;
    };
    let is_path_to_file_url = match &call.callee {
        Expression::Identifier(ident) => ident.name == "pathToFileURL",
        Expression::StaticMemberExpression(member) => member.property.name == "pathToFileURL",
        _ => false,
    };
    if !is_path_to_file_url {
        return Some(call);
    }
    match call
        .arguments
        .first()?
        .as_expression()?
        .get_inner_expression()
    {
        Expression::CallExpression(inner) => Some(inner),
        _ => None,
    }
}

/// The text of a string literal or of a template without expressions.
fn static_segment(arg: &Argument<'_>) -> Option<String> {
    match arg {
        Argument::StringLiteral(lit) => Some(lit.value.text().to_string()),
        Argument::TemplateLiteral(tpl) if tpl.expressions.is_empty() => tpl
            .quasis
            .first()
            .map(|quasi| quasi.value.raw.as_str().to_string()),
        _ => None,
    }
}

fn is_join_callee(callee: &Expression<'_>) -> bool {
    match callee {
        Expression::Identifier(ident) => JOIN_FUNCTIONS.contains(&ident.name.as_str()),
        Expression::StaticMemberExpression(member) => {
            JOIN_FUNCTIONS.contains(&member.property.name.as_str())
        }
        _ => false,
    }
}

/// The value of the `cwd` property of an options object.
fn cwd_option<'a, 'b>(arg: &'b Argument<'a>) -> Option<&'b Expression<'a>> {
    let Argument::ObjectExpression(object) = arg else {
        return None;
    };
    object.properties.iter().find_map(|prop| match prop {
        ObjectPropertyKind::ObjectProperty(prop)
            if prop.key.static_name().is_some_and(|name| name == "cwd") =>
        {
            Some(&prop.value)
        }
        _ => None,
    })
}

impl ModuleInfoExtractor {
    /// The path below the module directory that `expr` holds: the module
    /// directory, a `const` of such a path, or a join of one with literal
    /// segments.
    fn module_relative_path(&self, expr: &Expression<'_>) -> Option<ModuleRelativePath> {
        if self.is_current_module_dir_expression(expr) {
            return Some(ModuleRelativePath::default());
        }
        match expr {
            Expression::Identifier(ident) => self.bound_module_relative_path(ident.name.as_str()),
            Expression::CallExpression(call) => self.module_relative_join(call),
            Expression::ParenthesizedExpression(paren) => {
                self.module_relative_path(&paren.expression)
            }
            Expression::TSAsExpression(ts_as) => self.module_relative_path(&ts_as.expression),
            Expression::TSSatisfiesExpression(ts_sat) => {
                self.module_relative_path(&ts_sat.expression)
            }
            _ => None,
        }
    }

    /// The path of `join(<module-relative path>, "a", "b.js")` or of the same
    /// `resolve` call. The base proves that the call joins path segments, so
    /// the callee can belong to any path library.
    fn module_relative_join(&self, call: &CallExpression<'_>) -> Option<ModuleRelativePath> {
        if !is_join_callee(&call.callee) {
            return None;
        }
        let (base, segments) = call.arguments.split_first()?;
        if segments.is_empty() {
            return None;
        }
        let mut path = self.module_relative_path(base.as_expression()?)?;
        for segment in segments {
            if !path.push(&static_segment(segment)?) {
                return None;
            }
        }
        Some(path)
    }

    /// The path that the binding `name` holds, when the innermost
    /// scope that declares `name` is the scope of that binding.
    fn bound_module_relative_path(&self, name: &str) -> Option<ModuleRelativePath> {
        let (depth, path) = self.const_specifiers.module_paths.get(name)?;
        let declaring_depth = self
            .nested_declaration_stack
            .iter()
            .rposition(|scope| scope.contains(name))
            .map_or(0, |index| index + 1);
        (declaring_depth == *depth).then(|| path.clone())
    }

    pub(in crate::visitor) fn record_module_relative_path_binding(
        &mut self,
        name: &str,
        init: &Expression<'_>,
    ) {
        if let Some(path) = self.module_relative_path(init) {
            let depth = self.nested_declaration_stack.len();
            self.const_specifiers
                .module_paths
                .insert(name.to_string(), (depth, path));
        }
    }

    /// Record the folder of `new FileMigrationProvider({ migrationFolder })`
    /// as a glob over its files (issue #5294). Kysely imports each file with
    /// a dynamic `import()` and reads its `up` and `down` exports, so the
    /// pattern credits the whole module.
    pub(in crate::visitor) fn record_migration_folder_pattern(&mut self, expr: &NewExpression<'_>) {
        if !self.is_file_migration_provider(&expr.callee) {
            return;
        }
        let Some(Argument::ObjectExpression(options)) = expr.arguments.first() else {
            return;
        };
        let Some(folder) = options.properties.iter().find_map(|prop| match prop {
            ObjectPropertyKind::ObjectProperty(prop)
                if prop
                    .key
                    .static_name()
                    .is_some_and(|name| name == MIGRATION_FOLDER_OPTION) =>
            {
                Some(&prop.value)
            }
            _ => None,
        }) else {
            return;
        };
        let Some(path) = self.module_relative_path(folder) else {
            return;
        };
        let prefix = path.folder_glob_prefix();
        // The `*` of an include glob can cross a `/`, so an exclude keeps the
        // files of the sub-folders out. Kysely skips declaration files.
        let excludes = vec![
            format!("{prefix}*/**"),
            format!("{prefix}*.d.{{ts,mts,cts}}"),
        ];
        self.dynamic_import_patterns.push(DynamicImportPattern {
            prefix,
            suffix: Some(MIGRATION_FILE_SUFFIX.to_string()),
            excludes,
            span: expr.span,
            mechanism: ModuleLoadMechanism::EsModule,
            imported_name: ImportedName::Namespace,
        });
    }

    /// Whether `callee` is the `FileMigrationProvider` of a Kysely import:
    /// a named import, or a member of a namespace import.
    fn is_file_migration_provider(&self, callee: &Expression<'_>) -> bool {
        let (local_name, via_namespace) = match callee.get_inner_expression() {
            Expression::Identifier(ident) => (ident.name.as_str(), false),
            Expression::StaticMemberExpression(member)
                if member.property.name == FILE_MIGRATION_PROVIDER =>
            {
                let Expression::Identifier(object) = &member.object else {
                    return false;
                };
                (object.name.as_str(), true)
            }
            _ => return false,
        };
        self.imports.iter().any(|import| {
            FILE_MIGRATION_PROVIDER_SOURCES.contains(&import.source.as_str())
                && import.local_name == local_name
                && if via_namespace {
                    matches!(import.imported_name, ImportedName::Namespace)
                } else {
                    matches!(
                        &import.imported_name,
                        ImportedName::Named(name) if name == FILE_MIGRATION_PROVIDER
                    )
                }
        })
    }

    /// Record a join on the module directory that names a script file.
    pub(super) fn try_record_module_path_reference(&mut self, call: &CallExpression<'_>) {
        if let Some(source) = self
            .module_relative_join(call)
            .and_then(|path| path.script_specifier())
        {
            self.push_path_reference(source, call.span, PathConsumer::Unknown);
        }
    }

    /// Record the module that `import(pathToFileURL(<join>).href)`,
    /// `import(<join>)` or `require(<join>)` loads, where `<join>` is a join
    /// on the module directory. The reference credits every export.
    pub(in crate::visitor) fn record_import_of_module_path(&mut self, source: &Expression<'_>) {
        let Some(call) = file_url_path_argument(source) else {
            return;
        };
        if let Some(specifier) = self
            .module_relative_join(call)
            .and_then(|path| path.script_specifier())
        {
            self.push_path_reference(specifier, call.span, PathConsumer::Import);
        }
    }

    /// Record `require(path.join(__dirname, "tasks.js"))`.
    pub(super) fn try_record_require_of_module_path(&mut self, call: &CallExpression<'_>) {
        let is_require =
            matches!(&call.callee, Expression::Identifier(callee) if callee.name == "require");
        if is_require
            && call.arguments.len() == 1
            && let Some(source) = call.arguments.first().and_then(Argument::as_expression)
        {
            self.record_import_of_module_path(source);
        }
    }

    /// Record the script files in the argument array of a process call that
    /// sets `cwd` to a path below the module directory:
    /// `spawn(process.execPath, ["scripts/run.ts"], { cwd: REPO_ROOT })`.
    pub(super) fn try_record_process_script_arguments(&mut self, call: &CallExpression<'_>) {
        let Some(Argument::ArrayExpression(arguments)) = call.arguments.get(1) else {
            return;
        };
        if !matches!(self.command_api_of(&call.callee), Some(CommandApi::Program)) {
            return;
        }
        let Some(cwd) = call
            .arguments
            .iter()
            .skip(2)
            .find_map(cwd_option)
            .and_then(|cwd| self.module_relative_path(cwd))
        else {
            return;
        };
        for element in &arguments.elements {
            let ArrayExpressionElement::StringLiteral(lit) = element else {
                continue;
            };
            let mut path = cwd.clone();
            if !path.push(&lit.value.text()) {
                continue;
            }
            if let Some(source) = path.script_specifier() {
                self.push_path_reference(source, lit.span, PathConsumer::Unknown);
            }
        }
    }

    /// Add a speculative path reference, unless an enclosing expression
    /// already recorded the same target, as
    /// `fork(path.join(__dirname, "w.js"))` does.
    fn push_path_reference(&mut self, source: String, span: Span, consumer: PathConsumer) {
        let recorded = self.dynamic_imports.iter().any(|import| {
            import.source == source
                && import.span.start <= span.start
                && span.end <= import.span.end
        });
        if recorded {
            return;
        }
        let loads_module = matches!(consumer, PathConsumer::Import);
        self.dynamic_imports.push(DynamicImportInfo {
            source,
            span,
            destructured_names: Vec::new(),
            local_name: loads_module.then(String::new),
            is_speculative: true,
            value_escapes: false,
        });
        if !loads_module {
            self.mark_import_load_kind(span, ImportLoadKind::PathReference);
        }
    }

    /// Remove the path reference to `source`. A `fork` of a `const` path
    /// replaces the reference that the declaration of the `const` recorded.
    pub(super) fn drop_path_reference(&mut self, source: &str) {
        let marks = &mut self.import_load_kind_marks;
        self.dynamic_imports.retain(|import| {
            let is_path_reference = import.is_speculative
                && import.local_name.is_none()
                && import.source == source
                && marks.iter().any(|(span, kind)| {
                    *span == import.span && *kind == ImportLoadKind::PathReference
                });
            if is_path_reference {
                marks.retain(|(span, _)| *span != import.span);
            }
            !is_path_reference
        });
    }
}
