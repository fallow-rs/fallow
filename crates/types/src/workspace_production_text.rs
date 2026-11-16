//! The message text of the `production-excluded-file-imported` kind.

/// The message for a production pattern that excluded `file_count` files that
/// production code imports. `files` is the capped sample of those files.
pub(super) fn excluded_import_message(pattern: &str, file_count: u32, files: &[String]) -> String {
    let noun = if file_count == 1 { "file" } else { "files" };
    let sample = files.join(", ");
    let more = (file_count as usize)
        .checked_sub(files.len())
        .filter(|rest| *rest > 0)
        .map_or_else(String::new, |rest| format!(" and {rest} more"));
    format!(
        "The production pattern '{pattern}' excluded {file_count} {noun} that production code \
         imports: {sample}{more}. Fallow does not analyze these files in production mode, so an \
         export or a parameter that only they use can be reported. Move the files out of the \
         matched path, or turn production mode off for dead code with --no-production or \
         `production: {{ deadCode: false }}` in the config."
    )
}
