//! React / JSX structural extraction tests (Phase 0 foundation).

use fallow_types::extract::{ComponentFunctionKind, HookUseKind};

use crate::tests::{parse_ts, parse_tsx};

#[test]
fn capitalized_tag_records_render_edge() {
    let info = parse_tsx("export const App = () => <Child name=\"x\" id=\"y\" />;");
    assert_eq!(info.render_edges.len(), 1);
    let edge = &info.render_edges[0];
    assert_eq!(edge.parent_component, "App");
    assert_eq!(edge.child_component_name, "Child");
    assert_eq!(edge.attr_names, vec!["name".to_string(), "id".to_string()]);
    assert!(!edge.has_spread);
}

#[test]
fn member_expression_tag_records_render_edge() {
    let info = parse_tsx("export const App = () => <Foo.Bar value={1} />;");
    assert!(
        info.render_edges
            .iter()
            .any(|e| e.child_component_name == "Foo.Bar")
    );
}

#[test]
fn lowercase_host_element_is_not_a_render_edge() {
    let info = parse_tsx("export const App = () => <div className=\"a\"><span>hi</span></div>;");
    assert!(
        info.render_edges.is_empty(),
        "host elements must not be render edges, got {:?}",
        info.render_edges
    );
}

#[test]
fn jsx_spread_is_recorded() {
    let info = parse_tsx("export const App = (props) => <Child {...props} extra=\"z\" />;");
    let edge = &info.render_edges[0];
    assert!(edge.has_spread);
    assert_eq!(edge.attr_names, vec!["extra".to_string()]);
}

#[test]
fn host_element_wrapping_component_records_only_the_component() {
    let info = parse_tsx("export const App = () => <div><Child a=\"1\" /></div>;");
    assert_eq!(info.render_edges.len(), 1);
    assert_eq!(info.render_edges[0].child_component_name, "Child");
}

#[test]
fn arrow_component_is_identified() {
    let info = parse_tsx("export const App = () => <div />;");
    assert_eq!(info.component_functions.len(), 1);
    let component = &info.component_functions[0];
    assert_eq!(component.name, "App");
    assert_eq!(component.kind, ComponentFunctionKind::Arrow);
    assert!(component.is_exported);
}

#[test]
fn function_declaration_component_is_identified() {
    let info = parse_tsx("export function App() { return <div />; }");
    let component = &info.component_functions[0];
    assert_eq!(component.name, "App");
    assert_eq!(component.kind, ComponentFunctionKind::FnDecl);
    assert!(component.is_exported);
}

#[test]
fn non_exported_component_is_marked_not_exported() {
    let info = parse_tsx("const App = () => <div />;\nfunction render() { return App; }");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "App")
        .expect("App component");
    assert!(!component.is_exported);
}

#[test]
fn forward_ref_wrapper_is_identified() {
    let info = parse_tsx(
        "import { forwardRef } from 'react';\nexport const Input = forwardRef((props, ref) => <input ref={ref} />);",
    );
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Input")
        .expect("Input component");
    assert_eq!(component.kind, ComponentFunctionKind::ForwardRefWrapper);
}

#[test]
fn memo_wrapper_is_identified() {
    let info =
        parse_tsx("import { memo } from 'react';\nexport const Card = memo((props) => <div />);");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Card")
        .expect("Card component");
    assert_eq!(component.kind, ComponentFunctionKind::MemoWrapper);
}

#[test]
fn react_member_wrapper_is_identified() {
    let info = parse_tsx(
        "import React from 'react';\nexport const Card = React.memo((props) => <div />);",
    );
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Card")
        .expect("Card component");
    assert_eq!(component.kind, ComponentFunctionKind::MemoWrapper);
}

#[test]
fn destructured_props_are_harvested() {
    let info = parse_tsx("export const App = ({ name, count }) => <div>{name}{count}</div>;");
    let names: Vec<_> = info.react_props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"name"));
    assert!(names.contains(&"count"));
    let component = &info.component_functions[0];
    assert!(!component.has_unharvestable_props);
}

#[test]
fn renamed_destructured_prop_records_local_alias() {
    let info = parse_tsx("export const App = ({ name: label }) => <div>{label}</div>;");
    let prop = info
        .react_props
        .iter()
        .find(|p| p.name == "name")
        .expect("name prop");
    assert_eq!(prop.local, "label");
    // React has no template; the template-usage bit is always false.
    assert!(!prop.used_in_template);
}

