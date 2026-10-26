//! `markupExtensions`: server-side template extensions whose text counts as CSS
//! class references (issue #3917).

/// Template extensions that the CSS class reference surface always reads.
pub const DEFAULT_MARKUP_EXTENSIONS: &[&str] =
    &["blade.php", "handlebars", "hbs", "liquid", "njk", "twig"];

/// Extensions that fallow already reads as markup through source discovery or
/// the Markdown walk. A configured value from this list changes nothing.
const BUILT_IN_MARKUP_EXTENSIONS: &[&str] = &[
    "astro", "html", "htm", "jsx", "md", "mdx", "svelte", "tsx", "vue",
];

/// Script and stylesheet extensions. Their text as class references would
/// credit every class that a stylesheet defines, so they are a config error.
const REJECTED_MARKUP_EXTENSIONS: &[&str] = &[
    "cjs", "css", "cts", "gjs", "gts", "js", "json", "less", "mjs", "mts", "sass", "scss", "ts",
];

fn normalize(value: &str) -> String {
    value.trim().trim_start_matches('.').to_ascii_lowercase()
}

/// Read `markupExtensions` and fail the config load when a value is invalid.
pub(super) fn deserialize_markup_extensions<'de, D>(
    deserializer: D,
) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values = <Vec<String> as serde::Deserialize>::deserialize(deserializer)?;
    let errors = markup_extension_errors(&values);
    if errors.is_empty() {
        Ok(values)
    } else {
        Err(serde::de::Error::custom(errors.join("; ")))
    }
}

/// One error line for each `markupExtensions` value that is not a plain file
/// extension, or that names a script or stylesheet extension.
fn markup_extension_errors(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| {
            let extension = normalize(value);
            let plain = !extension.is_empty()
                && !extension.ends_with('.')
                && extension
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
            if !plain {
                return Some(format!(
                    "markupExtensions entry {value:?} is not a file extension: use only letters, digits, '.', '-', and '_', for example \"liquid\""
                ));
            }
            REJECTED_MARKUP_EXTENSIONS.contains(&extension.as_str()).then(|| {
                format!(
                    "markupExtensions entry {value:?} is a script or stylesheet extension: list only template extensions, for example \"liquid\""
                )
            })
        })
        .collect()
}

/// The configured extensions that add to the built-in markup set: normalized
/// (no leading dot, lowercase), without built-in and default extensions,
/// sorted, and deduplicated.
#[must_use]
pub fn resolve_markup_extensions(values: &[String]) -> Vec<String> {
    let mut out: Vec<String> = values
        .iter()
        .map(|value| normalize(value))
        .filter(|extension| {
            !extension.is_empty()
                && !BUILT_IN_MARKUP_EXTENSIONS.contains(&extension.as_str())
                && !DEFAULT_MARKUP_EXTENSIONS.contains(&extension.as_str())
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The template extension of `file_name` from the defaults and `configured`,
/// or `None` when the file is not a template file. The longest match wins, so
/// `welcome.blade.php` gives `blade.php` even when `php` is also configured.
#[must_use]
pub fn template_markup_extension<'a>(file_name: &str, configured: &'a [String]) -> Option<&'a str> {
    let lower = file_name.to_ascii_lowercase();
    DEFAULT_MARKUP_EXTENSIONS
        .iter()
        .map(|extension| -> &'a str { extension })
        .chain(configured.iter().map(String::as_str))
        .filter(|extension| {
            lower
                .strip_suffix(extension)
                .and_then(|rest| rest.strip_suffix('.'))
                .is_some_and(|stem| !stem.is_empty())
        })
        .max_by_key(|extension| extension.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn errors_name_invalid_and_rejected_values() {
        let errors = markup_extension_errors(&strings(&[
            "liquid", ".tpl", "erb", "", ".", "a/b", "*", "css", ".TS", "blade.",
        ]));
        assert_eq!(errors.len(), 7, "{errors:#?}");
        assert!(
            errors
                .iter()
                .all(|error| error.contains("markupExtensions"))
        );
        assert!(errors.iter().any(|error| error.contains("\"css\"")));
    }

    #[test]
    fn resolve_strips_dots_and_drops_built_in_values() {
        assert_eq!(
            resolve_markup_extensions(&strings(&[".TPL", "erb", "tpl", "html", "liquid"])),
            strings(&["erb", "tpl"])
        );
    }

    #[test]
    fn template_extension_matches_the_end_of_the_file_name() {
        let configured = strings(&["php", "tpl"]);
        assert_eq!(
            template_markup_extension("welcome.blade.php", &configured),
            Some("blade.php")
        );
        assert_eq!(
            template_markup_extension("index.php", &configured),
            Some("php")
        );
        assert_eq!(
            template_markup_extension("Product.LIQUID", &[]),
            Some("liquid")
        );
        assert_eq!(template_markup_extension("liquid", &[]), None);
        assert_eq!(template_markup_extension("notliquid", &[]), None);
        assert_eq!(template_markup_extension("index.html", &configured), None);
    }
}
