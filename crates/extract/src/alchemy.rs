//! Alchemy resource entries: the `main` property of a Worker or a function.
//!
//! An Alchemy resource names its bundle entry in a `main` property. Alchemy
//! bundles that file at deploy time, and the platform runtime reads its
//! exports. No module imports it. This pass reads the forms whose target is
//! certain from the source alone:
//!
//! - `new URL("./x.ts", import.meta.url)`, with an optional `.pathname` or
//!   `.href`, also inside `fileURLToPath(...)`;
//! - `` `${import.meta.dirname}/x.ts` `` and `` `${__dirname}/x.ts` ``;
//! - `path.join(import.meta.dirname, "x.ts")` and `path.resolve(...)`;
//! - a string literal next to a `rootDir` property of one of the forms
//!   above, or of `import.meta.dirname` or `__dirname`, resolved against
//!   that `rootDir`;
//! - a string literal in a stack file (`alchemy.run.*`). The CLI runs a
//!   default stack from its own directory, so the path is relative to it.
//!   A stack that a script runs from another directory resolves the literal
//!   against that directory. Then the target is missing, and the edge is
//!   dropped without a finding;
//! - `import.meta.url`, `import.meta.filename` and `import.meta.path`, which
//!   name the declaring file itself.
//!
//! A string literal in any other file is relative to the working directory
//! of the CLI, which the source does not name, so it is not read. A
//! `path.join` that is imported by name (`import { join } from "node:path"`)
//! and a string concatenation are not read either.
//!
//! The declaring file loads another target like a namespace import whose
//! whole object is used, so every value export of the target is credited
//! when the declaring file is reachable. For a declaring file that names
//! itself, the `default` export and each exported class with a platform base
//! class are credited. The platform calls the default handler and binds the
//! exported Durable Object, Workflow and Container classes by name. Both
//! credits count only while the declaring file is reachable. The pass runs
//! only on a file that imports `alchemy` or an `alchemy/*` subpath, so a
//! `main` key of an unrelated object is not read.

use std::path::Path;

use oxc_allocator::Allocator;
#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};

/// The package whose resources this pass reads.
const ALCHEMY_PACKAGE: &str = "alchemy";

/// The file stem of a default stack file.
const STACK_FILE_STEM: &str = "alchemy.run";

/// Cloudflare runtime modules whose base classes the platform binds by
/// class name: Durable Objects, Workflows, entrypoints and Containers.
const PLATFORM_CLASS_SOURCES: &[&str] = &["cloudflare:workers", "@cloudflare/containers"];

/// The bundle entries that the Alchemy resources of one file declare.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AlchemyMains {
    /// Other files, as paths relative to the directory of the declaring file
    /// (`./src/worker.ts`, `../worker.ts`), with an empty span at the end of
    /// the `main` value. The span does not start where a `new URL(...)` edge
    /// of the same value starts, so their load kinds stay apart.
    pub targets: Vec<(String, Span)>,
    /// The exports of the declaring file that the platform reads, when a
    /// `main` names the declaring file itself.
    pub own_exports: Vec<String>,
}

impl AlchemyMains {
    /// Whether the file declares no bundle entry.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty() && self.own_exports.is_empty()
    }
}

/// Read the `main` entries of the Alchemy resources in a JavaScript or
/// TypeScript file.
///
/// Returns an empty result without parsing when the source does not name the
/// package, which is the case for almost every file.
#[must_use]
pub fn extract_alchemy_mains(path: &Path, source: &str) -> AlchemyMains {
    if !source.contains(ALCHEMY_PACKAGE) {
        return AlchemyMains::default();
    }
    let Ok(source_type) = SourceType::from_path(path) else {
        return AlchemyMains::default();
    };
    let allocator = Allocator::default();
    let program = Parser::new(&allocator, source, source_type).parse().program;
    if !imports_alchemy(&program) {
        return AlchemyMains::default();
    }
    let mut collector = MainCollector {
        is_stack_file: is_stack_file(path),
        names_itself: false,
        targets: Vec::new(),
    };
    collector.visit_program(&program);
    let mut targets = collector.targets;
    targets.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.start.cmp(&b.1.start)));
    targets.dedup_by(|a, b| a.0 == b.0);
    let own_exports = if collector.names_itself {
        platform_exports(&program)
    } else {
        Vec::new()
    };
    AlchemyMains {
        targets,
        own_exports,
    }
}

