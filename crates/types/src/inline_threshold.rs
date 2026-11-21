//! Inline health threshold override comments.
//!
//! A `fallow-override-threshold-next-line` comment raises or lowers the health
//! ceilings of the function on the next line:
//!
//! ```text
//! // fallow-override-threshold-next-line maxCognitive=40 maxCrap=45 -- legacy parser
//! ```
//!
//! The keys are the keys of a `health.thresholdOverrides` entry. The reason
//! after ` -- ` is required.

/// The comment word of an inline threshold override.
pub const INLINE_THRESHOLD_MARKER: &str = "fallow-override-threshold-next-line";

/// The threshold keys that an inline threshold override comment accepts.
pub const INLINE_THRESHOLD_KEYS: [&str; 4] =
    ["maxCyclomatic", "maxCognitive", "maxCrap", "maxUnitSize"];

/// One inline threshold override comment that parsed without a problem.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineThresholdOverride {
    /// 1-based line that the comment applies to: the line after the comment.
    pub line: u32,
    /// 1-based line of the comment.
    pub comment_line: u32,
    /// Cyclomatic ceiling, when the comment sets one.
    pub max_cyclomatic: Option<u16>,
    /// Cognitive ceiling, when the comment sets one.
    pub max_cognitive: Option<u16>,
    /// CRAP ceiling, when the comment sets one.
    pub max_crap: Option<f64>,
    /// Unit-size ceiling, when the comment sets one.
    pub max_unit_size: Option<u32>,
    /// The reason after ` -- `.
    pub reason: String,
}

/// What is wrong with one inline threshold override comment. A comment with a
/// problem has no effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum InlineThresholdProblemKind {
    /// The comment has no reason after ` -- `.
    MissingReason,
    /// The comment sets no threshold.
    MissingThreshold,
    /// A token is not a known key, or a key occurs two times.
    InvalidKey,
    /// A value is not a valid number for its key.
    InvalidValue,
    /// `health.inlineThresholdOverrides` is `false`, so the comment form is off.
    Disabled,
}

impl InlineThresholdProblemKind {
    /// Stable one-byte code for cache storage.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::MissingReason => 1,
            Self::MissingThreshold => 2,
            Self::InvalidKey => 3,
            Self::InvalidValue => 4,
            Self::Disabled => 5,
        }
    }

    /// Read a code that [`Self::to_code`] wrote.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::MissingReason),
            2 => Some(Self::MissingThreshold),
            3 => Some(Self::InvalidKey),
            4 => Some(Self::InvalidValue),
            5 => Some(Self::Disabled),
            _ => None,
        }
    }

    /// One-sentence description for human output.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::MissingReason => {
                "the comment has no reason; add ` -- <reason>` after the thresholds"
            }
            Self::MissingThreshold => {
                "the comment sets no threshold; use maxCyclomatic, maxCognitive, maxCrap or maxUnitSize"
            }
            Self::InvalidKey => {
                "unknown or repeated key; write each threshold as key=value (maxCyclomatic, maxCognitive, maxCrap or maxUnitSize), each one time"
            }
            Self::InvalidValue => {
                "the value is not valid; use a whole number (a finite number for maxCrap), as in maxCognitive=40"
            }
            Self::Disabled => {
                "health.inlineThresholdOverrides is false, so the comment has no effect"
            }
        }
    }
}

/// One inline threshold override comment with a problem.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InlineThresholdProblem {
    /// 1-based line of the comment.
    pub line: u32,
    /// What is wrong with the comment.
    #[serde(rename = "problem")]
    pub kind: InlineThresholdProblemKind,
    /// The token that has the problem, when one token has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// The inline threshold override comments of one file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InlineThresholdOverrides {
    /// The comments that parsed without a problem.
    pub entries: Vec<InlineThresholdOverride>,
    /// The comments with a problem. They have no effect.
    pub problems: Vec<InlineThresholdProblem>,
}

impl InlineThresholdOverrides {
    /// True when the file has no inline threshold override comment.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.problems.is_empty()
    }

    /// Remove the comments on `comment_lines`.
    pub fn drop_comment_lines(&mut self, comment_lines: &[u32]) {
        self.entries
            .retain(|entry| !comment_lines.contains(&entry.comment_line));
        self.problems
            .retain(|problem| !comment_lines.contains(&problem.line));
    }
}

