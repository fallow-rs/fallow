//! React / JSX structural extraction tests (Phase 0 foundation).

use fallow_types::extract::{ComponentFunctionKind, HookUseKind};

use crate::tests::{parse_ts, parse_tsx};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn component_names(info: &crate::ModuleInfo) -> Vec<&str> {
    info.component_functions
        .iter()
        .map(|c| c.name.as_str())
        .collect()
}

fn render_child_names(info: &crate::ModuleInfo) -> Vec<&str> {
    info.render_edges
        .iter()
        .map(|e| e.child_component_name.as_str())
        .collect()
}

fn hook_kinds(info: &crate::ModuleInfo) -> Vec<HookUseKind> {
    info.hook_uses.iter().map(|h| h.kind).collect()
}

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
fn bare_props_passthrough_marks_thin_wrapper_candidate() {
    let info = parse_tsx("const App = (props) => <Child {...props} />;");
    let component = &info.component_functions[0];
    assert!(component.is_pure_passthrough);
    assert!(component.has_unharvestable_props);
    assert!(info.react_props.is_empty());
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

// ---------------------------------------------------------------------------
// Lines 523-557: classify_component_init parenthesized / TSAs / TSSatisfies
// branches, and classify_wrapper_call parenthesized-function-expression branch
// ---------------------------------------------------------------------------

#[test]
fn parenthesized_arrow_init_is_identified_as_arrow_component() {
    // The `ParenthesizedExpression` branch of `classify_component_init` recurses
    // into the inner expression; the result must still be `Arrow`.
    let info = parse_tsx("export const App = (() => <div />);");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "App")
        .expect("App should be detected through the parenthesized wrapper");
    assert_eq!(component.kind, ComponentFunctionKind::Arrow);
}

#[test]
fn ts_as_expression_init_is_identified_as_component() {
    // The `TSAsExpression` branch of `classify_component_init` strips the cast.
    let info = parse_tsx("export const App = (() => <div />) as React.FC;");
    assert!(
        component_names(&info).contains(&"App"),
        "App should be detected through TSAsExpression; got {:?}",
        component_names(&info),
    );
}

#[test]
fn ts_satisfies_expression_init_is_identified_as_component() {
    // The `TSSatisfiesExpression` branch strips the `satisfies` cast.
    let info = parse_tsx("export const App = (() => <div />) satisfies React.FC;");
    assert!(
        component_names(&info).contains(&"App"),
        "App should be detected through TSSatisfiesExpression; got {:?}",
        component_names(&info),
    );
}

#[test]
fn forward_ref_with_parenthesized_function_expression_is_classified() {
    // The `ParenthesizedExpression` branch inside `classify_wrapper_call`
    // covers `forwardRef((function(props, ref) { return <div /> }))`.
    let info = parse_tsx(
        "import { forwardRef } from 'react';\n\
         export const Input = forwardRef((function(props, ref) { return <input ref={ref} />; }));",
    );
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Input")
        .expect("Input should be detected through parenthesized function in forwardRef");
    assert_eq!(component.kind, ComponentFunctionKind::ForwardRefWrapper);
}

#[test]
fn memo_with_parenthesized_arrow_is_classified() {
    // Parenthesized arrow inside memo: `memo((props) => <div />)` where the
    // arrow is itself wrapped in parens.
    let info = parse_tsx(
        "import { memo } from 'react';\n\
         export const Card = memo(((props) => <div />));",
    );
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Card")
        .expect("Card should be detected through double-parenthesized arrow in memo");
    assert_eq!(component.kind, ComponentFunctionKind::MemoWrapper);
}

#[test]
fn function_expression_init_without_wrapper_is_identified_as_component() {
    // The `FunctionExpression` branch of `classify_component_init` (not a
    // wrapper call; uses `ComponentFunctionKind::Arrow` by convention).
    let info = parse_tsx("export const App = function() { return <div />; };");
    assert!(
        component_names(&info).contains(&"App"),
        "App defined via function expression should be detected; got {:?}",
        component_names(&info),
    );
}