/// The `default` export and the exported classes that extend a platform
/// base class. A base class counts when the root of its `extends` expression
/// is a binding imported from Alchemy or from a Cloudflare runtime module,
/// such as `Cloudflare.DurableObject<C>()(...)` or `DurableObject`. An
/// exported error class (`Schema.TaggedError<E>()(...)`) is not bound by the
/// platform, so it stays reportable.
fn platform_exports(program: &Program<'_>) -> Vec<String> {
    let platform_bindings = platform_bindings(program);
    let mut names = Vec::new();
    for statement in &program.body {
        match statement {
            Statement::ExportDefaultDeclaration(_) => names.push("default".to_owned()),
            Statement::ExportDeclaration(export) => {
                if let Declaration::ClassDeclaration(class) = &export.declaration
                    && let Some(id) = &class.id
                    && class
                        .heritage
                        .as_ref()
                        .and_then(|heritage| root_identifier(&heritage.expression))
                        .is_some_and(|root| platform_bindings.contains(&root))
                {
                    names.push(id.name.to_string());
                }
            }
            _ => {}
        }
    }
    names
}

fn imports_alchemy(program: &Program<'_>) -> bool {
    import_declarations(program).any(|import| is_alchemy_source(import.source.value.as_str()))
}

fn import_declarations<'b, 'a>(
    program: &'b Program<'a>,
) -> impl Iterator<Item = &'b ImportDeclaration<'a>> {
    program.body.iter().filter_map(|statement| match statement {
        Statement::ImportDeclaration(import) => Some(&**import),
        _ => None,
    })
}

fn is_alchemy_source(source: &str) -> bool {
    source == ALCHEMY_PACKAGE
        || source
            .strip_prefix(ALCHEMY_PACKAGE)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The local names that a program imports from a module whose classes the
/// platform binds by name.
fn platform_bindings<'a>(program: &Program<'a>) -> Vec<&'a str> {
    import_declarations(program)
        .filter(|import| {
            let source = import.source.value.as_str();
            is_alchemy_source(source) || PLATFORM_CLASS_SOURCES.contains(&source)
        })
        .flat_map(|import| import.specifiers.iter().flatten())
        .map(|specifier| specifier.local().name.as_str())
        .collect()
}

/// The identifier at the root of an `extends` expression: `Cloudflare` for
/// `Cloudflare.DurableObject<C>()("C", {})`.
fn root_identifier<'a>(expression: &Expression<'a>) -> Option<&'a str> {
    let mut current = expression.without_parentheses();
    loop {
        current = match current {
            Expression::Identifier(identifier) => return Some(identifier.name.as_str()),
            Expression::StaticMemberExpression(member) => &member.object,
            Expression::CallExpression(call) => &call.callee,
            Expression::TSInstantiationExpression(instantiation) => &instantiation.expression,
            _ => return None,
        }
        .without_parentheses();
    }
}

fn is_stack_file(path: &Path) -> bool {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem == STACK_FILE_STEM)
}

struct MainCollector {
    is_stack_file: bool,
    /// Whether a `main` names the declaring file itself.
    names_itself: bool,
    targets: Vec<(String, Span)>,
}

impl<'a> Visit<'a> for MainCollector {
    fn visit_object_expression(&mut self, object: &ObjectExpression<'a>) {
        if let Some(main) = property_value(object, "main") {
            if is_own_file(main) {
                self.names_itself = true;
            } else if let Some(path) = self.main_path(object, main) {
                let end = main.span().end;
                self.targets.push((path, Span::new(end, end)));
            }
        }
        walk::walk_object_expression(self, object);
    }
}

impl MainCollector {
    fn main_path(&self, object: &ObjectExpression<'_>, main: &Expression<'_>) -> Option<String> {
        if let Some(path) = file_relative_path(main) {
            return Some(path);
        }
        let raw = relative_target(string_value(main)?)?;
        match property_value(object, "rootDir") {
            Some(root_dir) => file_relative_dir(root_dir).map(|base| join(&base, raw)),
            None => self.is_stack_file.then(|| join(".", raw)),
        }
    }
}

/// The value of a non-computed `key: value` property of an object literal.
fn property_value<'b, 'a>(
    object: &'b ObjectExpression<'a>,
    key: &str,
) -> Option<&'b Expression<'a>> {
    object.properties.iter().find_map(|property| {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            return None;
        };
        if property.computed || property.kind != PropertyKind::Init {
            return None;
        }
        (property.key.static_name().as_deref() == Some(key)).then_some(&property.value)
    })
}

/// A string literal, or a template literal without expressions.
fn string_value<'b>(expression: &'b Expression<'_>) -> Option<&'b str> {
    match expression.without_parentheses() {
        Expression::StringLiteral(literal) => Some(literal.value.as_str()),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => template
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked.as_ref())
            .map(|cooked| cooked.as_str()),
        _ => None,
    }
}

/// A path relative to the directory of the declaring file, for an
/// expression whose value does not depend on the working directory.
fn file_relative_path(expression: &Expression<'_>) -> Option<String> {
    match expression.without_parentheses() {
        Expression::StaticMemberExpression(member)
            if matches!(member.property.name.as_str(), "pathname" | "href") =>
        {
            new_url_path(&member.object)
        }
        Expression::NewExpression(_) => new_url_path(expression),
        Expression::TemplateLiteral(template) => template_dir_path(template),
        Expression::CallExpression(call) => {
            path_join_call(call).or_else(|| file_url_to_path_call(call))
        }
        _ => None,
    }
}

