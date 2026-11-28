//! The memory budget for YAML alias expansion.
//!
//! The parser expands each alias into a copy of the node that it names, and
//! limits aliases only by the number of nodes that they replay. A short
//! source can replay one long scalar many times, so the node limit alone does
//! not bound memory.

use deser_core::de::{Deserialize, Sink, SinkHandle};
use deser_core::{Atom, Error, ErrorKind, State};

/// The memory that alias expansion may add to the parsed documents of one
/// source, in bytes. Real configs expand to a few megabytes at most.
const MAX_ALIAS_EXPANSION_BYTES: usize = 256 * 1024 * 1024;

/// The memory of one parsed node beyond its text, in bytes.
const NODE_OVERHEAD_BYTES: usize = 128;

/// The node limit of the parser for aliases in a whole stream.
const MAX_ALIAS_EVENTS: usize = 1_000_000;

/// Run `parse` within the alias budget. `parse` takes the node limit for
/// aliases in each document.
///
/// The first attempt uses a node limit that keeps any source within the
/// budget: a replayed node holds no more text than the source, so the limit
/// is the budget divided by the source length. Most files replay far fewer
/// nodes and parse once. When a file replays more, the size of its expanded
/// documents is counted without copies, and the file parses with the full
/// node limit only when that size stays within the budget.
///
/// # Errors
///
/// Returns the text of the parse error, or of the refusal when the aliases
/// expand above the budget or above the node limit.
pub(super) fn within_alias_budget<T>(
    source: &str,
    parse: impl Fn(usize) -> Result<T, Error>,
) -> Result<T, String> {
    let documents = document_count_bound(source);
    let full_limit = MAX_ALIAS_EVENTS / documents;
    let per_node = source.len().saturating_add(NODE_OVERHEAD_BYTES);
    let quick_limit = (MAX_ALIAS_EXPANSION_BYTES / per_node / documents).min(full_limit);
    match parse(quick_limit) {
        Err(error) if error.kind() == ErrorKind::LimitExceeded && quick_limit < full_limit => {}
        result => return result.map_err(|error| refusal_text(&error)),
    }
    match expanded_bytes(source, full_limit) {
        Ok(bytes) if bytes <= MAX_ALIAS_EXPANSION_BYTES => {
            parse(full_limit).map_err(|error| refusal_text(&error))
        }
        Ok(_) => Err(refuse(format!(
            "aliases expand to more than {} MiB",
            MAX_ALIAS_EXPANSION_BYTES / (1024 * 1024)
        ))),
        Err(error) => Err(refusal_text(&error)),
    }
}

/// The text of a parse error. An error of the node limit also gets a warning.
fn refusal_text(error: &Error) -> String {
    if error.kind() == ErrorKind::LimitExceeded {
        refuse(format!(
            "{error} (the limit is {MAX_ALIAS_EVENTS} replayed nodes for each file)"
        ))
    } else {
        error.to_string()
    }
}

/// Warn about a file that the alias budget refuses, and return the text.
///
/// The plugin registry reads each config file in a span that names it, so
/// the warning names the file.
fn refuse(text: String) -> String {
    tracing::warn!("YAML file skipped: {text}");
    text
}

/// An upper bound of the number of documents in a YAML stream. Each document
/// after the first starts at a line that starts with `---` or follows a line
/// that starts with `...`.
fn document_count_bound(source: &str) -> usize {
    1 + source
        .split(['\n', '\r'])
        .filter(|line| line.starts_with("---") || line.starts_with("..."))
        .count()
}

/// The memory that the parsed documents of `source` hold, counted from the
/// events of the parser without copies of the text.
fn expanded_bytes(source: &str, alias_limit: usize) -> Result<usize, Error> {
    let config = deser_yaml::DeserializerConfig::builder()
        .alias_limit(alias_limit)
        .build();
    let mut deserializer = deser_yaml::Deserializer::from_str_with_config(source, config);
    let mut total = 0_usize;
    for document in deserializer.iter::<ExpandedSize>() {
        total = total.saturating_add(document?.0);
    }
    Ok(total)
}

/// The memory of one parsed document, in bytes.
struct ExpandedSize(usize);

impl<'de> Deserialize<'de> for ExpandedSize {
    fn deserialize_into<'a>(out: &'a mut Option<Self>, state: &mut State) -> SinkHandle<'a, 'de> {
        SinkHandle::arena(SizeSink { out, bytes: 0 }, state)
    }
}

/// Counts the text and the nodes of a document. Nested values go to the same
/// sink, so one count covers the whole document.
struct SizeSink<'a> {
    out: &'a mut Option<ExpandedSize>,
    bytes: usize,
}

impl SizeSink<'_> {
    fn add_node(&mut self, text: usize) {
        self.bytes = self
            .bytes
            .saturating_add(text)
            .saturating_add(NODE_OVERHEAD_BYTES);
    }
}

impl<'de> Sink<'de> for SizeSink<'_> {
    fn atom(&mut self, atom: Atom, _state: &mut State) -> Result<(), Error> {
        let text = match &atom {
            Atom::Str(text) | Atom::Lexical(text) => text.len(),
            Atom::Implicit(implicit) => implicit.text().len(),
            Atom::Bytes(bytes) => bytes.len(),
            _ => 0,
        };
        self.add_node(text);
        Ok(())
    }

    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        self.add_node(0);
        Ok(())
    }

    fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
        self.add_node(0);
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(SinkHandle::to(self))
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(SinkHandle::to(self))
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        *self.out = Some(ExpandedSize(self.bytes));
        Ok(())
    }
}
