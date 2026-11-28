use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

use crate::parse_source_to_module;

/// Far past the depth at which the Oxc parser overflows a 16 MiB stack.
const DEEP: usize = 20_000;

fn parse(path: &str, source: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new(path), source, 0, true)
}

fn assert_degraded(info: &ModuleInfo) {
    assert!(
        info.parse_panicked,
        "a too-deep file must be a failed parse"
    );
    assert!(info.parse_error_count > 0);
    assert!(info.exports.is_empty());
    assert!(info.imports.is_empty());
}

#[test]
fn deeply_nested_parens_degrade_instead_of_overflowing() {
    let source = format!(
        "import a from './a';\nexport const x = {}1{};\n",
        "(".repeat(DEEP),
        ")".repeat(DEEP)
    );
    let info = parse("src/index.mjs", &source);
    assert_degraded(&info);
    assert_eq!(
        info.line_offsets.len(),
        3,
        "line offsets still describe the source"
    );
}

#[test]
fn deeply_nested_arrays_in_a_glimmer_file_degrade() {
    let source = format!("export default {};\n", "[".repeat(DEEP));
    assert_degraded(&parse("src/App.gts", &source));
}

#[test]
fn deep_nesting_inside_a_vue_script_degrades() {
    let source = format!(
        "<script setup>\nconst x = {}{};\n</script>\n",
        "{a:".repeat(DEEP),
        "}".repeat(DEEP)
    );
    assert_degraded(&parse("src/App.vue", &source));
}

#[test]
fn closers_in_literals_do_not_hide_real_nesting() {
    // Each level opens a real paren and hides a closer in a literal. A plain
    // byte count sees depth 0 or 1 here.
    for unit in ["(')',", "(\")\",", "(`)`,", "(/)/,", "(/*)*/", "(//)\n"] {
        let source = format!("f{};\n", unit.repeat(DEEP));
        assert_degraded(&parse("src/index.js", &source));
    }
}

#[test]
fn template_substitutions_count_as_nesting() {
    let source = format!("const s = {}{};\n", "`${".repeat(DEEP), "}`".repeat(DEEP));
    assert_degraded(&parse("src/index.js", &source));
}

#[test]
fn ordinary_nesting_still_parses() {
    let depth = 200;
    let source = format!(
        "import a from './a';\nexport const x = {}a{};\n",
        "[".repeat(depth),
        "]".repeat(depth)
    );
    let info = parse("src/index.ts", &source);
    assert!(!info.parse_panicked);
    assert_eq!(info.parse_error_count, 0);
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.imports.len(), 1);
}

#[test]
fn brackets_inside_literals_do_not_count_as_nesting() {
    // Serialized parser tables and regexes carry many unbalanced openers in
    // strings. They are not nesting and must not degrade the file.
    let openers = "(".repeat(DEEP);
    let source = format!(
        "export const a = '{openers}';\nexport const b = \"{openers}\";\n\
         export const c = `{openers}`;\nexport const d = /\\({{3}}[(]/;\n\
         // {openers}\n/* {openers} */\nexport const e = 1 / (2) / 3;\n"
    );
    let info = parse("src/index.js", &source);
    assert!(!info.parse_panicked);
    assert_eq!(info.exports.len(), 5);
}

fn deep_custom_property() -> String {
    format!(".a{{--x:{}1{};}}\n", "(".repeat(DEEP), ")".repeat(DEEP))
}

#[test]
fn deep_css_after_a_protocol_relative_url_degrades() {
    // Minified CSS is one line. A `//` in `url()` is not a comment in CSS, so
    // the nesting after it must still count.
    let source = format!(
        ".b{{background:url(//cdn.example.com/a.png)}}{}",
        deep_custom_property()
    );
    assert_degraded(&parse("src/styles.css", &source));
}

#[test]
fn css_module_exports_skip_a_too_deep_stylesheet() {
    let source = format!(".keep{{color:red}}\n{}", deep_custom_property());
    let exports = crate::css::extract_css_module_exports(&source, false);
    assert!(
        exports
            .iter()
            .any(|export| export.name.to_string() == "keep")
    );
}

