//! Conservative recognition of Tailwind utility class tokens.
//!
//! The unresolved-class check compares markup class tokens with authored CSS
//! classes. A Tailwind utility (`w-full`, `h-full`) is never authored in the
//! project's stylesheets, so in a Tailwind project it looks undefined and can be
//! one edit away from an authored class (`wl-full`). This module recognizes the
//! token shape of a Tailwind utility so the check can skip it.

/// Utilities that are a complete class without a `-value` suffix.
const STANDALONE_UTILITIES: &[&str] = &[
    "absolute",
    "antialiased",
    "block",
    "border",
    "capitalize",
    "collapse",
    "container",
    "contents",
    "filter",
    "fixed",
    "flex",
    "flow-root",
    "grayscale",
    "grid",
    "grow",
    "hidden",
    "inline",
    "invert",
    "invisible",
    "isolate",
    "italic",
    "lowercase",
    "outline",
    "overline",
    "relative",
    "ring",
    "rounded",
    "sepia",
    "shadow",
    "shrink",
    "static",
    "sticky",
    "table",
    "transform",
    "transition",
    "truncate",
    "underline",
    "uppercase",
    "visible",
];

/// The first `-` segment of a utility that takes a value or a modifier, such as
/// `w` in `w-full` or `min` in `min-h-screen`.
const UTILITY_ROOTS: &[&str] = &[
    "accent",
    "align",
    "animate",
    "appearance",
    "aspect",
    "auto",
    "backdrop",
    "backface",
    "basis",
    "bg",
    "blur",
    "border",
    "bottom",
    "box",
    "break",
    "brightness",
    "caption",
    "caret",
    "clear",
    "col",
    "columns",
    "content",
    "contrast",
    "cursor",
    "decoration",
    "delay",
    "divide",
    "drop",
    "duration",
    "ease",
    "end",
    "field",
    "fill",
    "flex",
    "float",
    "font",
    "forced",
    "from",
    "gap",
    "grayscale",
    "grid",
    "grow",
    "h",
    "hue",
    "hyphens",
    "indent",
    "inline",
    "inset",
    "invert",
    "isolation",
    "items",
    "justify",
    "leading",
    "left",
    "line",
    "list",
    "m",
    "mask",
    "max",
    "mb",
    "me",
    "min",
    "mix",
    "ml",
    "mr",
    "ms",
    "mt",
    "mx",
    "my",
    "no",
    "normal",
    "not",
    "object",
    "opacity",
    "order",
    "origin",
    "outline",
    "overflow",
    "overscroll",
    "p",
    "pb",
    "pe",
    "perspective",
    "pl",
    "place",
    "pointer",
    "pr",
    "ps",
    "pt",
    "px",
    "py",
    "resize",
    "right",
    "ring",
    "rotate",
    "rounded",
    "row",
    "saturate",
    "scale",
    "scroll",
    "select",
    "self",
    "sepia",
    "shadow",
    "shrink",
    "size",
    "skew",
    "snap",
    "space",
    "sr",
    "start",
    "stroke",
    "subpixel",
    "table",
    "text",
    "to",
    "top",
    "touch",
    "tracking",
    "transform",
    "transition",
    "translate",
    "underline",
    "via",
    "w",
    "whitespace",
    "will",
    "wrap",
    "z",
];

/// True when `token` has the shape of a Tailwind utility class: a standalone
/// utility (`hidden`, `truncate`) or a known utility root followed by a value
/// (`w-full`, `min-h-screen`, `-mt-4`, `!p-2`).
///
/// The check is shape based. It does not prove that Tailwind generates the
/// class, so callers use it only to abstain, never to report.
pub(super) fn is_tailwind_utility(token: &str) -> bool {
    let token = token.trim_start_matches(['!', '-']).trim_end_matches('!');
    if STANDALONE_UTILITIES.contains(&token) {
        return true;
    }
    let Some((root, value)) = token.split_once('-') else {
        return false;
    };
    !value.is_empty() && UTILITY_ROOTS.contains(&root)
}

/// True when a stylesheet loads Tailwind: a v4 `@import "tailwindcss"` or
/// `@reference "tailwindcss"` (also a subpath such as `tailwindcss/theme`), or a
/// v3 `@tailwind` directive.
pub(super) fn stylesheet_loads_tailwind(source: &str) -> bool {
    if source.contains("@tailwind ") {
        return true;
    }
    ["@import", "@reference"].iter().any(|at_rule| {
        source.match_indices(at_rule).any(|(start, _)| {
            let prelude = &source[start + at_rule.len()..];
            let end = prelude.find([';', '\n']).unwrap_or(prelude.len());
            let prelude = prelude[..end].trim_start();
            prelude.starts_with("\"tailwindcss") || prelude.starts_with("'tailwindcss")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_value_and_standalone_utilities() {
        for token in [
            "w-full",
            "h-full",
            "min-h-screen",
            "max-w-prose",
            "-mt-4",
            "!p-2",
            "grid-cols-3",
            "text-primary",
            "hidden",
            "truncate",
            "sr-only",
        ] {
            assert!(is_tailwind_utility(token), "{token} is a utility");
        }
    }

    #[test]
    fn rejects_authored_component_classes() {
        for token in [
            "wl-full",
            "card-title",
            "nav-chev",
            "consent",
            "w-",
            "panel",
        ] {
            assert!(!is_tailwind_utility(token), "{token} is not a utility");
        }
    }

    #[test]
    fn detects_tailwind_entry_stylesheets() {
        assert!(stylesheet_loads_tailwind("@import \"tailwindcss\";\n"));
        assert!(stylesheet_loads_tailwind(
            "@import 'tailwindcss/theme' layer(theme);"
        ));
        assert!(stylesheet_loads_tailwind("@reference \"tailwindcss\";"));
        assert!(stylesheet_loads_tailwind(
            "@tailwind base;\n@tailwind utilities;"
        ));
        assert!(!stylesheet_loads_tailwind(
            "@import \"./tailwindcss-like.css\";"
        ));
        assert!(!stylesheet_loads_tailwind(".card { color: red; }"));
    }
}
