//! Ownership risk analysis for hotspot files.
//!
//! Computes per-file ownership signals from git author history:
//!
//! - **Bus factor** (Avelino truck factor): minimum number of contributors who
//!   together account for at least 50% of recency-weighted commits in the
//!   analysis window.
//! - **Contributor count**: distinct authors after bot filtering.
//! - **Top contributor**: the single highest-share author with their share,
//!   commit count, and "stale days" since their last touch.
//! - **Recent contributors**: up to three additional authors by share, useful
//!   for review-routing in agent workflows.
//! - **Declared owner**: the CODEOWNERS-resolved owner for the file path,
//!   when a CODEOWNERS file exists and a rule matches.
//! - **Unowned (tristate)**: `Some(true)` if a CODEOWNERS file exists but no
//!   rule matches; `Some(false)` if a rule matches; `None` if no CODEOWNERS
//!   file was discovered for the repository.
//! - **Drift**: true when the file's original author (earliest first commit
//!   in the window) differs substantially from the current top contributor,
//!   the file is at least 30 days old, and the original author's share is
//!   below 10%. Pairs with a human-readable `drift_reason` string.
//!
//! # Privacy
//!
//! Author emails are emitted in one of three modes per [`EmailMode`]:
//!
//! - `Raw`: full email as it appears in git history.
//! - `Handle` (default): local-part only. A GitHub noreply address
//!   `12345+name@users.noreply.github.com` gives `name`.
//! - `Anonymized`: stable `xxh3:<16hex>` pseudonym derived from the raw email.
//!
//! Hashed mode is suitable for CI artifacts in regulated environments where
//! even local-parts are sensitive. The hash is non-cryptographic but stable
//! across runs.

use std::path::Path;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::churn::{AuthorContribution, FileChurn};
use fallow_config::EmailMode;

use crate::codeowners::CodeOwners;
use fallow_output::{
    ContributorEntry, ContributorIdentifierFormat, OwnershipMetrics, OwnershipState,
};

pub use super::contributor::ContributorId;

/// Seconds in one day.
const SECS_PER_DAY: u64 = 86_400;

/// Drift detection: the file must be at least this old (days) for drift to
/// be considered. Avoids flagging recently scaffolded files.
const DRIFT_MIN_FILE_AGE_DAYS: u64 = 30;

/// Drift detection: the original author's recency-weighted share must be
/// below this fraction for drift to fire. Tightens the boolean against
/// "scaffolded by one person, properly built by team" false positives.
const DRIFT_MAX_ORIGINAL_SHARE: f64 = 0.10;

/// Declared owner activity window in days.
const DECLARED_OWNER_ACTIVE_DAYS: u64 = 90;

/// A suggested reviewer has a commit to the file within this many days.
const SUGGESTED_REVIEWER_MAX_STALE_DAYS: u64 = 90;

/// Inputs needed to compute ownership for one file. Built once per analysis
/// run and reused for every hotspot.
pub struct OwnershipContext<'a> {
    /// Author email pool from [`crate::churn::ChurnResult::author_pool`].
    pub author_pool: &'a [String],
    /// Compiled bot-author globs from the ownership config's `bot_patterns`.
    pub bot_globs: &'a GlobSet,
    /// CODEOWNERS lookup, when one was discovered.
    pub codeowners: Option<&'a CodeOwners>,
    /// Privacy mode for emitted author emails.
    pub email_mode: EmailMode,
    /// Current Unix epoch seconds; injectable so tests are deterministic.
    pub now_secs: u64,
}

/// Compile bot-author glob patterns from configuration.
///
/// Each pattern is matched (via `globset`) against the local part of the author
/// email and against the full author value.
///
/// # Errors
/// Returns the first invalid glob pattern encountered.
pub fn compile_bot_globs(patterns: &[String]) -> Result<GlobSet, globset::Error> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        let mut glob = GlobBuilder::new(p);
        glob.backslash_escape(true);
        builder.add(glob.build()?);
    }
    builder.build()
}

