//! Extension aliases: the files that an import with a runtime extension can
//! name. TypeScript resolves `./a.js` to `a.ts` and `./a.ts` to `a.d.ts`.

/// The `extension_alias` table of the resolver.
///
/// A declaration file is the last item of each alias, so a declaration-only
/// module resolves without shadowing a source or runtime file.
pub(super) fn build_extension_aliases() -> Vec<(String, Vec<String>)> {
    let alias = |import_ext: &str, candidates: &[&str]| {
        (
            import_ext.to_string(),
            candidates.iter().map(|ext| (*ext).to_string()).collect(),
        )
    };
    vec![
        alias(".js", &[".ts", ".tsx", ".gts", ".js", ".gjs", ".d.ts"]),
        alias(".ts", &[".gts", ".ts", ".d.ts"]),
        alias(".jsx", &[".tsx", ".jsx"]),
        alias(".mjs", &[".mts", ".mjs", ".d.mts"]),
        alias(".cjs", &[".cts", ".cjs", ".d.cts"]),
    ]
}

/// True when a tsconfig alias target with the import extension `import_ext`
/// can name a file with the extension `candidate_ext`.
pub(super) fn extension_alias_matches(import_ext: &str, candidate_ext: &str) -> bool {
    let candidate_ext = candidate_ext.trim_start_matches('.');
    let aliases: &[&str] = match import_ext {
        "js" => &["ts", "tsx", "js"],
        "jsx" => &["tsx", "jsx"],
        "mjs" => &["mts", "mjs"],
        "cjs" => &["cts", "cjs"],
        _ => return false,
    };
    aliases
        .iter()
        .any(|alias| candidate_ext == *alias || candidate_ext.ends_with(&format!(".{alias}")))
}

#[cfg(test)]
mod tests {
    use super::{build_extension_aliases, extension_alias_matches};

    #[test]
    fn ts_import_falls_back_to_the_declaration_file_last() {
        let aliases = build_extension_aliases();
        let (_, ts) = aliases
            .iter()
            .find(|(import_ext, _)| import_ext == ".ts")
            .expect("ts alias");
        assert_eq!(ts, &[".gts", ".ts", ".d.ts"]);
    }

    #[test]
    fn extension_alias_matches_js_to_ts() {
        assert!(extension_alias_matches("js", ".ts"));
        assert!(extension_alias_matches("js", ".tsx"));
        assert!(extension_alias_matches("js", ".js"));
    }

    #[test]
    fn extension_alias_matches_jsx_to_tsx() {
        assert!(extension_alias_matches("jsx", ".tsx"));
        assert!(extension_alias_matches("jsx", ".jsx"));
        assert!(!extension_alias_matches("jsx", ".ts"));
    }

    #[test]
    fn extension_alias_matches_mjs_to_mts() {
        assert!(extension_alias_matches("mjs", ".mts"));
        assert!(extension_alias_matches("mjs", ".mjs"));
        assert!(!extension_alias_matches("mjs", ".ts"));
    }

    #[test]
    fn extension_alias_matches_cjs_to_cts() {
        assert!(extension_alias_matches("cjs", ".cts"));
        assert!(extension_alias_matches("cjs", ".cjs"));
        assert!(!extension_alias_matches("cjs", ".js"));
    }

    #[test]
    fn extension_alias_matches_unknown_import_ext_returns_false() {
        assert!(!extension_alias_matches("ts", ".js"));
    }
}