/// `new URL("./x.ts", import.meta.url)`.
fn new_url_path(expression: &Expression<'_>) -> Option<String> {
    let Expression::NewExpression(new) = expression.without_parentheses() else {
        return None;
    };
    let Expression::Identifier(callee) = &new.callee else {
        return None;
    };
    let [target, base] = new.arguments.as_slice() else {
        return None;
    };
    if callee.name != "URL" || !is_file_url(base.as_expression()?) {
        return None;
    }
    relative_target(string_value(target.as_expression()?)?).map(|target| join(".", target))
}

/// `fileURLToPath(new URL("./x.ts", import.meta.url))`.
fn file_url_to_path_call(call: &CallExpression<'_>) -> Option<String> {
    let Expression::Identifier(callee) = &call.callee else {
        return None;
    };
    let [argument] = call.arguments.as_slice() else {
        return None;
    };
    (callee.name == "fileURLToPath")
        .then(|| new_url_path(argument.as_expression()?))
        .flatten()
}

/// `` `${import.meta.dirname}/x.ts` `` or `` `${__dirname}/x.ts` ``.
fn template_dir_path(template: &TemplateLiteral<'_>) -> Option<String> {
    let [expression] = template.expressions.as_slice() else {
        return None;
    };
    let [head, tail] = template.quasis.as_slice() else {
        return None;
    };
    if !is_dir_name(expression) || !head.value.raw.is_empty() {
        return None;
    }
    let target = tail.value.cooked.as_ref()?.as_str().strip_prefix('/')?;
    relative_target(target).map(|target| join(".", target))
}

/// `path.join(import.meta.dirname, "x.ts")` or `path.resolve(...)` with only
/// string segments after the directory.
fn path_join_call(call: &CallExpression<'_>) -> Option<String> {
    let Expression::StaticMemberExpression(callee) = &call.callee else {
        return None;
    };
    let Expression::Identifier(object) = &callee.object else {
        return None;
    };
    if object.name != "path" || !matches!(callee.property.name.as_str(), "join" | "resolve") {
        return None;
    }
    let (first, rest) = call.arguments.split_first()?;
    if !is_dir_name(first.as_expression()?) || rest.is_empty() {
        return None;
    }
    let mut path = String::from(".");
    for argument in rest {
        let segment = relative_target(string_value(argument.as_expression()?)?)?;
        path = join(&path, segment);
    }
    Some(path)
}

/// `import.meta.url`, `import.meta.filename` or `import.meta.path`: the
/// declaring file itself.
fn is_own_file(expression: &Expression<'_>) -> bool {
    matches!(
        import_meta_property(expression),
        Some("url" | "filename" | "path")
    )
}

/// `import.meta.url`, the URL of the current file.
fn is_file_url(expression: &Expression<'_>) -> bool {
    import_meta_property(expression) == Some("url")
}

/// `import.meta.dirname` or `__dirname`, the directory of the current file.
fn is_dir_name(expression: &Expression<'_>) -> bool {
    match expression.without_parentheses() {
        Expression::Identifier(identifier) => identifier.name == "__dirname",
        expression => import_meta_property(expression) == Some("dirname"),
    }
}

fn import_meta_property<'b>(expression: &'b Expression<'_>) -> Option<&'b str> {
    let Expression::StaticMemberExpression(member) = expression.without_parentheses() else {
        return None;
    };
    matches!(member.object, Expression::ImportMeta(_)).then_some(member.property.name.as_str())
}

/// A relative target path. An absolute path, a URL with a scheme or a
/// Windows drive names no project file in a portable way.
fn relative_target(target: &str) -> Option<&str> {
    if target.is_empty() || target.starts_with('/') || target.contains(':') {
        return None;
    }
    Some(target)
}

