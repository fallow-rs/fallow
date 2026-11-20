//! The deprecated commands and options of the CLI.
//!
//! clap has no deprecation attribute, so this table marks each deprecated
//! command and option. The capability manifest (`fallow schema`), the CLI
//! reference (`fallow schema --cli-reference`) and the parse-time stderr
//! warning read it. The skill docs generator reads the `deprecated` field of
//! the manifest, so it keeps no list of its own.

use clap::Command;
use clap::parser::{ArgMatches, ValueSource};
use serde_json::{Value, json};

/// One deprecated command or option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deprecation {
    /// The command path, for example `dead-code`.
    pub command: &'static str,
    /// The long option, for example `--circular-deps`. `None` marks the
    /// command itself.
    pub option: Option<&'static str>,
    /// The invocation that replaces it, when one exists.
    pub replacement: Option<&'static str>,
}

/// Every deprecated command and option. The next major version removes them
/// (issue #3293 for the `dead-code` architecture options).
pub const DEPRECATIONS: &[Deprecation] = &[
    Deprecation {
        command: "dead-code",
        option: Some("--circular-deps"),
        replacement: Some("fallow architecture --cycles"),
    },
    Deprecation {
        command: "dead-code",
        option: Some("--re-export-cycles"),
        replacement: Some("fallow architecture --cycles"),
    },
    Deprecation {
        command: "dead-code",
        option: Some("--package-cycles"),
        replacement: Some("fallow architecture --cycles"),
    },
    Deprecation {
        command: "dead-code",
        option: Some("--boundary-violations"),
        replacement: Some("fallow architecture --boundaries"),
    },
    Deprecation {
        command: "dead-code",
        option: Some("--policy-violations"),
        replacement: Some("fallow architecture --policy"),
    },
    Deprecation {
        command: "setup-hooks",
        option: None,
        replacement: Some("fallow agent install"),
    },
];

/// The deprecation of a command (`option` is `None`) or of one of its options.
#[must_use]
pub fn deprecation(command: &str, option: Option<&str>) -> Option<&'static Deprecation> {
    DEPRECATIONS
        .iter()
        .find(|entry| entry.command == command && entry.option == option)
}

/// The `deprecated` value of the machine contracts, or `None` when the
/// command or option is not deprecated. A caller writes the field only when
/// this returns a value, so the field is additive.
#[must_use]
pub fn deprecation_json(command: &str, option: Option<&str>) -> Option<Value> {
    deprecation(command, option).map(|entry| json!({ "replacement": entry.replacement }))
}

/// The deprecated options that the command line of `matches` sets. Call it
/// before `FromArgMatches` takes the values out of `matches`.
#[must_use]
pub fn options_on_command_line(root: &Command, matches: &ArgMatches) -> Vec<&'static Deprecation> {
    let Some((name, sub_matches)) = matches.subcommand() else {
        return Vec::new();
    };
    let Some(sub) = root.find_subcommand(name) else {
        return Vec::new();
    };
    DEPRECATIONS
        .iter()
        .filter(|entry| entry.command == name)
        .filter(|entry| {
            entry.option.is_some_and(|option| {
                let long = option.trim_start_matches("--");
                sub.get_arguments()
                    .find(|arg| arg.get_long() == Some(long))
                    .is_some_and(|arg| {
                        sub_matches.value_source(arg.get_id().as_str())
                            == Some(ValueSource::CommandLine)
                    })
            })
        })
        .collect()
}

/// The stderr warning for a deprecated option.
#[must_use]
pub fn option_warning(entry: &Deprecation) -> String {
    let option = entry.option.unwrap_or_default();
    let replacement = entry
        .replacement
        .map_or_else(String::new, |replacement| format!("; use `{replacement}`"));
    format!(
        "warning: `fallow {} {option}` is deprecated and will be removed in the next major{replacement}.",
        entry.command
    )
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;
    use crate::Cli;

    fn used(argv: &[&str]) -> Vec<&'static Deprecation> {
        let root = Cli::command();
        let matches = root
            .clone()
            .try_get_matches_from(argv)
            .expect("argv parses");
        options_on_command_line(&root, &matches)
    }

    #[test]
    fn every_entry_names_a_live_command_and_option() {
        let root = Cli::command();
        for entry in DEPRECATIONS {
            let sub = root
                .find_subcommand(entry.command)
                .unwrap_or_else(|| panic!("{} is not a command", entry.command));
            if let Some(option) = entry.option {
                let long = option.trim_start_matches("--");
                assert!(
                    sub.get_arguments().any(|arg| arg.get_long() == Some(long)),
                    "{option} is not an option of {}",
                    entry.command
                );
            }
        }
    }

    #[test]
    fn a_deprecated_option_on_the_command_line_is_reported_once() {
        let found = used(&["fallow", "dead-code", "--circular-deps", "--unused-files"]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].option, Some("--circular-deps"));
    }

    #[test]
    fn the_legacy_check_alias_reports_the_deprecated_option() {
        let found = used(&["fallow", "check", "--policy-violations"]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].option, Some("--policy-violations"));
    }

    #[test]
    fn current_options_report_nothing() {
        assert!(used(&["fallow", "dead-code", "--unused-files"]).is_empty());
        assert!(used(&["fallow", "architecture", "--cycles"]).is_empty());
        assert!(used(&["fallow"]).is_empty());
    }

    #[test]
    fn the_warning_names_the_replacement() {
        let entry = deprecation("dead-code", Some("--boundary-violations")).expect("entry");
        assert_eq!(
            option_warning(entry),
            "warning: `fallow dead-code --boundary-violations` is deprecated and will be removed in the next major; use `fallow architecture --boundaries`."
        );
    }
}
