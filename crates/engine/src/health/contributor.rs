//! Contributor identity for ownership analysis and reviewer routing.

use globset::GlobSet;
use rustc_hash::{FxHashMap, FxHashSet};
use xxhash_rust::xxh3::xxh3_64;

use fallow_config::EmailMode;

/// Start of the domain of a GitHub noreply address
/// (`users.noreply.github.com`, or `users.noreply.<host>` on GitHub
/// Enterprise).
const GITHUB_NOREPLY_DOMAIN_PREFIX: &str = "users.noreply.";

/// One contributor, identified by the author value of the churn history:
/// an email for git history, an opaque id for imported churn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContributorId<'a> {
    raw: &'a str,
}

impl<'a> ContributorId<'a> {
    /// Wrap an author value.
    #[must_use]
    pub const fn new(raw: &'a str) -> Self {
        Self { raw }
    }

    /// The author value as the churn history gives it.
    #[must_use]
    pub(crate) const fn raw(self) -> &'a str {
        self.raw
    }

    /// Display handle: the local part, except that a GitHub noreply
    /// address `12345+alice@users.noreply.github.com` gives `alice`. A plus
    /// tag on another domain stays in the handle, because two people can
    /// share a tag.
    #[must_use]
    pub fn handle(self) -> &'a str {
        let local = self.local_part();
        if local.is_empty() {
            return self.raw;
        }
        if self.has_github_noreply_domain()
            && let Some((account_id, name)) = local.split_once('+')
            && !account_id.is_empty()
            && account_id.bytes().all(|byte| byte.is_ascii_digit())
            && !name.is_empty()
        {
            return name;
        }
        local
    }

    /// True when both values name one person: the same address, or the
    /// same handle. ASCII case is ignored.
    #[must_use]
    pub fn same_person(self, other: ContributorId<'_>) -> bool {
        self.raw.eq_ignore_ascii_case(other.raw)
            || self.handle().eq_ignore_ascii_case(other.handle())
    }

    /// True when a bot pattern matches the local part or the full value.
    #[must_use]
    pub(crate) fn is_bot(self, bot_globs: &GlobSet) -> bool {
        bot_globs.is_match(self.local_part()) || bot_globs.is_match(self.raw)
    }

    /// Text before the first `@`. The full value when there is no `@`.
    fn local_part(self) -> &'a str {
        self.raw
            .split_once('@')
            .map_or(self.raw, |(local, _)| local)
    }

    /// Text after the first `@`. Empty when there is no `@`.
    fn domain(self) -> &'a str {
        self.raw.split_once('@').map_or("", |(_, domain)| domain)
    }

    fn has_github_noreply_domain(self) -> bool {
        let prefix = GITHUB_NOREPLY_DOMAIN_PREFIX.as_bytes();
        self.domain()
            .as_bytes()
            .get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    }

    /// Stable non-cryptographic pseudonym, `xxh3:<16 hex>`. It keeps raw
    /// emails out of CI artifacts. It is not a security primitive: a known
    /// list of emails gives the same pseudonyms.
    fn pseudonym(self) -> String {
        format!("xxh3:{:016x}", xxh3_64(self.raw.as_bytes()))
    }

    fn hash_suffix(self) -> String {
        format!("id{:016x}", xxh3_64(self.raw.as_bytes()))
    }

    fn domain_suffix(self) -> String {
        let domain = self.domain();
        if domain.is_empty() {
            return self.hash_suffix();
        }
        domain
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                    ch.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect()
    }
}