#[test]
fn css_analytics_skip_a_too_deep_stylesheet() {
    assert!(crate::css_metrics::compute_css_analytics(&deep_custom_property()).is_none());
}

/// Depth of the nested color functions. lightningcss parses the content of an
/// unclosed `light-dark(`, `rgb(` or `hsl(` twice at each level, so at this
/// depth a parse takes far longer than the time limit of the test.
const COLOR_DEPTH: usize = 40;

/// Extract the CSS Modules classes of `source` on another thread and fail when
/// the extraction does not end in time.
fn css_module_exports_in_time(source: String) -> Vec<String> {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let names = crate::css::extract_css_module_exports(&source, false)
            .iter()
            .map(|export| export.name.to_string())
            .collect::<Vec<_>>();
        let _ = done.send(names);
    });
    finished
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("CSS Modules extraction must end in time")
}

/// In CSS a `}` or `]` inside an open `(` is a plain token, so it does not end
/// the color functions that lightningcss still nests.
#[test]
fn a_brace_inside_a_color_function_does_not_close_it() {
    for repeated in ["light-dark(}", "rgb(]", "light-dark(i}\n.b{x:"] {
        let source = format!(".keep{{color:red}}\n.a{{x:{}", repeated.repeat(COLOR_DEPTH));
        let names = css_module_exports_in_time(source.clone());
        assert!(names.contains(&"keep".to_string()), "{repeated}: {names:?}");
        assert!(
            crate::parse_style_bounded(&source, || ()).is_none(),
            "{repeated} nesting must skip the parser"
        );
    }
}

/// CSS reads an unquoted `url(` body up to its first `)`, so a `{` in the body
/// does not hide the color functions after it.
#[test]
fn a_brace_in_a_url_does_not_hide_color_functions() {
    let source = format!(
        ".keep{{color:red}}\n.a{{x:url(a{{b)}}\n.b{{x:{}",
        "light-dark(".repeat(COLOR_DEPTH)
    );
    let names = css_module_exports_in_time(source.clone());
    assert!(names.contains(&"keep".to_string()), "{names:?}");
    assert!(crate::parse_style_bounded(&source, || ()).is_none());
}

/// lightningcss panics when a math function where it expects a percentage,
/// such as `opacity`, `font-stretch` or a color channel, does not reduce to
/// one percentage. The declarations have no closing `}`, as in the fuzz input,
/// because a `}` inside an open `(` is a plain token.
#[test]
fn a_math_function_that_panics_the_css_parser_skips_it() {
    for declaration in [
        "o:lab(max(4,nan",
        "color:lab(1 MAX(4%, NaN) 1)",
        "opacity:max(4%,5)",
        "opacity:clamp(1%,2,3%)",
        "opacity:calc(4% + 5)",
        "opacity:calc(max(4%,5))",
        "opacity:abs(4%)",
        "color:rgb(round(up,4%,5) 1 1)",
        "opacity:m\\61 x(4%,5)",
        "font-stretch:calc(1 / 2)",
        "-webkit-text-size-adjust:max(1, 2)",
        "color:rgb(10%, 10%, calc(1/2))",
        "--x:var(--y, rgba(1%,2%,max(1,2)))",
        "--x:max(lab(max(4%,5) 1 1))",
        "color:calc(1 + lab(1 rgb(abs(4%) 1 1) 1))",
        // The tokenizer reads escapes, `url(` tokens and bad strings, so
        // they hide no math function.
        "color:r\\67 b(max(4%,5) 1 1)",
        "b:\\75rl(x( );opacity:max(4%,5)",
        "b:\"max(\n;opacity:max(4%,5)",
    ] {
        let source = format!(".keep{{color:red}}\n.a{{{declaration}");
        let names = css_module_exports_in_time(source.clone());
        assert!(
            names.contains(&"keep".to_string()),
            "{declaration}: {names:?}"
        );
        assert!(
            crate::css_metrics::compute_css_analytics(&source).is_none(),
            "{declaration} must skip the parser"
        );
    }
}

