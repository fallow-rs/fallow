//! Literal commands of process-spawning calls, for the `unlisted-binaries`
//! detector.
//!
//! The extractor records a call only when its callee is an import or a
//! `require()` binding of a known process API, or the `Bun` global. A binding
//! that a nested scope shadows is not a process API.

#[allow(clippy::wildcard_imports, reason = "many process call AST types used")]
use oxc_ast::ast::*;

use crate::js_text::JsText;
use fallow_types::extract::CommandInvocation;

use super::is_child_process_source;
use crate::ImportedName;
use crate::visitor::ModuleInfoExtractor;

/// The kind of process API that a local binding names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandApi {
    /// The first argument names a program (`spawn`, `execFile`, `execa`).
    Program,
    /// The first argument is shell text (`exec`, `execSync`, `execaCommand`).
    Shell,
    /// A template tag that runs shell-like text (`$` of `zx`, `execa`, `bun`).
    Template,
    /// The `Bun.spawn` form, which takes an argument array.
    BunSpawn,
    /// A `child_process` namespace whose members are process APIs.
    ChildProcessNamespace,
}

/// The global that exposes the Bun process APIs.
const BUN_GLOBAL: &str = "Bun";

/// The process API that the export `name` of the module `source` is.
fn command_api_export(source: &str, name: &str) -> Option<CommandApi> {
    if is_child_process_source(source) {
        return child_process_member(name);
    }
    match (source, name) {
        ("execa", "execa" | "execaSync") => Some(CommandApi::Program),
        ("execa", "execaCommand" | "execaCommandSync") => Some(CommandApi::Shell),
        ("execa" | "zx" | "bun", "$") => Some(CommandApi::Template),
        _ => None,
    }
}

/// The process API that a member of the `child_process` namespace is.
fn child_process_member(name: &str) -> Option<CommandApi> {
    match name {
        "spawn" | "spawnSync" | "execFile" | "execFileSync" => Some(CommandApi::Program),
        "exec" | "execSync" => Some(CommandApi::Shell),
        _ => None,
    }
}

/// The static text of a string literal or of a template without
/// expressions, and the start offset of the literal.
fn static_string(expr: &Expression<'_>) -> Option<(String, u32)> {
    match expr {
        Expression::StringLiteral(lit) => Some((lit.value.text().to_string(), lit.span.start)),
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
            template_head(tpl).map(|head| (head, tpl.span.start))
        }
        Expression::ParenthesizedExpression(paren) => static_string(&paren.expression),
        Expression::TSAsExpression(ts_as) => static_string(&ts_as.expression),
        _ => None,
    }
}

/// The static text of a template up to its first expression.
fn template_head(tpl: &TemplateLiteral<'_>) -> Option<String> {
    let quasi = tpl.quasis.first()?;
    let text = quasi.value.cooked.map_or_else(
        || quasi.value.raw.as_str().to_string(),
        |cooked| cooked.text().to_string(),
    );
    Some(text)
}

