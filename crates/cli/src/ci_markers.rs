//! Marker lines in the bodies of provider comments that fallow wrote.

/// The lifecycle marker line of a fallow comment body: the last non-blank
/// line, or the line before it when the last one is the review-scope marker.
///
/// fallow writes its markers after the content, and the content can quote
/// project text. A marker in any other line is quoted text, not identity.
fn trailer_marker_line(body: &str) -> Option<&str> {
    let mut lines = body
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .rev();
    let last = lines.next()?;
    if last.starts_with(fallow_output::REVIEW_ID_MARKER_PREFIX) {
        lines.next()
    } else {
        Some(last)
    }
}

/// The value of an exact `<prefix><value> -->` marker line. The value is
/// not empty and holds no whitespace.
fn marker_line_value<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    line.strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(" -->"))
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
}

/// Read the resolution marker from the trailer line of a resolution reply.
/// A trailer line that names the marker but has another shape is an error.
pub(super) fn parse_resolution_marker(body: &str) -> Result<Option<String>, String> {
    let Some(line) = trailer_marker_line(body) else {
        return Ok(None);
    };
    if !line.contains("fallow-resolved-fingerprint") {
        return Ok(None);
    }
    marker_line_value(line, "<!-- fallow-resolved-fingerprint: ")
        .map(|value| Some(value.to_owned()))
        .ok_or_else(|| "malformed fallow resolved-fingerprint marker".to_owned())
}