/// A number-only math function makes lightningcss panic in a keyframe
/// selector and in the `initial-value` of a `@property`.
#[test]
fn a_number_math_function_where_a_percentage_is_required_skips_the_css_parser() {
    for source in [
        ".keep{ color: red }\n@keyframes k{ calc(1 / 2) { opacity: 1 } }",
        ".keep{ color: red }\n@property --p{ syntax: '<percentage>'; inherits: false; initial-value: calc(1 / 2) }",
    ] {
        let names = css_module_exports_in_time(source.to_string());
        assert!(names.contains(&"keep".to_string()), "{source}: {names:?}");
        assert!(
            crate::parse_style_bounded(source, || ()).is_none(),
            "{source}"
        );
    }
}

/// lightningcss parses the number first where a property takes a number or a
/// percentage, so a number-only math function there is safe.
#[test]
fn a_number_math_function_where_a_number_is_allowed_still_parses() {
    let source = ".a{z-index:calc(1 + 1);line-height:calc(3 / 2);opacity:calc(1 - 0.5)}\n\
                  .b{color:hsl(calc(200 + 10) 50% 50%);background:lab(calc(50 / 2) 1 1)}\n\
                  .c{opacity:min(1, 0.5);font-weight:clamp(100, 400, 900);aspect-ratio:calc(16 / 9)}\n\
                  :root{--text-xs--line-height:calc(1 / 0.75)}\n\
                  .d:hover{transform:scale(calc(1 + 0.1))}";
    assert!(crate::parse_style_bounded(source, || ()).is_some());
    assert!(crate::css_metrics::compute_css_analytics(source).is_some());
}

/// A hex escape takes one whitespace after it, so a newline there does not
/// end a string, and the real closing quote does not start a new one.
#[test]
fn a_newline_after_a_hex_escape_does_not_end_a_string() {
    for escape in ["\\a\r", "\\a\n", "\\a\r\n", "\\1\u{c}", "\\00000a\n"] {
        let source = format!(
            "a{{--x:\"{escape}\"{}1{}}}",
            "(".repeat(DEEP),
            ")".repeat(DEEP)
        );
        assert!(
            crate::parse_style_bounded(&source, || ()).is_none(),
            "{escape:?}"
        );
    }
}

/// A bad string ends at its newline, so a `/*` in it does not hide the
/// nesting after it from the stack estimate.
#[test]
fn a_comment_opener_in_a_bad_string_does_not_hide_nesting() {
    let source = format!("a{{--x:\"/*\n;}}\n{}", "a{".repeat(DEEP));
    assert!(crate::parse_style_bounded(&source, || ()).is_none());
    assert_degraded(&parse("src/a.css", &source));
}

/// Each level of nested math functions doubles the parse time of lightningcss
/// when the innermost value is not of the expected type.
#[test]
fn deeply_nested_math_functions_skip_the_css_parser() {
    for name in ["calc(", "abs(", "round(1,", "clamp(1,", "atan2(1px,"] {
        let depth = if name.starts_with("atan2") { 3 } else { 26 };
        let source = format!(".keep{{color:red}}\n.a{{width:{}1px", name.repeat(depth));
        let names = css_module_exports_in_time(source.clone());
        assert!(names.contains(&"keep".to_string()), "{name}: {names:?}");
        assert!(
            crate::parse_style_bounded(&source, || ()).is_none(),
            "{name} nesting must skip the parser"
        );
    }
}

#[test]
fn ordinary_math_functions_still_parse() {
    let source = ".a{width:calc(100% - 2rem);font-size:clamp(1rem, 2.5vw, 2rem)}\n\
                  .e{font-size:calc(1rem * clamp(1, 1.2, 1.5));width:calc(100% * min(1, 2))}\n\
                  .b{opacity:calc(50% * 2);width:max(10%, 5px);height:calc(var(--h) / 2)}\n\
                  .c{grid-template-columns:repeat(3, calc(100% / 3));--x:min(1px, 2px)}\n\
                  .nan{animation-name:nan;width:var(--nan, 1px)}\n\
                  @media (min-width: calc(40rem + 1px)) {.d{opacity:max(50%, 20%)}}";
    assert!(crate::parse_style_bounded(source, || ()).is_some());
    assert!(crate::css_metrics::compute_css_analytics(source).is_some());
}