#[test]
fn classify_wrapper_call_rejects_unknown_wrapper_name() {
    // A call to an unknown wrapper (`withSomething(...)`) must NOT classify as a
    // component; this keeps the component list clean.
    let info = parse_tsx("export const App = withSomething(() => <div />);");
    assert!(
        info.component_functions.is_empty(),
        "unknown wrapper must not classify as a component; got {:?}",
        info.component_functions,
    );
}

// ---------------------------------------------------------------------------
// Lines 611-643: statement_returns_jsx branches (if, block, switch, try)
// ---------------------------------------------------------------------------

#[test]
fn function_component_with_if_returning_jsx_is_identified() {
    // `statement_returns_jsx` IfStatement branch: consequent returns JSX.
    let info = parse_tsx(
        "export function App({ ok }) {\n\
           if (ok) return <span />;\n\
           return null;\n\
         }",
    );
    assert!(
        component_names(&info).contains(&"App"),
        "App returning JSX from an if-consequent should be detected; got {:?}",
        component_names(&info),
    );
}

#[test]
fn function_component_with_else_returning_jsx_is_identified() {
    // `statement_returns_jsx` IfStatement alternate branch.
    let info = parse_tsx(
        "export function App({ ok }) {\n\
           if (!ok) return null;\n\
           else return <div />;\n\
         }",
    );
    assert!(
        component_names(&info).contains(&"App"),
        "App returning JSX from an if-alternate should be detected; got {:?}",
        component_names(&info),
    );
}

#[test]
fn function_component_with_nested_block_returning_jsx_is_identified() {
    // `statement_returns_jsx` BlockStatement branch: JSX returned inside a
    // nested block (`{ { return <div />; } }`).
    let info = parse_tsx(
        "export function App() {\n\
           {\n\
             return <div />;\n\
           }\n\
         }",
    );
    assert!(
        component_names(&info).contains(&"App"),
        "App returning JSX from a nested block should be detected; got {:?}",
        component_names(&info),
    );
}

#[test]
fn function_component_with_switch_returning_jsx_is_identified() {
    // `statement_returns_jsx` SwitchStatement branch.
    let info = parse_tsx(
        "export function App({ kind }) {\n\
           switch (kind) {\n\
             case 'a': return <span />;\n\
             default: return null;\n\
           }\n\
         }",
    );
    assert!(
        component_names(&info).contains(&"App"),
        "App returning JSX from a switch case should be detected; got {:?}",
        component_names(&info),
    );
}

#[test]
fn function_component_with_try_returning_jsx_is_identified() {
    // `statement_returns_jsx` TryStatement block branch.
    let info = parse_tsx(
        "export function App() {\n\
           try {\n\
             return <div />;\n\
           } catch (e) {\n\
             return null;\n\
           }\n\
         }",
    );
    assert!(
        component_names(&info).contains(&"App"),
        "App returning JSX from a try block should be detected; got {:?}",
        component_names(&info),
    );
}

#[test]
fn function_component_with_catch_returning_jsx_is_identified() {
    // `statement_returns_jsx` TryStatement handler branch (catch clause).
    let info = parse_tsx(
        "export function App() {\n\
           try {\n\
             return null;\n\
           } catch (e) {\n\
             return <div />;\n\
           }\n\
         }",
    );
    assert!(
        component_names(&info).contains(&"App"),
        "App returning JSX from a catch clause should be detected; got {:?}",
        component_names(&info),
    );
}

#[test]
fn function_component_with_finally_returning_jsx_is_identified() {
    // `statement_returns_jsx` TryStatement finalizer branch.
    let info = parse_tsx(
        "export function App() {\n\
           try {\n\
             doSomething();\n\
           } finally {\n\
             return <div />;\n\
           }\n\
         }",
    );
    assert!(
        component_names(&info).contains(&"App"),
        "App returning JSX from a finally clause should be detected; got {:?}",
        component_names(&info),
    );
}

