/// Shields.io-compatible flat SVG badge generation.
///
/// Generates self-contained SVG badges with embedded Verdana 11px character
/// width data for accurate text measurement. No external dependencies required.
use crate::report::sink::outln;
use std::process::ExitCode;

use crate::health_types::HealthReport;

/// Escape a string for safe interpolation in XML attributes and element content.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Hex color for each letter grade.
const fn grade_color(grade: &str) -> &str {
    match grade.as_bytes() {
        b"A" => "#4c1",
        b"B" => "#97ca00",
        b"C" => "#dfb317",
        b"D" => "#fe7d37",
        _ => "#e05d44",
    }
}

/// Verdana 11px character widths (from shields.io / anafanafo data).
///
/// Characters outside the table fall back to the width of 'm' (10.7).
#[expect(
    clippy::match_same_arms,
    reason = "lookup table — each character has its own metric"
)]
const fn char_width(c: char) -> f64 {
    match c {
        ' ' => 3.87,
        '!' => 4.33,
        '"' => 5.05,
        '#' => 9.0,
        '$' => 6.99,
        '%' => 11.84,
        '&' => 7.99,
        '\'' => 2.95,
        '(' | ')' => 5.0,
        '*' => 6.99,
        '+' => 9.0,
        ',' => 4.0,
        '-' => 5.0,
        '.' => 4.0,
        '/' => 5.0,
        '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => 6.99,
        ':' | ';' => 5.0,
        '<' | '=' | '>' => 9.0,
        '?' => 6.0,
        '@' => 11.0,
        'A' => 7.52,
        'B' => 7.54,
        'C' => 7.68,
        'D' => 8.48,
        'E' => 6.96,
        'F' => 6.32,
        'G' => 8.53,
        'H' => 8.27,
        'I' => 4.63,
        'J' => 5.0,
        'K' => 7.62,
        'L' => 6.12,
        'M' => 9.27,
        'N' => 8.23,
        'O' => 8.66,
        'P' => 6.63,
        'Q' => 8.66,
        'R' => 7.65,
        'S' => 7.52,
        'T' => 6.78,
        'U' => 8.05,
        'V' => 7.52,
        'W' => 10.88,
        'X' => 7.54,
        'Y' => 6.77,
        'Z' => 7.54,
        '[' | '\\' | ']' => 5.0,
        '^' => 9.0,
        '_' | '`' => 6.99,
        'a' => 6.61,
        'b' => 6.85,
        'c' => 5.73,
        'd' => 6.85,
        'e' => 6.55,
        'f' => 3.87,
        'g' => 6.85,
        'h' => 6.96,
        'i' => 3.02,
        'j' => 3.79,
        'k' => 6.51,
        'l' => 3.02,
        'm' => 10.7,
        'n' => 6.96,
        'o' => 6.68,
        'p' | 'q' => 6.85,
        'r' => 4.69,
        's' => 5.73,
        't' => 4.33,
        'u' => 6.96,
        'v' => 6.51,
        'w' => 9.0,
        'x' | 'y' => 6.51,
        'z' => 5.78,
        '{' => 6.98,
        '|' => 5.0,
        '}' => 6.98,
        '~' => 9.0,
        _ => 10.7,
    }
}

/// Compute text width in integer pixels using Verdana 11px metrics.
///
/// Follows shields.io convention: sum character widths, floor to integer,
/// then round up to nearest odd number.
fn text_width(s: &str) -> u32 {
    if s.is_empty() {
        return 0;
    }
    let raw: f64 = s.chars().map(char_width).sum();
    let floored = raw as u32;
    if floored.is_multiple_of(2) {
        floored + 1
    } else {
        floored
    }
}

/// Simple hash for generating unique SVG element IDs.
///
/// Prevents ID collisions when multiple badges are inlined on the same page
/// (e.g., a GitHub profile README with several fallow badges).
fn svg_id_suffix(label: &str, message: &str) -> String {
    let mut h: u32 = 0;
    for b in label.bytes().chain(message.bytes()) {
        h = h.wrapping_mul(31).wrapping_add(u32::from(b));
    }
    format!("{h:x}")
}