/// The scan reads each escape once, so a long run of escapes stays linear.
#[test]
fn a_long_escape_chain_scans_in_time() {
    let source = format!(".keep{{color:red}}\n.a{{x:({}", "\\!".repeat(500_000));
    let names = css_module_exports_in_time(source);
    assert!(names.contains(&"keep".to_string()));
}

#[test]
fn deeply_nested_color_functions_skip_the_css_parser() {
    for name in [
        "light-dark(",
        "rgb(from red r g b / ",
        "HSL(",
        "light-dark(red, ",
    ] {
        let source = format!(".keep{{color:red}}\n.a{{x:{}]}}", name.repeat(COLOR_DEPTH));
        let names = css_module_exports_in_time(source.clone());
        assert!(names.contains(&"keep".to_string()), "{name}: {names:?}");
        assert!(
            crate::parse_style_bounded(&source, || ()).is_none(),
            "{name} nesting must skip the parser"
        );
        assert!(crate::css_metrics::compute_css_analytics(&source).is_none());
    }
}

#[test]
fn ordinary_color_function_nesting_still_parses() {
    let source = ".a{color:color-mix(in srgb, light-dark(rgb(from var(--x) r g b / 50%), \
                  hsl(0 0% 0%)), red)}\n.b{--c:light-dark(rgb(0 0 0), hsl(0 0% 100%))}";
    assert!(crate::parse_style_bounded(source, || ()).is_some());
    assert!(crate::css_metrics::compute_css_analytics(source).is_some());
}

/// Open color functions stop only the lightningcss parse. The bracket depth
/// of a stylesheet must still count every bracket after them.
#[test]
fn color_functions_do_not_hide_bracket_nesting() {
    let source = format!(".a{{x:{}{}", "rgb(".repeat(COLOR_DEPTH), "(".repeat(5000));
    assert!(crate::nesting::parse_file_bounded(Path::new("a.css"), &source, || ()).is_none());
    assert_degraded(&parse("a.css", &source));
}

#[test]
fn a_hash_or_at_url_prefix_does_not_hide_nesting() {
    for prefix in ["#url(", "@url(", "url("] {
        let source = format!(".a{{--x:{prefix}{}}}", "(".repeat(DEEP));
        assert!(
            crate::parse_style_bounded(&source, || ()).is_none(),
            "{prefix} hid the nesting"
        );
    }
}

/// The parser reads each level of `r<({[` three times: as type arguments,
/// as the parameters of an arrow function and as an expression. Seven levels
/// stay under the bracket cap, but the parse time grows exponentially.
#[test]
fn nested_type_arguments_in_parenthesized_patterns_degrade() {
    let source = format!("export const x = r{};\n", "<({[r".repeat(7));
    let info = parse("src/index.ts", &source);
    assert!(info.parse_nesting_too_deep);
    assert_degraded(&info);
}

/// Depth of the nested destructure defaults. Work that doubles at each level
/// does not end in the time limit at this depth.
const DEFAULT_DEPTH: usize = 40;

/// Extract `source` on another thread and fail when it does not end in time.
fn assert_extracts_in_time(source: String) {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let info = parse("src/index.ts", &source);
        let _ = done.send(info.parse_error_count);
    });
    let errors = finished
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("extraction must end in time");
    assert_eq!(errors, 0);
}

#[test]
fn nested_array_destructure_defaults_cost_linear_work() {
    let mut pattern = String::from("{ m }");
    for _ in 0..DEFAULT_DEPTH {
        pattern = format!("[{pattern} = [s]]");
    }
    assert_extracts_in_time(format!(
        "declare const s: any;\nconst {pattern} = [s];\nfor (const {pattern} of [[s]]) {{}}\n"
    ));
}

