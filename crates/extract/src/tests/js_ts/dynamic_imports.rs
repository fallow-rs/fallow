use crate::tests::parse_ts as parse_source;

#[test]
fn extracts_template_literal_dynamic_import_pattern() {
    let info = parse_source("const m = import(`./locales/${lang}.json`);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./locales/");
    assert_eq!(
        info.dynamic_import_patterns[0].suffix,
        Some(".json".to_string())
    );
}

#[test]
fn extracts_concat_dynamic_import_pattern() {
    let info = parse_source("const m = import('./pages/' + name);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./pages/");
    assert!(info.dynamic_import_patterns[0].suffix.is_none());
}

#[test]
fn extracts_concat_with_suffix() {
    let info = parse_source("const m = import('./pages/' + name + '.tsx');");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./pages/");
    assert_eq!(
        info.dynamic_import_patterns[0].suffix,
        Some(".tsx".to_string())
    );
}

#[test]
fn no_substitution_template_treated_as_exact() {
    let info = parse_source("const m = import(`./exact-module`);");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./exact-module");
    assert!(info.dynamic_import_patterns.is_empty());
}

#[test]
fn fully_dynamic_import_still_ignored() {
    let info = parse_source("const m = import(variable);");
    assert!(info.dynamic_imports.is_empty());
    assert!(info.dynamic_import_patterns.is_empty());
}

#[test]
fn non_relative_template_ignored() {
    let info = parse_source("const m = import(`lodash/${fn}`);");
    assert!(info.dynamic_import_patterns.is_empty());
}

#[test]
fn multi_expression_template_uses_globstar() {
    let info = parse_source("const m = import(`./plugins/${cat}/${name}.js`);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./plugins/**/");
    assert_eq!(
        info.dynamic_import_patterns[0].suffix,
        Some(".js".to_string())
    );
}

#[test]
fn extracts_import_meta_glob_pattern() {
    let info = parse_source("const mods = import.meta.glob('./components/*.tsx');");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./components/*.tsx");
}

#[test]
fn extracts_import_meta_glob_array() {
    let info = parse_source("const mods = import.meta.glob(['./pages/*.ts', './layouts/*.ts']);");
    assert_eq!(info.dynamic_import_patterns.len(), 2);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./pages/*.ts");
    assert_eq!(info.dynamic_import_patterns[1].prefix, "./layouts/*.ts");
}

#[test]
fn extracts_require_context_pattern() {
    let info = parse_source("const ctx = require.context('./icons', false);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./icons/");
}

#[test]
fn extracts_require_context_recursive() {
    let info = parse_source("const ctx = require.context('./icons', true);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./icons/**/");
}

#[test]
fn vitest_mock_records_target_and_auto_mock_sibling() {
    let info = parse_source("vi.mock('./services/api');");
    assert_eq!(info.dynamic_imports.len(), 2);
    let sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|imp| imp.source.as_str())
        .collect();
    assert!(
        sources.contains(&"./services/api"),
        "target itself must be credited so vi.mock-only consumers do not flag it as unused-file, got {sources:?}"
    );
    assert!(
        sources.contains(&"./services/__mocks__/api"),
        "auto-mock sibling must still be synthesized when no factory is provided, got {sources:?}"
    );
    let target = info
        .dynamic_imports
        .iter()
        .find(|imp| imp.source == "./services/api")
        .expect("target import should be recorded");
    assert_eq!(target.local_name, None);
    let auto_mock = info
        .dynamic_imports
        .iter()
        .find(|imp| imp.source == "./services/__mocks__/api")
        .expect("auto-mock import should be recorded");
    assert_eq!(auto_mock.local_name, Some(String::new()));
    assert!(
        auto_mock.is_speculative,
        "auto-mock sibling synthesised by fallow must carry is_speculative=true so the resolver drops it silently when no __mocks__/<file> exists on disk (issue #378)"
    );
    assert!(
        !target.is_speculative,
        "vi.mock target itself is real user code and must not be marked speculative"
    );
}

