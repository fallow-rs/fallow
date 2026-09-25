//! Module Federation runtime calls: `registerRemotes`, `loadRemote`, `init`
//! and `createInstance`.
//!
//! The Federation runtime can register a remote container and load a module
//! from it at run time, with no static config to read. This pass reads a call
//! only when its argument is a static literal, the same rule the config
//! readers use. A call with any other argument is recorded with no remote, so
//! the analysis can say that part of the file was not read. `init` and
//! `createInstance` declare remotes under the `remotes` key of their options,
//! in the same form as the `registerRemotes` argument. The `loadRemote` and
//! `registerRemotes` methods of the instance they return are read too.
//!
//! A call counts only when the file imports the function from a Federation
//! runtime package, by name or through a namespace import. A same-named local
//! function registers nothing. In a Vue or Svelte file, the `<script>` blocks
//! are read together, so an import in one block covers a call in another.

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, ArrayExpressionElement, BindingPattern, CallExpression, Expression,
    ImportDeclarationSpecifier, ObjectPropertyKind, Program, PropertyKey, Statement,
    VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashMap;

use fallow_types::extract::{FederationRuntimeCall, FederationRuntimeRemoteFact, SemanticFact};

/// Packages whose runtime API registers and loads remotes.
const RUNTIME_PACKAGES: &[&str] = &[
    "@module-federation/runtime",
    "@module-federation/enhanced/runtime",
];

/// Properties of a `registerRemotes` element that name the remote. `alias`
/// is an import prefix of its own, so each literal one is read.
const REMOTE_NAME_KEYS: &[&str] = &["name", "alias"];

/// Read the Federation runtime calls of a JavaScript, TypeScript, Vue or
/// Svelte file.
///
/// Returns an empty list without parsing when the source does not name a
/// runtime package, which is the case for almost every file.
#[must_use]
pub fn extract_federation_runtime_facts(path: &Path, source: &str) -> Vec<SemanticFact> {
    if !RUNTIME_PACKAGES
        .iter()
        .any(|package| source.contains(package))
    {
        return Vec::new();
    }
    let scripts = script_sources(path, source);
    let allocators: Vec<Allocator> = scripts.iter().map(|_| Allocator::default()).collect();
    let programs: Vec<Program<'_>> = scripts
        .iter()
        .zip(&allocators)
        .map(|((text, source_type), allocator)| {
            Parser::new(allocator, text, *source_type).parse().program
        })
        .collect();
    let mut bindings = RuntimeBindings::default();
    for program in &programs {
        bindings.collect(program);
    }
    if bindings.is_empty() {
        return Vec::new();
    }
    for program in &programs {
        let mut instances = InstanceCollector {
            bindings: &bindings,
            instances: Vec::new(),
        };
        instances.visit_program(program);
        bindings.instances.extend(instances.instances);
    }
    let mut collector = RuntimeCallCollector {
        bindings: &bindings,
        facts: Vec::new(),
    };
    for program in &programs {
        collector.visit_program(program);
    }
    collector.facts
}

/// The script text of a file with its source type: each inline `<script>`
/// block of a Vue or Svelte file, or the whole file otherwise.
fn script_sources<'s>(
    path: &Path,
    source: &'s str,
) -> Vec<(std::borrow::Cow<'s, str>, SourceType)> {
    let is_sfc = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "vue" | "svelte"));
    if !is_sfc {
        let source_type = SourceType::from_path(path).unwrap_or_default();
        return vec![(std::borrow::Cow::Borrowed(source), source_type)];
    }
    crate::sfc::extract_sfc_scripts(source)
        .into_iter()
        .filter(|script| script.src.is_none())
        .map(|script| {
            let source_type = crate::sfc::source_type_for_script(&script);
            (std::borrow::Cow::Owned(script.body), source_type)
        })
        .collect()
}

/// The local names under which a file imports the runtime functions.
#[derive(Default)]
struct RuntimeBindings {
    /// Named imports: local name to runtime function.
    functions: FxHashMap<String, FederationRuntimeCall>,
    /// Local names of namespace imports of a runtime package.
    namespaces: Vec<String>,
    /// Local names that hold the instance an `init` or `createInstance` call
    /// returns.
    instances: Vec<String>,
}

