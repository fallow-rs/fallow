//! The GitHub Action and the GitLab CI template keep hand-written command
//! lists. These tests bind those lists to
//! `fallow_types::command_surfaces::COMMAND_ENVELOPES`, so a new analysis
//! command that the CI integrations do not run fails here unless its row says
//! why.

use std::collections::BTreeSet;
use std::path::PathBuf;

use fallow_types::command_surfaces::{COMMAND_ENVELOPES, CiIntegration};

/// Commands that the CI integrations accept but that have no analysis row:
/// the bare run, the legacy `check` alias of `dead-code`, and `fix`.
const CI_COMMANDS_WITHOUT_ROW: &[&str] = &["", "check", "fix"];

fn repo_file(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The alternatives of the first `case` arm that starts with `""|` and ends
/// with `) ;;`: the valid-command list of the input validation.
fn valid_command_list(source: &str, file: &str) -> BTreeSet<String> {
    let line = source
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("\"\"|") && line.ends_with(") ;;"))
        .unwrap_or_else(|| panic!("{file}: no valid-command `case` arm"));
    line.trim_end_matches(") ;;")
        .split('|')
        .map(|alternative| alternative.trim_matches('"').to_string())
        .collect()
}

/// Every command that a `case` arm in `select_summary_script` routes.
fn summary_routed_commands(source: &str) -> BTreeSet<String> {
    let start = source
        .find("select_summary_script() {")
        .expect("summary.sh defines select_summary_script");
    let body = &source[start..];
    let end = body.find("esac").expect("select_summary_script has a case");
    body[..end]
        .lines()
        .filter_map(|line| line.trim().split_once(')'))
        .filter(|(pattern, _)| !pattern.is_empty() && !pattern.contains('('))
        .flat_map(|(pattern, _)| pattern.split('|'))
        .map(|alternative| alternative.trim().trim_matches('"').to_string())
        .filter(|alternative| alternative != "*")
        .collect()
}

#[test]
fn ci_command_lists_follow_the_command_table() {
    let action = valid_command_list(
        &repo_file("action/scripts/analyze.sh"),
        "action/scripts/analyze.sh",
    );
    for file in ["ci/gitlab-ci.yml", "crates/cli/templates/ci/gitlab-ci.yml"] {
        assert_eq!(
            valid_command_list(&repo_file(file), file),
            action,
            "{file} must accept the same commands as the GitHub Action"
        );
    }

    let mut expected: BTreeSet<String> = CI_COMMANDS_WITHOUT_ROW
        .iter()
        .map(|command| (*command).to_string())
        .collect();
    for row in COMMAND_ENVELOPES {
        match row.ci {
            CiIntegration::Routed => {
                expected.insert(row.command.to_string());
            }
            CiIntegration::Omitted(reason) => {
                assert!(!reason.is_empty(), "{}: give a reason", row.command);
            }
        }
    }
    assert_eq!(
        action, expected,
        "the Action and GitLab valid-command lists must equal the routed rows of \
         COMMAND_ENVELOPES plus {CI_COMMANDS_WITHOUT_ROW:?}; mark a command that CI \
         does not run as CiIntegration::Omitted with a reason"
    );

    let routed = summary_routed_commands(&repo_file("action/scripts/summary.sh"));
    for command in &action {
        assert!(
            routed.contains(command),
            "action/scripts/summary.sh does not route `{command}` to a summary script"
        );
    }
}