#[test]
fn nested_object_destructure_assignment_defaults_cost_linear_work() {
    let mut pattern = String::from("{ m }");
    for _ in 0..DEFAULT_DEPTH {
        pattern = format!("{{ k: {pattern} = s }}");
    }
    assert_extracts_in_time(format!(
        "declare const s: any;\nlet m: unknown;\n({pattern} = s);\n"
    ));
}

/// Elements before the class instance in the wide destructure sources. Far
/// past any per-walk step count, so a shared step budget loses the last one.
const WIDE_ELEMENTS: usize = 1_500;

/// Whether `info` reads `member` of the local class `Service`.
fn reads_service_member(info: &ModuleInfo, member: &str) -> bool {
    info.member_accesses
        .iter()
        .any(|access| access.object == "Service" && access.member == member)
}

#[test]
fn wide_for_of_destructure_credits_the_last_element() {
    let others = "{ viaWideForOf: () => {} }, ".repeat(WIDE_ELEMENTS);
    let source = format!(
        "export class Service {{ viaWideForOf() {{}} }}\nconst service = new Service();\n\
         for (const {{ viaWideForOf }} of [{others}service]) viaWideForOf();\n"
    );
    let info = parse("src/index.ts", &source);
    assert!(reads_service_member(&info, "viaWideForOf"));
}

#[test]
fn wide_array_destructure_credits_the_last_element() {
    let holes = ", ".repeat(WIDE_ELEMENTS);
    let others = "{}, ".repeat(WIDE_ELEMENTS);
    let wrapped = "[{}], ".repeat(WIDE_ELEMENTS);
    let source = format!(
        "export class Service {{ viaWideArray() {{}} viaWideNested() {{}} }}\n\
         const service = new Service();\n\
         const [{holes}{{ viaWideArray }}] = [{others}service];\n\
         for (const [{{ viaWideNested }}] of [{wrapped}[service]]) viaWideNested();\n\
         viaWideArray();\n"
    );
    let info = parse("src/index.ts", &source);
    assert!(reads_service_member(&info, "viaWideArray"));
    assert!(reads_service_member(&info, "viaWideNested"));
}

/// Nesting levels of the spread source.
const SPREAD_DEPTH: usize = 20;

#[test]
fn nested_spread_sources_cost_bounded_work() {
    let mut pattern = String::from("{ m }");
    let mut source = String::from("s");
    for _ in 0..SPREAD_DEPTH {
        pattern = format!("{{ k: {pattern} }}");
        source = format!("{{ ...a, ...b, k: {source} }}");
    }
    assert_extracts_in_time(format!(
        "declare const s: any, a: any, b: any;\nconst {pattern} = {source};\n"
    ));
}

/// Elements and pattern properties of the wide-by-wide destructure.
const WIDE_BY_WIDE: usize = 2_000;

