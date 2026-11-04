//! Parse the minimum clone occurrence count.

/// Parse `--min-occurrences` and reject values below 2. A single occurrence
/// is not a duplicate; silently clamping would diverge from the config-file
/// validator, which also rejects `< 2`.
pub(super) fn parse_min_occurrences(s: &str) -> Result<usize, String> {
    let value: usize = s
        .parse()
        .map_err(|_| format!("`{s}` is not a non-negative integer"))?;
    if value < 2 {
        return Err(format!(
            "must be at least 2 (got {value}); a single occurrence isn't a duplicate"
        ));
    }
    Ok(value)
}
