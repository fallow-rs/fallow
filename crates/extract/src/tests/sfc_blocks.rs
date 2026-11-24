//! The `<script>` and `<style>` block scans of Vue and Svelte files must cost
//! linear time on input with many unclosed attribute quotes.

use std::time::{Duration, Instant};

use crate::sfc::{extract_sfc_scripts, extract_sfc_styles, extract_sfc_template_regions};

/// Number of repeated units in each input below.
const UNITS: usize = 8_000;

/// Time limit for the scans of one input. A scan that reads the rest of the
/// file again for each block takes many seconds on these inputs.
const BUDGET: Duration = Duration::from_secs(2);

/// Each unit opens a tag whose quoted attribute value closes only in the next
/// unit, so that tag never ends. The inner tag of each unit is a valid block.
/// The leading non-ASCII character makes the input non-ASCII.
fn unclosed_attribute_units(tag: &str) -> String {
    format!("é{}", format!("<{tag} '<{tag}>x</{tag}>'").repeat(UNITS))
}

#[test]
fn repeated_unclosed_script_attributes_remain_fast() {
    let source = unclosed_attribute_units("script");
    let start = Instant::now();
    let scripts = extract_sfc_scripts(&source);
    let regions = extract_sfc_template_regions(&source);
    let elapsed = start.elapsed();
    assert_eq!(scripts.len(), UNITS);
    assert!(scripts.iter().all(|script| script.body == "x"));
    assert_eq!(scripts[0].byte_offset, "é<script '<script>".len());
    assert_eq!(regions[0].body, "é<script '");
    assert!(
        elapsed < BUDGET,
        "the script block scan took {elapsed:?} on {} bytes",
        source.len()
    );
}

#[test]
fn repeated_unclosed_style_attributes_remain_fast() {
    let source = unclosed_attribute_units("style");
    let start = Instant::now();
    let styles = extract_sfc_styles(&source);
    let regions = extract_sfc_template_regions(&source);
    let elapsed = start.elapsed();
    assert_eq!(styles.len(), UNITS);
    assert!(styles.iter().all(|style| style.body == "x"));
    assert_eq!(styles[0].byte_offset, "é<style '<style>".len());
    assert_eq!(regions[0].body, "é<style '");
    assert!(
        elapsed < BUDGET,
        "the style block scan took {elapsed:?} on {} bytes",
        source.len()
    );
}

mod regex_parity {
    use std::sync::LazyLock;

    use proptest::prelude::*;

    use crate::sfc_blocks::{SfcBlock, script_blocks, style_blocks};

    /// The script block regex that the scan replaces. It is quadratic on
    /// some input, so it is safe only on the short input of these tests.
    static SCRIPT_BLOCK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        crate::static_regex(
            r#"(?is)<script\b(?P<attrs>(?:[^>"'{]|"[^"]*"|'[^']*'|\{(?:[^{}"'`]|"[^"]*"|'[^']*'|`[^`]*`|\{(?:[^{}"'`]|"[^"]*"|'[^']*'|`[^`]*`|\{[^{}]*\})*\})*\}|\{)*)>(?P<body>[\s\S]*?)</script>"#,
        )
    });

    /// The style block regex that the scan replaces.
    static STYLE_BLOCK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        crate::static_regex(
            r#"(?is)<style\b(?P<attrs>(?:[^>"']|"[^"]*"|'[^']*')*)>(?P<body>[\s\S]*?)</style>"#,
        )
    });

    /// Source fragments that change the result of a tag scan.
    const FRAGMENTS: &[&str] = &[
        "<script",
        "<SCRIPT",
        "<scripts",
        "<\u{17f}\x63ript",
        "</script>",
        "</SCRIPT>",
        "</script",
        "<style",
        "<STYLE",
        "<styled",
        "</style>",
        "</STYLE>",
        ">",
        "\"",
        "'",
        "`",
        "{",
        "}",
        "{}",
        " ",
        "a",
        "\n",
        "é",
        "_",
        "<!--",
        "-->",
    ];

    fn regex_blocks(regex: &regex::Regex, source: &str) -> Vec<SfcBlock> {
        regex
            .captures_iter(source)
            .map(|caps| SfcBlock {
                span: caps.get(0).expect("whole match").range(),
                attrs: caps.name("attrs").expect("attrs group").range(),
                body: caps.name("body").expect("body group").range(),
            })
            .collect()
    }

    fn assert_parity(source: &str) {
        assert_eq!(
            script_blocks(source),
            regex_blocks(&SCRIPT_BLOCK_RE, source),
            "script blocks differ for {source:?}"
        );
        assert_eq!(
            style_blocks(source),
            regex_blocks(&STYLE_BLOCK_RE, source),
            "style blocks differ for {source:?}"
        );
    }

    #[test]
    fn the_scan_matches_the_regex_on_known_shapes() {
        for source in [
            "<script>const a = 1;</script>",
            "<script setup lang=\"ts\" generic=\"T extends Record<string, { a: number }>\">x</script>",
            "<script src=\"/x.js\" onload={() => { if (a > 1) {} }}></script>",
            "<script src=\"/y.js\" data-x={`>${'}'}`}></script>",
            "<script data-x={ lang=\"ts\">const a: number = 1;</script>",
            "<script a={b>c}>x</script>y</script>",
            "<script a={{{{b}}}}>x</script>",
            "<script a='>'>x</script><style b=\">\">y</style>",
            "<script '<script>x</script>'<script '<script>y</script>'",
            "<scriptX>no</script><script\u{0300}>no</script>",
            "<script>a</script>b</SCRIPT>",
        ] {
            assert_parity(source);
        }
    }

    proptest! {
        #[test]
        fn the_scan_matches_the_regex_on_generated_input(
            fragments in prop::collection::vec(prop::sample::select(FRAGMENTS), 0..48)
        ) {
            assert_parity(&fragments.concat());
        }
    }
}