/// Parse the text after the marker word of one inline threshold override
/// comment. `targets` is the text before ` -- `, and `reason` is the text
/// after it.
///
/// # Errors
///
/// Returns the problem of the comment. A comment with a problem has no effect.
pub fn parse_inline_threshold_override(
    line: u32,
    comment_line: u32,
    targets: &str,
    reason: Option<String>,
) -> Result<InlineThresholdOverride, InlineThresholdProblem> {
    let problem = |kind, token: Option<&str>| InlineThresholdProblem {
        line: comment_line,
        kind,
        token: token.map(str::to_string),
    };
    let mut entry = InlineThresholdOverride {
        line,
        comment_line,
        max_cyclomatic: None,
        max_cognitive: None,
        max_crap: None,
        max_unit_size: None,
        reason: String::new(),
    };
    let mut seen: Vec<&str> = Vec::new();
    for token in targets
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|token| !token.is_empty())
    {
        let Some((key, value)) = token.split_once('=') else {
            return Err(problem(InlineThresholdProblemKind::InvalidKey, Some(token)));
        };
        if !INLINE_THRESHOLD_KEYS.contains(&key) || seen.contains(&key) {
            return Err(problem(InlineThresholdProblemKind::InvalidKey, Some(token)));
        }
        seen.push(key);
        let invalid = || problem(InlineThresholdProblemKind::InvalidValue, Some(token));
        match key {
            "maxCyclomatic" => entry.max_cyclomatic = Some(value.parse().map_err(|_| invalid())?),
            "maxCognitive" => entry.max_cognitive = Some(value.parse().map_err(|_| invalid())?),
            "maxUnitSize" => entry.max_unit_size = Some(value.parse().map_err(|_| invalid())?),
            _ => {
                let crap: f64 = value.parse().map_err(|_| invalid())?;
                if !crap.is_finite() || crap < 0.0 {
                    return Err(invalid());
                }
                entry.max_crap = Some(crap);
            }
        }
    }
    if seen.is_empty() {
        return Err(problem(InlineThresholdProblemKind::MissingThreshold, None));
    }
    let Some(reason) = reason else {
        return Err(problem(InlineThresholdProblemKind::MissingReason, None));
    };
    entry.reason = reason;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(
        targets: &str,
        reason: Option<&str>,
    ) -> Result<InlineThresholdOverride, InlineThresholdProblem> {
        parse_inline_threshold_override(5, 4, targets, reason.map(str::to_string))
    }

    #[test]
    fn parses_every_key() {
        let entry = parse(
            "maxCyclomatic=30 maxCognitive=40, maxCrap=45.5 maxUnitSize=120",
            Some("legacy"),
        )
        .unwrap();
        assert_eq!(entry.line, 5);
        assert_eq!(entry.comment_line, 4);
        assert_eq!(entry.max_cyclomatic, Some(30));
        assert_eq!(entry.max_cognitive, Some(40));
        assert_eq!(entry.max_crap, Some(45.5));
        assert_eq!(entry.max_unit_size, Some(120));
        assert_eq!(entry.reason, "legacy");
    }

    #[test]
    fn rejects_a_comment_without_reason() {
        let problem = parse("maxCognitive=40", None).unwrap_err();
        assert_eq!(problem.kind, InlineThresholdProblemKind::MissingReason);
        assert_eq!(problem.line, 4);
    }

    #[test]
    fn rejects_a_comment_without_threshold() {
        let problem = parse("", Some("why")).unwrap_err();
        assert_eq!(problem.kind, InlineThresholdProblemKind::MissingThreshold);
    }

    #[test]
    fn rejects_bad_keys_and_values() {
        for (targets, kind, token) in [
            (
                "maxCognitive:40",
                InlineThresholdProblemKind::InvalidKey,
                "maxCognitive:40",
            ),
            (
                "maxFileLines=900",
                InlineThresholdProblemKind::InvalidKey,
                "maxFileLines=900",
            ),
            (
                "maxCognitive=40 maxCognitive=50",
                InlineThresholdProblemKind::InvalidKey,
                "maxCognitive=50",
            ),
            (
                "maxCognitive=forty",
                InlineThresholdProblemKind::InvalidValue,
                "maxCognitive=forty",
            ),
            (
                "maxCyclomatic=-1",
                InlineThresholdProblemKind::InvalidValue,
                "maxCyclomatic=-1",
            ),
            (
                "maxCrap=NaN",
                InlineThresholdProblemKind::InvalidValue,
                "maxCrap=NaN",
            ),
            (
                "maxCrap=-2",
                InlineThresholdProblemKind::InvalidValue,
                "maxCrap=-2",
            ),
        ] {
            let problem = parse(targets, Some("why")).unwrap_err();
            assert_eq!(problem.kind, kind, "{targets}");
            assert_eq!(problem.token.as_deref(), Some(token), "{targets}");
        }
    }

    #[test]
    fn problem_codes_round_trip() {
        for kind in [
            InlineThresholdProblemKind::MissingReason,
            InlineThresholdProblemKind::MissingThreshold,
            InlineThresholdProblemKind::InvalidKey,
            InlineThresholdProblemKind::InvalidValue,
            InlineThresholdProblemKind::Disabled,
        ] {
            assert_eq!(
                InlineThresholdProblemKind::from_code(kind.to_code()),
                Some(kind)
            );
        }
        assert_eq!(InlineThresholdProblemKind::from_code(0), None);
    }
}
