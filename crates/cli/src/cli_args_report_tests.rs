use clap::Parser;

use super::{Cli, Command, MutationNotRunArg};

fn parse_report(extra: &[&str]) -> Result<Command, clap::Error> {
    let argv = ["fallow", "report", "--from", "r.json"]
        .iter()
        .chain(extra)
        .copied();
    Cli::try_parse_from(argv).map(|cli| cli.command.expect("a subcommand"))
}

#[test]
fn report_takes_a_mutation_report_path() {
    let Command::Report {
        mutation_report,
        mutation_not_run,
        ..
    } = parse_report(&["--mutation-report", "m.json"]).expect("the flag parses")
    else {
        panic!("the report subcommand");
    };
    assert_eq!(mutation_report, Some(std::path::PathBuf::from("m.json")));
    assert_eq!(mutation_not_run, None);
}

#[test]
fn report_takes_each_closed_not_run_reason() {
    let reasons = [
        ("event-refused", MutationNotRunArg::EventRefused),
        ("fork-refused", MutationNotRunArg::ForkRefused),
        ("no-change-scope", MutationNotRunArg::NoChangeScope),
        ("interrupted", MutationNotRunArg::Interrupted),
    ];
    for (text, expected) in reasons {
        let Command::Report {
            mutation_not_run, ..
        } = parse_report(&["--mutation-not-run", text]).expect("the reason parses")
        else {
            panic!("the report subcommand");
        };
        assert_eq!(mutation_not_run, Some(expected), "{text}");
    }
    let Err(error) = parse_report(&["--mutation-not-run", "free text"]) else {
        panic!("the reasons are a closed set");
    };
    assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
}

#[test]
fn the_two_mutation_flags_conflict() {
    let Err(error) = parse_report(&[
        "--mutation-report",
        "m.json",
        "--mutation-not-run",
        "interrupted",
    ]) else {
        panic!("a job passes one mutation input only");
    };
    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn report_without_the_mutation_flags_has_no_mutation_input() {
    let Command::Report {
        mutation_report,
        mutation_not_run,
        ..
    } = parse_report(&[]).expect("bare report parses")
    else {
        panic!("the report subcommand");
    };
    assert_eq!(mutation_report, None);
    assert_eq!(mutation_not_run, None);
}