// ---------------------------------------------------------------------------
// Lines 630-644: is_jsx_expression branches (conditional, logical, TSNonNull)
// ---------------------------------------------------------------------------

#[test]
fn conditional_return_that_might_be_jsx_classifies_as_component() {
    // `is_jsx_expression` ConditionalExpression branch: at least one arm is JSX.
    let info = parse_tsx("export const App = ({ ok }) => ok ? <div /> : null;");
    assert!(
        component_names(&info).contains(&"App"),
        "App with conditional JSX expression should be detected; got {:?}",
        component_names(&info),
    );
}

#[test]
fn logical_and_jsx_return_classifies_as_component() {
    // `is_jsx_expression` LogicalExpression branch: the right side is JSX.
    let info = parse_tsx("export const App = ({ ok }) => ok && <div />;");
    assert!(
        component_names(&info).contains(&"App"),
        "App with logical-and JSX expression should be detected; got {:?}",
        component_names(&info),
    );
}

#[test]
fn ts_non_null_jsx_return_classifies_as_component() {
    // `is_jsx_expression` TSNonNullExpression branch.
    let info = parse_tsx("export const App = () => (<div />)!;");
    assert!(
        component_names(&info).contains(&"App"),
        "App with non-null assertion on JSX should be detected; got {:?}",
        component_names(&info),
    );
}

// ---------------------------------------------------------------------------
// Lines 893-934: unwrap_single_passthrough_element branches
// (fragment wrapping, parenthesized, TSAs, TSSatisfies, TSNonNull)
// ---------------------------------------------------------------------------

#[test]
fn fragment_wrapping_single_child_is_pure_passthrough() {
    // `unwrap_single_passthrough_element` JSXFragment branch: a fragment
    // containing exactly one JSX element child is unwrapped.
    let info = parse_tsx("const Wrap = (props) => <><Child {...props} /></>;");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Wrap")
        .expect("Wrap component");
    assert!(
        component.is_pure_passthrough,
        "fragment with single child spread must be detected as pure passthrough",
    );
}

#[test]
fn fragment_with_multiple_children_is_not_pure_passthrough() {
    // `single_element_child` returns None when there is more than one element
    // child; the passthrough mark must NOT fire.
    let info = parse_tsx("const Wrap = (props) => <><A {...props} /><B /></>;");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Wrap")
        .expect("Wrap component");
    assert!(
        !component.is_pure_passthrough,
        "fragment with multiple children must NOT be marked as pure passthrough",
    );
}

#[test]
fn passthrough_detected_through_parenthesized_element() {
    // `unwrap_single_passthrough_element` ParenthesizedExpression branch.
    let info = parse_tsx("const Wrap = (props) => (<Child {...props} />);");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Wrap")
        .expect("Wrap component");
    assert!(
        component.is_pure_passthrough,
        "parenthesized JSX spread should be detected as pure passthrough",
    );
}

#[test]
fn passthrough_detected_through_ts_as_expression() {
    // `unwrap_single_passthrough_element` TSAsExpression branch.
    let info = parse_tsx("const Wrap = (props) => (<Child {...props} />) as JSX.Element;");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Wrap")
        .expect("Wrap component");
    assert!(
        component.is_pure_passthrough,
        "TSAs-cast JSX spread should be detected as pure passthrough",
    );
}

#[test]
fn passthrough_detected_through_ts_satisfies_expression() {
    // `unwrap_single_passthrough_element` TSSatisfiesExpression branch.
    let info = parse_tsx("const Wrap = (props) => (<Child {...props} />) satisfies JSX.Element;");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Wrap")
        .expect("Wrap component");
    assert!(
        component.is_pure_passthrough,
        "satisfies-cast JSX spread should be detected as pure passthrough",
    );
}

