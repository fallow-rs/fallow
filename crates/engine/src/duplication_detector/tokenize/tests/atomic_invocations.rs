use super::*;

/// True when an atomic invocation span covers the first occurrence of `marker`.
fn marker_is_atomic(code: &str, marker: &str) -> bool {
    let offset = u32::try_from(code.find(marker).expect("marker in code")).unwrap();
    let path = PathBuf::from("test.ts");
    tokenize_file(&path, code, false)
        .atomic_invocation_spans
        .iter()
        .any(|span| span.start <= offset && offset < span.end)
}

#[test]
fn call_with_data_arguments_is_atomic() {
    let code = "createWidget(project.id, { source: 'settings', reason: MARKER });";
    assert!(marker_is_atomic(code, "MARKER"));
}

#[test]
fn call_with_callback_argument_is_not_atomic() {
    let code = "router.get('/a', (ctx) => { return ctx.input + MARKER; });";
    assert!(!marker_is_atomic(code, "MARKER"));
}

#[test]
fn call_with_object_method_argument_is_not_atomic() {
    let code = "export default defineComponent({\n  name: 'a',\n  computed: {\n    total(): number {\n      return this.subtotal + MARKER;\n    },\n  },\n});";
    assert!(!marker_is_atomic(code, "MARKER"));
}

#[test]
fn call_with_arrow_property_in_nested_object_is_not_atomic() {
    let code = "configure({ handlers: { onSave: (value) => value + MARKER } });";
    assert!(!marker_is_atomic(code, "MARKER"));
}

#[test]
fn immediately_invoked_function_is_not_atomic() {
    let code = "(function () { const value = MARKER + 1; return value; })();";
    assert!(!marker_is_atomic(code, "MARKER"));
}

#[test]
fn returned_call_with_data_arguments_is_atomic() {
    let code = "function build() { return createWidget(project.id, MARKER); }";
    assert!(marker_is_atomic(code, "MARKER"));
}