impl RuntimeBindings {
    /// Take the runtime imports of one program.
    fn collect(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            let Statement::ImportDeclaration(import) = statement else {
                continue;
            };
            if import.import_kind.is_type()
                || !RUNTIME_PACKAGES.contains(&import.source.value.as_str())
            {
                continue;
            }
            for specifier in import.specifiers.iter().flatten() {
                match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(named) => {
                        if named.import_kind.is_type() {
                            continue;
                        }
                        if let Some(call) = runtime_call(named.imported.name().as_str()) {
                            self.functions.insert(named.local.name.to_string(), call);
                        }
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                        self.namespaces.push(namespace.local.name.to_string());
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {}
                }
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.namespaces.is_empty()
    }

    /// The runtime function a callee names, if any.
    fn call_for(&self, callee: &Expression<'_>) -> Option<FederationRuntimeCall> {
        match callee.without_parentheses() {
            Expression::Identifier(identifier) => {
                self.functions.get(identifier.name.as_str()).copied()
            }
            Expression::StaticMemberExpression(member) => match &member.object {
                Expression::Identifier(object)
                    if self
                        .namespaces
                        .iter()
                        .any(|name| name == object.name.as_str()) =>
                {
                    runtime_call(member.property.name.as_str())
                }
                Expression::Identifier(object)
                    if self
                        .instances
                        .iter()
                        .any(|name| name == object.name.as_str()) =>
                {
                    runtime_call(member.property.name.as_str())
                        .filter(|call| !declares_options(*call))
                }
                _ => None,
            },
            _ => None,
        }
    }
}

fn runtime_call(name: &str) -> Option<FederationRuntimeCall> {
    match name {
        "registerRemotes" => Some(FederationRuntimeCall::RegisterRemotes),
        "loadRemote" => Some(FederationRuntimeCall::LoadRemote),
        "init" => Some(FederationRuntimeCall::Init),
        "createInstance" => Some(FederationRuntimeCall::CreateInstance),
        _ => None,
    }
}

/// Whether a runtime call takes options that declare `remotes`, and returns
/// an instance.
const fn declares_options(call: FederationRuntimeCall) -> bool {
    matches!(
        call,
        FederationRuntimeCall::Init | FederationRuntimeCall::CreateInstance
    )
}

/// The local names that a runtime `init` or `createInstance` call is bound
/// to, such as `const mf = createInstance({ ... })`.
struct InstanceCollector<'b> {
    bindings: &'b RuntimeBindings,
    instances: Vec<String>,
}

impl<'a> Visit<'a> for InstanceCollector<'_> {
    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if let (
            BindingPattern::BindingIdentifier(identifier),
            Some(Expression::CallExpression(call)),
        ) = (
            &declarator.id,
            declarator
                .init
                .as_ref()
                .map(Expression::without_parentheses),
        ) && self
            .bindings
            .call_for(&call.callee)
            .is_some_and(declares_options)
        {
            self.instances.push(identifier.name.to_string());
        }
        walk::walk_variable_declarator(self, declarator);
    }
}

struct RuntimeCallCollector<'b> {
    bindings: &'b RuntimeBindings,
    facts: Vec<SemanticFact>,
}

impl RuntimeCallCollector<'_> {
    fn push(&mut self, call: FederationRuntimeCall, remote: Option<String>) {
        let fact =
            SemanticFact::FederationRuntimeRemote(FederationRuntimeRemoteFact { call, remote });
        if !self.facts.contains(&fact) {
            self.facts.push(fact);
        }
    }

    fn read_call(&mut self, call: FederationRuntimeCall, arguments: &[Argument<'_>]) {
        let remotes = arguments
            .first()
            .and_then(Argument::as_expression)
            .and_then(|argument| match call {
                FederationRuntimeCall::RegisterRemotes => registered_remotes(argument),
                FederationRuntimeCall::LoadRemote => {
                    loaded_remote(argument).map(|remote| vec![remote])
                }
                FederationRuntimeCall::Init | FederationRuntimeCall::CreateInstance => {
                    option_remotes(argument)
                }
            });
        match remotes {
            Some(remotes) => {
                for remote in remotes {
                    self.push(call, Some(remote));
                }
            }
            None => self.push(call, None),
        }
    }
}

impl<'a> Visit<'a> for RuntimeCallCollector<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Some(runtime) = self.bindings.call_for(&call.callee) {
            self.read_call(runtime, &call.arguments);
        }
        walk::walk_call_expression(self, call);
    }
}