#[test]
fn bare_props_identifier_abstains() {
    let info = parse_tsx("export const App = (props) => <div>{props.name}</div>;");
    assert!(info.react_props.is_empty());
    assert!(info.component_functions[0].has_unharvestable_props);
}

#[test]
fn rest_spread_in_props_abstains() {
    let info = parse_tsx("export const App = ({ name, ...rest }) => <div {...rest}>{name}</div>;");
    assert!(info.component_functions[0].has_unharvestable_props);
}

#[test]
fn hooks_are_recorded_with_kinds() {
    let info = parse_tsx(
        "import { useState, useEffect } from 'react';\nexport const App = () => { const [n] = useState(0); useEffect(() => {}, [n]); return <div />; };",
    );
    let kinds: Vec<_> = info.hook_uses.iter().map(|h| h.kind).collect();
    assert!(kinds.contains(&HookUseKind::UseState));
    assert!(kinds.contains(&HookUseKind::UseEffect));
}

#[test]
fn custom_hook_is_recorded() {
    let info = parse_tsx(
        "export const App = () => { const v = useCustomThing(); return <div>{v}</div>; };",
    );
    assert!(info.hook_uses.iter().any(|h| h.kind == HookUseKind::Custom));
}

#[test]
fn use_effect_dep_array_arity_is_captured_only_when_literal() {
    let info = parse_tsx(
        "import { useEffect } from 'react';\nexport const App = () => { useEffect(() => {}, [a, b]); return <div />; };",
    );
    let hook = info
        .hook_uses
        .iter()
        .find(|h| h.kind == HookUseKind::UseEffect)
        .expect("useEffect");
    assert_eq!(hook.dep_array_arity, Some(2));
}

#[test]
fn use_effect_without_dep_array_has_no_arity() {
    let info = parse_tsx(
        "import { useEffect } from 'react';\nexport const App = () => { useEffect(() => {}); return <div />; };",
    );
    let hook = info
        .hook_uses
        .iter()
        .find(|h| h.kind == HookUseKind::UseEffect)
        .expect("useEffect");
    assert_eq!(hook.dep_array_arity, None);
}

#[test]
fn use_state_has_no_dep_arity() {
    let info = parse_tsx(
        "import { useState } from 'react';\nexport const App = () => { const [n] = useState(0); return <div>{n}</div>; };",
    );
    let hook = info
        .hook_uses
        .iter()
        .find(|h| h.kind == HookUseKind::UseState)
        .expect("useState");
    assert_eq!(hook.dep_array_arity, None);
}

#[test]
fn non_jsx_file_is_a_no_op() {
    // A `.ts` file with no JSX must record zero React IR (perf gate + no false
    // component identification on plain TS that happens to use uppercase
    // bindings).
    let info =
        parse_ts("export const App = () => 42;\nexport function helper() { return useState; }");
    assert!(info.component_functions.is_empty());
    assert!(info.render_edges.is_empty());
    assert!(info.hook_uses.is_empty());
    assert!(info.react_props.is_empty());
}

#[test]
fn nested_render_edges_carry_correct_parent() {
    let info = parse_tsx(
        "export const Outer = () => <div><Inner /></div>;\nexport const Other = () => <Sibling />;",
    );
    let inner = info
        .render_edges
        .iter()
        .find(|e| e.child_component_name == "Inner")
        .expect("Inner edge");
    assert_eq!(inner.parent_component, "Outer");
    let sibling = info
        .render_edges
        .iter()
        .find(|e| e.child_component_name == "Sibling")
        .expect("Sibling edge");
    assert_eq!(sibling.parent_component, "Other");
}

#[test]
fn hook_outside_component_is_not_recorded() {
    // A `use*` call at module scope (not inside an identified component) is not a
    // component hook, so it must not be recorded.
    let info = parse_tsx("const x = useThing();\nexport const App = () => <div />;");
    assert!(
        info.hook_uses.is_empty(),
        "module-scope hook call should not be recorded as a component hook"
    );
}

#[test]
fn jsx_fragment_returning_arrow_is_a_component() {
    let info = parse_tsx("export const App = () => <><Child /></>;");
    assert_eq!(info.component_functions[0].name, "App");
    assert!(
        info.render_edges
            .iter()
            .any(|e| e.child_component_name == "Child")
    );
}
