//! Ownership-aware reviewer routing (6.D).
//!
//! Per changed file, name the expert(s) to route the review to: CODEOWNERS
//! declared owner plus git-blame / recency contributors, and flag a bus-factor-1
//! risk (the only qualified owner is one person). Reuses the health ownership /
//! bus-factor machinery (`compute_ownership_with_identities`, `CodeOwners`, churn) rather than a
//! parallel implementation.
//!
//! This is the people-layer of the review direction: it answers "who do I ask?".
//! Advisory brief data; never gates.

use std::path::{Path, PathBuf};

pub use fallow_output::{RoutingFacts, RoutingUnit};
use rustc_hash::FxHashSet;

use fallow_config::ResolvedConfig;
use fallow_engine::churn::{ChurnResult, ChurnWindowUnit, SinceDuration, analyze_churn_cached};
use fallow_engine::health::ownership::{
    ContributorId, OwnershipContext, OwnershipWithIdentities, compile_bot_globs,
    compute_ownership_with_identities,
};

/// Churn window of the audit brief, in months: enough history to name the
/// per-file experts and to find hidden change coupling. It matches the
/// default change coupling window of `fallow health`, so both commands share
/// one churn cache entry.
const AUDIT_CHURN_WINDOW_MONTHS: u64 = 12;

/// Read the audit brief churn through the shared churn cache. `None` when git
/// history is unavailable (not a repository, or a branch without commits).
#[must_use]
pub fn audit_churn(root: &Path, config: &ResolvedConfig) -> Option<ChurnResult> {
    let since = SinceDuration::relative(
        AUDIT_CHURN_WINDOW_MONTHS,
        ChurnWindowUnit::Months,
        format!("{AUDIT_CHURN_WINDOW_MONTHS} months"),
    );
    analyze_churn_cached(root, &since, &config.cache_dir, config.no_cache).map(|(churn, _)| churn)
}

/// Compute the routing section for the changed files. Best-effort: returns an
/// empty `RoutingFacts` when churn is unavailable (non-git repo, shallow clone
/// with no history). CODEOWNERS is consulted when present: the configured
/// `codeowners` path when set, else the standard locations.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "callers always pass the audit changed-file FxHashSet; generalizing the hasher adds noise"
)]
pub fn compute_routing(
    root: &Path,
    config: &ResolvedConfig,
    changed_files: &FxHashSet<PathBuf>,
) -> RoutingFacts {
    audit_churn(root, config).map_or_else(RoutingFacts::default, |churn| {
        compute_routing_from_churn(root, config, changed_files, &churn)
    })
}

/// [`compute_routing`] over a churn pass that the caller already read with
/// [`audit_churn`], so the audit brief reads the history once.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "callers always pass the audit changed-file FxHashSet; generalizing the hasher adds noise"
)]
pub fn compute_routing_from_churn(
    root: &Path,
    config: &ResolvedConfig,
    changed_files: &FxHashSet<PathBuf>,
    churn_result: &ChurnResult,
) -> RoutingFacts {
    let ownership_cfg = &config.health.ownership;
    let Ok(bot_globs) = compile_bot_globs(&ownership_cfg.bot_patterns) else {
        return RoutingFacts::default();
    };
    // The same file the ownership section reads, so both use one owner
    // vocabulary.
    let codeowners = crate::ownership::load_codeowners(root, config)
        .ok()
        .flatten();
    // Reuse the churn run's clock so routing and the health ownership block
    // agree on "now" and neither flips a staleness threshold between runs.
    let now_secs = churn_result.clock.epoch_secs();
    let ctx = OwnershipContext {
        author_pool: &churn_result.author_pool,
        bot_globs: &bot_globs,
        codeowners: codeowners.as_ref(),
        email_mode: ownership_cfg.email_mode,
        now_secs,
    };

    // The current reviewer (git user) is excluded from routing: you do not "ask
    // yourself". On a solo repo every file routes to the author, so this is what
    // turns "ask: bart (bus-factor 1)" on every decision into silence.
    let reviewer = Reviewer {
        ids: fallow_engine::repo_refs::current_user_identities(root),
        email: fallow_engine::repo_refs::current_user_email(root),
    };

    let mut units: Vec<RoutingUnit> = changed_files
        .iter()
        .filter_map(|abs| route_one(abs, root, churn_result, &ctx, &reviewer))
        .collect();
    units.sort_by(|a, b| a.file.cmp(&b.file));
    RoutingFacts { units }
}