#[test]
fn vitest_mock_records_target_and_auto_mock_sibling_from_import_argument() {
    let info = parse_source("vi.mock(import('./services/api'));");
    let sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|imp| imp.source.as_str())
        .collect();
    assert!(
        sources.contains(&"./services/api"),
        "target itself must be credited even when wrapped in `import(...)`, got {sources:?}"
    );
    assert!(
        sources.contains(&"./services/__mocks__/api"),
        "auto-mock sibling must still be synthesized for `vi.mock(import(...))` without a factory, got {sources:?}"
    );
    let target = info
        .dynamic_imports
        .iter()
        .find(|imp| imp.source == "./services/api")
        .expect("target import should be recorded");
    assert_eq!(target.local_name, None);
    let auto_mock = info
        .dynamic_imports
        .iter()
        .find(|imp| imp.source == "./services/__mocks__/api")
        .expect("auto-mock import should be recorded");
    assert_eq!(auto_mock.local_name, Some(String::new()));
}

#[test]
fn vitest_mock_with_factory_credits_target_only() {
    let info = parse_source("vi.mock('../../bar/foo', () => ({ x: 1 }));");
    assert_eq!(
        info.dynamic_imports.len(),
        1,
        "factory form should emit one import (the target), not two"
    );
    assert_eq!(info.dynamic_imports[0].source, "../../bar/foo");
    assert_eq!(info.dynamic_imports[0].local_name, None);
}

#[test]
fn vitest_mock_with_function_expression_factory_credits_target_only() {
    let info = parse_source("vi.mock('./pkg', function () { return { x: 1 }; });");
    let sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|imp| imp.source.as_str())
        .collect();
    assert_eq!(
        sources,
        vec!["./pkg"],
        "function-expression factory should suppress auto-mock synthesis just like arrow factory, got {sources:?}"
    );
    assert_eq!(info.dynamic_imports[0].local_name, None);
}

#[test]
fn vitest_mock_with_nested_parenthesized_factory_credits_target_only() {
    let info = parse_source("vi.mock('./pkg', (((() => ({ x: 1 })))));");
    let sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|imp| imp.source.as_str())
        .collect();
    assert_eq!(
        sources,
        vec!["./pkg"],
        "nested parenthesized arrow factory must suppress auto-mock synthesis, got {sources:?}"
    );
    assert_eq!(info.dynamic_imports[0].local_name, None);
}

#[test]
fn vitest_mock_with_options_object_still_synthesizes_auto_mock() {
    let info = parse_source("vi.mock('./services/api', { spy: true });");
    let sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|imp| imp.source.as_str())
        .collect();
    assert!(
        sources.contains(&"./services/__mocks__/api"),
        "auto-mock options form should still synthesize the __mocks__ sibling, got {sources:?}"
    );
    let target = info
        .dynamic_imports
        .iter()
        .find(|imp| imp.source == "./services/api")
        .expect("target import should be recorded");
    assert_eq!(target.local_name, None);
    let auto_mock = info
        .dynamic_imports
        .iter()
        .find(|imp| imp.source == "./services/__mocks__/api")
        .expect("auto-mock import should be recorded");
    assert_eq!(auto_mock.local_name, Some(String::new()));
}

#[test]
fn dynamic_import_await_captures_local_name() {
    let info = parse_source(
        "async function f() { const mod = await import('./service'); mod.doStuff(); }",
    );
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./service");
    assert_eq!(info.dynamic_imports[0].local_name, Some("mod".to_string()));
    assert!(info.dynamic_imports[0].destructured_names.is_empty());
}

#[test]
fn dynamic_import_without_await_captures_local_name() {
    let info = parse_source("const mod = import('./service');");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./service");
    assert_eq!(info.dynamic_imports[0].local_name, Some("mod".to_string()));
}

#[test]
fn dynamic_import_destructured_captures_names() {
    let info =
        parse_source("async function f() { const { foo, bar } = await import('./module'); }");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./module");
    assert!(info.dynamic_imports[0].local_name.is_none());
    assert_eq!(
        info.dynamic_imports[0].destructured_names,
        vec!["foo", "bar"]
    );
}

#[test]
fn dynamic_import_destructured_with_rest_is_namespace() {
    let info =
        parse_source("async function f() { const { foo, ...rest } = await import('./module'); }");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./module");
    assert!(info.dynamic_imports[0].local_name.is_none());
    assert!(info.dynamic_imports[0].destructured_names.is_empty());
}

