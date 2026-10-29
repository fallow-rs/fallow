use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum JsonStyle {
    #[default]
    Compact,
    Pretty,
    /// The `--lean` profile. The document goes through the lean pass in
    /// [`fallow_output::apply_lean_profile`] before it is written.
    Lean(LeanStyle),
}

/// Options of the `--lean` profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeanStyle {
    /// `--pretty` was also given.
    pub pretty: bool,
    /// Maximum rows per health table. `None` keeps every row (`--full-tables`).
    pub table_cap: Option<usize>,
}

impl JsonStyle {
    pub fn serialize<T: Serialize + ?Sized>(self, value: &T) -> Result<String, serde_json::Error> {
        match self {
            Self::Compact => serde_json::to_string(value),
            Self::Pretty => serde_json::to_string_pretty(value),
            Self::Lean(lean) => {
                let mut document = serde_json::to_value(value)?;
                fallow_output::apply_lean_profile(&mut document, lean.table_cap);
                if lean.pretty {
                    serde_json::to_string_pretty(&document)
                } else {
                    serde_json::to_string(&document)
                }
            }
        }
    }
}

/// Global flags that change how the JSON document is written.
#[derive(clap::Args, Clone, Copy, Debug, Default)]
pub struct JsonPresentationArgs {
    /// Indent JSON output for manual inspection. Requires the final output format to be JSON.
    #[arg(hide_short_help = true, long, global = true)]
    pub pretty: bool,

    /// Trim JSON for agent context: leave out empty arrays and zero dead-code summary counts,
    /// group workspace diagnostics, cap health tables, and move repeated action text to
    /// `_meta.actions`. Requires JSON output. Applies to dead-code, health and the combined
    /// report; other commands reject it. Run without `--lean` for the full diagnostic list
    /// and the complete tables.
    #[arg(hide_short_help = true, long, global = true)]
    pub lean: bool,

    /// With `--lean`, keep every row of the health tables (`hotspots`, `file_scores`,
    /// `large_functions`).
    #[arg(hide_short_help = true, long, global = true, requires = "lean")]
    pub full_tables: bool,
}
