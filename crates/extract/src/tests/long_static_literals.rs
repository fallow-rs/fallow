//! A static string longer than the limit of a static sink literal stays a
//! static value.
//!
//! Extraction keeps the text of a static string binding up to a fixed limit.
//! A longer binding has no known text, but it is still static: a sink that
//! reads it must not see a dynamic argument. Each test extracts the same
//! source with a short value and with a value past the limit, and compares
//! the sink sites without their literal text.

use std::fmt::Write as _;

use fallow_types::extract::ModuleInfo;

use super::parse_at_path;

/// Past the limit of a static sink literal (16 KiB).
const LONG: usize = 18 * 1024;

/// An SVG sprite of about `bytes` bytes.
fn sprite(bytes: usize) -> String {
    let mut sprite = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
    let mut index = 0;
    while sprite.len() < bytes {
        let _ = write!(
            sprite,
            "<symbol id=\"icon-{index}\" viewBox=\"0 0 24 24\"><path d=\"M0 0h24v24H0z\"/></symbol>"
        );
        index += 1;
    }
    sprite.push_str("</svg>");
    sprite
}

/// `value` as a double-quoted JavaScript string literal.
fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The sink sites of `info` without their literal text.
fn sink_shapes(info: &ModuleInfo) -> Vec<String> {
    let mut shapes: Vec<String> = info
        .security_sinks
        .iter()
        .map(|sink| {
            format!(
                "{} #{} non_literal={} kind={:?}",
                sink.callee_path, sink.arg_index, sink.arg_is_non_literal, sink.arg_kind
            )
        })
        .collect();
    shapes.sort();
    shapes
}

/// Extract `source(value)` at `path` with a short value and with a value
/// past the limit, and check that both give the same sink sites.
fn assert_long_value_stays_static(path: &str, source: impl Fn(&str) -> String) {
    let short = parse_at_path(path, &source(&sprite(64)));
    let long = parse_at_path(path, &source(&sprite(LONG)));
    assert_eq!(
        sink_shapes(&long),
        sink_shapes(&short),
        "a long static value must give the sink sites of a short one"
    );
}

#[test]
fn long_static_binding_passed_to_insert_adjacent_html_is_not_dynamic() {
    assert_long_value_stays_static("a.ts", |value| {
        format!(
            "const SPRITE = {};\n\
             document.body.insertAdjacentHTML(\"afterbegin\", SPRITE);\n",
            quoted(value)
        )
    });
}

#[test]
fn long_static_template_assigned_to_inner_html_is_not_dynamic() {
    assert_long_value_stays_static("a.ts", |value| {
        format!(
            "const TEMPLATE = `{value}`;\n\
             const WRAPPED = `<div>${{TEMPLATE}}</div>`;\n\
             const el = document.createElement(\"div\");\n\
             el.innerHTML = TEMPLATE;\n\
             el.innerHTML = WRAPPED;\n\
             el.innerHTML = String(TEMPLATE);\n"
        )
    });
}

#[test]
fn long_static_binding_passed_to_exec_is_not_dynamic() {
    assert_long_value_stays_static("a.ts", |value| {
        format!(
            "import {{ exec }} from \"node:child_process\";\n\
             const SCRIPT = {};\n\
             exec(SCRIPT);\n",
            quoted(&format!("echo '{value}'"))
        )
    });
}

#[test]
fn long_static_enum_member_passed_to_a_sink_is_not_dynamic() {
    assert_long_value_stays_static("a.ts", |value| {
        format!(
            "enum Markup {{ Sprite = {} }}\n\
             document.body.insertAdjacentHTML(\"afterbegin\", Markup.Sprite);\n",
            quoted(value)
        )
    });
}

#[test]
fn long_static_script_binding_in_markup_is_not_dynamic() {
    assert_long_value_stays_static("a.vue", |value| {
        format!(
            "<script setup>\nconst SPRITE = {};\n</script>\n\
             <template>\n<div v-html=\"SPRITE\"></div>\n\
             <button @click=\"document.body.insertAdjacentHTML('afterbegin', SPRITE)\">x</button>\n\
             </template>\n",
            quoted(value)
        )
    });
}

#[test]
fn dynamic_import_specifier_keeps_its_full_text_near_and_past_the_limit() {
    for length in [16 * 1024 - 16, 16 * 1024, LONG] {
        let specifier = format!("./{}.js", "a".repeat(length));
        let info = parse_at_path(
            "a.ts",
            &format!(
                "const SPECIFIER = \"{specifier}\";\nexport const load = () => import(SPECIFIER);\n"
            ),
        );
        let sources: Vec<&str> = info
            .dynamic_imports
            .iter()
            .map(|import| import.source.as_str())
            .collect();
        assert_eq!(sources, [specifier.as_str()], "specifier of {length} bytes");
    }
}
