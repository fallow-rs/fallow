//! The message text of the run notices: the decisions of the resolver and
//! the flags of the request that a surface shows as one note each.

use super::WorkspaceDiagnosticKind;

/// The message of one run notice. `display` is the project-relative path of
/// the diagnostic. Any other kind renders as an empty string.
pub(super) fn run_notice_message(display: &str, kind: &WorkspaceDiagnosticKind) -> String {
    match kind {
        WorkspaceDiagnosticKind::InertFlag {
            flag,
            analysis,
            reason,
            key,
        } => inert_flag_message(flag, analysis, reason, key.as_deref()),
        WorkspaceDiagnosticKind::ConfigLoadFellBack { error } => format!(
            "The config did not load ({error}), so this run uses the default config. Fix the \
             config file to use its settings."
        ),
        WorkspaceDiagnosticKind::ConfigFileShadowed { used, ignored } => format!(
            "Two fallow config files exist in one directory: this run loads '{used}' and \
             ignores '{display}' ({ignored}). Remove the file that you do not use."
        ),
        WorkspaceDiagnosticKind::UnknownSecurityCategory { id } => format!(
            "The security category '{id}' in security.categories is not known, so the run \
             ignores it. Run fallow schema to see the valid ids."
        ),
        WorkspaceDiagnosticKind::EnvValueIgnored { name } => format!(
            "The value of {name} is not a bool, so the run ignores the variable. Set it to \
             true or false."
        ),
        _ => String::new(),
    }
}

fn inert_flag_message(flag: &str, analysis: &str, reason: &str, key: Option<&str>) -> String {
    match (reason, key) {
        ("off-by-config", Some(key)) => format!(
            "{flag} has no effect: {key} is false in the config, so the {analysis} analysis \
             does not run."
        ),
        ("not-read-by-command", _) => format!(
            "{flag} has no effect: this command does not read the production mode of the \
             {analysis} analysis. Remove the flag."
        ),
        _ => format!("{flag} has no effect on the {analysis} analysis ({reason})."),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::WorkspaceDiagnostic;
    use super::*;

    fn message(kind: WorkspaceDiagnosticKind) -> String {
        WorkspaceDiagnostic::new(Path::new("/project"), Path::new("/project").into(), kind).message
    }

    #[test]
    fn the_inert_dupes_baseline_message_names_the_key() {
        assert_eq!(
            message(WorkspaceDiagnosticKind::InertFlag {
                flag: "--dupes-baseline".to_owned(),
                analysis: "duplication".to_owned(),
                reason: "off-by-config".to_owned(),
                key: Some("duplicates.enabled".to_owned()),
            }),
            "--dupes-baseline has no effect: duplicates.enabled is false in the config, so the \
             duplication analysis does not run."
        );
    }

    #[test]
    fn the_inert_flag_and_the_unknown_security_category_do_not_degrade_the_run() {
        let notices = [
            WorkspaceDiagnosticKind::InertFlag {
                flag: "--production".to_owned(),
                analysis: "dead-code".to_owned(),
                reason: "not-read-by-command".to_owned(),
                key: None,
            },
            WorkspaceDiagnosticKind::ConfigLoadFellBack {
                error: "bad".to_owned(),
            },
            WorkspaceDiagnosticKind::ConfigFileShadowed {
                used: ".fallowrc.json".to_owned(),
                ignored: "fallow.toml".to_owned(),
            },
            WorkspaceDiagnosticKind::UnknownSecurityCategory { id: "x".to_owned() },
            WorkspaceDiagnosticKind::EnvValueIgnored {
                name: "FALLOW_PRODUCTION".to_owned(),
            },
        ];
        let degrading: Vec<bool> = notices.iter().map(|kind| kind.warns_on_stderr()).collect();
        // The run ignores an unknown category id, and each analysis runs on
        // its full input, so the kind is not a reason to distrust findings.
        assert_eq!(degrading, [false, true, true, false, true]);
        assert!(notices.iter().all(WorkspaceDiagnosticKind::is_run_notice));
        assert!(
            notices
                .iter()
                .all(|kind| !run_notice_message(".", kind).is_empty())
        );
    }

    #[test]
    fn the_env_notice_serializes_the_name_only() {
        let value = serde_json::to_value(WorkspaceDiagnostic::new(
            Path::new("/project"),
            Path::new("/project").into(),
            WorkspaceDiagnosticKind::EnvValueIgnored {
                name: "FALLOW_PRODUCTION".to_owned(),
            },
        ))
        .expect("diagnostic serializes");
        assert_eq!(value["kind"], "env-value-ignored");
        assert_eq!(value["name"], "FALLOW_PRODUCTION");
        assert_eq!(value["degrades_analysis"], true);
    }
}