#[test]
fn wide_pattern_over_wide_source_records_bounded_entries() {
    let keys = (0..WIDE_BY_WIDE)
        .map(|index| format!("m{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let elements = "service, ".repeat(WIDE_BY_WIDE);
    let source = format!(
        "export class Service {{}}\nconst service = new Service();\n\
         for (const {{ {keys} }} of [{elements}]) {{}}\n"
    );
    let info = parse("src/index.ts", &source);
    assert!(
        info.member_accesses.len() < WIDE_BY_WIDE * WIDE_BY_WIDE / 4,
        "one destructure recorded {} member accesses",
        info.member_accesses.len()
    );
}

#[test]
fn deep_inline_html_script_skips_its_body() {
    // The quotes on the first line make the code view of the whole-file
    // scan read the deep script as one string, and the JSX view reads it as
    // the text of the `div`, so only the guard of the inline parse sees the
    // depth.
    let source = format!(
        "<div><p>'</p><script type=\"module\">import \"./deep.js\"; const x = {}1{};</script><p>'</p>\n\
         <script type=\"module\">import \"./ok.js\";</script></div>\n",
        "[".repeat(DEEP),
        "]".repeat(DEEP)
    );
    let info = parse("index.html", &source);
    assert!(!info.parse_panicked, "the page itself must still parse");
    let sources: Vec<&str> = info
        .imports
        .iter()
        .map(|import| import.source.as_str())
        .collect();
    assert!(sources.contains(&"./ok.js"), "{sources:?}");
    assert!(!sources.contains(&"./deep.js"), "{sources:?}");
}

/// OSS-Fuzz test case 5735794955059200: lightningcss normalizes the hue of
/// `hwb(3e35 ...)` to a value outside one turn, and a debug assertion in
/// `cssparser-color` then fails. A hue this large is never real CSS.
#[test]
fn an_extreme_hue_skips_the_css_parser() {
    for declaration in [
        "@d hwb(3e35 3 3",
        ".a{color:hsl(3e35 50% 50%)}",
        ".a{color:hsla(calc(3e35deg) 50% 50%)}",
        ".a{color:hsl(calc(1e6 * 1e6) 50% 50%)}",
        ".a{color:hsl(calc(infinity) 50% 50%)}",
        ".a{color:hsl(calc(-infinity) 50% 50%)}",
        ".a{color:hwb(calc(1e6 * 1e6 * 1e6) 5% 5%)}",
    ] {
        let source = format!(".keep{{color:red}}\n{declaration}");
        let names = css_module_exports_in_time(source.clone());
        assert!(
            names.contains(&"keep".to_string()),
            "{declaration}: {names:?}"
        );
        assert!(
            crate::parse_style_bounded(&source, || ()).is_none(),
            "{declaration} must skip the parser"
        );
    }
    for source in [
        ".a{color:hsl(720deg 50% 50%)}",
        ".a{color:hwb(1e5 10% 10%)}",
        ".a{color:lch(50% 30 3e35)}",
        ".a{color:hsl(calc(var(--h) + 10) 50% 50%)}",
        ".a{color:hsl(calc(120deg * 2) 50% 50%)}",
    ] {
        assert!(
            crate::parse_style_bounded(source, || ()).is_some(),
            "{source}"
        );
    }
}

/// OSS-Fuzz test case 5117349517393920: lightningcss prints an empty
/// `grid-template-areas` with an indent that it never opened, and the indent
/// subtraction overflows.
#[test]
fn an_empty_grid_template_areas_value_is_not_rendered() {
    for source in [
        "r{grid-template-areas:",
        ".a{grid-template-areas: ;color:red}",
    ] {
        let analytics = crate::css_metrics::compute_css_analytics(source);
        assert!(analytics.is_some(), "{source}");
    }
}

/// Nested type-argument lists past the cap of 10 levels. Twelve levels keep
/// the parse short when a guard misses them.
fn nested_type_arguments() -> String {
    format!("{}b{}", "t<{[".repeat(12), "]".repeat(12))
}

/// A template expression is parsed on its own, so the whole-file scan, which
/// reads an attribute value as a string, does not see its nesting.
#[test]
fn nested_type_arguments_in_a_template_attribute_degrade() {
    let nested = nested_type_arguments();
    let refused = [
        (
            "src/App.vue",
            format!("<template><p v-html=\"{nested}\"></p></template>\n"),
        ),
        (
            "src/Title.vue",
            format!(
                "<script setup lang=\"ts\">\nimport {{ b }} from './b';\n</script>\n\
                 <template><p :title=\"{nested}\"></p></template>\n"
            ),
        ),
        (
            "src/Title.svelte",
            format!(
                "<script lang=\"ts\">\nimport {{ b }} from './b';\n</script>\n\
                 <p title=\"{{{nested}}}\"></p>\n"
            ),
        ),
    ]
    .into_iter()
    .map(|(path, source)| (path, parse(path, &source).parse_nesting_too_deep))
    .collect::<Vec<_>>();
    let parsed: Vec<&str> = refused
        .iter()
        .filter(|(_, too_deep)| !too_deep)
        .map(|(path, _)| *path)
        .collect();
    assert!(parsed.is_empty(), "parsed: {parsed:?}");
}

/// A backtick in an HTML comment of a component file does not hide the
/// nesting of the script block after it.
#[test]
fn a_backtick_in_markup_does_not_hide_a_nested_script_block() {
    let source = format!(
        "<template><!-- ` --><p>x</p></template>\n\
         <script setup lang=\"ts\">\nexport const x = {};\n</script>\n<!-- ` -->\n",
        nested_type_arguments()
    );
    let info = parse("src/App.vue", &source);
    assert!(info.parse_nesting_too_deep);
}

/// Real template expressions stay parsed.
#[test]
fn ordinary_template_expressions_still_parse() {
    let source = "<script setup lang=\"ts\">\nimport { format } from './format';\n</script>\n\
                  <template><p :title=\"format(a < b ? [1] : { k: [c] })\">{{ format(1) }}</p></template>\n";
    let info = parse("src/App.vue", source);
    assert!(!info.parse_nesting_too_deep);
    assert!(!info.parse_panicked);
}

/// A release-notes page whose prose holds many bare `<`. The text of a markup
/// host is not script, so the whole-file scan must not refuse it.
fn release_notes_page() -> String {
    let filler = "The layout code now keeps the column in place. ".repeat(16);
    let items = format!("<li>Fix a crash when the width is < 3 px. {filler}</li>\n").repeat(60);
    format!(
        "<!doctype html>\n<html><body><ul>\n{items}</ul>\n\
         <script type=\"module\" src=\"./main.ts\"></script></body></html>\n"
    )
}

/// An MDX page whose prose escapes many `<` as `\<`.
fn escaped_less_than_page() -> String {
    format!(
        "import {{ Note }} from './Note';\n\n<Note />\n\n{}",
        "The value \\< limit stays open in this line of prose text.\n".repeat(400)
    )
}

fn import_sources(info: &ModuleInfo) -> Vec<&str> {
    info.imports
        .iter()
        .map(|import| import.source.as_str())
        .collect()
}

#[test]
fn bare_less_than_in_markup_prose_keeps_the_page_parsed() {
    let html = parse("index.html", &release_notes_page());
    assert!(!html.parse_nesting_too_deep);
    assert!(
        import_sources(&html).contains(&"./main.ts"),
        "{:?}",
        import_sources(&html)
    );
    let mdx = parse("docs/page.mdx", &escaped_less_than_page());
    assert!(!mdx.parse_nesting_too_deep);
    assert!(
        import_sources(&mdx).contains(&"./Note"),
        "{:?}",
        import_sources(&mdx)
    );
}

/// A quote pair in HTML comments hides an Astro template expression from the
/// whole-file scan, so the expression gets its own scan.
#[test]
fn a_hidden_astro_template_expression_degrades() {
    let source = format!(
        "---\nconst b = 1;\n---\n<div><!-- ' --><p>{{{}}}</p><!-- ' --></div>\n",
        nested_type_arguments()
    );
    assert!(parse("src/pages/index.astro", &source).parse_nesting_too_deep);
}

/// The same quote pair hides an inline script of an Astro page.
#[test]
fn a_hidden_astro_inline_script_degrades() {
    let source = format!(
        "---\nconst b = 1;\n---\n<!-- ' --><script>export const x = {};</script><!-- ' -->\n",
        nested_type_arguments()
    );
    assert!(parse("src/pages/index.astro", &source).parse_nesting_too_deep);
}

/// The module federation scan parses each script block of a component on
/// its own, so each block gets its own scan.
#[test]
fn a_nested_script_block_refuses_the_federation_scan() {
    let source = format!(
        "<template><!-- ` --><p>x</p></template>\n<script setup lang=\"ts\">\n\
         import {{ init }} from '@module-federation/runtime';\nexport const x = {};\n\
         </script>\n<!-- ` -->\n",
        nested_type_arguments()
    );
    crate::nesting::take_refused_fragment();
    let _ = crate::federation_runtime::extract_federation_runtime_facts(
        Path::new("src/App.vue"),
        &source,
    );
    assert!(crate::nesting::take_refused_fragment());
}