#[test]
fn passthrough_detected_through_ts_non_null_expression() {
    // `unwrap_single_passthrough_element` TSNonNullExpression branch.
    let info = parse_tsx("const Wrap = (props) => (<Child {...props} />)!;");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Wrap")
        .expect("Wrap component");
    assert!(
        component.is_pure_passthrough,
        "non-null asserted JSX spread should be detected as pure passthrough",
    );
}

// ---------------------------------------------------------------------------
// Lines 74-84: react_enter_function with pre-registered pending arrow
// (forwardRef/memo function-expression pre-scan path)
// ---------------------------------------------------------------------------

#[test]
fn forward_ref_with_function_expression_inner_harvests_props() {
    // The inner function in forwardRef is a FunctionExpression (not an arrow).
    // `react_prescan_variable_declaration` pre-registers it; `react_enter_function`
    // consumes the pending entry and harvests props.
    let info = parse_tsx(
        "import { forwardRef } from 'react';\n\
         export const Input = forwardRef(function(props, ref) {\n\
           const { value, onChange } = props;\n\
           return <input value={value} onChange={onChange} ref={ref} />;\n\
         });",
    );
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Input")
        .expect("Input component via forwardRef function expression");
    assert_eq!(component.kind, ComponentFunctionKind::ForwardRefWrapper);
    // The function-expression inner form uses the bare-props identifier `props`,
    // so props are unharvestable (destructuring happens inside the body).
    assert!(
        component.has_unharvestable_props,
        "bare identifier props param must be unharvestable",
    );
}

// ---------------------------------------------------------------------------
// Lines 179-208 (scattered): context-provider tag and children-as-function marks
// ---------------------------------------------------------------------------

#[test]
fn provider_tag_marks_renders_provider_on_enclosing_component() {
    // `react_record_jsx_element` calls `jsx_is_provider_tag` and sets
    // `renders_provider` on the current component.
    let info = parse_tsx(
        "export const App = () => <MyContext.Provider value={42}><Child /></MyContext.Provider>;",
    );
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "App")
        .expect("App component");
    assert!(
        component.renders_provider,
        "a *.Provider child must set renders_provider on the enclosing component",
    );
}

#[test]
fn render_prop_attribute_marks_has_children_as_function() {
    // `jsx_has_function_render_prop` returns true when an attribute value is an
    // arrow function; this must set `has_children_as_function`.
    let info = parse_tsx("export const App = () => <List render={() => <Item />} />;");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "App")
        .expect("App component");
    assert!(
        component.has_children_as_function,
        "render-prop attribute must set has_children_as_function on the enclosing component",
    );
}

#[test]
fn children_as_function_in_jsx_body_marks_flag() {
    // `jsx_children_has_function` returns true when JSX children include an
    // arrow function expression container.
    let info = parse_tsx("export const App = () => <List>{() => <Item />}</List>;");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "App")
        .expect("App component");
    assert!(
        component.has_children_as_function,
        "children-as-function must set has_children_as_function on the enclosing component",
    );
}

#[test]
fn clone_element_call_marks_uses_clone_element() {
    // `is_clone_element_callee` matches bare `cloneElement`; the flag must be set.
    let info = parse_tsx(
        "export const App = ({ children }) => {\n\
           const el = cloneElement(children, { extra: true });\n\
           return <div>{el}</div>;\n\
         };",
    );
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "App")
        .expect("App component");
    assert!(
        component.uses_clone_element,
        "cloneElement call must set uses_clone_element on the enclosing component",
    );
}

#[test]
fn react_clone_element_member_call_marks_uses_clone_element() {
    // `is_clone_element_callee` also matches `React.cloneElement`.
    let info = parse_tsx(
        "export const App = ({ children }) => {\n\
           const el = React.cloneElement(children, { extra: true });\n\
           return <div>{el}</div>;\n\
         };",
    );
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "App")
        .expect("App component");
    assert!(
        component.uses_clone_element,
        "React.cloneElement call must set uses_clone_element on the enclosing component",
    );
}