/// Join a relative segment onto a relative base and normalize `.` and `..`
/// segments lexically. The result keeps leading `..` segments and starts
/// with `./` when it does not climb.
fn join(base: &str, segment: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in base.split('/').chain(segment.split('/')) {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let joined = parts.join("/");
    if parts.first() == Some(&"..") {
        joined
    } else {
        format!("./{joined}")
    }
}

/// A directory relative to the declaring file: a form that
/// [`file_relative_path`] reads, `import.meta.dirname` or `__dirname`.
fn file_relative_dir(expression: &Expression<'_>) -> Option<String> {
    if is_dir_name(expression) {
        return Some(".".to_owned());
    }
    file_relative_path(expression)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mains(path: &str, source: &str) -> Vec<String> {
        extract_alchemy_mains(Path::new(path), source)
            .targets
            .into_iter()
            .map(|(path, _)| path)
            .collect()
    }

    #[test]
    fn new_url_main_forms_are_read() {
        let source = r#"
            import * as Cloudflare from "alchemy/Cloudflare";
            export const a = Cloudflare.Worker("a", { main: new URL("./src/a.ts", import.meta.url).pathname });
            export const b = Cloudflare.Worker("b", { main: new URL("./b.ts", import.meta.url).href });
            export const c = Cloudflare.Worker("c", { main: new URL("../c.ts", import.meta.url) });
            export const d = Cloudflare.Worker("d", { main: fileURLToPath(new URL("./d.ts", import.meta.url)) });
        "#;
        assert_eq!(
            mains("apps/web/infra.ts", source),
            vec!["../c.ts", "./b.ts", "./d.ts", "./src/a.ts"]
        );
    }

    #[test]
    fn dirname_main_forms_are_read() {
        let source = r#"
            import path from "node:path";
            import * as Cloudflare from "alchemy/Cloudflare";
            const a = { main: `${import.meta.dirname}/src/a.ts` };
            const b = { main: path.join(import.meta.dirname, "src", "b.ts") };
            const c = { main: path.resolve(__dirname, "../c.ts") };
        "#;
        assert_eq!(
            mains("apps/web/infra.ts", source),
            vec!["../c.ts", "./src/a.ts", "./src/b.ts"]
        );
    }

    #[test]
    fn string_main_resolves_against_a_file_relative_root_dir() {
        let source = r#"
            import * as Cloudflare from "alchemy/Cloudflare";
            const rootDir = new URL("..", import.meta.url).pathname;
            export const web = { rootDir: new URL("..", import.meta.url).pathname, main: "src/entry.ts" };
            export const unknown = { rootDir, main: "src/other.ts" };
        "#;
        assert_eq!(
            mains("apps/web/src/worker.ts", source),
            vec!["../src/entry.ts"]
        );
    }

    #[test]
    fn string_main_resolves_against_a_dirname_root_dir() {
        let source = r#"
            import * as Cloudflare from "alchemy/Cloudflare";
            export const a = { rootDir: import.meta.dirname, main: "src/a.ts" };
            export const b = { rootDir: __dirname, main: "..b/b.ts" };
        "#;
        assert_eq!(
            mains("apps/web/infra.ts", source),
            vec!["./..b/b.ts", "./src/a.ts"]
        );
    }

    #[test]
    fn string_main_is_read_only_in_a_stack_file() {
        let source = r#"
            import * as Cloudflare from "alchemy/Cloudflare";
            export const Worker = Cloudflare.Worker("Worker", { main: "./src/worker.ts" });
        "#;
        assert_eq!(mains("app/alchemy.run.ts", source), vec!["./src/worker.ts"]);
        assert!(mains("app/src/infra.ts", source).is_empty());
    }

    #[test]
    fn main_that_names_the_declaring_file_credits_default_and_classes() {
        let source = r#"
            import { DurableObject } from "cloudflare:workers";
            import * as Schema from "effect/Schema";
            import * as Cloudflare from "alchemy/Cloudflare";
            export const Cache = Cloudflare.KV.Namespace("cache");
            export class Counter extends Cloudflare.DurableObject<Counter>()("Counter", {}) {}
            export class Room extends DurableObject {}
            export class Failed extends Schema.TaggedError<Failed>()("Failed", {}) {}
            export class Plain {}
            export default class Api extends Cloudflare.Worker<Api>()("Api", { main: import.meta.url }) {}
        "#;
        let result = extract_alchemy_mains(Path::new("src/Api.ts"), source);
        assert!(result.targets.is_empty());
        assert_eq!(result.own_exports, vec!["Counter", "Room", "default"]);
    }

    #[test]
    fn file_without_an_alchemy_import_is_not_read() {
        let source = r#"
            import { something } from "alchemy-like";
            // alchemy
            export const config = { main: new URL("./src/a.ts", import.meta.url).pathname };
            export default { main: import.meta.url };
        "#;
        assert!(extract_alchemy_mains(Path::new("src/config.ts"), source).is_empty());
    }

    #[test]
    fn dynamic_and_absolute_mains_are_not_read() {
        let source = r#"
            import * as AWS from "alchemy/AWS";
            const a = { main: entry };
            const b = { main: `${dir}/src/b.ts` };
            const c = { main: new URL(name, import.meta.url).pathname };
            const d = { main: "/abs/d.ts", rootDir: new URL(".", import.meta.url).pathname };
            const e = { [key]: new URL("./e.ts", import.meta.url).pathname };
        "#;
        assert!(extract_alchemy_mains(Path::new("alchemy.run.ts"), source).is_empty());
    }
}
