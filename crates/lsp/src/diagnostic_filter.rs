use rustc_hash::{FxHashMap, FxHashSet};

use ls_types::{Diagnostic, NumberOrString, Uri};

/// Drop diagnostics whose string `code` is in the `disabled` set, cloning the
/// survivors. A diagnostic with no code or a numeric code is always kept.
pub fn filter_disabled_diagnostics(
    diags: &[Diagnostic],
    disabled: &FxHashSet<String>,
) -> Vec<Diagnostic> {
    if disabled.is_empty() {
        return diags.to_vec();
    }
    diags
        .iter()
        .filter(|d| {
            d.code.as_ref().is_none_or(|code| match code {
                NumberOrString::String(s) => !disabled.contains(s.as_str()),
                NumberOrString::Number(_) => true,
            })
        })
        .cloned()
        .collect()
}

/// Stamp `Diagnostic.data` with `{ "changedSince": "<git_ref>" }` on every
/// diagnostic when the LSP applied a `changedSince` filter to this run.
///
/// AI agents reading the Problems panel via `vscode.languages
/// .getDiagnostics()` can use this payload to verify that the filter is
/// active and skip "fixing" findings that the user has explicitly
/// baselined out. Standard LSP `Diagnostic.data` slot, no invented
/// top-level field. No-op when `changed_since` is `None` so unfiltered
/// runs ship a clean schema.
///
/// Merges into any existing `data` object rather than overwriting, so a
/// future `build_diagnostics` that stamps `data` for `codeAction/resolve`
/// tokens (the natural next step for code-action performance) does not
/// silently lose its payload to this stamp. If `data` is already a
/// non-object (string / number / array), the existing value is left alone
/// and `changedSince` is not stamped on that one diagnostic; that case is
/// not used by `build_diagnostics` today and is logged via the structured
/// fact that `data` for any fallow diagnostic should be an object.
pub fn attach_changed_since_data(
    diagnostics_by_file: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    changed_since: Option<&str>,
) {
    let Some(git_ref) = changed_since else {
        return;
    };
    let value = serde_json::Value::String(git_ref.to_string());
    for diags in diagnostics_by_file.values_mut() {
        for d in diags {
            match d.data.as_mut() {
                None => {
                    d.data = Some(serde_json::json!({ "changedSince": git_ref }));
                }
                Some(serde_json::Value::Object(obj)) => {
                    obj.insert("changedSince".to_string(), value.clone());
                }
                Some(_) => {}
            }
        }
    }
}
