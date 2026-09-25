//! Template names of `nuxt-og-image` calls.
//!
//! `defineOgImage('Docs.takumi')` and `defineOgImageComponent('Docs')` render
//! an OG image template component that the string names, so no import or
//! template tag reaches the file. The extractor records each string as an
//! auto-import candidate with [`OG_IMAGE_CANDIDATE_PREFIX`], and the Nuxt
//! plugin gives each template file a rule under the same key. The prefix
//! cannot collide with an identifier.

/// Prefix of the auto-import candidate that a template name gives.
pub const OG_IMAGE_CANDIDATE_PREFIX: &str = "og-image:";

/// Calls that name an OG image template in their first argument.
const TEMPLATE_CALLS: &[&str] = &["defineOgImage", "defineOgImageComponent"];

/// Renderer suffixes that `nuxt-og-image` strips before it compares names.
const RENDERER_SUFFIXES: &[&str] = &["Satori", "Browser", "Takumi"];

/// Component directory prefixes that `nuxt-og-image` strips before it
/// compares names, longest first.
const DIRECTORY_PREFIXES: &[&str] = &["OgImageCommunity", "OgImageTemplate", "OgImage"];

/// Whether `callee` is a call that names an OG image template.
#[must_use]
pub fn is_template_call(callee: &str) -> bool {
    TEMPLATE_CALLS.contains(&callee)
}

/// The auto-import candidate for an OG image template name or for the
/// PascalCase component name of a template file, or `None` for an empty name.
///
/// This follows the `nuxt-og-image` name match: dot segments join in
/// PascalCase (`Docs.takumi` is `DocsTakumi`), then a renderer suffix and a
/// directory prefix are stripped. `Docs.takumi`, `Docs`, `OgImageDocs` and
/// the file `og-image/Docs.takumi.vue` (component `OgImageDocsTakumi`) all
/// give `og-image:Docs`.
#[must_use]
pub fn template_candidate(name: &str) -> Option<String> {
    let mut joined = String::with_capacity(name.len());
    for (idx, segment) in name.split('.').enumerate() {
        let mut chars = segment.chars();
        if idx > 0
            && let Some(first) = chars.next()
        {
            joined.extend(first.to_uppercase());
            joined.push_str(chars.as_str());
        } else {
            joined.push_str(segment);
        }
    }
    let base = strip_nonempty_suffix(&joined, RENDERER_SUFFIXES);
    let base = strip_nonempty_prefix(base, DIRECTORY_PREFIXES);
    (!base.is_empty()).then(|| format!("{OG_IMAGE_CANDIDATE_PREFIX}{base}"))
}

fn strip_nonempty_suffix<'a>(name: &'a str, suffixes: &[&str]) -> &'a str {
    suffixes
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix).filter(|rest| !rest.is_empty()))
        .unwrap_or(name)
}

fn strip_nonempty_prefix<'a>(name: &'a str, prefixes: &[&str]) -> &'a str {
    prefixes
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix).filter(|rest| !rest.is_empty()))
        .unwrap_or(name)
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    #[test]
    fn call_names_and_component_names_share_one_key() {
        for name in [
            "Docs",
            "Docs.takumi",
            "DocsTakumi",
            "OgImageDocs",
            "OgImageDocsTakumi",
            "OgImageTemplateDocs",
            "OgImageCommunityDocs.satori",
        ] {
            assert_eq!(
                template_candidate(name).as_deref(),
                Some("og-image:Docs"),
                "{name}"
            );
        }
        assert_eq!(
            template_candidate("BlogPost.browser").as_deref(),
            Some("og-image:BlogPost")
        );
    }

    #[test]
    fn a_name_that_is_only_a_prefix_or_suffix_stays() {
        assert_eq!(
            template_candidate("OgImage").as_deref(),
            Some("og-image:OgImage")
        );
        assert_eq!(
            template_candidate("Takumi").as_deref(),
            Some("og-image:Takumi")
        );
        assert_eq!(template_candidate(""), None);
    }

    #[test]
    fn a_static_template_name_becomes_a_candidate() {
        let info = crate::tests::parse_ts(
            "defineOgImage('Docs.takumi', {});\ndefineOgImageComponent(`Blog`);\ndefineOgImage(name);\ndefineOgImage(`A${x}`);\nog.defineOgImage('Member');\n",
        );
        let og: Vec<&str> = info
            .auto_import_candidates
            .iter()
            .map(String::as_str)
            .filter(|name| name.starts_with(OG_IMAGE_CANDIDATE_PREFIX))
            .collect();
        assert_eq!(og, ["og-image:Blog", "og-image:Docs"]);
        assert!(
            info.auto_import_candidates
                .iter()
                .any(|name| name == "defineOgImage"),
            "the call itself stays a candidate"
        );
    }

    #[test]
    fn only_the_template_calls_match() {
        assert!(is_template_call("defineOgImage"));
        assert!(is_template_call("defineOgImageComponent"));
        assert!(!is_template_call("defineOgImageScreenshot"));
        assert!(!is_template_call("useSeoMeta"));
    }
}
