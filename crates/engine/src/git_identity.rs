//! Git identity of the current user, for reviewer routing.

use std::path::Path;

use crate::health::ownership::ContributorId;
use crate::repo_refs::run_git;

/// Git identities for the current user in forms useful for self-routing.
///
/// Includes `user.email`, its mailmap-resolved address, the handle of
/// each, and `user.name`. Missing config values are ignored.
#[must_use]
pub fn current_user_identities(root: &Path) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let name = read_git_config(root, "user.name");
    let raw = read_git_config(root, "user.email");
    let resolved = raw
        .as_deref()
        .and_then(|email| mailmap_email(root, name.as_deref(), email));
    for email in [raw, resolved].into_iter().flatten() {
        let handle = ContributorId::new(&email).handle().to_owned();
        for id in [handle, email] {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    if let Some(name) = name {
        ids.push(name);
    }
    ids
}

/// `user.email` of the current user after mailmap, which is the form of
/// the author emails in the churn history. `None` when it is not set.
#[must_use]
pub fn current_user_email(root: &Path) -> Option<String> {
    let email = read_git_config(root, "user.email")?;
    let name = read_git_config(root, "user.name");
    Some(mailmap_email(root, name.as_deref(), &email).unwrap_or(email))
}

fn read_git_config(root: &Path, key: &str) -> Option<String> {
    run_git(root, &["config", "--get", key])
}

/// The address that the repository mailmap gives for `email`. The name
/// matters, because a mailmap entry can match on the commit name and email.
fn mailmap_email(root: &Path, name: Option<&str>, email: &str) -> Option<String> {
    let contact = name.map_or_else(|| format!("<{email}>"), |name| format!("{name} <{email}>"));
    let resolved = run_git(root, &["check-mailmap", &contact])?;
    let (_, rest) = resolved.rsplit_once('<')?;
    let (address, _) = rest.split_once('>')?;
    (!address.is_empty()).then(|| address.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) {
        let status = crate::git_env::git_command(root)
            .args(args)
            .current_dir(root)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn repo_with_email(email: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create repo");
        git(dir.path(), &["init", "--quiet"]);
        git(dir.path(), &["config", "user.name", "Test User"]);
        git(dir.path(), &["config", "user.email", email]);
        dir
    }

    #[test]
    fn current_user_email_follows_the_mailmap() {
        let dir = repo_with_email("test@example.com");
        let root = dir.path();
        assert_eq!(
            current_user_email(root).as_deref(),
            Some("test@example.com")
        );

        std::fs::write(
            root.join(".mailmap"),
            "Test User <canonical@example.com> <test@example.com>\n",
        )
        .expect("write mailmap");

        assert_eq!(
            current_user_email(root).as_deref(),
            Some("canonical@example.com")
        );
        let ids = current_user_identities(root);
        for expected in [
            "test",
            "test@example.com",
            "canonical",
            "canonical@example.com",
            "Test User",
        ] {
            assert!(
                ids.iter().any(|id| id == expected),
                "missing {expected}: {ids:?}"
            );
        }
    }

    #[test]
    fn current_user_email_follows_a_mailmap_entry_keyed_by_name() {
        let dir = repo_with_email("test@example.com");
        let root = dir.path();
        std::fs::write(
            root.join(".mailmap"),
            "Canonical User <canonical@example.com> Test User <test@example.com>\n",
        )
        .expect("write mailmap");

        assert_eq!(
            current_user_email(root).as_deref(),
            Some("canonical@example.com")
        );
    }

    #[test]
    fn plus_tag_stays_in_the_handle_of_the_current_user() {
        let dir = repo_with_email("bart+work@corp.test");
        let ids = current_user_identities(dir.path());
        assert!(ids.iter().any(|id| id == "bart+work"), "{ids:?}");
        assert!(!ids.iter().any(|id| id == "work"), "{ids:?}");
    }
}
