//! The human text of a [`StaleSuppression`]: the marker, the explanation of
//! why it is stale, and the per-format display message.

use crate::suppress::closest_known_kind_name;

use super::{StaleSuppression, SuppressionOrigin};

impl StaleSuppression {
    /// Produce a human-readable description of this stale suppression.
    #[must_use]
    pub fn description(&self) -> String {
        match &self.origin {
            SuppressionOrigin::Comment {
                issue_kind,
                reason,
                is_file_level,
                is_range,
                ..
            } => {
                let directive = if *is_range {
                    "fallow-ignore-start"
                } else if *is_file_level {
                    "fallow-ignore-file"
                } else {
                    "fallow-ignore-next-line"
                };
                match issue_kind {
                    Some(kind) => match reason {
                        Some(reason) => format!("// {directive} {kind} -- {reason}"),
                        None => format!("// {directive} {kind}"),
                    },
                    None => match reason {
                        Some(reason) => format!("// {directive} -- {reason}"),
                        None => format!("// {directive}"),
                    },
                }
            }
            SuppressionOrigin::JsdocTag {
                export_name,
                reason,
            } => match reason {
                Some(reason) => format!("@expected-unused on {export_name} -- {reason}"),
                None => format!("@expected-unused on {export_name}"),
            },
        }
    }

    /// Produce an explanation of why this suppression is stale.
    ///
    /// For comment suppressions where `kind_known == false`, surfaces the
    /// unknown token plus a Levenshtein "did you mean?" hint when one is
    /// within edit distance 2. Other tokens on the same comment line still
    /// apply normally (see issue #449).
    #[must_use]
    pub fn explanation(&self) -> String {
        match &self.origin {
            SuppressionOrigin::Comment {
                issue_kind,
                is_file_level,
                kind_known,
                ..
            } => {
                if self.missing_reason {
                    return "suppression is missing a reason".to_string();
                }
                let scope = if *is_file_level {
                    "in this file"
                } else {
                    "on the next line"
                };
                match issue_kind {
                    Some(kind) if !*kind_known => match closest_known_kind_name(kind) {
                        Some(suggestion) => format!(
                            "'{kind}' is not a recognized fallow issue kind. Did you mean '{suggestion}'? Other tokens on this line still apply."
                        ),
                        None => format!(
                            "'{kind}' is not a recognized fallow issue kind. Other tokens on this line still apply."
                        ),
                    },
                    Some(kind) => format!("no {kind} issue found {scope}{}", self.next_line_hint()),
                    None => format!("no issues found {scope}{}", self.next_line_hint()),
                }
            }
            SuppressionOrigin::JsdocTag { export_name, .. } => {
                if self.missing_reason {
                    return "suppression is missing a reason".to_string();
                }
                format!("{export_name} is now used")
            }
        }
    }

    /// The cause that the line after a stale next-line marker shows, with a
    /// leading `; `, or an empty string when the line shows no cause.
    fn next_line_hint(&self) -> String {
        if let Some(kinds) = self.next_line_kind_list() {
            let found = if self.next_line_kinds.len() == 1 {
                let article = if kinds.starts_with(['a', 'e', 'i', 'o', 'u']) {
                    "an"
                } else {
                    "a"
                };
                format!("{article} {kinds} issue")
            } else {
                format!("{kinds} issues")
            };
            return format!(
                "; the next line has {found}, use `fallow-ignore-next-line {}`",
                self.next_line_kinds.join(", ")
            );
        }
        if self.next_line_blank {
            return "; the next line is blank, and a next-line marker applies only to the line directly after it".to_string();
        }
        String::new()
    }

    /// The kinds on the next line, joined for prose, or `None` when empty.
    fn next_line_kind_list(&self) -> Option<String> {
        match self.next_line_kinds.as_slice() {
            [] => None,
            [only] => Some(only.clone()),
            [rest @ .., last] => Some(format!("{} and {last}", rest.join(", "))),
        }
    }

    /// Per-format display message combining `description()` and `explanation()`
    /// for the unknown-kind case so SARIF, CodeClimate, and compact consumers
    /// surface the typo-fix copy and Levenshtein hint without needing to
    /// branch on `origin.kind_known` themselves. A stale next-line marker
    /// whose target line shows the cause (`next_line_kinds` or
    /// `next_line_blank`) also carries the explanation (issue #5164). Other
    /// stale-but-known and JSDoc origins keep the bare `description()`, so
    /// their wire bytes stay unchanged. See issue #449.
    #[must_use]
    pub fn display_message(&self) -> String {
        match &self.origin {
            SuppressionOrigin::Comment {
                kind_known: false, ..
            } => format!("{} ({})", self.description(), self.explanation()),
            SuppressionOrigin::Comment { .. } | SuppressionOrigin::JsdocTag { .. }
                if self.missing_reason
                    || !self.next_line_kinds.is_empty()
                    || self.next_line_blank =>
            {
                format!("{} ({})", self.description(), self.explanation())
            }
            SuppressionOrigin::Comment { .. } | SuppressionOrigin::JsdocTag { .. } => {
                self.description()
            }
        }
    }
}
