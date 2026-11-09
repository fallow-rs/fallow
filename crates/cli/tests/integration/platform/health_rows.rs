//! Feed Windows path forms into the grouped health markdown, which gets the
//! project root and each finding path as text.

use proptest::prelude::*;

use super::{proptest_config, tail};

proptest! {
    #![proptest_config(proptest_config())]

    /// The grouped health markdown gets the root and the finding paths as
    /// text. A live Windows run can give the root with backslashes and the
    /// paths with forward slashes, or the reverse. Each row must show the
    /// root-relative path with `/`.
    #[test]
    fn grouped_health_rows_ignore_the_separator_and_drive_root(
        drive in prop_oneof![Just(None), Just(Some('C')), Just(Some('d'))],
        root_backslash in any::<bool>(),
        path_backslash in any::<bool>(),
        segments in tail(),
        line in 1u32..500,
    ) {
        let (root, path) = match drive {
            None => ("/work/repo".to_owned(), format!("/work/repo/{}", segments.join("/"))),
            Some(letter) => {
                let in_form = |text: String, backslash: bool| {
                    if backslash { text.replace('/', "\\") } else { text }
                };
                (
                    in_form(format!("{letter}:/work/repo"), root_backslash),
                    in_form(format!("{letter}:/work/repo/{}", segments.join("/")), path_backslash),
                )
            }
        };
        let envelope = serde_json::json!({
            "grouped_by": "owner",
            "groups": [{
                "key": "@team/web",
                "files_analyzed": 1,
                "functions_above_threshold": 1,
                "findings": [{ "path": path, "line": line, "name": "work" }]
            }]
        });
        let markdown = fallow_api::build_health_groups_markdown(&envelope, &root);
        let row = format!("| `{}:{line}` | `work` |", segments.join("/"));
        prop_assert!(markdown.contains(&row), "root {} path {}:\n{}", root, path, markdown);
    }
}