/// Render one identifier per contributor, in input order.
///
/// In handle mode, contributors that share a handle get `~<domain>`.
/// Contributors that still share an identifier get `~id<hash>` too, so two
/// different author values never render the same.
pub(super) fn render_identifiers(ids: &[ContributorId<'_>], mode: EmailMode) -> Vec<String> {
    match mode {
        EmailMode::Raw => ids.iter().map(|id| id.raw.to_string()).collect(),
        EmailMode::Handle => {
            let mut rendered: Vec<String> = ids.iter().map(|id| id.handle().to_string()).collect();
            disambiguate(ids, &mut rendered, ContributorId::domain_suffix);
            disambiguate(ids, &mut rendered, ContributorId::hash_suffix);
            rendered
        }
        EmailMode::Anonymized | EmailMode::Hash => ids.iter().map(|id| id.pseudonym()).collect(),
    }
}

/// Append `~<suffix>` to each identifier that two different author values
/// share.
fn disambiguate<'a>(
    ids: &[ContributorId<'a>],
    rendered: &mut [String],
    suffix: impl Fn(ContributorId<'a>) -> String,
) {
    let mut raws_by_label: FxHashMap<&str, FxHashSet<&str>> = FxHashMap::default();
    for (id, label) in ids.iter().zip(rendered.iter()) {
        raws_by_label
            .entry(label.as_str())
            .or_default()
            .insert(id.raw);
    }
    let shared: FxHashSet<String> = raws_by_label
        .into_iter()
        .filter(|(_, raws)| raws.len() > 1)
        .map(|(label, _)| label.to_string())
        .collect();
    for (id, label) in ids.iter().zip(rendered.iter_mut()) {
        if shared.contains(label.as_str()) {
            *label = format!("{label}~{}", suffix(*id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::ownership::compile_bot_globs;

    fn handle(raw: &str) -> &str {
        ContributorId::new(raw).handle()
    }

    fn handles(raws: &[&str]) -> Vec<String> {
        let ids: Vec<ContributorId<'_>> = raws.iter().copied().map(ContributorId::new).collect();
        render_identifiers(&ids, EmailMode::Handle)
    }

    #[test]
    fn handle_is_the_local_part() {
        assert_eq!(handle("alice@example.com"), "alice");
        assert_eq!(handle("alice"), "alice");
        assert_eq!(handle("@example.com"), "@example.com");
    }

    #[test]
    fn handle_unwraps_only_the_github_noreply_form() {
        assert_eq!(handle("12345+alice@users.noreply.github.com"), "alice");
        assert_eq!(handle("12345+alice@USERS.NOREPLY.github.com"), "alice");
        assert_eq!(handle("john+github@gmail.com"), "john+github");
        assert_eq!(handle("team+alice@users.noreply.github.com"), "team+alice");
    }

    #[test]
    fn same_person_compares_address_and_handle() {
        let bart = ContributorId::new("bart@corp.com");
        assert!(bart.same_person(ContributorId::new("Bart@Corp.com")));
        assert!(bart.same_person(ContributorId::new("bart@gmail.com")));
        assert!(bart.same_person(ContributorId::new("1+bart@users.noreply.github.com")));
        assert!(!bart.same_person(ContributorId::new("alice@corp.com")));
    }

    #[test]
    fn bot_pattern_matches_local_part_or_full_value() {
        let local = compile_bot_globs(&["release-bot".to_string()]).expect("valid glob");
        assert!(ContributorId::new("release-bot@corp.com").is_bot(&local));
        assert!(ContributorId::new("release-bot").is_bot(&local));
        assert!(!ContributorId::new("alice@corp.com").is_bot(&local));

        let domain = compile_bot_globs(&["*@bots.corp.com".to_string()]).expect("valid glob");
        assert!(ContributorId::new("deploy@bots.corp.com").is_bot(&domain));
    }

    #[test]
    fn pseudonym_is_stable_and_differs_per_input() {
        let first = ContributorId::new("alice@example.com").pseudonym();
        assert_eq!(first, ContributorId::new("alice@example.com").pseudonym());
        assert!(first.starts_with("xxh3:"));
        assert_eq!(first.len(), "xxh3:".len() + 16);
        assert_ne!(
            ContributorId::new("alice@x").pseudonym(),
            ContributorId::new("bob@x").pseudonym()
        );
    }

    #[test]
    fn render_identifiers_gives_distinct_labels_to_distinct_values() {
        assert_eq!(
            handles(&["john+github@gmail.com", "jane+github@gmail.com"]),
            vec!["john+github", "jane+github"]
        );
        assert_eq!(
            handles(&["alice@contractor.io", "alice@company.com"]),
            vec!["alice~contractor.io", "alice~company.com"]
        );
        let noreply = handles(&[
            "1+alice@users.noreply.github.com",
            "2+alice@users.noreply.github.com",
        ]);
        assert_ne!(noreply[0], noreply[1]);
        for label in &noreply {
            assert!(
                label.starts_with("alice~users.noreply.github.com~id"),
                "{label}"
            );
        }
    }
}