/// True when `expert` names the current reviewer (case-insensitive, `@`-tolerant
/// so a CODEOWNERS `@handle` matches the bare git handle).
fn expert_is_self(expert: &str, self_ids: &[String]) -> bool {
    let normalized = expert.trim_start_matches('@').to_ascii_lowercase();
    self_ids
        .iter()
        .any(|id| id.trim_start_matches('@').to_ascii_lowercase() == normalized)
}

/// The current reviewer, as git config names that person.
struct Reviewer {
    /// Name, email and handle strings, for a declared CODEOWNERS owner.
    ids: Vec<String>,
    /// Email of the reviewer after mailmap, for contributor identities.
    email: Option<String>,
}

impl Reviewer {
    /// True when a contributor is the reviewer. The identity comparison
    /// does not depend on the privacy mode of `rendered`.
    fn is_contributor(&self, id: ContributorId<'_>, rendered: &str) -> bool {
        expert_is_self(rendered, &self.ids)
            || self
                .email
                .as_deref()
                .is_some_and(|email| ContributorId::new(email).same_person(id))
    }
}

/// The experts to ask about one file, without the reviewer: the declared
/// CODEOWNERS owner, otherwise the top contributor and up to two suggested
/// reviewers.
fn select_experts(owned: &OwnershipWithIdentities<'_>, reviewer: &Reviewer) -> Vec<String> {
    let metrics = &owned.metrics;
    if let Some(owner) = &metrics.declared_owner {
        if expert_is_self(owner, &reviewer.ids) {
            return Vec::new();
        }
        return vec![owner.clone()];
    }
    let top = (&metrics.top_contributor, owned.top_contributor);
    let reviewers = metrics
        .suggested_reviewers
        .iter()
        .zip(owned.suggested_reviewers.iter().copied())
        .take(2);
    let mut expert: Vec<String> = Vec::new();
    for (entry, id) in std::iter::once(top).chain(reviewers) {
        if reviewer.is_contributor(id, &entry.identifier) || expert.contains(&entry.identifier) {
            continue;
        }
        expert.push(entry.identifier.clone());
    }
    expert
}

/// Route a single changed file: resolve its experts and bus-factor flag from the
/// ownership machinery. Returns `None` when the file has no churn record (no
/// signal to route on).
fn route_one(
    abs: &Path,
    root: &Path,
    churn_result: &ChurnResult,
    ctx: &OwnershipContext<'_>,
    reviewer: &Reviewer,
) -> Option<RoutingUnit> {
    let file_churn = churn_result.files.get(abs)?;
    let relative = abs.strip_prefix(root).unwrap_or(abs);
    let owned = compute_ownership_with_identities(file_churn, relative, ctx)?;
    // A unit whose every expert is the reviewer carries no routing signal and
    // is omitted, so a solo repo emits no routing noise.
    let expert = select_experts(&owned, reviewer);
    if expert.is_empty() {
        return None;
    }

    Some(RoutingUnit {
        file: relative.to_string_lossy().replace('\\', "/"),
        expert,
        bus_factor_one: owned.metrics.bus_factor == 1,
    })
}

#[cfg(test)]
mod tests {
    use fallow_engine::health::ownership::{ContributorId, OwnershipWithIdentities};
    use fallow_output::{
        ContributorEntry, ContributorIdentifierFormat, OwnershipMetrics, OwnershipState,
    };

    fn contributor(id: &str) -> ContributorEntry {
        ContributorEntry {
            identifier: id.to_string(),
            format: ContributorIdentifierFormat::Handle,
            share: 1.0,
            stale_days: 1,
            commits: 5,
        }
    }

    fn metrics(declared: Option<&str>, bus_factor: u32) -> OwnershipMetrics {
        OwnershipMetrics {
            bus_factor,
            contributor_count: 1,
            top_contributor: contributor("alice"),
            recent_contributors: vec![],
            suggested_reviewers: vec![contributor("bob")],
            declared_owner: declared.map(str::to_string),
            unowned: None,
            ownership_state: OwnershipState::Active,
            drift: false,
            drift_reason: None,
        }
    }