/// Ownership metrics of one file, with the identity of each contributor
/// that reviewer routing can name. The identities are not part of the
/// report, so the rendered identifiers keep the configured privacy mode.
pub struct OwnershipWithIdentities<'a> {
    /// The metrics that the report shows.
    pub metrics: OwnershipMetrics,
    /// Identity of `metrics.top_contributor`.
    pub top_contributor: ContributorId<'a>,
    /// Identities of `metrics.suggested_reviewers`, in the same order.
    pub suggested_reviewers: Vec<ContributorId<'a>>,
}

/// Compute ownership signals for one file, with the contributor
/// identities. Returns `None` when the file has no recorded authors or all
/// authors are filtered out as bots.
#[expect(
    clippy::cast_possible_truncation,
    reason = "contributor counts and bus factor are bounded by author pool size"
)]
pub fn compute_ownership_with_identities<'a>(
    churn: &'a FileChurn,
    relative_path: &Path,
    ctx: &OwnershipContext<'a>,
) -> Option<OwnershipWithIdentities<'a>> {
    if churn.authors.is_empty() {
        return None;
    }

    let filtered = rank_non_bot_authors(churn, ctx);
    if filtered.is_empty() {
        return None;
    }

    let total_weighted: f64 = filtered
        .iter()
        .map(|a| a.contribution.weighted_commits)
        .sum();
    if total_weighted <= 0.0 {
        return None;
    }

    let bus_factor = compute_bus_factor(&filtered, total_weighted);
    let format = identifier_format(ctx.email_mode);

    let (top_contributor, recent_contributors, suggested_reviewers) =
        build_ownership_contributors(&filtered, total_weighted, format, ctx.now_secs);

    let (raw_drift, raw_drift_reason) = compute_drift(&filtered, total_weighted, ctx.now_secs);

    let (declared_owner, unowned) = ctx.codeowners.map_or((None, None), |co| {
        co.owner_of(relative_path)
            .map_or((None, Some(true)), |owner| {
                (Some(owner.to_string()), Some(false))
            })
    });
    let ownership_state = classify_ownership_state(
        declared_owner.as_deref(),
        unowned,
        raw_drift,
        &filtered,
        ctx.now_secs,
    );
    let (drift, drift_reason) = if ownership_state == OwnershipState::Drifting {
        (raw_drift, raw_drift_reason)
    } else {
        (false, None)
    };

    let top_identity = filtered[0].id;
    let reviewer_identities: Vec<ContributorId<'a>> = filtered
        .iter()
        .skip(1)
        .take(3)
        .filter(|author| {
            stale_days(author.contribution.last_commit_ts, ctx.now_secs)
                < SUGGESTED_REVIEWER_MAX_STALE_DAYS
        })
        .map(|author| author.id)
        .collect();

    debug_assert_eq!(reviewer_identities.len(), suggested_reviewers.len());

    Some(OwnershipWithIdentities {
        metrics: OwnershipMetrics {
            bus_factor,
            contributor_count: filtered.len() as u32,
            top_contributor,
            recent_contributors,
            suggested_reviewers,
            declared_owner,
            unowned,
            ownership_state,
            drift,
            drift_reason,
        },
        top_contributor: top_identity,
        suggested_reviewers: reviewer_identities,
    })
}

/// Build the non-bot author ranking for one file: filtered, identifier-rendered,
/// and sorted by recency-weighted commits (then raw email for determinism).
fn rank_non_bot_authors<'a>(
    churn: &'a FileChurn,
    ctx: &OwnershipContext<'a>,
) -> Vec<RankedAuthor<'a>> {
    let mut filtered: Vec<RankedAuthor<'a>> = churn
        .authors
        .iter()
        .filter_map(|(idx, contribution)| {
            let id = ContributorId::new(ctx.author_pool.get(*idx as usize)?);
            if id.is_bot(ctx.bot_globs) {
                return None;
            }
            Some(RankedAuthor {
                idx: *idx,
                id,
                contribution,
                rendered: String::new(),
            })
        })
        .collect();

    if filtered.is_empty() {
        return filtered;
    }

    let ids: Vec<ContributorId<'_>> = filtered.iter().map(|author| author.id).collect();
    let rendered = super::contributor::render_identifiers(&ids, ctx.email_mode);
    for (author, rendered) in filtered.iter_mut().zip(rendered) {
        author.rendered = rendered;
    }

    filtered.sort_by(|a, b| {
        b.contribution
            .weighted_commits
            .partial_cmp(&a.contribution.weighted_commits)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.raw().cmp(b.id.raw()))
    });

    filtered
}

