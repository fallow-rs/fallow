use std::ffi::OsStr;

use clap::CommandFactory;

use crate::Cli;

pub const SECURITY_UNSUPPORTED_GLOBAL_LONGS: &[&str] = &[
    "baseline",
    "save-baseline",
    "production",
    "no-production",
    "group-by",
    "performance",
    "explain-skipped",
    "fail-on-regression",
    "regression-baseline",
    "save-regression-baseline",
    "dupes-mode",
    "dupes-threshold",
    "dupes-min-tokens",
    "dupes-min-lines",
    "dupes-min-occurrences",
    "dupes-skip-local",
    "dupes-cross-language",
    "dupes-ignore-imports",
    "include-entry-exports",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityHelpTarget {
    Parent,
    Survivors,
    BlindSpots,
}

pub fn security_help_target<I, S>(args: I) -> Option<SecurityHelpTarget>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string_lossy().into_owned())
        .collect();

    if args.first().is_some_and(|arg| arg == "help") {
        return match args.get(1).map(String::as_str) {
            Some("security") if args.len() == 2 => Some(SecurityHelpTarget::Parent),
            Some("security") if args.get(2).is_some_and(|arg| arg == "survivors") => {
                Some(SecurityHelpTarget::Survivors)
            }
            Some("security") if args.get(2).is_some_and(|arg| arg == "blind-spots") => {
                Some(SecurityHelpTarget::BlindSpots)
            }
            _ => None,
        };
    }

    let mut saw_security = false;
    let mut security_subcommand = None;
    for arg in args {
        if arg == "security" {
            saw_security = true;
            continue;
        }
        if saw_security && is_security_subcommand(&arg) {
            security_subcommand = Some(arg);
            continue;
        }
        if saw_security && matches!(arg.as_str(), "--help" | "-h") {
            return match security_subcommand.as_deref() {
                Some("survivors") => Some(SecurityHelpTarget::Survivors),
                Some("blind-spots") => Some(SecurityHelpTarget::BlindSpots),
                Some("help") => None,
                _ => Some(SecurityHelpTarget::Parent),
            };
        }
    }
    None
}

fn is_security_subcommand(arg: &str) -> bool {
    matches!(arg, "survivors" | "blind-spots" | "help")
}

pub fn render_security_help(target: SecurityHelpTarget) -> String {
    match target {
        SecurityHelpTarget::Parent => render_security_parent_help(),
        SecurityHelpTarget::Survivors => render_security_survivors_help(),
        SecurityHelpTarget::BlindSpots => render_security_blind_spots_help(),
    }
}

fn render_security_parent_help() -> String {
    let mut root = Cli::command().mut_args(|arg| {
        if arg.get_long().is_some_and(security_unsupported_global_long) {
            arg.hide(true)
        } else {
            arg
        }
    });
    match root.try_get_matches_from_mut(["fallow", "security", "--help"]) {
        Ok(_) => String::new(),
        Err(err) => err.to_string(),
    }
}

fn render_security_survivors_help() -> String {
    "\
Render verifier-retained survivor candidates from fallow output plus verifier verdicts.

Usage: fallow security survivors --candidates <PATH> --verdicts <PATH> [OPTIONS]

Options:
      --candidates <PATH>                      Raw `fallow security --format json` candidate output
      --verdicts <PATH>                        Verifier verdict JSON file
      --require-verdict-for-each-candidate     Fail when any candidate has no matching verdict
  -f, --format <FORMAT>                        Output format: human or json [default: human]
  -o, --output-file <PATH>                     Write the report to a file instead of stdout
  -h, --help                                   Print help

Verdict JSON:
  [{\"schema_version\":\"fallow-security-verdict/v1\",\"finding_id\":\"sec-a\",\"verdict\":\"survivor\"}]

Repo-local docs: docs/security-agent-verification.md
"
    .to_owned()
}

fn render_security_blind_spots_help() -> String {
    "\
Group unresolved security callees into actionable blind-spot output.

Usage: fallow security blind-spots [OPTIONS]

Options:
  -r, --root <ROOT>                  Project root directory
  -c, --config <CONFIG>              Path to config file
  -f, --format <FORMAT>              Output format: human or json [default: human]
  -q, --quiet                        Suppress progress output
      --no-cache                     Disable incremental caching
      --threads <THREADS>            Number of parser threads
      --changed-since <REF>          Scope analysis to files changed since this git ref
      --diff-file <PATH>             Unified diff for line-level scoping
      --diff-stdin                   Read the unified diff from stdin
      --file <PATH>                  Scope diagnostics to selected files
  -w, --workspace <WORKSPACE>        Scope output to selected workspaces
      --changed-workspaces <REF>     Scope output to workspaces touched since this git ref
  -o, --output-file <PATH>           Write the report to a file instead of stdout
  -h, --help                         Print help
"
    .to_owned()
}

pub fn security_unsupported_global_long(long: &str) -> bool {
    SECURITY_UNSUPPORTED_GLOBAL_LONGS.contains(&long)
}