/// The remote names of a `registerRemotes` array, or `None` when any part of
/// the array is not a static literal.
fn registered_remotes(argument: &Expression<'_>) -> Option<Vec<String>> {
    let Expression::ArrayExpression(array) = argument.without_parentheses() else {
        return None;
    };
    let mut remotes = Vec::new();
    for element in &array.elements {
        let ArrayExpressionElement::ObjectExpression(object) = element else {
            return None;
        };
        let mut named = false;
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                return None;
            };
            let Some(key) = static_key(&property.key) else {
                continue;
            };
            if !REMOTE_NAME_KEYS.contains(&key) {
                continue;
            }
            let remote = static_string(&property.value).filter(|name| is_remote_name(name))?;
            named |= key == "name";
            remotes.push(remote);
        }
        if !named {
            return None;
        }
    }
    Some(remotes)
}

/// The remote names that the options of an `init` or `createInstance` call
/// declare under `remotes`, or `None` when the options are not a static
/// object literal or `remotes` is not a static literal array. Options with no
/// `remotes` key declare no remote.
fn option_remotes(argument: &Expression<'_>) -> Option<Vec<String>> {
    let Expression::ObjectExpression(options) = argument.without_parentheses() else {
        return None;
    };
    let mut remotes = Vec::new();
    for property in &options.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            return None;
        };
        if static_key(&property.key) == Some("remotes") {
            remotes = registered_remotes(&property.value)?;
        }
    }
    Some(remotes)
}

/// The remote alias of a `loadRemote` request, or `None` when the request is
/// not a static literal.
fn loaded_remote(argument: &Expression<'_>) -> Option<String> {
    let request = static_string(argument)?;
    let alias = remote_alias(&request);
    is_remote_name(alias).then(|| alias.to_string())
}

/// The remote part of a `remote/module` request. A scoped remote name keeps
/// its first two segments, the same split a package specifier uses.
fn remote_alias(request: &str) -> &str {
    let segments = if request.starts_with('@') { 2 } else { 1 };
    match request.match_indices('/').nth(segments - 1) {
        Some((index, _)) => &request[..index],
        None => request,
    }
}

/// Whether a name can be imported as a bare specifier, which is the only form
/// a provider rule can cover.
fn is_remote_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.starts_with('/')
        && !name.contains(':')
        && !name.contains('\\')
        && !name.chars().any(char::is_whitespace)
}

fn static_key<'k>(key: &'k PropertyKey<'_>) -> Option<&'k str> {
    match key {
        PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.as_str()),
        PropertyKey::StringLiteral(literal) => Some(literal.value.as_str()),
        _ => None,
    }
}