#[test]
fn dynamic_import_side_effect_only() {
    let info = parse_source("async function f() { await import('./side-effect'); }");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./side-effect");
    assert!(info.dynamic_imports[0].local_name.is_none());
    assert!(info.dynamic_imports[0].destructured_names.is_empty());
}

#[test]
fn dynamic_import_no_duplicate_entries() {
    let info = parse_source("async function f() { const mod = await import('./service'); }");
    assert_eq!(info.dynamic_imports.len(), 1);
}

#[test]
fn conditional_dynamic_import_namespace_credits_both_branches() {
    let info = parse_source(
        "async function f() { const backend = await import(target === 'mobile' ? './android.mjs' : './web.mjs'); }",
    );
    let mut sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|imp| imp.source.as_str())
        .collect();
    sources.sort_unstable();
    assert_eq!(sources, vec!["./android.mjs", "./web.mjs"]);
    assert!(
        info.dynamic_imports
            .iter()
            .all(|imp| imp.local_name.as_deref() == Some("backend")),
        "both branches should carry the namespace binding, got {:?}",
        info.dynamic_imports
    );
}

#[test]
fn conditional_dynamic_import_destructure_credits_names_on_both_branches() {
    let info = parse_source(
        "async function f() { const { run } = await import(cond ? './x.mjs' : './y.mjs'); }",
    );
    assert_eq!(info.dynamic_imports.len(), 2);
    for imp in &info.dynamic_imports {
        assert!(imp.local_name.is_none());
        assert_eq!(imp.destructured_names, vec!["run"]);
    }
    let mut sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|imp| imp.source.as_str())
        .collect();
    sources.sort_unstable();
    assert_eq!(sources, vec!["./x.mjs", "./y.mjs"]);
}

#[test]
fn bare_conditional_dynamic_import_is_side_effect_on_both_branches() {
    let info = parse_source("async function f() { await import(cond ? './a.mjs' : './b.mjs'); }");
    assert_eq!(info.dynamic_imports.len(), 2);
    assert!(
        info.dynamic_imports
            .iter()
            .all(|imp| imp.local_name.is_none() && imp.destructured_names.is_empty()),
    );
}

#[test]
fn logical_fallback_dynamic_import_credits_literal_branch() {
    let info =
        parse_source("async function f() { const m = await import(override || './default.mjs'); }");
    let sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|imp| imp.source.as_str())
        .collect();
    assert_eq!(sources, vec!["./default.mjs"]);
    assert_eq!(info.dynamic_imports[0].local_name, Some("m".to_string()));
}

#[test]
fn conditional_dynamic_import_with_runtime_branch_credits_only_literal() {
    let info = parse_source(
        "async function f() { const m = await import(cond ? './real.mjs' : runtimeVar); }",
    );
    let sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|imp| imp.source.as_str())
        .collect();
    assert_eq!(
        sources,
        vec!["./real.mjs"],
        "the runtime branch is unresolvable and must be skipped, not guessed"
    );
}

#[test]
fn fully_runtime_conditional_dynamic_import_records_nothing() {
    let info = parse_source("async function f() { const m = await import(cond ? a : b); }");
    assert!(
        info.dynamic_imports.is_empty() && info.dynamic_import_patterns.is_empty(),
        "a conditional of two runtime values stays unresolvable"
    );
}

#[test]
fn conditional_arrow_wrapped_import_credits_default_on_both_branches() {
    let info = parse_source("const C = React.lazy(() => import(flag ? './A.jsx' : './B.jsx'));");
    assert_eq!(info.dynamic_imports.len(), 2);
    for imp in &info.dynamic_imports {
        assert_eq!(
            imp.destructured_names,
            vec!["default"],
            "a lazy wrapper consumes the default export, which must be credited on every branch"
        );
    }
    let mut sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|imp| imp.source.as_str())
        .collect();
    sources.sort_unstable();
    assert_eq!(sources, vec!["./A.jsx", "./B.jsx"]);
}

#[test]
fn conditional_then_callback_credits_member_on_both_branches() {
    let info = parse_source("import(cond ? './a.mjs' : './b.mjs').then(m => m.Widget);");
    assert_eq!(info.dynamic_imports.len(), 2);
    for imp in &info.dynamic_imports {
        assert_eq!(imp.destructured_names, vec!["Widget"]);
    }
}

