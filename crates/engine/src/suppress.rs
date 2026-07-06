//! Suppression helpers exposed for editor and embedding surfaces.

pub use fallow_extract::suppress::parse_suppressions_from_source;
pub use fallow_types::suppress::{IssueKind, Suppression, is_file_suppressed, is_suppressed};