/// Derive the top, recent (next three), and suggested-reviewer contributor
/// entries from the ranked author list.
fn build_ownership_contributors(
    filtered: &[RankedAuthor<'_>],
    total_weighted: f64,
    format: ContributorIdentifierFormat,
    now_secs: u64,
) -> (
    ContributorEntry,
    Vec<ContributorEntry>,
    Vec<ContributorEntry>,
) {
    let top = &filtered[0];
    let top_contributor = ContributorEntry {
        identifier: top.rendered.clone(),
        format,
        share: round3(top.contribution.weighted_commits / total_weighted),
        stale_days: stale_days(top.contribution.last_commit_ts, now_secs),
        commits: top.contribution.commits,
    };

    let recent_contributors: Vec<ContributorEntry> = filtered
        .iter()
        .skip(1)
        .take(3)
        .map(|a| ContributorEntry {
            identifier: a.rendered.clone(),
            format,
            share: round3(a.contribution.weighted_commits / total_weighted),
            stale_days: stale_days(a.contribution.last_commit_ts, now_secs),
            commits: a.contribution.commits,
        })
        .collect();

    let suggested_reviewers: Vec<ContributorEntry> = recent_contributors
        .iter()
        .filter(|c| c.stale_days < SUGGESTED_REVIEWER_MAX_STALE_DAYS)
        .cloned()
        .collect();

    (top_contributor, recent_contributors, suggested_reviewers)
}

/// Per-author working entry used during ranking.
struct RankedAuthor<'a> {
    /// Interned author pool index. Stable identifier for equality checks
    /// across `min_by_key` / sorted-position comparisons in drift detection.
    idx: u32,
    id: ContributorId<'a>,
    contribution: &'a AuthorContribution,
    rendered: String,
}