#[test]
fn conditional_route_property_callback_credits_default_on_both_branches() {
    let info = parse_source(
        "const route = { loadComponent: () => import(mobile ? './m.component.mjs' : './w.component.mjs') };",
    );
    assert_eq!(info.dynamic_imports.len(), 2);
    for imp in &info.dynamic_imports {
        assert_eq!(imp.destructured_names, vec!["default"]);
    }
}

#[test]
fn parenthesized_bare_conditional_dynamic_import_credits_both_branches() {
    let info = parse_source("async function f() { await import((cond ? './a.mjs' : './b.mjs')); }");
    let mut sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|imp| imp.source.as_str())
        .collect();
    sources.sort_unstable();
    assert_eq!(
        sources,
        vec!["./a.mjs", "./b.mjs"],
        "a paren-wrapped conditional source must trace exactly like the bare form"
    );
}

#[test]
fn repeated_branch_literal_dedupes_to_one_edge() {
    let info = parse_source(
        "async function f() { await import(a ? './x.mjs' : b ? './x.mjs' : './y.mjs'); }",
    );
    let mut sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|imp| imp.source.as_str())
        .collect();
    sources.sort_unstable();
    assert_eq!(
        sources,
        vec!["./x.mjs", "./y.mjs"],
        "a literal repeated across branches must yield one edge, not duplicates"
    );
}

#[test]
fn require_context_with_json_regex() {
    let info = parse_source(r"const ctx = require.context('./locale', false, /\.json$/);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./locale/");
    assert_eq!(
        info.dynamic_import_patterns[0].suffix,
        Some(".json".to_string())
    );
}

#[test]
fn dynamic_import_concat_prefix_only() {
    let info = parse_source("const m = import('./pages/' + name);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./pages/");
    assert!(
        info.dynamic_import_patterns[0].suffix.is_none(),
        "Concat with only prefix and variable should have no suffix"
    );
}

#[test]
fn dynamic_import_concat_prefix_and_suffix() {
    let info = parse_source("const m = import('./views/' + name + '.vue');");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./views/");
    assert_eq!(
        info.dynamic_import_patterns[0].suffix,
        Some(".vue".to_string())
    );
}

#[test]
fn arrow_wrapped_import_expression_body() {
    let info = parse_source("const Foo = React.lazy(() => import('./Foo'));");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./Foo");
    assert_eq!(info.dynamic_imports[0].destructured_names, vec!["default"]);
}

#[test]
fn arrow_wrapped_import_block_body() {
    let info = parse_source("const Foo = lazy(() => { return import('./Foo'); });");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./Foo");
    assert_eq!(info.dynamic_imports[0].destructured_names, vec!["default"]);
}

#[test]
fn arrow_wrapped_import_function_expression() {
    let info = parse_source("const Foo = loadable(function() { return import('./Foo'); });");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./Foo");
    assert_eq!(info.dynamic_imports[0].destructured_names, vec!["default"]);
}

#[test]
fn arrow_wrapped_import_vue_define_async() {
    let info = parse_source("const Comp = defineAsyncComponent(() => import('./MyComp'));");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./MyComp");
    assert_eq!(info.dynamic_imports[0].destructured_names, vec!["default"]);
}

#[test]
fn arrow_wrapped_import_no_duplicate() {
    let info = parse_source("React.lazy(() => import('./Foo'));");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].destructured_names, vec!["default"]);
}

#[test]
fn non_import_arrow_not_extracted() {
    let info = parse_source("const result = someFunc(() => doSomething());");
    assert_eq!(info.dynamic_imports.len(), 0);
}

#[test]
fn arrow_wrapped_import_second_argument() {
    let info = parse_source("const Foo = createLazy(config, () => import('./Foo'));");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./Foo");
    assert_eq!(info.dynamic_imports[0].destructured_names, vec!["default"]);
}

#[test]
fn arrow_wrapped_import_async_arrow() {
    let info = parse_source("const Foo = lazy(async () => import('./Foo'));");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./Foo");
}