// ---------------------------------------------------------------------------
// Lines 698-787 (scattered): collect_jsx_attributes - forward_attrs,
// has_complex_forward, member-expression attribute values, namespaced attrs,
// element/fragment attribute values
// ---------------------------------------------------------------------------

#[test]
fn identifier_valued_attribute_records_forward_attr() {
    // `classify_attr_value` identifier branch records `{ attr, root }` in
    // `forward_attrs`.
    let info = parse_tsx("export const App = ({ label }) => <Child text={label} />;");
    let edge = info
        .render_edges
        .iter()
        .find(|e| e.child_component_name == "Child")
        .expect("render edge to Child");
    assert!(
        edge.forward_attrs
            .iter()
            .any(|fa| fa.attr == "text" && fa.root == "label"),
        "identifier prop value must be recorded as forward_attr; got {:?}",
        edge.forward_attrs,
    );
}

#[test]
fn member_expression_attribute_value_records_forward_attr_with_root() {
    // `classify_attr_value` member-expression branch: `{x.y.z}` records root `x`.
    let info = parse_tsx("export const App = ({ data }) => <Child value={data.count} />;");
    let edge = info
        .render_edges
        .iter()
        .find(|e| e.child_component_name == "Child")
        .expect("render edge to Child");
    assert!(
        edge.forward_attrs
            .iter()
            .any(|fa| fa.attr == "value" && fa.root == "data"),
        "member-expression prop value must record root identifier as forward_attr root; got {:?}",
        edge.forward_attrs,
    );
}

#[test]
fn call_expression_attribute_value_sets_has_complex_forward() {
    // `classify_attr_value`: a call expression sets `has_complex_forward`.
    let info = parse_tsx("export const App = ({ fn }) => <Child handler={fn()} />;");
    let edge = info
        .render_edges
        .iter()
        .find(|e| e.child_component_name == "Child")
        .expect("render edge to Child");
    assert!(
        edge.has_complex_forward,
        "call expression attribute value must set has_complex_forward; got {:?}",
        edge.forward_attrs,
    );
}

#[test]
fn element_as_prop_sets_has_complex_forward() {
    // `classify_attr_value`: passing a JSX element as an attribute value
    // (`icon={<Icon />}`) is treated as complex forward.
    let info = parse_tsx("export const App = () => <Button icon={<Icon />} />;");
    let edge = info
        .render_edges
        .iter()
        .find(|e| e.child_component_name == "Button")
        .expect("render edge to Button");
    assert!(
        edge.has_complex_forward,
        "element-as-prop attribute value must set has_complex_forward",
    );
}

#[test]
fn namespaced_attribute_name_is_collected() {
    // `collect_jsx_attributes` NamespacedName branch: `xmlns:xl` -> `"xmlns:xl"`.
    let info = parse_tsx("export const App = () => <Svg xmlns:xl=\"http://example.com\" />;");
    let edge = info
        .render_edges
        .iter()
        .find(|e| e.child_component_name == "Svg")
        .expect("render edge to Svg");
    assert!(
        edge.attr_names.iter().any(|n| n == "xmlns:xl"),
        "namespaced attribute name must be collected; got {:?}",
        edge.attr_names,
    );
}

#[test]
fn jsx_member_expression_tag_flattens_to_dotted_path() {
    // `jsx_member_path` with a two-level member expression `Foo.Bar`.
    let info = parse_tsx("export const App = () => <Foo.Bar />;");
    assert!(
        render_child_names(&info).contains(&"Foo.Bar"),
        "Foo.Bar member-expression tag must produce child_component_name 'Foo.Bar'; got {:?}",
        render_child_names(&info),
    );
}

#[test]
fn jsx_deep_member_expression_tag_flattens_to_dotted_path() {
    // `jsx_member_path` recursing for three-level `A.B.C`.
    let info = parse_tsx("export const App = () => <A.B.C />;");
    assert!(
        render_child_names(&info).contains(&"A.B.C"),
        "three-level member-expression tag must produce 'A.B.C'; got {:?}",
        render_child_names(&info),
    );
}

