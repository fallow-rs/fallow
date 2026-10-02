//! Signed difference between two issue counts.

/// Returns `current - previous` as an `i64`, saturated at the `i64` bounds.
///
/// Counts are `usize`, so a plain `as i64` cast can wrap on a 64-bit target.
pub fn signed_delta(current: usize, previous: usize) -> i64 {
    if current >= previous {
        i64::try_from(current - previous).unwrap_or(i64::MAX)
    } else {
        i64::try_from(previous - current).map_or(i64::MIN, |gap| -gap)
    }
}

/// Converts a count to `i64`, saturated at `i64::MAX`.
pub fn count_to_i64(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_is_signed() {
        assert_eq!(signed_delta(7, 3), 4);
        assert_eq!(signed_delta(3, 7), -4);
        assert_eq!(signed_delta(5, 5), 0);
    }

    #[test]
    fn delta_saturates_instead_of_wrapping() {
        assert_eq!(signed_delta(usize::MAX, 0), i64::MAX);
        assert_eq!(signed_delta(0, usize::MAX), i64::MIN);
    }

    #[test]
    fn count_saturates_instead_of_wrapping() {
        assert_eq!(count_to_i64(42), 42);
        assert_eq!(count_to_i64(usize::MAX), i64::MAX);
    }
}