#[test]
fn arrow_wrapped_import_with_non_import_first_arg() {
    let info = parse_source("const Foo = wrapper('options', () => import('./Foo'));");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./Foo");
    assert_eq!(info.dynamic_imports[0].destructured_names, vec!["default"]);
}

#[test]
fn arrow_wrapped_template_literal_source() {
    let info = parse_source("const Foo = lazy(() => import(`./pages/${name}`));");
    assert_eq!(info.dynamic_imports.len(), 0);
    assert_eq!(info.dynamic_import_patterns.len(), 1);
}

#[test]
fn then_callback_expression_body_member_access() {
    let info = parse_source("import('./dashboard').then(m => m.DashboardComponent);");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./dashboard");
    assert_eq!(
        info.dynamic_imports[0].destructured_names,
        vec!["DashboardComponent"]
    );
    assert!(info.dynamic_imports[0].local_name.is_none());
}

#[test]
fn then_callback_destructured_param() {
    let info = parse_source("import('./lib').then(({ foo, bar }) => { console.log(foo, bar); });");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./lib");
    assert_eq!(
        info.dynamic_imports[0].destructured_names,
        vec!["foo", "bar"]
    );
    assert!(info.dynamic_imports[0].local_name.is_none());
}

#[test]
fn then_callback_namespace_block_body() {
    let info = parse_source("import('./service').then(m => { m.doStuff(); m.doMore(); });");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./service");
    assert!(info.dynamic_imports[0].destructured_names.is_empty());
    assert_eq!(info.dynamic_imports[0].local_name, Some("m".to_string()));
}

#[test]
fn then_callback_angular_routes_pattern() {
    let info = parse_source(
        r"
        const routes = [
            {
                path: 'dashboard',
                loadComponent: () => import('./dashboard.component').then(m => m.DashboardComponent),
            },
            {
                path: 'settings',
                loadComponent: () => import('./settings.component').then(m => m.SettingsComponent),
            },
        ];
        ",
    );
    assert_eq!(info.dynamic_imports.len(), 2);
    assert_eq!(info.dynamic_imports[0].source, "./dashboard.component");
    assert_eq!(
        info.dynamic_imports[0].destructured_names,
        vec!["DashboardComponent"]
    );
    assert_eq!(info.dynamic_imports[1].source, "./settings.component");
    assert_eq!(
        info.dynamic_imports[1].destructured_names,
        vec!["SettingsComponent"]
    );
}

#[test]
fn then_callback_object_literal_body() {
    let info = parse_source(
        "const Comp = React.lazy(() => import('./Foo').then(m => ({ default: m.FooComponent })));",
    );
    assert!(
        info.dynamic_imports
            .iter()
            .any(|d| d.source == "./Foo"
                && d.destructured_names.contains(&"FooComponent".to_string()))
    );
}

#[test]
fn then_callback_no_duplicate_side_effect() {
    let info = parse_source("import('./lib').then(m => m.foo);");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].destructured_names, vec!["foo"]);
}

#[test]
fn then_callback_function_expression() {
    let info = parse_source("import('./lib').then(function(m) { return m.foo; });");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./lib");
    assert_eq!(info.dynamic_imports[0].local_name, Some("m".to_string()));
}

#[test]
fn then_callback_destructured_with_rest_is_namespace() {
    let info = parse_source("import('./lib').then(({ foo, ...rest }) => { });");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert!(info.dynamic_imports[0].destructured_names.is_empty());
    assert!(info.dynamic_imports[0].local_name.is_none());
}

#[test]
fn then_callback_non_import_callee_ignored() {
    let info = parse_source("fetch('/api').then(r => r.json());");
    assert!(info.dynamic_imports.is_empty());
}

fn dynamic_sources(source: &str) -> Vec<String> {
    parse_source(source)
        .dynamic_imports
        .into_iter()
        .map(|import| import.source)
        .collect()
}

#[test]
fn child_process_fork_named_import_credits_literal_runner() {
    let sources = dynamic_sources(
        "import { fork } from 'node:child_process';\n\
         fork('./direct-runner.js');",
    );

    assert!(sources.contains(&"./direct-runner.js".to_string()));
}

#[test]
fn child_process_fork_namespace_import_credits_literal_runner() {
    let sources = dynamic_sources(
        "import * as childProcess from 'child_process';\n\
         childProcess.fork('./direct-runner.js');",
    );

    assert!(sources.contains(&"./direct-runner.js".to_string()));
}