// ---------------------------------------------------------------------------
// Lines 300-333 (scattered): prop usage tracking (used vs unused, forwarded)
// ---------------------------------------------------------------------------

#[test]
fn used_prop_sets_used_in_script_flag() {
    // A prop referenced in the component body must have `used_in_script = true`.
    let info = parse_tsx("export const App = ({ name }) => <div>{name}</div>;");
    let prop = info
        .react_props
        .iter()
        .find(|p| p.name == "name")
        .expect("name prop");
    assert!(
        prop.used_in_script,
        "prop read in body must be marked used_in_script",
    );
    assert!(
        !prop.used_in_template,
        "React has no template; used_in_template must always be false",
    );
}

#[test]
fn unused_prop_has_used_in_script_false() {
    // A declared prop that is never referenced must have `used_in_script = false`.
    let info = parse_tsx("export const App = ({ dead }) => <div />;");
    let prop = info
        .react_props
        .iter()
        .find(|p| p.name == "dead")
        .expect("dead prop");
    assert!(
        !prop.used_in_script,
        "prop never read in body must have used_in_script = false",
    );
}

#[test]
fn prop_used_only_as_forwarded_attribute_is_used_but_not_outside_forward() {
    // A prop passed unchanged to a child attribute is `used_in_script = true`
    // but `used_outside_forward = false` (a pure forward signal).
    let info = parse_tsx("export const App = ({ label }) => <Child text={label} />;");
    let prop = info
        .react_props
        .iter()
        .find(|p| p.name == "label")
        .expect("label prop");
    assert!(
        prop.used_in_script,
        "forwarded prop must count as used_in_script"
    );
    assert!(
        !prop.used_outside_forward,
        "prop used only as attribute value must NOT be used_outside_forward",
    );
}

#[test]
fn prop_used_in_text_body_is_used_outside_forward() {
    // A prop read in non-attribute context (children, local variable) sets
    // `used_outside_forward = true`.
    let info = parse_tsx("export const App = ({ title }) => <div><span>{title}</span></div>;");
    let prop = info
        .react_props
        .iter()
        .find(|p| p.name == "title")
        .expect("title prop");
    assert!(
        prop.used_in_script,
        "prop read in body must be used_in_script"
    );
    assert!(
        prop.used_outside_forward,
        "prop read in JSX children must be used_outside_forward",
    );
}

// ---------------------------------------------------------------------------
// Lines 420-457 (scattered): harvest_destructured_props edge cases
// (assignment-pattern default, string-key prop, computed key abstain)
// ---------------------------------------------------------------------------

#[test]
fn prop_with_default_value_is_harvested() {
    // `binding_pattern_local_name` AssignmentPattern branch: `{ count = 0 }`
    // should harvest `count` as a prop with local `count`.
    let info = parse_tsx("export const App = ({ count = 0 }) => <div>{count}</div>;");
    let prop = info
        .react_props
        .iter()
        .find(|p| p.name == "count")
        .expect("count prop with default");
    assert_eq!(prop.local, "count");
    assert!(prop.used_in_script);
}

#[test]
fn renamed_prop_with_default_value_is_harvested() {
    // `binding_pattern_local_name` AssignmentPattern branch on a rename:
    // `{ value: v = 0 }` harvests name=`value`, local=`v`.
    let info = parse_tsx("export const App = ({ value: v = 0 }) => <div>{v}</div>;");
    let prop = info
        .react_props
        .iter()
        .find(|p| p.name == "value")
        .expect("value prop");
    assert_eq!(prop.local, "v");
    assert!(prop.used_in_script);
}

#[test]
fn string_key_prop_is_harvested() {
    // `harvest_destructured_props` StringLiteral key branch.
    let info = parse_tsx("export const App = ({ 'data-id': dataId }) => <div id={dataId} />;");
    let prop = info
        .react_props
        .iter()
        .find(|p| p.name == "data-id")
        .expect("data-id prop");
    assert_eq!(prop.local, "dataId");
}