/// Render a shields.io-compatible flat-style SVG badge.
fn render_badge(label: &str, message: &str, color: &str) -> String {
    let horiz_padding: u32 = 5;

    let label_w = text_width(label);
    let message_w = text_width(message);

    let left_width = label_w + 2 * horiz_padding;
    let right_width = message_w + 2 * horiz_padding;
    let total_width = left_width + right_width;
    let height: u32 = 20;

    let label_margin: u32 = 1;
    let label_x = 10 * (label_margin + label_w / 2 + horiz_padding);
    let label_text_len = 10 * label_w;

    let msg_margin = left_width - 1;
    let msg_x = 10 * (msg_margin + message_w / 2 + horiz_padding);
    let msg_text_len = 10 * message_w;

    let label = xml_escape(label);
    let message = xml_escape(message);
    let accessible = format!("{label}: {message}");

    let suffix = svg_id_suffix(&label, &message);
    let grad_id = format!("s-{suffix}");
    let clip_id = format!("r-{suffix}");

    let label_bg = "#555";
    let white = "#fff";
    let shadow = "#010101";
    let gradient_stop = "#bbb";
    let font = "Verdana,Geneva,DejaVu Sans,sans-serif";

    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{total_width}" height="{height}" role="img" aria-label="{accessible}">
<title>{accessible}</title>
<linearGradient id="{grad_id}" x2="0" y2="100%">
<stop offset="0" stop-color="{gradient_stop}" stop-opacity=".1"/>
<stop offset="1" stop-opacity=".1"/>
</linearGradient>
<clipPath id="{clip_id}">
<rect width="{total_width}" height="{height}" rx="3" fill="{white}"/>
</clipPath>
<g clip-path="url(#{clip_id})">
<rect width="{left_width}" height="{height}" fill="{label_bg}"/>
<rect x="{left_width}" width="{right_width}" height="{height}" fill="{color}"/>
<rect width="{total_width}" height="{height}" fill="url(#{grad_id})"/>
</g>
<g fill="{white}" text-anchor="middle" font-family="{font}" text-rendering="geometricPrecision" font-size="110">
<text aria-hidden="true" x="{label_x}" y="150" fill="{shadow}" fill-opacity=".3" transform="scale(.1)" textLength="{label_text_len}">{label}</text>
<text x="{label_x}" y="140" transform="scale(.1)" fill="{white}" textLength="{label_text_len}">{label}</text>
<text aria-hidden="true" x="{msg_x}" y="150" fill="{shadow}" fill-opacity=".3" transform="scale(.1)" textLength="{msg_text_len}">{message}</text>
<text x="{msg_x}" y="140" transform="scale(.1)" fill="{white}" textLength="{msg_text_len}">{message}</text>
</g>
</svg>"#
    )
}

/// Print a health score badge as shields.io-compatible SVG to stdout.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "health score is 0-100, always fits in u32"
)]
pub fn print_health_badge(report: &HealthReport) -> ExitCode {
    let Some(ref score) = report.health_score else {
        eprintln!("Error: badge format requires --score (run `fallow health --format badge`)");
        return ExitCode::from(2);
    };

    let rounded = score.score as u32;
    let message = format!("{} ({rounded})", score.grade);
    let color = grade_color(score.grade);
    let svg = render_badge("fallow", &message, color);

    outln!("{svg}");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_width_rounds_to_odd() {
        let w = text_width("A");
        assert_eq!(w % 2, 1, "text width should be odd, got {w}");
    }

    #[test]
    fn text_width_empty_string() {
        assert_eq!(text_width(""), 0, "empty string returns zero width");
    }

    #[test]
    fn grade_colors_cover_all_grades() {
        assert_eq!(grade_color("A"), "#4c1");
        assert_eq!(grade_color("B"), "#97ca00");
        assert_eq!(grade_color("C"), "#dfb317");
        assert_eq!(grade_color("D"), "#fe7d37");
        assert_eq!(grade_color("F"), "#e05d44");
    }

    #[test]
    fn render_badge_contains_svg_elements() {
        let svg = render_badge("test", "100", "#4c1");
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("</svg>"));
        assert!(svg.contains("test"));
        assert!(svg.contains("100"));
        assert!(svg.contains("#4c1"));
    }

    #[test]
    fn render_badge_has_accessibility() {
        let svg = render_badge("fallow", "A (87)", "#4c1");
        assert!(svg.contains(r#"aria-label="fallow: A (87)""#));
        assert!(svg.contains("<title>fallow: A (87)</title>"));
    }

    #[test]
    fn render_badge_unique_ids() {
        let a = render_badge("fallow", "A (90)", "#4c1");
        let b = render_badge("fallow", "B (76)", "#97ca00");
        let extract_id = |svg: &str| -> String {
            let start = svg.find("id=\"s-").unwrap() + 4;
            let end = svg[start..].find('"').unwrap() + start;
            svg[start..end].to_string()
        };
        assert_ne!(extract_id(&a), extract_id(&b));
    }

    #[test]
    fn render_badge_width_increases_with_longer_text() {
        let short = render_badge("a", "b", "#4c1");
        let long = render_badge("fallow health", "100 A", "#4c1");

        let extract_width = |svg: &str| -> u32 {
            let start = svg.find("width=\"").unwrap() + 7;
            let end = svg[start..].find('"').unwrap() + start;
            svg[start..end].parse().unwrap()
        };

        assert!(extract_width(&long) > extract_width(&short));
    }

    fn empty_report() -> HealthReport {
        use crate::health_types::HealthSummary;

        HealthReport {
            summary: HealthSummary {
                max_cyclomatic_threshold: 10,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn print_health_badge_no_score() {
        let report = empty_report();
        let code = print_health_badge(&report);
        assert_ne!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn print_health_badge_with_score() {
        use crate::health_types::{
            HEALTH_SCORE_FORMULA_VERSION, HealthScore, HealthScorePenalties,
        };

        let mut report = empty_report();
        report.health_score = Some(HealthScore {
            formula_version: HEALTH_SCORE_FORMULA_VERSION,
            score: 87.3,
            grade: "A",
            penalties: HealthScorePenalties {
                dead_files: None,
                dead_exports: None,
                complexity: 5.0,
                p90_complexity: 3.0,
                maintainability: None,
                hotspots: None,
                unused_deps: None,
                circular_deps: None,
                unit_size: None,
                coupling: None,
                duplication: None,
                prop_drilling: None,
            },
        });
        let code = print_health_badge(&report);
        assert_eq!(code, ExitCode::SUCCESS);
    }
}