#[test]
fn child_process_fork_destructured_require_credits_literal_runner() {
    let sources = dynamic_sources(
        "const { fork } = require('child_process');\n\
         fork('./direct-runner.js');",
    );

    assert!(sources.contains(&"./direct-runner.js".to_string()));
}

#[test]
fn child_process_fork_namespace_require_credits_literal_runner() {
    let sources = dynamic_sources(
        "const childProcess = require('node:child_process');\n\
         childProcess.fork('./direct-runner.js');",
    );

    assert!(sources.contains(&"./direct-runner.js".to_string()));
}

#[test]
fn child_process_fork_credits_static_path_resolve_runner_binding() {
    let sources = dynamic_sources(
        "import path from 'node:path';\n\
         import { fork } from 'node:child_process';\n\
         import { fileURLToPath } from 'node:url';\n\
         const filename = fileURLToPath(import.meta.url);\n\
         const runner = path.resolve(filename, '../runner.js');\n\
         fork(runner);",
    );

    assert!(sources.contains(&"./runner.js".to_string()));
}

#[test]
fn child_process_fork_credits_static_runner_inside_top_level_block() {
    let sources = dynamic_sources(
        "import path from 'node:path';\n\
         import { fork } from 'node:child_process';\n\
         import { fileURLToPath } from 'node:url';\n\
         const filename = fileURLToPath(import.meta.url);\n\
         const runner = path.resolve(filename, '../runner.js');\n\
         for (const branch of requested_branches) {\n\
           fork(runner, [], { stdio: 'inherit' });\n\
         }",
    );

    assert!(sources.contains(&"./runner.js".to_string()));
}

#[test]
fn child_process_fork_credits_static_runner_inside_callback() {
    let sources = dynamic_sources(
        "import path from 'node:path';\n\
         import { fork } from 'node:child_process';\n\
         import { fileURLToPath } from 'node:url';\n\
         const filename = fileURLToPath(import.meta.url);\n\
         const runner = path.resolve(filename, '../runner.js');\n\
         await new Promise((fulfil, reject) => {\n\
           const child = fork(runner, [], { stdio: 'inherit' });\n\
           child.on('message', fulfil);\n\
           child.on('error', reject);\n\
         });",
    );

    assert!(sources.contains(&"./runner.js".to_string()));
}

#[test]
fn child_process_fork_credits_new_url_runner_binding() {
    let sources = dynamic_sources(
        "import { fork } from 'node:child_process';\n\
         const runner = new URL('./runner.js', import.meta.url);\n\
         fork(runner);",
    );

    assert!(sources.contains(&"./runner.js".to_string()));
}

#[test]
fn unrelated_fork_call_is_not_credited() {
    let sources = dynamic_sources(
        "import { fork } from './process-utils';\n\
         fork('./runner.js');",
    );

    assert!(!sources.contains(&"./runner.js".to_string()));
}

#[test]
fn shadowed_fork_call_is_not_credited() {
    let sources = dynamic_sources(
        "import { fork } from 'node:child_process';\n\
         function run(fork) { fork('./runner.js'); }",
    );

    assert!(!sources.contains(&"./runner.js".to_string()));
}

#[test]
fn shadowed_runner_binding_is_not_credited() {
    let sources = dynamic_sources(
        "import path from 'node:path';\n\
         import { fork } from 'node:child_process';\n\
         import { fileURLToPath } from 'node:url';\n\
         const filename = fileURLToPath(import.meta.url);\n\
         const runner = path.resolve(filename, '../runner.js');\n\
         { const runner = './other.js'; fork(runner); }",
    );

    assert!(!sources.contains(&"./runner.js".to_string()));
    assert!(!sources.contains(&"./other.js".to_string()));
}

#[test]
fn shadowed_fork_callback_runner_parameter_is_not_credited() {
    let sources = dynamic_sources(
        "import path from 'node:path';\n\
         import { fork } from 'node:child_process';\n\
         import { fileURLToPath } from 'node:url';\n\
         const filename = fileURLToPath(import.meta.url);\n\
         const runner = path.resolve(filename, '../runner.js');\n\
         function run(runner) { fork(runner); }",
    );

    assert!(!sources.contains(&"./runner.js".to_string()));
}