#[test]
fn nested_destructured_prop_abstains() {
    // `binding_pattern_local_name` returns None for ObjectPattern (nested
    // destructure); the whole component must mark has_unharvestable_props.
    let info = parse_tsx("export const App = ({ user: { name } }) => <div>{name}</div>;");
    assert!(
        info.component_functions[0].has_unharvestable_props,
        "nested destructure must mark has_unharvestable_props",
    );
    assert!(
        info.react_props.is_empty(),
        "nested destructure must yield no harvested props",
    );
}

#[test]
fn computed_key_prop_abstains() {
    // `harvest_destructured_props` returns None on a computed key; whole
    // component abstains.
    let info =
        parse_tsx("const KEY = 'x';\nexport const App = ({ [KEY]: val }) => <div>{val}</div>;");
    assert!(
        info.component_functions[0].has_unharvestable_props,
        "computed key must mark has_unharvestable_props",
    );
}

// ---------------------------------------------------------------------------
// Lines 953-1031 (scattered): hook dep-array arity for useMemo, useCallback,
// custom hooks, and hook-outside-component gate
// ---------------------------------------------------------------------------

#[test]
fn use_memo_dep_array_arity_is_captured() {
    let info = parse_tsx(
        "import { useMemo } from 'react';\n\
         export const App = () => {\n\
           const v = useMemo(() => 1, [a, b, c]);\n\
           return <div>{v}</div>;\n\
         };",
    );
    let hook = info
        .hook_uses
        .iter()
        .find(|h| h.kind == HookUseKind::UseMemo)
        .expect("useMemo hook");
    assert_eq!(
        hook.dep_array_arity,
        Some(3),
        "useMemo dep array with 3 elements should have arity 3",
    );
}

#[test]
fn use_callback_dep_array_arity_is_captured() {
    let info = parse_tsx(
        "import { useCallback } from 'react';\n\
         export const App = () => {\n\
           const fn = useCallback(() => {}, [x]);\n\
           return <div />;\n\
         };",
    );
    let hook = info
        .hook_uses
        .iter()
        .find(|h| h.kind == HookUseKind::UseCallback)
        .expect("useCallback hook");
    assert_eq!(
        hook.dep_array_arity,
        Some(1),
        "useCallback dep array with 1 element should have arity 1",
    );
}

#[test]
fn use_effect_empty_dep_array_has_arity_zero() {
    let info = parse_tsx(
        "import { useEffect } from 'react';\n\
         export const App = () => {\n\
           useEffect(() => {}, []);\n\
           return <div />;\n\
         };",
    );
    let hook = info
        .hook_uses
        .iter()
        .find(|h| h.kind == HookUseKind::UseEffect)
        .expect("useEffect hook");
    assert_eq!(
        hook.dep_array_arity,
        Some(0),
        "useEffect with empty dep array should have arity 0",
    );
}

#[test]
fn custom_hook_has_no_dep_array_arity() {
    // `hook_dep_array_arity` returns None for `HookUseKind::Custom`.
    let info = parse_tsx(
        "export const App = () => {\n\
           const v = useCustomData([a, b]);\n\
           return <div>{v}</div>;\n\
         };",
    );
    let hook = info
        .hook_uses
        .iter()
        .find(|h| h.kind == HookUseKind::Custom)
        .expect("custom hook");
    assert_eq!(
        hook.dep_array_arity, None,
        "custom hooks always have dep_array_arity = None",
    );
}

