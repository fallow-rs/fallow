//! Layout of the sticky PR summary comment.

/// Which parts of the sticky PR comment to render. The `comment-layout`
/// input of the GitHub Action and `FALLOW_PR_COMMENT_LAYOUT` select it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrCommentLayout {
    /// The verdict, the status note, one collapsed table per section, and the
    /// trailing sections.
    Default,
    /// The verdict, the status note, and one row per section with its counts,
    /// in place of the finding tables.
    Compact,
    /// The verdict and the status note only, for a repository where the check
    /// run is the main PR surface.
    GateOnly,
    /// The same parts as `Default`, with each section open.
    Details,
}
