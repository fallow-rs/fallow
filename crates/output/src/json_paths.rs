//! Shared JSON path post-processing for output contracts.

/// Recursively strip a project-root prefix from all string values in a JSON
/// tree.
///
/// This keeps machine output relative to the analyzed root even when upstream
/// analysis stages temporarily carry absolute paths.
pub fn strip_root_prefix(value: &mut serde_json::Value, prefix: &str) {
    match value {
        serde_json::Value::String(s) => strip_root_prefix_from_string(s, prefix),
        serde_json::Value::Array(items) => {
            for item in items {
                strip_root_prefix(item, prefix);
            }
        }
        serde_json::Value::Object(map) => {
            for value in map.values_mut() {
                strip_root_prefix(value, prefix);
            }
        }
        _ => {}
    }
}

fn strip_root_prefix_from_string(value: &mut String, prefix: &str) {
    if let Some(rest) = value.strip_prefix(prefix) {
        *value = rest.to_string();
        return;
    }

    let normalized = normalize_output_path(value);
    let normalized_prefix = normalize_output_path(prefix);
    if let Some(rest) = normalized.strip_prefix(&normalized_prefix) {
        *value = rest.to_string();
    } else if let Some(stripped) = strip_embedded_root_prefixes(&normalized, &normalized_prefix) {
        *value = stripped;
    }
}

fn normalize_output_path(path: &str) -> String {
    normalize_uri(path)
}

/// Normalize a path string to a valid URI: forward slashes and percent-encoded
/// brackets.
///
/// Brackets (`[`, `]`) are not valid in URI path segments per RFC 3986 and
/// cause SARIF / CodeClimate validation warnings for framework routes such as
/// Next.js dynamic segments.
#[must_use]
pub fn normalize_uri(path: &str) -> String {
    path.replace('\\', "/")
        .replace('[', "%5B")
        .replace(']', "%5D")
}

fn strip_embedded_root_prefixes(value: &str, prefix: &str) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut changed = false;
    let mut last = 0;
    let mut search_from = 0;

    while let Some(offset) = value[search_from..].find(prefix) {
        let index = search_from + offset;
        let can_strip = index > 0
            && value[..index]
                .chars()
                .next_back()
                .is_some_and(is_embedded_path_boundary);

        if can_strip {
            output.push_str(&value[last..index]);
            last = index + prefix.len();
            changed = true;
        }

        search_from = index + prefix.len();
    }

    if changed {
        output.push_str(&value[last..]);
        Some(output)
    } else {
        None
    }
}

fn is_embedded_path_boundary(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '(' | '[' | '{' | ':' | '=')
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn strips_root_from_nested_strings() {
        let mut value = json!({
            "path": "/project/src/index.ts",
            "items": ["/project/src/a.ts", { "path": "/project/src/b.ts" }]
        });

        strip_root_prefix(&mut value, "/project/");

        assert_eq!(value["path"], "src/index.ts");
        assert_eq!(value["items"][0], "src/a.ts");
        assert_eq!(value["items"][1]["path"], "src/b.ts");
    }

    #[test]
    fn normalizes_windows_separators_before_stripping() {
        let mut value = json!("C:\\repo\\src\\index.ts");

        strip_root_prefix(&mut value, "C:/repo/");

        assert_eq!(value, json!("src/index.ts"));
    }

    #[test]
    fn rewrites_embedded_path_strings() {
        let mut value = json!("See /project/src/a.ts and /project/src/b.ts");

        strip_root_prefix(&mut value, "/project/");

        assert_eq!(value, json!("See src/a.ts and src/b.ts"));
    }

    #[test]
    fn leaves_non_matching_strings_unchanged() {
        let mut value = json!("src/index.ts");

        strip_root_prefix(&mut value, "/project/");

        assert_eq!(value, json!("src/index.ts"));
    }

    #[test]
    fn normalize_uri_rewrites_backslashes_and_brackets() {
        assert_eq!(
            normalize_uri("app\\[lang]\\posts\\[id].tsx"),
            "app/%5Blang%5D/posts/%5Bid%5D.tsx"
        );
    }
}