fn static_string(expr: &Expression<'_>) -> Option<String> {
    match expr.without_parentheses() {
        Expression::StringLiteral(literal) => Some(literal.value.to_string()),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => template
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked.as_ref())
            .map(ToString::to_string),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(source: &str) -> Vec<(FederationRuntimeCall, Option<String>)> {
        extract_federation_runtime_facts(Path::new("src/index.ts"), source)
            .into_iter()
            .filter_map(|fact| match fact {
                SemanticFact::FederationRuntimeRemote(fact) => Some((fact.call, fact.remote)),
                _ => None,
            })
            .collect()
    }

    fn remote(call: FederationRuntimeCall, name: &str) -> (FederationRuntimeCall, Option<String>) {
        (call, Some(name.to_string()))
    }

    #[test]
    fn literal_calls_name_their_remotes() {
        let found = facts(
            r"
            import { registerRemotes, loadRemote } from '@module-federation/runtime';
            registerRemotes([
                { name: 'checkout', entry: 'https://example.test/mf.js' },
                { name: 'cart', alias: 'basket', entry: `https://example.test/cart.js` },
            ]);
            loadRemote('checkout/Button');
            loadRemote(`@scope/remote/Widget`);
            ",
        );
        assert_eq!(
            found,
            vec![
                remote(FederationRuntimeCall::RegisterRemotes, "checkout"),
                remote(FederationRuntimeCall::RegisterRemotes, "cart"),
                remote(FederationRuntimeCall::RegisterRemotes, "basket"),
                remote(FederationRuntimeCall::LoadRemote, "checkout"),
                remote(FederationRuntimeCall::LoadRemote, "@scope/remote"),
            ]
        );
    }

    #[test]
    fn a_dynamic_argument_names_no_remote() {
        let found = facts(
            r"
            import * as mf from '@module-federation/enhanced/runtime';
            mf.registerRemotes(remotes);
            mf.registerRemotes([{ name: remoteName, entry }]);
            mf.registerRemotes([{ entry: 'https://example.test/mf.js' }]);
            mf.registerRemotes([...more]);
            mf.loadRemote(`${scope}/Button`);
            mf.loadRemote(id);
            mf.loadRemote();
            ",
        );
        assert_eq!(
            found,
            vec![
                (FederationRuntimeCall::RegisterRemotes, None),
                (FederationRuntimeCall::LoadRemote, None),
            ]
        );
    }

    #[test]
    fn a_call_without_a_runtime_import_is_ignored() {
        assert!(
            facts(
                r"
                import { registerRemotes } from './local';
                import type { loadRemote } from '@module-federation/runtime';
                registerRemotes([{ name: 'checkout', entry: 'x' }]);
                loadRemote('checkout/Button');
                "
            )
            .is_empty()
        );
        assert!(
            facts(
                r"
                import { loadRemote } from '@module-federation/runtime-core';
                loadRemote('checkout/Button');
                "
            )
            .is_empty()
        );
    }

    #[test]
    fn a_renamed_import_is_followed() {
        assert_eq!(
            facts(
                r"
                import { loadRemote as load } from '@module-federation/runtime';
                load('checkout/Button');
                "
            ),
            vec![remote(FederationRuntimeCall::LoadRemote, "checkout")]
        );
    }

    fn facts_at(path: &str, source: &str) -> Vec<(FederationRuntimeCall, Option<String>)> {
        extract_federation_runtime_facts(Path::new(path), source)
            .into_iter()
            .filter_map(|fact| match fact {
                SemanticFact::FederationRuntimeRemote(fact) => Some((fact.call, fact.remote)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn init_and_create_instance_register_their_literal_remotes() {
        assert_eq!(
            facts(
                r"
                import { init, createInstance } from '@module-federation/enhanced/runtime';
                init({ name: 'host', remotes: [{ name: 'checkout', entry: 'https://example.test/mf.js' }] });
                const mf = createInstance({ name: 'host', remotes: [{ name: 'cart', alias: 'basket', entry: 'x' }] });
                mf.loadRemote('search/Box');
                mf.registerRemotes([{ name: 'profile', entry: 'y' }]);
                init({ name: 'plain' });
                "
            ),
            vec![
                remote(FederationRuntimeCall::Init, "checkout"),
                remote(FederationRuntimeCall::CreateInstance, "cart"),
                remote(FederationRuntimeCall::CreateInstance, "basket"),
                remote(FederationRuntimeCall::LoadRemote, "search"),
                remote(FederationRuntimeCall::RegisterRemotes, "profile"),
            ]
        );
    }

    #[test]
    fn init_with_unreadable_remotes_names_no_remote() {
        assert_eq!(
            facts(
                r"
                import { init } from '@module-federation/runtime';
                init(options);
                init({ name: 'host', remotes });
                init({ name: 'host', ...rest });
                "
            ),
            vec![(FederationRuntimeCall::Init, None)]
        );
    }

    #[test]
    fn sfc_script_blocks_are_read() {
        let vue = r#"
<template><Widget /></template>
<script setup lang="ts">
import { loadRemote } from '@module-federation/enhanced/runtime';
const Widget = defineAsyncComponent(() => loadRemote('checkout/Widget'));
</script>
"#;
        assert_eq!(
            facts_at("src/App.vue", vue),
            vec![remote(FederationRuntimeCall::LoadRemote, "checkout")]
        );
        let svelte = r"
<script>
  import { init } from '@module-federation/runtime';
  init({ name: 'host', remotes: [{ name: 'cart', entry: 'x' }] });
</script>
<main>{name}</main>
";
        assert_eq!(
            facts_at("src/App.svelte", svelte),
            vec![remote(FederationRuntimeCall::Init, "cart")]
        );
        let split = r"
<script>
import { loadRemote } from '@module-federation/runtime';
</script>
<script setup>
loadRemote('search/Box');
</script>
";
        assert_eq!(
            facts_at("src/Split.vue", split),
            vec![remote(FederationRuntimeCall::LoadRemote, "search")]
        );
        let ungated = r"
<script setup>
import { loadRemote } from './local';
loadRemote('checkout/Widget');
</script>
<!-- @module-federation/runtime -->
";
        assert!(facts_at("src/Local.vue", ungated).is_empty());
    }
}