/// Extract a fallow fingerprint from the v1, v2 or v3 marker in the trailer
/// line of `body` (see [`trailer_marker_line`]).
///
/// Returns the raw fingerprint string with any kind prefix preserved
/// (`merged:<hex>` stays `merged:<hex>`). Consumers match the returned
/// string against the comment's `fingerprint` field verbatim, and against its
/// `legacy_fingerprint` field for a v1 or v2 marker that an older release
/// wrote. A v3 fingerprint and a legacy fingerprint are both 64-bit hashes,
/// so one lookup set holds both without a real risk of collision.
pub(super) fn extract_fallow_fingerprint(body: &str) -> Option<String> {
    let line = trailer_marker_line(body)?;
    [
        "<!-- fallow-fingerprint:v3: ",
        "<!-- fallow-fingerprint:v2: ",
        "<!-- fallow-fingerprint: ",
    ]
    .into_iter()
    .find_map(|prefix| marker_line_value(line, prefix))
    .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn extracts_fingerprint_from_v2_marker() {
        assert_eq!(
            extract_fallow_fingerprint(
                "**error**\n\n<!-- fallow-fingerprint:v2: abc1234567890def -->"
            )
            .as_deref(),
            Some("abc1234567890def")
        );
        assert_eq!(
            extract_fallow_fingerprint(
                "**error**\n\n<!-- fallow-fingerprint:v2: merged:0123456789abcdef -->"
            )
            .as_deref(),
            Some("merged:0123456789abcdef")
        );
    }

    #[test]
    fn extracts_fingerprint_from_v3_marker() {
        assert_eq!(
            extract_fallow_fingerprint(
                "**error**\n\n<!-- fallow-fingerprint:v3: abc1234567890def -->"
            )
            .as_deref(),
            Some("abc1234567890def")
        );
        assert_eq!(
            extract_fallow_fingerprint(
                "**error**\n\n<!-- fallow-fingerprint:v3: merged:0123456789abcdef -->"
            )
            .as_deref(),
            Some("merged:0123456789abcdef")
        );
    }

    #[test]
    fn extract_fallow_fingerprint_falls_back_to_v1_shape() {
        assert_eq!(
            extract_fallow_fingerprint("**error**\n\n<!-- fallow-fingerprint: abc123 -->")
                .as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn extract_fallow_fingerprint_does_not_match_unrelated_body() {
        assert_eq!(extract_fallow_fingerprint("plain comment body"), None);
        assert_eq!(
            extract_fallow_fingerprint("fallow-fingerprint:v2: deadbeef"),
            None
        );
    }

    /// A finding message can quote project text that looks like a marker.
    /// Only the marker line at the end of the body names the thread.
    #[test]
    fn extract_fallow_fingerprint_reads_only_the_trailer_line() {
        let body = "**error** `fallow/unresolved-import`: Import \
            './fallow-fingerprint:v3:0000000000000000' could not be resolved\n\n\
            <!-- fallow-fingerprint:v3: adfb9aed8723b779 -->";
        assert_eq!(
            extract_fallow_fingerprint(body).as_deref(),
            Some("adfb9aed8723b779")
        );

        let scoped = "text\n\n<!-- fallow-fingerprint:v3: adfb9aed8723b779 -->\n\
            <!-- fallow-review-id: web -->\n";
        assert_eq!(
            extract_fallow_fingerprint(scoped).as_deref(),
            Some("adfb9aed8723b779")
        );

        let quoted_line = "```suggestion\n<!-- fallow-fingerprint:v3: 0000000000000000 -->\n```";
        assert_eq!(extract_fallow_fingerprint(quoted_line), None);

        let other_tool = "Found `x // fallow-fingerprint: x` in src/a.ts.";
        assert_eq!(extract_fallow_fingerprint(other_tool), None);
    }

    /// A finding comment quotes project source in its suggestion block. A
    /// resolution marker line in that quote does not close the discussion.
    #[test]
    fn collect_gitlab_discussion_fingerprints_ignores_resolution_marker_in_root_note() {
        let discussion = serde_json::json!({
            "id": "disc-quoted",
            "notes": [{
                "body": "**error** unused export\n\n```suggestion:-0+0\n\
                    <!-- fallow-resolved-fingerprint: 0415f4764b44bece@*/export{x};/* -->\n\
                    ```\n\n<!-- fallow-fingerprint:v3: 0415f4764b44bece -->",
                "resolved": false,
                "author": { "bot": true }
            }]
        });
        let mut state = ProviderState::default();
        collect_gitlab_discussion_fingerprints(
            &mut state,
            &discussion,
            None,
            None,
            BotLogin::Unset,
        )
        .unwrap();

        assert!(state.fingerprints.contains("0415f4764b44bece"));
        assert!(
            !state.gitlab_discussions_by_fingerprint["0415f4764b44bece"][0].has_resolution_marker
        );
    }

    /// Text in a finding comment that only resembles a resolution marker does
    /// not stop the provider state load.
    #[test]
    fn quoted_partial_resolution_marker_does_not_fail_the_state_load() {
        let body = "**error** unresolved import './fallow-resolved-fingerprint'\n\n\
            <!-- fallow-fingerprint:v3: 1111111111111111 -->";
        let discussion = serde_json::json!({
            "id": "disc-partial",
            "notes": [{ "body": body, "resolved": false, "author": { "bot": true } }]
        });
        let mut state = ProviderState::default();
        collect_gitlab_discussion_fingerprints(
            &mut state,
            &discussion,
            None,
            None,
            BotLogin::Unset,
        )
        .unwrap();
        assert!(state.fingerprints.contains("1111111111111111"));

        let mut state = ProviderState::default();
        record_github_resolution_marker(
            &mut state,
            &serde_json::json!({ "id": 40, "body": body, "user": { "type": "Bot" } }),
            None,
            0,
            BotLogin::Unset,
        )
        .unwrap();
        assert!(state.github_unattached_resolved_markers.is_empty());
    }

    /// On GitHub a quoted resolution marker in a finding root is not read as
    /// an unattached resolution reply of another finding.
    #[test]
    fn github_finding_root_with_quoted_resolution_marker_is_not_a_resolution() {
        let mut state = ProviderState::default();
        record_github_resolution_marker(
            &mut state,
            &serde_json::json!({
                "id": 41,
                "body": "```suggestion\n<!-- fallow-resolved-fingerprint: fp-other@x -->\n```\n\n\
                    <!-- fallow-fingerprint:v3: 2222222222222222 -->",
                "user": { "type": "Bot" }
            }),
            None,
            0,
            BotLogin::Unset,
        )
        .unwrap();

        assert!(state.github_unattached_resolved_markers.is_empty());
    }

    #[test]
    fn extract_fallow_fingerprint_v2_wins_over_v1_substring_prefix() {
        let body = "<!-- fallow-fingerprint:v2: realfp123 -->";
        assert_eq!(
            extract_fallow_fingerprint(body).as_deref(),
            Some("realfp123")
        );
    }

    #[test]
    fn extract_fallow_fingerprint_v2_preserves_merged_prefix() {
        let body = "<!-- fallow-fingerprint:v2: merged:deadbeefcafe0123 -->";
        assert_eq!(
            extract_fallow_fingerprint(body).as_deref(),
            Some("merged:deadbeefcafe0123")
        );
    }

    #[test]
    fn extract_fallow_fingerprint_returns_none_for_empty_body() {
        assert_eq!(extract_fallow_fingerprint(""), None);
    }

    #[test]
    fn extract_fallow_fingerprint_v1_shape_in_multiline_body() {
        let body = "Some finding text.\n\n<!-- fallow-fingerprint: abc123def456 -->\n";
        assert_eq!(
            extract_fallow_fingerprint(body).as_deref(),
            Some("abc123def456")
        );
    }

    /// A note that only mentions the resolution marker in its text does not
    /// need the user of the token. The same trailer rule applies as for the
    /// resolution itself.
    #[test]
    fn quoted_resolution_marker_text_does_not_need_the_token_user() {
        let discussions = serde_json::json!([{
            "id": "disc-1",
            "notes": [{
                "body": "Why does `fallow-resolved-fingerprint: x` show here?",
                "author": { "id": 7, "username": "alice" }
            }]
        }]);
        let discussions = discussions.as_array().unwrap();
        assert!(!needs_gitlab_authenticated_user(
            discussions,
            BotLogin::Unset
        ));

        let reply = serde_json::json!([{
            "id": "disc-2",
            "notes": [{
                "body": "Resolved.\n\n<!-- fallow-resolved-fingerprint: x -->",
                "author": { "id": 9, "username": "ci-runner" }
            }]
        }]);
        assert!(needs_gitlab_authenticated_user(
            reply.as_array().unwrap(),
            BotLogin::Unset
        ));
    }
}