/// Avelino truck factor. Sort by weighted commits descending (already done
/// by the caller), accumulate until we cross 50% of total weighted commits.
fn compute_bus_factor(ranked: &[RankedAuthor<'_>], total_weighted: f64) -> u32 {
    let mut acc = 0.0;
    let mut count: u32 = 0;
    for entry in ranked {
        acc += entry.contribution.weighted_commits;
        count += 1;
        if acc / total_weighted >= 0.5 {
            break;
        }
    }
    count
}

/// Drift detection. Original author = earliest first-commit-ts among
/// non-bot contributors. Drift fires only when:
///
/// 1. Original author differs from current top contributor.
/// 2. File age >= [`DRIFT_MIN_FILE_AGE_DAYS`] (avoids flagging fresh files).
/// 3. Original author's recent share < [`DRIFT_MAX_ORIGINAL_SHARE`] (avoids
///    flagging files where the author still actively maintains).
fn compute_drift(
    ranked: &[RankedAuthor<'_>],
    total_weighted: f64,
    now_secs: u64,
) -> (bool, Option<String>) {
    let Some(original) = ranked.iter().min_by_key(|a| a.contribution.first_commit_ts) else {
        return (false, None);
    };
    let top = &ranked[0];

    if original.idx == top.idx {
        return (false, None);
    }

    let file_age_days = stale_days(original.contribution.first_commit_ts, now_secs);
    let original_share = original.contribution.weighted_commits / total_weighted;
    let top_share = top.contribution.weighted_commits / total_weighted;

    if file_age_days < DRIFT_MIN_FILE_AGE_DAYS || original_share >= DRIFT_MAX_ORIGINAL_SHARE {
        return (false, None);
    }

    let reason = format!(
        "original author {} now has {:.0}% share; current top is {} ({:.0}%)",
        original.rendered,
        original_share * 100.0,
        top.rendered,
        top_share * 100.0,
    );
    (true, Some(reason))
}

fn classify_ownership_state(
    declared_owner: Option<&str>,
    unowned: Option<bool>,
    raw_drift: bool,
    ranked: &[RankedAuthor<'_>],
    now_secs: u64,
) -> OwnershipState {
    if unowned == Some(true) {
        return OwnershipState::Unowned;
    }

    if let Some(owner) = declared_owner {
        if declared_owner_is_active(owner, ranked, now_secs) {
            return OwnershipState::Active;
        }
        return OwnershipState::DeclaredInactive;
    }

    if raw_drift {
        OwnershipState::Drifting
    } else {
        OwnershipState::Active
    }
}

fn declared_owner_is_active(owner: &str, ranked: &[RankedAuthor<'_>], now_secs: u64) -> bool {
    ranked.iter().any(|author| {
        stale_days(author.contribution.last_commit_ts, now_secs) < DECLARED_OWNER_ACTIVE_DAYS
            && declared_owner_matches_author(owner, author)
    })
}

fn declared_owner_matches_author(owner: &str, author: &RankedAuthor<'_>) -> bool {
    let owner = owner.trim();
    if owner.is_empty() {
        return false;
    }

    if !owner.starts_with('@') && owner.eq_ignore_ascii_case(author.id.raw()) {
        return true;
    }

    let owner_handle = owner.trim_start_matches('@');
    if owner_handle.is_empty() || owner_handle.contains('/') {
        return false;
    }

    let raw_handle = author.id.handle();
    let rendered_handle = author
        .rendered
        .split('~')
        .next()
        .unwrap_or(&author.rendered);
    owner_handle.eq_ignore_ascii_case(raw_handle)
        || owner_handle.eq_ignore_ascii_case(rendered_handle)
}

fn stale_days(commit_ts: u64, now_secs: u64) -> u64 {
    now_secs.saturating_sub(commit_ts) / SECS_PER_DAY
}

/// Map the configured [`EmailMode`] to the public format discriminator
/// emitted in [`ContributorEntry::format`].
const fn identifier_format(mode: EmailMode) -> ContributorIdentifierFormat {
    match mode {
        EmailMode::Raw => ContributorIdentifierFormat::Raw,
        EmailMode::Handle => ContributorIdentifierFormat::Handle,
        EmailMode::Anonymized => ContributorIdentifierFormat::Anonymized,
        EmailMode::Hash => ContributorIdentifierFormat::Hash,
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashMap;

    const NOW: u64 = 1_750_000_000;

    fn ts_days_ago(days: u64) -> u64 {
        NOW.saturating_sub(days * SECS_PER_DAY)
    }

    fn churn_with_authors(path: &str, authors: &[(u32, u32, f64, u64, u64)]) -> FileChurn {
        let mut map: FxHashMap<u32, AuthorContribution> = FxHashMap::default();
        for &(idx, commits, weighted, first_ts, last_ts) in authors {
            map.insert(
                idx,
                AuthorContribution {
                    commits,
                    weighted_commits: weighted,
                    first_commit_ts: first_ts,
                    last_commit_ts: last_ts,
                },
            );
        }
        FileChurn {
            path: std::path::PathBuf::from(path),
            commits: authors.iter().map(|a| a.1).sum(),
            weighted_commits: authors.iter().map(|a| a.2).sum(),
            lines_added: 0,
            lines_deleted: 0,
            trend: crate::churn::ChurnTrend::Stable,
            authors: map,
        }
    }

    fn empty_globs() -> GlobSet {
        GlobSet::empty()
    }

    fn ctx_with<'a>(
        pool: &'a [String],
        globs: &'a GlobSet,
        codeowners: Option<&'a CodeOwners>,
    ) -> OwnershipContext<'a> {
        OwnershipContext {
            author_pool: pool,
            bot_globs: globs,
            codeowners,
            email_mode: EmailMode::Raw,
            now_secs: NOW,
        }
    }

    fn compute_ownership<'a>(
        churn: &'a FileChurn,
        relative_path: &Path,
        ctx: &OwnershipContext<'a>,
    ) -> Option<OwnershipMetrics> {
        compute_ownership_with_identities(churn, relative_path, ctx).map(|owned| owned.metrics)
    }

    /// Raw mode is the default: `compute_ownership` reports the unmodified
    /// address and labels the identifier format accordingly.
    #[test]
    fn raw_mode_reports_the_unmodified_address() {
        let pool = vec!["alice@example.com".to_string()];
        let churn = churn_with_authors("f.ts", &[(0, 5, 5.0, ts_days_ago(60), ts_days_ago(1))]);
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(m.top_contributor.identifier, "alice@example.com");
        assert_eq!(m.top_contributor.format, ContributorIdentifierFormat::Raw);
    }

    /// Handle mode with no local-part collision strips the domain and adds no
    /// disambiguation suffix.
    #[test]
    fn handle_mode_strips_domain_without_collision() {
        let pool = vec!["alice@example.com".to_string()];
        let churn = churn_with_authors("f.ts", &[(0, 5, 5.0, ts_days_ago(60), ts_days_ago(1))]);
        let globs = empty_globs();
        let mut ctx = ctx_with(&pool, &globs, None);
        ctx.email_mode = EmailMode::Handle;
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(m.top_contributor.identifier, "alice");
        assert_eq!(
            m.top_contributor.format,
            ContributorIdentifierFormat::Handle
        );
    }

    #[test]
    fn handle_mode_disambiguates_same_local_part_collisions() {
        let pool = vec![
            "alice@contractor.io".to_string(),
            "alice@company.com".to_string(),
        ];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 8, 8.0, ts_days_ago(80), ts_days_ago(3)),
                (1, 9, 9.0, ts_days_ago(70), ts_days_ago(1)),
            ],
        );
        let globs = empty_globs();
        let mut ctx = ctx_with(&pool, &globs, None);
        ctx.email_mode = EmailMode::Handle;
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(m.top_contributor.identifier, "alice~company.com");
        assert_eq!(m.recent_contributors[0].identifier, "alice~contractor.io");
    }

    #[test]
    fn anonymized_mode_hides_raw_email_and_handle() {
        let pool = vec!["alice@example.com".to_string()];
        let churn = churn_with_authors("f.ts", &[(0, 5, 5.0, ts_days_ago(60), ts_days_ago(1))]);
        let globs = empty_globs();
        let mut ctx = ctx_with(&pool, &globs, None);
        ctx.email_mode = EmailMode::Anonymized;
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert!(m.top_contributor.identifier.starts_with("xxh3:"));
        assert!(!m.top_contributor.identifier.contains("alice"));
        assert_eq!(
            m.top_contributor.format,
            ContributorIdentifierFormat::Anonymized
        );
    }

    #[test]
    fn identities_follow_the_metrics_in_anonymized_mode() {
        let pool = vec![
            "alice@example.com".to_string(),
            "bob@example.com".to_string(),
        ];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 9, 9.0, ts_days_ago(60), ts_days_ago(1)),
                (1, 5, 5.0, ts_days_ago(50), ts_days_ago(2)),
            ],
        );
        let globs = empty_globs();
        let mut ctx = ctx_with(&pool, &globs, None);
        ctx.email_mode = EmailMode::Anonymized;
        let owned = compute_ownership_with_identities(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(owned.top_contributor.raw(), "alice@example.com");
        assert_eq!(owned.suggested_reviewers.len(), 1);
        assert_eq!(owned.suggested_reviewers[0].raw(), "bob@example.com");
        assert_eq!(owned.metrics.suggested_reviewers.len(), 1);
        assert!(
            owned
                .metrics
                .top_contributor
                .identifier
                .starts_with("xxh3:")
        );
    }

    #[test]
    fn handle_mode_keeps_plus_addressed_contributors_distinct() {
        let pool = vec![
            "john+github@gmail.com".to_string(),
            "jane+github@gmail.com".to_string(),
        ];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 8, 8.0, ts_days_ago(80), ts_days_ago(3)),
                (1, 9, 9.0, ts_days_ago(70), ts_days_ago(1)),
            ],
        );
        let globs = empty_globs();
        let mut ctx = ctx_with(&pool, &globs, None);
        ctx.email_mode = EmailMode::Handle;
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(m.top_contributor.identifier, "jane+github");
        assert_eq!(m.recent_contributors[0].identifier, "john+github");
    }

    #[test]
    fn bot_pattern_without_wildcard_excludes_the_bot() {
        let pool = vec!["alice@x".to_string(), "release-bot@corp.com".to_string()];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 1, 1.0, ts_days_ago(60), ts_days_ago(10)),
                (1, 100, 100.0, ts_days_ago(30), ts_days_ago(1)),
            ],
        );
        let globs = compile_bot_globs(&["release-bot".to_string()]).unwrap();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(m.contributor_count, 1);
        assert_eq!(m.top_contributor.identifier, "alice@x");
    }

    #[test]
    fn bot_globs_match_default_patterns() {
        let globs = compile_bot_globs(&[
            r"*\[bot\]*".to_string(),
            "dependabot*".to_string(),
            "github-actions*".to_string(),
        ])
        .unwrap();
        assert!(ContributorId::new("dependabot[bot]@users.noreply.github.com").is_bot(&globs));
        assert!(ContributorId::new("dependabot@github.com").is_bot(&globs));
        assert!(ContributorId::new("github-actions@users.noreply.github.com").is_bot(&globs));
        assert!(!ContributorId::new("alice@example.com").is_bot(&globs));
    }

    #[test]
    fn human_github_noreply_is_not_a_bot() {
        let globs =
            compile_bot_globs(&fallow_config::OwnershipConfig::default().bot_patterns).unwrap();
        assert!(!ContributorId::new("49056869+sapphi-red@users.noreply.github.com").is_bot(&globs));
        assert!(!ContributorId::new("12345+alice@users.noreply.github.com").is_bot(&globs));
        assert!(
            ContributorId::new("41898282+github-actions[bot]@users.noreply.github.com")
                .is_bot(&globs)
        );
    }

    #[test]
    fn bus_factor_single_dominant_author_is_one() {
        let pool = vec!["alice@x".to_string(), "bob@x".to_string()];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 9, 9.0, ts_days_ago(60), ts_days_ago(1)),
                (1, 1, 1.0, ts_days_ago(30), ts_days_ago(20)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(m.bus_factor, 1);
        assert_eq!(m.contributor_count, 2);
        assert_eq!(m.top_contributor.identifier, "alice@x");
    }

    #[test]
    fn bus_factor_even_split_three_authors_is_two() {
        let pool = vec!["a@x".to_string(), "b@x".to_string(), "c@x".to_string()];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 4, 4.0, ts_days_ago(50), ts_days_ago(1)),
                (1, 3, 3.0, ts_days_ago(40), ts_days_ago(2)),
                (2, 3, 3.0, ts_days_ago(30), ts_days_ago(3)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(m.bus_factor, 2);
        assert_eq!(m.contributor_count, 3);
    }

    #[test]
    fn bus_factor_excludes_bots() {
        let pool = vec![
            "alice@x".to_string(),
            "dependabot[bot]@users.noreply.github.com".to_string(),
        ];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 1, 1.0, ts_days_ago(60), ts_days_ago(10)),
                (1, 100, 100.0, ts_days_ago(30), ts_days_ago(1)),
            ],
        );
        let globs = compile_bot_globs(&[r"*\[bot\]*".to_string()]).unwrap();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(m.bus_factor, 1);
        assert_eq!(m.contributor_count, 1);
        assert_eq!(m.top_contributor.identifier, "alice@x");
    }

    #[test]
    fn recent_contributors_takes_top_three_excluding_top() {
        let pool = (0..6).map(|i| format!("u{i}@x")).collect::<Vec<_>>();
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 10, 10.0, ts_days_ago(60), ts_days_ago(1)),
                (1, 5, 5.0, ts_days_ago(50), ts_days_ago(2)),
                (2, 4, 4.0, ts_days_ago(40), ts_days_ago(3)),
                (3, 3, 3.0, ts_days_ago(30), ts_days_ago(4)),
                (4, 2, 2.0, ts_days_ago(20), ts_days_ago(5)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(m.recent_contributors.len(), 3);
        assert_eq!(m.recent_contributors[0].identifier, "u1@x");
        assert_eq!(m.recent_contributors[1].identifier, "u2@x");
        assert_eq!(m.recent_contributors[2].identifier, "u3@x");
    }

    #[test]
    fn drift_fires_when_original_author_inactive_old_file() {
        let pool = vec!["alice@x".to_string(), "bob@x".to_string()];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 1, 0.5, ts_days_ago(200), ts_days_ago(200)),
                (1, 20, 20.0, ts_days_ago(60), ts_days_ago(1)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert!(m.drift);
        assert_eq!(m.ownership_state, OwnershipState::Drifting);
        let reason = m.drift_reason.expect("drift_reason should be set");
        assert!(reason.contains("alice@x"));
        assert!(reason.contains("bob@x"));
    }

    #[test]
    fn drift_does_not_fire_for_recently_scaffolded_file() {
        let pool = vec!["alice@x".to_string(), "bob@x".to_string()];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 1, 0.5, ts_days_ago(10), ts_days_ago(10)),
                (1, 20, 20.0, ts_days_ago(8), ts_days_ago(1)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert!(!m.drift);
        assert_eq!(m.ownership_state, OwnershipState::Active);
        assert!(m.drift_reason.is_none());
    }

    #[test]
    fn drift_does_not_fire_when_original_still_active() {
        let pool = vec!["alice@x".to_string(), "bob@x".to_string()];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 6, 6.0, ts_days_ago(200), ts_days_ago(2)),
                (1, 14, 14.0, ts_days_ago(60), ts_days_ago(1)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert!(!m.drift);
    }

    #[test]
    fn drift_does_not_fire_when_original_is_top_contributor() {
        let pool = vec!["alice@x".to_string()];
        let churn = churn_with_authors("f.ts", &[(0, 10, 10.0, ts_days_ago(200), ts_days_ago(1))]);
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert!(!m.drift);
    }

    #[test]
    fn unowned_tristate_some_true_when_no_rule_matches() {
        let co = CodeOwners::parse("/src/ @frontend\n").unwrap();
        let pool = vec!["alice@x".to_string()];
        let churn =
            churn_with_authors("README.md", &[(0, 5, 5.0, ts_days_ago(60), ts_days_ago(1))]);
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, Some(&co));
        let m = compute_ownership(&churn, Path::new("README.md"), &ctx).unwrap();
        assert_eq!(m.unowned, Some(true));
        assert_eq!(m.ownership_state, OwnershipState::Unowned);
        assert!(m.declared_owner.is_none());
    }

    #[test]
    fn unowned_tristate_some_false_when_rule_matches() {
        let co = CodeOwners::parse("/src/ @frontend\n").unwrap();
        let pool = vec!["alice@x".to_string()];
        let churn = churn_with_authors(
            "src/app.ts",
            &[(0, 5, 5.0, ts_days_ago(60), ts_days_ago(1))],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, Some(&co));
        let m = compute_ownership(&churn, Path::new("src/app.ts"), &ctx).unwrap();
        assert_eq!(m.unowned, Some(false));
        assert_eq!(m.declared_owner.as_deref(), Some("@frontend"));
        assert_eq!(m.ownership_state, OwnershipState::DeclaredInactive);
    }

    #[test]
    fn declared_owner_active_suppresses_git_history_drift() {
        let co = CodeOwners::parse("/src/ @bob\n").unwrap();
        let pool = vec!["alice@x".to_string(), "bob@x".to_string()];
        let churn = churn_with_authors(
            "src/app.ts",
            &[
                (0, 1, 0.5, ts_days_ago(200), ts_days_ago(200)),
                (1, 20, 20.0, ts_days_ago(60), ts_days_ago(1)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, Some(&co));
        let m = compute_ownership(&churn, Path::new("src/app.ts"), &ctx).unwrap();
        assert_eq!(m.declared_owner.as_deref(), Some("@bob"));
        assert_eq!(m.ownership_state, OwnershipState::Active);
        assert!(!m.drift);
        assert!(m.drift_reason.is_none());
    }

    #[test]
    fn declared_owner_inactive_suppresses_vague_drift() {
        let co = CodeOwners::parse("/src/ @carol\n").unwrap();
        let pool = vec!["alice@x".to_string(), "bob@x".to_string()];
        let churn = churn_with_authors(
            "src/app.ts",
            &[
                (0, 1, 0.5, ts_days_ago(200), ts_days_ago(200)),
                (1, 20, 20.0, ts_days_ago(60), ts_days_ago(1)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, Some(&co));
        let m = compute_ownership(&churn, Path::new("src/app.ts"), &ctx).unwrap();
        assert_eq!(m.declared_owner.as_deref(), Some("@carol"));
        assert_eq!(m.ownership_state, OwnershipState::DeclaredInactive);
        assert!(!m.drift);
        assert!(m.drift_reason.is_none());
    }

    #[test]
    fn unowned_tristate_none_when_no_codeowners_file() {
        let pool = vec!["alice@x".to_string()];
        let churn = churn_with_authors(
            "src/app.ts",
            &[(0, 5, 5.0, ts_days_ago(60), ts_days_ago(1))],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("src/app.ts"), &ctx).unwrap();
        assert_eq!(m.unowned, None);
        assert_eq!(m.ownership_state, OwnershipState::Active);
        assert!(m.declared_owner.is_none());
    }

    #[test]
    fn returns_none_when_no_authors() {
        let pool: Vec<String> = vec![];
        let churn = churn_with_authors("f.ts", &[]);
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        assert!(compute_ownership(&churn, Path::new("f.ts"), &ctx).is_none());
    }

    #[test]
    fn returns_none_when_only_bot_authors() {
        let pool = vec!["dependabot[bot]@users.noreply.github.com".to_string()];
        let churn = churn_with_authors("f.ts", &[(0, 5, 5.0, ts_days_ago(60), ts_days_ago(1))]);
        let globs = compile_bot_globs(&[r"*\[bot\]*".to_string()]).unwrap();
        let ctx = ctx_with(&pool, &globs, None);
        assert!(compute_ownership(&churn, Path::new("f.ts"), &ctx).is_none());
    }

    #[test]
    fn stale_days_clamps_at_zero_for_future_timestamps() {
        assert_eq!(stale_days(NOW + 1000, NOW), 0);
    }

    #[test]
    fn stale_days_basic() {
        assert_eq!(stale_days(ts_days_ago(7), NOW), 7);
        assert_eq!(stale_days(ts_days_ago(0), NOW), 0);
    }

    #[test]
    fn shares_are_rounded_to_three_decimals() {
        let pool = vec!["a@x".to_string(), "b@x".to_string(), "c@x".to_string()];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 1, 1.0, ts_days_ago(50), ts_days_ago(1)),
                (1, 1, 1.0, ts_days_ago(40), ts_days_ago(2)),
                (2, 1, 1.0, ts_days_ago(30), ts_days_ago(3)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert!((m.top_contributor.share - 0.333).abs() < f64::EPSILON);
    }
}
