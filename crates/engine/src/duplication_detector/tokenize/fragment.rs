use std::path::Path;

#[cfg(test)]
use std::path::PathBuf;

use super::{FileTokens, tokenize_file};

/// Context in which an extracted fragment is tokenized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentTokenizationStrategy {
    /// Preserve the source extension when it affects stable fingerprint identity.
    Fingerprint,
    /// Parse an extracted function body with the closest JavaScript language mode.
    Function,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FragmentTokenizationKind {
    JavaScript,
    Jsx,
    Mjs,
    Cjs,
    TypeScript,
    Tsx,
    Mts,
    Cts,
    Css,
    Scss,
    Sass,
    Less,
}

impl FragmentTokenizationKind {
    pub fn path(self) -> &'static Path {
        Path::new(match self {
            Self::JavaScript => "fragment.js",
            Self::Jsx => "fragment.jsx",
            Self::Mjs => "fragment.mjs",
            Self::Cjs => "fragment.cjs",
            Self::TypeScript => "fragment.ts",
            Self::Tsx => "fragment.tsx",
            Self::Mts => "fragment.mts",
            Self::Cts => "fragment.cts",
            Self::Css => "fragment.css",
            Self::Scss => "fragment.scss",
            Self::Sass => "fragment.sass",
            Self::Less => "fragment.less",
        })
    }
}

pub fn fragment_tokenization_kind(
    path: &Path,
    strategy: FragmentTokenizationStrategy,
) -> FragmentTokenizationKind {
    let extension = path.extension().and_then(|extension| extension.to_str());
    match strategy {
        FragmentTokenizationStrategy::Fingerprint => match extension {
            Some("js") => FragmentTokenizationKind::JavaScript,
            Some("jsx") => FragmentTokenizationKind::Jsx,
            Some("mjs") => FragmentTokenizationKind::Mjs,
            Some("cjs") => FragmentTokenizationKind::Cjs,
            Some("tsx") => FragmentTokenizationKind::Tsx,
            Some("mts") => FragmentTokenizationKind::Mts,
            Some("cts") => FragmentTokenizationKind::Cts,
            Some("css") => FragmentTokenizationKind::Css,
            Some("scss") => FragmentTokenizationKind::Scss,
            Some("sass") => FragmentTokenizationKind::Sass,
            Some("less") => FragmentTokenizationKind::Less,
            _ => FragmentTokenizationKind::TypeScript,
        },
        FragmentTokenizationStrategy::Function => match extension {
            Some("js" | "mjs" | "cjs") => FragmentTokenizationKind::JavaScript,
            Some("jsx") => FragmentTokenizationKind::Jsx,
            Some("ts" | "mts" | "cts") => FragmentTokenizationKind::TypeScript,
            _ => FragmentTokenizationKind::Tsx,
        },
    }
}

pub fn tokenize_fragment(
    source_path: &Path,
    fragment: &str,
    strategy: FragmentTokenizationStrategy,
) -> FileTokens {
    let kind = fragment_tokenization_kind(source_path, strategy);
    tokenize_file(kind.path(), fragment, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_strategy_preserves_existing_extension_mapping() {
        let cases = [
            ("a.js", FragmentTokenizationKind::JavaScript),
            ("a.jsx", FragmentTokenizationKind::Jsx),
            ("a.mjs", FragmentTokenizationKind::Mjs),
            ("a.cjs", FragmentTokenizationKind::Cjs),
            ("a.ts", FragmentTokenizationKind::TypeScript),
            ("a.tsx", FragmentTokenizationKind::Tsx),
            ("a.mts", FragmentTokenizationKind::Mts),
            ("a.cts", FragmentTokenizationKind::Cts),
            ("a.css", FragmentTokenizationKind::Css),
            ("a.scss", FragmentTokenizationKind::Scss),
            ("a.sass", FragmentTokenizationKind::Sass),
            ("a.less", FragmentTokenizationKind::Less),
            ("a.vue", FragmentTokenizationKind::TypeScript),
            ("a.svelte", FragmentTokenizationKind::TypeScript),
            ("a.astro", FragmentTokenizationKind::TypeScript),
        ];

        for (path, expected) in cases {
            assert_eq!(
                fragment_tokenization_kind(
                    &PathBuf::from(path),
                    FragmentTokenizationStrategy::Fingerprint
                ),
                expected,
                "unexpected fingerprint mapping for {path}"
            );
        }
    }

    #[test]
    fn function_strategy_preserves_existing_parser_modes() {
        let cases = [
            ("a.js", FragmentTokenizationKind::JavaScript),
            ("a.mjs", FragmentTokenizationKind::JavaScript),
            ("a.cjs", FragmentTokenizationKind::JavaScript),
            ("a.jsx", FragmentTokenizationKind::Jsx),
            ("a.ts", FragmentTokenizationKind::TypeScript),
            ("a.mts", FragmentTokenizationKind::TypeScript),
            ("a.cts", FragmentTokenizationKind::TypeScript),
            ("a.tsx", FragmentTokenizationKind::Tsx),
            ("a.vue", FragmentTokenizationKind::Tsx),
            ("a.svelte", FragmentTokenizationKind::Tsx),
            ("a.astro", FragmentTokenizationKind::Tsx),
            ("a.css", FragmentTokenizationKind::Tsx),
        ];

        for (path, expected) in cases {
            assert_eq!(
                fragment_tokenization_kind(
                    &PathBuf::from(path),
                    FragmentTokenizationStrategy::Function
                ),
                expected,
                "unexpected function mapping for {path}"
            );
        }
    }
}