#[test]
fn is_custom_hook_name_requires_uppercase_after_use() {
    // `is_custom_hook_name`: `usefoo` (lowercase after 'use') is NOT a hook;
    // `useFoo` (uppercase) is. This is tested indirectly via a component that
    // calls both.
    let info = parse_tsx(
        "export const App = () => {\n\
           useFoo();\n\
           usefoo();\n\
           return <div />;\n\
         };",
    );
    let kinds = hook_kinds(&info);
    let custom_count = kinds.iter().filter(|&&k| k == HookUseKind::Custom).count();
    assert_eq!(
        custom_count, 1,
        "only useFoo (uppercase) should be recorded as a custom hook; got {kinds:?}",
    );
}

#[test]
fn use_memo_without_dep_array_has_no_arity() {
    // `hook_dep_array_arity` returns None when the dep-array argument is absent.
    let info = parse_tsx(
        "import { useMemo } from 'react';\n\
         export const App = () => {\n\
           const v = useMemo(() => 1);\n\
           return <div>{v}</div>;\n\
         };",
    );
    let hook = info
        .hook_uses
        .iter()
        .find(|h| h.kind == HookUseKind::UseMemo)
        .expect("useMemo hook");
    assert_eq!(
        hook.dep_array_arity, None,
        "useMemo without dep array should have arity None",
    );
}

// ---------------------------------------------------------------------------
// Lines 833-843 (scattered): passthrough_spread_root variants
// ---------------------------------------------------------------------------

#[test]
fn object_rest_spread_root_is_detected_as_passthrough_candidate() {
    // `passthrough_spread_root` ObjectPattern with rest: `{ a, ...rest }` -> `rest`.
    // Since there is a rest element, the component is unharvestable, but the
    // spread root IS extracted and used for the is_pure_passthrough test.
    let info = parse_tsx("const Wrap = ({ className, ...rest }) => <Child {...rest} />;");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Wrap")
        .expect("Wrap component");
    // has_unharvestable_props fires because of the rest element.
    assert!(
        component.has_unharvestable_props,
        "rest spread in props must mark has_unharvestable_props",
    );
    // but a bare spread-forward of `rest` still qualifies for passthrough
    assert!(
        component.is_pure_passthrough,
        "spreading the rest local into the single child must mark is_pure_passthrough",
    );
}

#[test]
fn non_identifier_child_in_passthrough_is_not_pure() {
    // `body_is_pure_passthrough` requires the spread root to match: if the
    // props root is `props` but the spread is `{...other}`, it must not fire.
    let info = parse_tsx("const Wrap = (props) => <Child {...other} />;");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Wrap")
        .expect("Wrap component");
    assert!(
        !component.is_pure_passthrough,
        "spreading a different object must not mark is_pure_passthrough",
    );
}

#[test]
fn component_with_extra_local_statement_is_not_pure_passthrough() {
    // `body_is_pure_passthrough` requires exactly one statement; a local
    // variable declaration before the return disqualifies.
    let info = parse_tsx(
        "const Wrap = (props) => {\n\
           const extra = 1;\n\
           return <Child {...props} />;\n\
         };",
    );
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Wrap")
        .expect("Wrap component");
    assert!(
        !component.is_pure_passthrough,
        "body with more than one statement must not be pure passthrough",
    );
}

#[test]
fn named_attribute_alongside_spread_is_not_pure_passthrough() {
    // `jsx_element_is_bare_props_spread` returns false when there is any
    // named attribute alongside the spread.
    let info = parse_tsx("const Wrap = (props) => <Child {...props} extra=\"x\" />;");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Wrap")
        .expect("Wrap component");
    assert!(
        !component.is_pure_passthrough,
        "named attribute alongside spread must not be pure passthrough",
    );
}

#[test]
fn host_element_child_in_passthrough_is_not_pure() {
    // `jsx_element_is_bare_props_spread` checks `jsx_component_tag_name`; a
    // host element (`div`) must NOT qualify.
    let info = parse_tsx("const Wrap = (props) => <div {...props} />;");
    let component = info
        .component_functions
        .iter()
        .find(|c| c.name == "Wrap")
        .expect("Wrap component");
    assert!(
        !component.is_pure_passthrough,
        "spread onto a host element must not be pure passthrough",
    );
}
