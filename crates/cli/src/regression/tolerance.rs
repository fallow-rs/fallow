/// How much increase is allowed before a regression is flagged.
#[derive(Debug, Clone, Copy)]
pub enum Tolerance {
    /// Percentage increase relative to the baseline total (e.g., 2.0 means 2%).
    Percentage(f64),
    /// Absolute increase in issue count.
    Absolute(usize),
}

impl Tolerance {
    /// The largest issue-count increase this tolerance allows, in the same unit
    /// as the delta it is compared against.
    ///
    /// Mirrors [`Self::exceeded`] exactly, floor included, so a consumer
    /// comparing a published delta against a published allowance reaches the
    /// same verdict the gate did. A percentage tolerance against an empty
    /// baseline allows nothing, which is the branch `exceeded` takes when
    /// `baseline_total` is zero.
    #[must_use]
    pub fn allowed_delta(&self, baseline_total: usize) -> f64 {
        match *self {
            Self::Percentage(pct) => {
                if baseline_total == 0 {
                    return 0.0;
                }
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "an issue count never approaches the f64 integer limit"
                )]
                let baseline = baseline_total as f64;
                (baseline * pct / 100.0).floor()
            }
            #[expect(
                clippy::cast_precision_loss,
                reason = "an issue-count tolerance never approaches the f64 integer limit"
            )]
            Self::Absolute(count) => count as f64,
        }
    }

    /// The tolerance as the user spelled it: `"50%"` or `"5"`.
    ///
    /// Published beside the allowance so the unit is recoverable from the entry
    /// alone, which the grouped envelope needs because it carries no
    /// `regression` object to read `tolerance_kind` from.
    #[must_use]
    pub fn label(&self) -> String {
        match *self {
            Self::Percentage(percent) => format!("{percent}%"),
            Self::Absolute(count) => count.to_string(),
        }
    }

    /// Parse a tolerance string: `"2%"` for percentage, `"5"` for absolute.
    /// Default when no value is given: `Absolute(0)` (zero tolerance).
    ///
    /// # Errors
    ///
    /// Returns an error if the string is not a valid number or percentage,
    /// or if a percentage value is negative.
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(Self::Absolute(0));
        }
        if let Some(pct_str) = s.strip_suffix('%') {
            let pct: f64 = pct_str
                .trim()
                .parse()
                .map_err(|_| format!("invalid tolerance percentage: {s}"))?;
            if pct < 0.0 {
                return Err(format!("tolerance percentage must be non-negative: {s}"));
            }
            Ok(Self::Percentage(pct))
        } else {
            let abs: usize = s
                .parse()
                .map_err(|_| format!("invalid tolerance value: {s} (use a number or N%)"))?;
            Ok(Self::Absolute(abs))
        }
    }

    /// Check whether the delta exceeds this tolerance.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "percentage of a count is bounded by the count itself"
    )]
    pub fn exceeded(&self, baseline_total: usize, current_total: usize) -> bool {
        if current_total <= baseline_total {
            return false;
        }
        let delta = current_total - baseline_total;
        match *self {
            Self::Percentage(pct) => {
                if baseline_total == 0 {
                    return delta > 0;
                }
                let allowed = (baseline_total as f64 * pct / 100.0).floor() as usize;
                delta > allowed
            }
            Self::Absolute(abs) => delta > abs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The allowance must be in the delta's unit, or a percentage tolerance
    /// renders as a count on every surface that shows the two side by side.
    #[test]
    fn allowed_delta_is_in_the_same_unit_as_the_delta() {
        assert!((Tolerance::Absolute(5).allowed_delta(100) - 5.0).abs() < f64::EPSILON);
        assert!((Tolerance::Percentage(50.0).allowed_delta(100) - 50.0).abs() < f64::EPSILON);
        assert!((Tolerance::Percentage(50.0).allowed_delta(3) - 1.0).abs() < f64::EPSILON);
        assert!((Tolerance::Percentage(50.0).allowed_delta(0) - 0.0).abs() < f64::EPSILON);
    }

    /// The published allowance and the gate must agree at the boundary.
    #[test]
    fn allowed_delta_agrees_with_exceeded() {
        for tolerance in [
            Tolerance::Absolute(0),
            Tolerance::Absolute(5),
            Tolerance::Percentage(50.0),
            Tolerance::Percentage(10.0),
        ] {
            for baseline in [0_usize, 1, 3, 10, 100] {
                for delta in 0_usize..12 {
                    let current = baseline + delta;
                    #[expect(clippy::cast_precision_loss, reason = "small test values")]
                    let delta_f = delta as f64;
                    assert_eq!(
                        tolerance.exceeded(baseline, current),
                        delta_f > tolerance.allowed_delta(baseline),
                        "{tolerance:?} baseline {baseline} delta {delta}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_label_is_the_spelling_the_user_used() {
        assert_eq!(Tolerance::Percentage(50.0).label(), "50%");
        assert_eq!(Tolerance::Absolute(5).label(), "5");
    }

    #[test]
    fn parse_percentage_tolerance() {
        let t = Tolerance::parse("2%").unwrap();
        assert!(matches!(t, Tolerance::Percentage(p) if (p - 2.0).abs() < f64::EPSILON));
    }

    #[test]
    fn parse_absolute_tolerance() {
        let t = Tolerance::parse("5").unwrap();
        assert!(matches!(t, Tolerance::Absolute(5)));
    }

    #[test]
    fn parse_zero_tolerance() {
        let t = Tolerance::parse("0").unwrap();
        assert!(matches!(t, Tolerance::Absolute(0)));
    }

    #[test]
    fn parse_empty_defaults_to_zero() {
        let t = Tolerance::parse("").unwrap();
        assert!(matches!(t, Tolerance::Absolute(0)));
    }

    #[test]
    fn parse_invalid_percentage() {
        assert!(Tolerance::parse("abc%").is_err());
    }

    #[test]
    fn parse_negative_percentage() {
        assert!(Tolerance::parse("-1%").is_err());
    }

    #[test]
    fn parse_invalid_absolute() {
        assert!(Tolerance::parse("abc").is_err());
    }

    #[test]
    fn zero_tolerance_detects_any_increase() {
        let t = Tolerance::Absolute(0);
        assert!(t.exceeded(10, 11));
        assert!(!t.exceeded(10, 10));
        assert!(!t.exceeded(10, 9));
    }

    #[test]
    fn absolute_tolerance_allows_within_range() {
        let t = Tolerance::Absolute(3);
        assert!(!t.exceeded(10, 12));
        assert!(!t.exceeded(10, 13));
        assert!(t.exceeded(10, 14));
    }

    #[test]
    fn percentage_tolerance_allows_within_range() {
        let t = Tolerance::Percentage(10.0);
        assert!(!t.exceeded(100, 109));
        assert!(!t.exceeded(100, 110));
        assert!(t.exceeded(100, 111));
    }

    #[test]
    fn percentage_tolerance_from_zero_baseline() {
        let t = Tolerance::Percentage(10.0);
        assert!(t.exceeded(0, 1));
        assert!(!t.exceeded(0, 0));
    }

    #[test]
    fn decrease_never_exceeds() {
        let t = Tolerance::Absolute(0);
        assert!(!t.exceeded(10, 5));
        let t = Tolerance::Percentage(0.0);
        assert!(!t.exceeded(10, 5));
    }

    #[test]
    fn parse_whitespace_padded_tolerance() {
        let t = Tolerance::parse("  5  ").unwrap();
        assert!(matches!(t, Tolerance::Absolute(5)));
    }

    #[test]
    fn parse_whitespace_only_defaults_to_zero() {
        let t = Tolerance::parse("   ").unwrap();
        assert!(matches!(t, Tolerance::Absolute(0)));
    }

    #[test]
    fn parse_zero_percent_tolerance() {
        let t = Tolerance::parse("0%").unwrap();
        assert!(matches!(t, Tolerance::Percentage(p) if p == 0.0));
    }

    #[test]
    fn parse_decimal_percentage_tolerance() {
        let t = Tolerance::parse("1.5%").unwrap();
        assert!(matches!(t, Tolerance::Percentage(p) if (p - 1.5).abs() < f64::EPSILON));
    }

    #[test]
    fn parse_large_absolute_tolerance() {
        let t = Tolerance::parse("1000").unwrap();
        assert!(matches!(t, Tolerance::Absolute(1000)));
    }

    #[test]
    fn parse_negative_absolute_is_err() {
        assert!(Tolerance::parse("-1").is_err());
    }

    #[test]
    fn parse_whitespace_padded_percentage() {
        let t = Tolerance::parse("  3.5%  ").unwrap();
        assert!(matches!(t, Tolerance::Percentage(p) if (p - 3.5).abs() < f64::EPSILON));
    }

    #[test]
    fn zero_pct_tolerance_detects_any_increase() {
        let t = Tolerance::Percentage(0.0);
        assert!(t.exceeded(100, 101));
        assert!(!t.exceeded(100, 100));
        assert!(!t.exceeded(100, 99));
    }

    #[test]
    fn percentage_tolerance_with_small_baseline() {
        let t = Tolerance::Percentage(10.0);
        assert!(t.exceeded(3, 4));
        assert!(!t.exceeded(3, 3));
    }

    #[test]
    fn percentage_tolerance_large_percentage() {
        let t = Tolerance::Percentage(100.0);
        assert!(!t.exceeded(10, 20));
        assert!(t.exceeded(10, 21));
    }

    #[test]
    fn absolute_tolerance_at_exact_boundary() {
        let t = Tolerance::Absolute(5);
        assert!(!t.exceeded(10, 15));
        assert!(t.exceeded(10, 16));
    }

    #[test]
    fn decrease_never_exceeds_for_all_variants() {
        let t = Tolerance::Absolute(0);
        assert!(!t.exceeded(10, 0));
        let t = Tolerance::Percentage(0.0);
        assert!(!t.exceeded(10, 0));
    }

    #[test]
    fn equal_values_never_exceed() {
        assert!(!Tolerance::Absolute(0).exceeded(0, 0));
        assert!(!Tolerance::Percentage(0.0).exceeded(0, 0));
        assert!(!Tolerance::Absolute(0).exceeded(100, 100));
        assert!(!Tolerance::Percentage(0.0).exceeded(100, 100));
    }
}
