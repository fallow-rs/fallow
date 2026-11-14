//! The argument recording wraps of the exported functions.

#![expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]

use fallow_mutation::{MutationOptions, SourceKind, instrument};

const SOURCE: &str = "export function add(a, b) {\n  return a + b;\n}\n\
export const sub = (a, b) => a - b;\n\
function hidden(n) {\n  return n * 2;\n}\n\
export { hidden as twice };\n\
export const limit = 1 + 1;\n";

fn recorded(source: &str, ranges: Option<Vec<(u32, u32)>>) -> String {
    let options = MutationOptions {
        ranges,
        record_exports: true,
        ..MutationOptions::default()
    };
    instrument(source, "src/a.js", SourceKind::Js, &options)
        .unwrap()
        .code
}

#[test]
fn each_exported_function_with_a_mutant_gets_a_wrap_with_its_key() {
    let code = recorded(SOURCE, None);
    assert!(
        code.contains("add = __fallow_m_w(\"src/a.js#add\", add);"),
        "{code}"
    );
    assert!(
        code.contains("export const sub = __fallow_m_w(\"src/a.js#sub\", (a, b) =>"),
        "{code}"
    );
    assert!(
        code.contains("hidden = __fallow_m_w(\"src/a.js#twice\", hidden);"),
        "{code}"
    );
    assert!(
        !code.contains("#limit"),
        "a value export gets no wrap: {code}"
    );
    assert!(code.contains("function __fallow_m_w(k, f)"), "{code}");
}

#[test]
fn a_scoped_run_wraps_only_the_functions_of_its_mutants() {
    let start = u32::try_from(SOURCE.find("a - b").unwrap()).unwrap();
    let code = recorded(SOURCE, Some(vec![(start, start + 5)]));
    assert!(code.contains("\"src/a.js#sub\""), "{code}");
    assert!(!code.contains("\"src/a.js#add\""), "{code}");
}

#[test]
fn without_recording_the_output_has_no_wrap_and_the_same_ids() {
    let plain = instrument(
        SOURCE,
        "src/a.js",
        SourceKind::Js,
        &MutationOptions::default(),
    )
    .unwrap();
    assert!(!plain.code.contains("__fallow_m_w"), "{}", plain.code);
    let options = MutationOptions {
        record_exports: true,
        ..MutationOptions::default()
    };
    let wrapped = instrument(SOURCE, "src/a.js", SourceKind::Js, &options).unwrap();
    let ids = |mutants: &[fallow_mutation::Mutant]| -> Vec<String> {
        mutants.iter().map(|mutant| mutant.id.to_string()).collect()
    };
    assert_eq!(ids(&plain.mutants), ids(&wrapped.mutants));
}

#[test]
fn a_typescript_overload_and_a_declare_function_get_no_wrap() {
    let source = "export function pick(a: string): string;\nexport function pick(a: number): number;\nexport function pick(a: any): any {\n  return a + 1;\n}\nexport declare function outer(n: number): number;\n";
    let options = MutationOptions {
        record_exports: true,
        ..MutationOptions::default()
    };
    let code = instrument(source, "src/a.ts", SourceKind::Ts, &options)
        .unwrap()
        .code;
    assert_eq!(
        code.matches("__fallow_m_w(\"src/a.ts#pick\"").count(),
        1,
        "{code}"
    );
    assert!(!code.contains("#outer"), "{code}");
}

#[test]
fn a_re_export_names_no_local_and_gives_no_wrap() {
    let source = "function helper(n) {\n  return n + 1;\n}\nexport { helper } from \"./b\";\n";
    let code = recorded(source, None);
    assert!(!code.contains("#helper"), "{code}");
}

#[test]
fn a_wrap_keeps_the_lines_of_the_file() {
    let code = recorded(SOURCE, None);
    let plain = instrument(
        SOURCE,
        "src/a.js",
        SourceKind::Js,
        &MutationOptions::default(),
    )
    .unwrap()
    .code;
    assert_eq!(code.lines().count(), plain.lines().count(), "{code}");
}

#[test]
fn a_source_that_declares_the_record_function_is_refused() {
    let source = "export function __fallow_m_w(a) {\n  return a + 1;\n}\n";
    let error = instrument(
        source,
        "src/a.js",
        SourceKind::Js,
        &MutationOptions::default(),
    )
    .unwrap_err();
    assert!(format!("{error:?}").contains("__fallow_m_w"), "{error:?}");
}