    #[test]
    fn current_reviewer_is_excluded_from_routing() {
        let self_ids = vec![
            "bart".to_string(),
            "bart@waardenburg.dev".to_string(),
            "Bart Waardenburg".to_string(),
        ];
        // The reviewer matches by handle, raw email, name, and CODEOWNERS @form,
        // case-insensitively.
        assert!(super::expert_is_self("bart", &self_ids));
        assert!(super::expert_is_self("Bart", &self_ids));
        assert!(super::expert_is_self("@bart", &self_ids));
        assert!(super::expert_is_self("bart@waardenburg.dev", &self_ids));
        // A different contributor is never self.
        assert!(!super::expert_is_self("alice", &self_ids));
        assert!(!super::expert_is_self("@team/ui", &self_ids));
        // No identities -> never self (best-effort: git config unreadable).
        assert!(!super::expert_is_self("bart", &[]));
    }

    fn owned<'a>(
        metrics: OwnershipMetrics,
        top: &'a str,
        reviewers: &[&'a str],
    ) -> OwnershipWithIdentities<'a> {
        OwnershipWithIdentities {
            metrics,
            top_contributor: ContributorId::new(top),
            suggested_reviewers: reviewers.iter().copied().map(ContributorId::new).collect(),
        }
    }

    fn reviewer(ids: &[&str], email: Option<&str>) -> super::Reviewer {
        super::Reviewer {
            ids: ids.iter().map(|id| (*id).to_string()).collect(),
            email: email.map(str::to_string),
        }
    }

    #[test]
    fn declared_owner_wins() {
        let owned = owned(
            metrics(Some("@team/web"), 3),
            "alice@x.test",
            &["bob@x.test"],
        );
        assert_eq!(
            super::select_experts(&owned, &reviewer(&[], None)),
            vec!["@team/web".to_string()]
        );
    }

    #[test]
    fn falls_back_to_git_contributors() {
        let owned = owned(metrics(None, 2), "alice@x.test", &["bob@x.test"]);
        assert_eq!(
            super::select_experts(&owned, &reviewer(&[], None)),
            vec!["alice".to_string(), "bob".to_string()]
        );
    }

    #[test]
    fn anonymized_reviewer_is_not_routed_to_self() {
        let mut m = metrics(None, 1);
        m.top_contributor = contributor("xxh3:0123456789abcdef");
        m.suggested_reviewers = vec![];
        let experts = super::select_experts(
            &owned(m, "bart@corp.com", &[]),
            &reviewer(&["bart", "bart@corp.com"], Some("bart@corp.com")),
        );
        assert!(experts.is_empty(), "{experts:?}");
    }

    #[test]
    fn reviewer_with_a_second_email_is_not_routed_to_self() {
        let mut m = metrics(None, 1);
        m.top_contributor = contributor("bart~corp.com");
        m.suggested_reviewers = vec![contributor("bart~gmail.com")];
        let experts = super::select_experts(
            &owned(m, "bart@corp.com", &["bart@gmail.com"]),
            &reviewer(&["bart", "bart@gmail.com"], Some("bart@gmail.com")),
        );
        assert!(experts.is_empty(), "{experts:?}");
    }

    #[test]
    fn other_contributor_stays_an_expert_in_each_mode() {
        let me = reviewer(&["bart", "bart@corp.com"], Some("bart@corp.com"));
        for identifier in ["alice", "alice@corp.com", "xxh3:00000000000000aa"] {
            let mut m = metrics(None, 1);
            m.top_contributor = contributor(identifier);
            m.suggested_reviewers = vec![];
            assert_eq!(
                super::select_experts(&owned(m, "alice@corp.com", &[]), &me),
                vec![identifier.to_string()]
            );
        }
    }

    #[test]
    fn declared_owner_that_is_the_reviewer_is_dropped() {
        let me = reviewer(&["bart"], None);
        let own = owned(metrics(Some("@bart"), 1), "alice@x.test", &["bob@x.test"]);
        assert!(super::select_experts(&own, &me).is_empty());
        let team = owned(
            metrics(Some("@team/web"), 1),
            "alice@x.test",
            &["bob@x.test"],
        );
        assert_eq!(
            super::select_experts(&team, &me),
            vec!["@team/web".to_string()]
        );
    }
}