#[test]
fn unresolved_or_computed_fork_targets_are_not_credited() {
    let sources = dynamic_sources(
        "import { fork } from 'node:child_process';\n\
         fork(runner);\n\
         fork(`./${name}.js`);\n\
         fork('worker');",
    );

    assert!(sources.is_empty());
}

#[test]
fn node_module_register_named_import_credits_loader() {
    let info = parse_source(
        "import { register } from 'node:module';\n\
         import { pathToFileURL } from 'node:url';\n\
         register('@swc-node/register/esm', pathToFileURL('./'));",
    );
    let loader = info
        .dynamic_imports
        .iter()
        .find(|imp| imp.source == "@swc-node/register/esm")
        .expect("register('@swc-node/register/esm') should record a dynamic import");
    assert!(loader.local_name.is_none());
    assert!(loader.destructured_names.is_empty());
}

#[test]
fn node_module_register_aliased_named_import_credits_loader() {
    let info = parse_source(
        "import { register as registerLoader } from 'node:module';\n\
         registerLoader('tsx/esm', import.meta.url);",
    );
    assert!(
        info.dynamic_imports
            .iter()
            .any(|imp| imp.source == "tsx/esm"),
        "alias `register as registerLoader` should still credit the loader"
    );
}

#[test]
fn node_module_register_namespace_import_credits_loader() {
    let info = parse_source(
        "import * as Module from 'node:module';\n\
         Module.register('@swc-node/register/esm', import.meta.url);",
    );
    assert!(
        info.dynamic_imports
            .iter()
            .any(|imp| imp.source == "@swc-node/register/esm"),
        "`Module.register(...)` via namespace import should credit the loader"
    );
}

#[test]
fn node_module_register_unprefixed_module_specifier_supported() {
    let info = parse_source(
        "import { register } from 'module';\n\
         register('tsx/esm', import.meta.url);",
    );
    assert!(
        info.dynamic_imports
            .iter()
            .any(|imp| imp.source == "tsx/esm")
    );
}

#[test]
fn unrelated_register_call_not_credited() {
    let info = parse_source(
        "import { register } from './service-locator';\n\
         register('not-a-loader', config);",
    );
    assert!(
        info.dynamic_imports.is_empty(),
        "register() from a non-`node:module` import should not record a dynamic import"
    );
}

#[test]
fn node_module_register_non_string_first_argument_ignored() {
    let info = parse_source(
        "import { register } from 'node:module';\n\
         register(loaderUrl, import.meta.url);",
    );
    assert!(info.dynamic_imports.is_empty());
}

#[test]
fn node_module_register_new_url_binding_credits_loader_hook_exports() {
    let info = parse_source(
        "import { register } from 'node:module';\n\
         const url = new URL('./hooks/json-loader.ts', import.meta.url);\n\
         register(url);",
    );
    let loader = info
        .dynamic_imports
        .iter()
        .find(|imp| {
            imp.source == "./hooks/json-loader.ts"
                && imp.destructured_names.iter().any(|name| name == "load")
        })
        .expect("register(url) should credit loader hook exports");

    assert!(loader.destructured_names.contains(&"resolve".to_string()));
    assert!(
        loader
            .destructured_names
            .contains(&"initialize".to_string())
    );
}

#[test]
fn node_module_register_relative_string_credits_loader_hook_exports() {
    let info = parse_source(
        "import { register } from 'node:module';\n\
         register('./hooks/json-loader.ts', import.meta.url);",
    );

    assert!(
        info.dynamic_imports.iter().any(|imp| {
            imp.source == "./hooks/json-loader.ts"
                && ["initialize", "resolve", "load", "globalPreload"]
                    .iter()
                    .all(|name| imp.destructured_names.contains(&(*name).to_string()))
        }),
        "relative module.register specifiers should credit Node loader hooks"
    );
}

