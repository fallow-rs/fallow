//! Namespace bindings that get the full export credit of their target.
//!
//! A namespace binding (`import * as Ns`, or a named import of an
//! `export * as Ns` re-export) that the code uses as a whole value can read
//! any member through a computed key. The graph then credits every export of
//! the target module instead of the members that the code reads by name. The
//! credit sites record a [`NamespaceEscape`] here, so the analysis can report
//! the bindings that hide exports from the unused-export check (issue #5054).
//!
//! The record is written by the same branches that give the full credit, so
//! the reported set and the credited set cannot differ.

use fallow_types::discover::FileId;

/// Why a namespace binding gets the full credit of its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NamespaceEscapeReason {
    /// The code uses the binding as a value: a cast, an assignment, a call
    /// argument, a spread, a computed member read, or a reflective call such
    /// as `Object.keys(Ns)`.
    WholeObjectUse,
    /// A non-entry module re-exports the binding with `export { Ns }`, so the
    /// graph cannot see which members its consumers read.
    ReExportedBinding,
    /// The code reads no member of the binding by name and does not use it
    /// as a value (for example only `typeof Ns`).
    NoMemberRead,
}

/// One namespace binding that gets the full export credit of `target`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NamespaceEscape {
    /// The file that declares the binding.
    pub consumer: FileId,
    /// The span of the import that declares the binding.
    #[serde(with = "crate::cache::span_serde")]
    pub import_span: oxc_span::Span,
    /// The local name of the binding.
    pub local_name: String,
    /// The module whose exports get the full credit.
    pub target: FileId,
    /// Why the binding gets the full credit.
    pub reason: NamespaceEscapeReason,
    /// The members that the code also reads by name through the binding.
    /// The full credit does not hide these members.
    pub read_members: Vec<String>,
}

/// Sort the records of one graph build and keep one record per binding.
///
/// A binding can reach the credit code once per namespace lane (type and
/// value), and both lanes give the same record.
pub(in crate::graph) fn normalize(escapes: &mut Vec<NamespaceEscape>) {
    escapes.sort_by(|a, b| {
        (a.consumer.0, a.import_span.start, a.target.0, &a.local_name).cmp(&(
            b.consumer.0,
            b.import_span.start,
            b.target.0,
            &b.local_name,
        ))
    });
    escapes.dedup_by(|a, b| {
        a.consumer == b.consumer
            && a.import_span == b.import_span
            && a.target == b.target
            && a.local_name == b.local_name
    });
}
