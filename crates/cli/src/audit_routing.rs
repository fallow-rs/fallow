//! Ownership-aware reviewer routing (6.D).
//!
//! Per changed file, name the expert(s) to route the review to: CODEOWNERS
//! declared owner plus git-blame / recency contributors, and flag a bus-factor-1
//! risk (the only qualified owner is one person). Reuses the health ownership /
//! bus-factor machinery (`compute_ownership`, `CodeOwners`, churn) rather than a
//! parallel implementation.
//!
//! This is the people-layer of the review direction: it answers "who do I ask?".
//! Advisory brief data; never gates.

use std::path::{Path, PathBuf};

pub use fallow_output::{RoutingFacts, RoutingUnit};
use rustc_hash::FxHashSet;

use crate::codeowners::CodeOwners;
use crate::health::ownership::{OwnershipContext, compile_bot_globs, compute_ownership};
use fallow_config::ResolvedConfig;
use fallow_engine::{ChurnResult, SinceDuration, analyze_churn};

/// Default churn window for routing: one year of history is enough to identify
/// the per-file experts without an unbounded `git log`.
const ROUTING_CHURN_WINDOW: &str = "1 year ago";

/// Compute the routing section for the changed files. Best-effort: returns an
/// empty `RoutingFacts` when churn is unavailable (non-git repo, shallow clone
/// with no history). CODEOWNERS is consulted when present.
#[must_use]
#[allow(
    clippy::implicit_hasher,
    reason = "callers always pass the audit changed-file FxHashSet; generalizing the hasher adds noise"
)]
pub fn compute_routing(
    root: &Path,
    config: &ResolvedConfig,
    changed_files: &FxHashSet<PathBuf>,
) -> RoutingFacts {
    let since = SinceDuration {
        git_after: ROUTING_CHURN_WINDOW.to_string(),
        display: "1 year".to_string(),
    };
    let Some(churn_result) = analyze_churn(root, &since) else {
        return RoutingFacts::default();
    };

    let ownership_cfg = &config.health.ownership;
    let Ok(bot_globs) = compile_bot_globs(&ownership_cfg.bot_patterns) else {
        return RoutingFacts::default();
    };
    let codeowners = CodeOwners::load(root, None).ok();
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
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
    let self_ids = current_user_identities(root);

    let mut units: Vec<RoutingUnit> = changed_files
        .iter()
        .filter_map(|abs| route_one(abs, root, &churn_result, &ctx, &self_ids))
        .collect();
    units.sort_by(|a, b| a.file.cmp(&b.file));
    RoutingFacts { units }
}

/// Identifiers for the current git user (the reviewer). Used to drop self-routing:
/// the raw `user.email`, its handle (local-part, GitHub no-reply unwrapped), and
/// `user.name`. Empty when git config is unreadable (best-effort, no exclusion).
fn current_user_identities(root: &Path) -> Vec<String> {
    let read = |key: &str| -> Option<String> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["config", "--get", key])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (!value.is_empty()).then_some(value)
    };
    let mut ids = Vec::new();
    if let Some(email) = read("user.email") {
        if let Some((local, _)) = email.split_once('@') {
            // GitHub no-reply unwrap: `1234+handle@users.noreply.github.com` -> `handle`.
            ids.push(local.rsplit('+').next().unwrap_or(local).to_string());
        }
        ids.push(email);
    }
    if let Some(name) = read("user.name") {
        ids.push(name);
    }
    ids
}

/// True when `expert` names the current reviewer (case-insensitive, `@`-tolerant
/// so a CODEOWNERS `@handle` matches the bare git handle).
fn expert_is_self(expert: &str, self_ids: &[String]) -> bool {
    let normalized = expert.trim_start_matches('@').to_ascii_lowercase();
    self_ids
        .iter()
        .any(|id| id.trim_start_matches('@').to_ascii_lowercase() == normalized)
}

/// Route a single changed file: resolve its experts and bus-factor flag from the
/// ownership machinery. Returns `None` when the file has no churn record (no
/// signal to route on).
fn route_one(
    abs: &Path,
    root: &Path,
    churn_result: &ChurnResult,
    ctx: &OwnershipContext<'_>,
    self_ids: &[String],
) -> Option<RoutingUnit> {
    let file_churn = churn_result.files.get(abs)?;
    let relative = abs.strip_prefix(root).unwrap_or(abs);
    let metrics = compute_ownership(file_churn, relative, ctx)?;

    // Prefer the declared CODEOWNERS owner; otherwise the top contributor, then
    // the suggested reviewers. Deduped, capped to keep the routing line tight.
    let mut expert: Vec<String> = Vec::new();
    if let Some(owner) = &metrics.declared_owner {
        expert.push(owner.clone());
    }
    if expert.is_empty() {
        expert.push(metrics.top_contributor.identifier.clone());
        for reviewer in metrics.suggested_reviewers.iter().take(2) {
            if !expert.contains(&reviewer.identifier) {
                expert.push(reviewer.identifier.clone());
            }
        }
    }

    // Drop the current reviewer: there is no one to "ask" if you own it. A unit
    // whose every expert is the reviewer carries no routing signal and is omitted
    // (same doctrine as a file with no ownership signal), so a solo repo emits no
    // routing noise.
    expert.retain(|e| !expert_is_self(e, self_ids));
    if expert.is_empty() {
        return None;
    }

    Some(RoutingUnit {
        file: relative.to_string_lossy().replace('\\', "/"),
        expert,
        bus_factor_one: metrics.bus_factor == 1,
    })
}

#[cfg(test)]
mod tests {
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

    /// `route_one`'s expert-selection logic, exercised through a small shim that
    /// mirrors its branching without needing a live git repo.
    fn select_expert(metrics: &OwnershipMetrics) -> (Vec<String>, bool) {
        let mut expert: Vec<String> = Vec::new();
        if let Some(owner) = &metrics.declared_owner {
            expert.push(owner.clone());
        }
        if expert.is_empty() {
            expert.push(metrics.top_contributor.identifier.clone());
            for reviewer in metrics.suggested_reviewers.iter().take(2) {
                if !expert.contains(&reviewer.identifier) {
                    expert.push(reviewer.identifier.clone());
                }
            }
        }
        (expert, metrics.bus_factor == 1)
    }

    #[test]
    fn declared_owner_wins() {
        let (expert, _) = select_expert(&metrics(Some("@team/web"), 3));
        assert_eq!(expert, vec!["@team/web".to_string()]);
    }

    #[test]
    fn falls_back_to_git_contributors() {
        let (expert, _) = select_expert(&metrics(None, 2));
        assert_eq!(expert, vec!["alice".to_string(), "bob".to_string()]);
    }

    #[test]
    fn bus_factor_one_is_flagged() {
        let (_, bus1) = select_expert(&metrics(None, 1));
        assert!(bus1);
        let (_, bus2) = select_expert(&metrics(None, 2));
        assert!(!bus2);
    }
}
