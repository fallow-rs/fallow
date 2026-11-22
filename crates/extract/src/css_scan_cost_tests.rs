//! Tests for the cost of the stylesheet scans: how often a scan reads each
//! byte and how often it masks comments.

use super::*;

fn line_lookup_bytes(run: impl FnOnce()) -> usize {
    LINE_LOOKUP_BYTES.with(|read| read.set(0));
    run();
    LINE_LOOKUP_BYTES.with(std::cell::Cell::get)
}

/// One dense line with `count` `@apply` directives and `var()` reads,
/// followed by a few short lines.
fn dense_token_stylesheet(count: usize) -> String {
    use std::fmt::Write as _;
    let mut src = String::from("@theme { --color-brand: red; }\n.x {");
    for i in 0..count {
        let _ = write!(src, " @apply p-{i}; color: var(--color-{i});");
    }
    src.push_str(" }\n.y { @apply tail; color: var(--color-tail); }\n");
    src
}

#[test]
fn located_token_scans_read_each_byte_once_for_lines() {
    for count in [400, 800] {
        let src = dense_token_stylesheet(count);
        let apply = line_lookup_bytes(|| {
            assert_eq!(extract_apply_tokens_located(&src).len(), count + 1);
        });
        let var_reads = line_lookup_bytes(|| {
            assert_eq!(extract_css_var_reads_located(&src).len(), count + 1);
        });
        let combined = line_lookup_bytes(|| {
            let _ = scan_stylesheet_tokens(&src);
        });
        // A prefix rescan for each match reads about count * len / 2
        // bytes, which is far above len.
        assert!(apply <= src.len(), "@apply read {apply} of {}", src.len());
        assert!(
            var_reads <= src.len(),
            "var() read {var_reads} of {}",
            src.len()
        );
        assert!(
            combined <= 2 * src.len(),
            "scan read {combined} of {}",
            src.len()
        );
    }
}

fn comment_mask_passes(run: impl FnOnce()) -> u32 {
    COMMENT_MASK_PASSES.with(|passes| passes.set(0));
    run();
    COMMENT_MASK_PASSES.with(std::cell::Cell::get)
}

#[test]
fn a_css_module_parse_masks_comments_once() {
    let source = "/* a */ @import './b.css';\n.card { @apply rounded; } // c\n";
    for name in ["styles.module.css", "styles.module.scss", "styles.css"] {
        let passes = comment_mask_passes(|| {
            parse_css_to_module(FileId(0), Path::new(name), source, 0);
        });
        assert_eq!(passes, 1, "comment mask passes for {name}");
    }
}

#[test]
fn a_stylesheet_token_scan_masks_comments_once() {
    let source = "/* x */\n@theme {\n  --color-brand: red;\n  --color-alt: var(--color-brand);\n}\n.a { @apply bg-brand p-2; color: var(--color-alt); }\n";
    let mut scan = StylesheetTokens::default();
    let passes = comment_mask_passes(|| scan = scan_stylesheet_tokens(source));
    assert_eq!(passes, 1);
    assert_eq!(scan.theme, scan_theme_blocks(source));
    assert_eq!(
        scan.apply_tokens_located,
        extract_apply_tokens_located(source)
    );
    assert_eq!(
        scan.css_var_reads_located,
        extract_css_var_reads_located(source)
    );
}

#[test]
fn a_stylesheet_token_scan_skips_the_mask_without_directives() {
    let source = "/* plain */ .a { color: red; }";
    let mut scan = StylesheetTokens::default();
    let passes = comment_mask_passes(|| scan = scan_stylesheet_tokens(source));
    assert_eq!(passes, 0);
    assert_eq!(scan, StylesheetTokens::default());
}