#[test]
fn node_module_register_conditional_url_binding_credits_both_loader_targets() {
    let info = parse_source(
        "import { register } from 'node:module';\n\
         const srcUrl = new URL('./hooks/src-loader.ts', import.meta.url);\n\
         const distUrl = new URL('./hooks/dist-loader.ts', import.meta.url);\n\
         const url = process.env.SRC ? srcUrl : distUrl;\n\
         register(url);",
    );

    for expected in ["./hooks/src-loader.ts", "./hooks/dist-loader.ts"] {
        assert!(
            info.dynamic_imports.iter().any(|imp| {
                imp.source == expected && imp.destructured_names.contains(&"load".to_string())
            }),
            "{expected} should be credited as a registered Node loader target"
        );
    }
}

#[test]
fn node_module_register_url_bindings_accumulate_across_shadowing() {
    let info = parse_source(
        "import { register } from 'node:module';\n\
         const url = new URL('./hooks/top-loader.ts', import.meta.url);\n\
         if (enabled) {\n\
           const url = new URL('./hooks/block-loader.ts', import.meta.url);\n\
           register(url);\n\
         }\n\
         register(url);",
    );

    for expected in ["./hooks/top-loader.ts", "./hooks/block-loader.ts"] {
        assert!(
            info.dynamic_imports.iter().any(|imp| {
                imp.source == expected && imp.destructured_names.contains(&"load".to_string())
            }),
            "{expected} should be credited with loader hook exports"
        );
    }
}

#[test]
fn node_module_register_credits_legacy_loader_hook_exports() {
    let info = parse_source(
        "import { register } from 'node:module';\n\
         register('./hooks/json-loader.ts', import.meta.url);",
    );
    let loader = info
        .dynamic_imports
        .iter()
        .find(|imp| imp.source == "./hooks/json-loader.ts" && !imp.destructured_names.is_empty())
        .expect("relative module.register specifier should credit loader hook exports");

    for legacy in ["getFormat", "getSource", "transformSource"] {
        assert!(
            loader.destructured_names.contains(&legacy.to_string()),
            "{legacy} legacy hook should be credited so loader files that still \
             export it survive `unused-export` detection. destructured_names={:?}",
            loader.destructured_names
        );
    }

    assert!(
        !loader
            .destructured_names
            .contains(&"getGlobalPreload".to_string()),
        "getGlobalPreload is not a hook name; do not credit it"
    );
}

#[test]
fn node_module_register_template_literal_specifier_supported() {
    let info = parse_source(
        "import { register } from 'node:module';\n\
         register(`tsx/esm`, import.meta.url);",
    );
    assert!(
        info.dynamic_imports
            .iter()
            .any(|imp| imp.source == "tsx/esm")
    );
}

// Issue #840: new URL("./dir", import.meta.url) for a directory target must not
// produce an unresolved-import finding. Directory-pointing specifiers (no file
// extension) are marked speculative so the resolver drops them silently when no
// module can be found. File-pointing specifiers (with an extension) keep
// is_speculative = false so genuinely missing files are still reported.

#[test]
fn new_url_extensionless_specifier_is_speculative() {
    let info = parse_source("const dir = new URL('./services', import.meta.url);");
    let imp = info
        .dynamic_imports
        .iter()
        .find(|i| i.source == "./services")
        .expect("new URL('./services', ...) should emit a dynamic import");
    assert!(
        imp.is_speculative,
        "extensionless new URL specifier must be marked speculative so a \
         directory target is silently dropped rather than reported unresolved"
    );
}

#[test]
fn new_url_extensioned_specifier_is_not_speculative() {
    let info = parse_source("const w = new URL('./worker.js', import.meta.url);");
    let imp = info
        .dynamic_imports
        .iter()
        .find(|i| i.source == "./worker.js")
        .expect("new URL('./worker.js', ...) should emit a dynamic import");
    assert!(
        !imp.is_speculative,
        "file-extension new URL specifier must NOT be marked speculative so \
         a genuinely missing file is still reported as unresolved-import"
    );
}

#[test]
fn new_url_parent_relative_extensionless_specifier_is_speculative() {
    let info = parse_source("const dir = new URL('../bin', import.meta.url);");
    let imp = info
        .dynamic_imports
        .iter()
        .find(|i| i.source == "../bin")
        .expect("new URL('../bin', ...) should emit a dynamic import");
    assert!(
        imp.is_speculative,
        "parent-relative extensionless new URL specifier must be marked speculative"
    );
}
