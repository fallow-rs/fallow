//! The line-free input of the stable id of a security finding.

/// The detector facts that name a security finding without its line.
///
/// The stable id step reads these facts and the anchor file, and hashes the
/// result. A finding without these facts gets no `stable_finding_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityStableIdInput {
    /// The byte span of the full sink expression in the anchor file, after a
    /// leading byte order mark. For an assignment sink it is the full
    /// assignment. For a call sink it is the full call with all arguments.
    SinkSpan {
        /// Byte offset of the first byte of the sink.
        start: u32,
        /// Byte offset after the last byte of the sink.
        end: u32,
    },
    /// The detector signal of a hard-coded secret: a provider family such as
    /// `Stripe key`, or `entropy` for a high-entropy value. The secret value
    /// itself never enters the hash input.
    SecretSignal(String),
    /// The env secrets that the server module of a client-server leak reads,
    /// such as `process.env.DATABASE_PASSWORD`, sorted and without duplicates.
    SecretReads(Vec<String>),
}