impl ModuleInfoExtractor {
    /// The process API that a callee or a template tag names.
    fn command_api_of(&self, callee: &Expression<'_>) -> Option<CommandApi> {
        match callee {
            Expression::Identifier(ident) => self.bound_command_api(ident.name.as_str()),
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(object) = &member.object else {
                    return None;
                };
                let property = member.property.name.as_str();
                if object.name == BUN_GLOBAL
                    && !self.nested_scope_shadows(BUN_GLOBAL)
                    && !self
                        .imports
                        .iter()
                        .any(|import| import.local_name == BUN_GLOBAL)
                {
                    return match property {
                        "spawn" | "spawnSync" => Some(CommandApi::BunSpawn),
                        "$" => Some(CommandApi::Template),
                        _ => None,
                    };
                }
                match self.bound_command_api(object.name.as_str())? {
                    CommandApi::ChildProcessNamespace => child_process_member(property),
                    _ => None,
                }
            }
            Expression::ParenthesizedExpression(paren) => self.command_api_of(&paren.expression),
            _ => None,
        }
    }

    /// The process API that the module-scope binding `name` names: an import
    /// or a `require()` of a process module that no nested scope shadows.
    fn bound_command_api(&self, name: &str) -> Option<CommandApi> {
        if self.nested_scope_shadows(name) {
            return None;
        }
        let imported = self.imports.iter().find_map(|import| {
            if import.local_name != name || import.is_type_only {
                return None;
            }
            match &import.imported_name {
                ImportedName::Named(imported) => command_api_export(&import.source, imported),
                ImportedName::Default | ImportedName::Namespace => {
                    is_child_process_source(&import.source)
                        .then_some(CommandApi::ChildProcessNamespace)
                }
                ImportedName::SideEffect => None,
            }
        });
        imported.or_else(|| {
            self.require_calls.iter().find_map(|require| {
                if require.local_name.as_deref() == Some(name) {
                    return is_child_process_source(&require.source)
                        .then_some(CommandApi::ChildProcessNamespace);
                }
                require
                    .destructured_names
                    .iter()
                    .any(|destructured| destructured == name)
                    .then(|| command_api_export(&require.source, name))
                    .flatten()
            })
        })
    }

    /// Record the literal command of a process-spawning call.
    pub(super) fn try_record_command_call(&mut self, expr: &CallExpression<'_>) {
        let Some(api) = self.command_api_of(&expr.callee) else {
            return;
        };
        let Some(first) = expr.arguments.first().and_then(Argument::as_expression) else {
            return;
        };
        let invocation = match api {
            // `$("cmd", args)` of execa runs a program like `execa("cmd", args)`.
            CommandApi::Program | CommandApi::Template => static_string(first)
                .map(|(command, span_start)| program_invocation(command, span_start)),
            CommandApi::Shell => {
                static_string(first).map(|(command, span_start)| CommandInvocation {
                    command,
                    shell: true,
                    span_start,
                })
            }
            CommandApi::BunSpawn => bun_spawn_program(first),
            CommandApi::ChildProcessNamespace => None,
        };
        if let Some(invocation) = invocation.filter(|i| !i.command.trim().is_empty()) {
            self.command_invocations.push(invocation);
        }
    }

    /// Record the static head of a process template (`` $`cmd ${arg}` ``).
    pub(in crate::visitor) fn try_record_command_template(
        &mut self,
        expr: &TaggedTemplateExpression<'_>,
    ) {
        let tag_api = match &expr.tag {
            // `$({ cwd })`cmd`` configures the tag before the template runs.
            Expression::CallExpression(call) => self.command_api_of(&call.callee),
            tag => self.command_api_of(tag),
        };
        if !matches!(tag_api, Some(CommandApi::Template | CommandApi::Program)) {
            return;
        }
        let Some(head) = template_head(&expr.quasi) else {
            return;
        };
        // `` $`tool${suffix}` `` builds the program name at runtime, so a head
        // that ends inside the first word names no binary.
        let ends_inside_first_word =
            !expr.quasi.expressions.is_empty() && !head.trim_start().contains(char::is_whitespace);
        if head.trim().is_empty() || ends_inside_first_word {
            return;
        }
        self.command_invocations.push(CommandInvocation {
            command: head,
            shell: true,
            span_start: expr.quasi.span.start,
        });
    }
}

/// A program call. A program name with whitespace runs through a shell
/// (`spawn("git status", { shell: true })`), so the shell parser reads it.
fn program_invocation(command: String, span_start: u32) -> CommandInvocation {
    let shell = command.trim().contains(char::is_whitespace);
    CommandInvocation {
        command,
        shell,
        span_start,
    }
}

/// The program of `Bun.spawn(["cmd", ...])` or `Bun.spawn({ cmd: ["cmd"] })`.
fn bun_spawn_program(first: &Expression<'_>) -> Option<CommandInvocation> {
    let array = match first {
        Expression::ArrayExpression(array) => array,
        Expression::ObjectExpression(object) => object.properties.iter().find_map(|prop| {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                return None;
            };
            if prop.key.static_name().is_none_or(|name| name != "cmd") {
                return None;
            }
            match &prop.value {
                Expression::ArrayExpression(array) => Some(array),
                _ => None,
            }
        })?,
        _ => return None,
    };
    let element = array.elements.first()?.as_expression()?;
    let (command, span_start) = static_string(element)?;
    Some(CommandInvocation {
        command,
        shell: false,
        span_start,
    })
}
