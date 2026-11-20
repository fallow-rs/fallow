use clap::CommandFactory;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::Value;

use super::super::exit_code_table::COMMAND_EXIT_CODES;
use super::*;
use crate::Cli;

fn reference() -> Value {
    build_cli_reference(&Cli::command())
}

/// Every command of the document, at each depth, by its path.
fn commands_by_path(doc: &Value) -> FxHashMap<String, Value> {
    fn walk(commands: &Value, out: &mut FxHashMap<String, Value>) {
        for command in commands.as_array().expect("commands is an array") {
            let path = command["path"].as_str().expect("path is a string");
            out.insert(path.to_string(), command.clone());
            walk(&command["subcommands"], out);
        }
    }
    let mut out = FxHashMap::default();
    walk(&doc["commands"], &mut out);
    out
}

/// Every command of the clap tree, at each depth, by its path, with the
/// long names of its own options.
fn clap_commands() -> Vec<(String, Vec<String>)> {
    fn walk(cmd: &clap::Command, path: &str, out: &mut Vec<(String, Vec<String>)>) {
        for sub in cmd.get_subcommands() {
            if sub.get_name() == "help" {
                continue;
            }
            let sub_path = if path.is_empty() {
                sub.get_name().to_string()
            } else {
                format!("{path} {}", sub.get_name())
            };
            let options = sub
                .get_arguments()
                .filter(|arg| !arg.is_global_set())
                .filter_map(|arg| arg.get_long().map(|long| format!("--{long}")))
                .filter(|long| long != "--help")
                .collect();
            out.push((sub_path.clone(), options));
            walk(sub, &sub_path, out);
        }
    }
    let mut root = Cli::command();
    root.build();
    let mut out = Vec::new();
    walk(&root, "", &mut out);
    out
}

fn option_longs(command: &Value) -> FxHashSet<String> {
    command["options"]
        .as_array()
        .expect("options is an array")
        .iter()
        .filter_map(|option| option["long"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn every_clap_command_and_option_is_in_the_reference() {
    let doc = reference();
    let documented = commands_by_path(&doc);
    let tree = clap_commands();
    assert!(tree.len() > 60, "the clap tree walk found too few commands");
    for (path, options) in &tree {
        let command = documented
            .get(path)
            .unwrap_or_else(|| panic!("command `{path}` is missing from the CLI reference"));
        let longs = option_longs(command);
        for option in options {
            assert!(
                longs.contains(option),
                "option `{option}` of `{path}` is missing from the CLI reference"
            );
        }
    }
    assert_eq!(
        documented.len(),
        tree.len(),
        "the reference lists a command clap does not have"
    );
}

#[test]
fn global_options_are_listed_once_at_the_top() {
    let doc = reference();
    let globals: FxHashSet<String> = doc["global_options"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|option| option["long"].as_str().map(str::to_string))
        .collect();
    for long in [
        "--root",
        "--config",
        "--format",
        "--quiet",
        "--changed-since",
    ] {
        assert!(globals.contains(long), "global option {long} is missing");
    }
    for command in commands_by_path(&doc).values() {
        let longs = option_longs(command);
        assert!(
            !longs.contains("--format"),
            "`{}` repeats the global --format option",
            command["path"]
        );
    }
}

#[test]
fn commands_follow_the_order_and_groups_of_the_root_help() {
    let doc = reference();
    let commands = doc["commands"].as_array().unwrap();
    assert_eq!(commands[0]["name"], "dead-code");
    assert_eq!(commands[0]["help_group"], "Analysis");
    let schema = commands
        .iter()
        .find(|command| command["name"] == "schema")
        .unwrap();
    assert_eq!(schema["help_group"], "Reference");
    for command in commands.iter().filter(|command| command["hidden"] == false) {
        assert!(
            command["help_group"].is_string(),
            "visible command `{}` has no help group",
            command["name"]
        );
    }
}

#[test]
fn nested_subcommands_carry_their_options_and_usage() {
    let doc = reference();
    let commands = commands_by_path(&doc);
    let coverage = &commands["coverage"];
    let names: Vec<&str> = coverage["subcommands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|sub| sub["name"].as_str().unwrap())
        .collect();
    for expected in [
        "setup",
        "analyze",
        "upload-inventory",
        "upload-static-findings",
    ] {
        assert!(
            names.contains(&expected),
            "coverage {expected} is missing: {names:?}"
        );
    }
    let upload = &commands["coverage upload-inventory"];
    assert!(!upload["options"].as_array().unwrap().is_empty());
    assert!(
        upload["usage"]
            .as_str()
            .unwrap()
            .starts_with("fallow coverage upload-inventory"),
        "{}",
        upload["usage"]
    );
    assert!(upload["long_about"].is_string() || upload["about"].is_string());
}

#[test]
fn options_carry_value_names_aliases_and_long_help() {
    let doc = reference();
    let format = doc["global_options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["long"] == "--format")
        .unwrap();
    assert_eq!(format["short"], "-f");
    assert_eq!(format["value_name"], "FORMAT");
    assert_eq!(format["default"], "human");
    assert!(
        format["aliases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|alias| alias["name"] == "--output" && alias["visible"] == true)
    );
    assert!(
        format["possible_values"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["name"] == "markdown" && value["aliases"][0] == "md")
    );

    let commands = commands_by_path(&doc);
    let dupes = &commands["dupes"];
    let trace = dupes["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["long"] == "--trace")
        .unwrap();
    assert!(trace["takes_value"].as_bool().unwrap());
    assert!(trace["value_name"].is_string());
    let flag = dupes["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["takes_value"] == false)
        .unwrap();
    assert!(flag["possible_values"].as_array().unwrap().is_empty());
    assert!(flag["default"].is_null());
}

#[test]
fn negation_pairs_name_an_existing_flag() {
    let doc = reference();
    let globals: FxHashSet<String> = doc["global_options"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|option| option["long"].as_str().map(str::to_string))
        .collect();
    let mut pairs = 0;
    for command in commands_by_path(&doc).values() {
        let mut longs = option_longs(command);
        longs.extend(globals.iter().cloned());
        for option in command["options"].as_array().unwrap() {
            if let Some(positive) = option["negates"].as_str() {
                pairs += 1;
                assert!(
                    longs.contains(positive),
                    "{positive} of {}",
                    command["path"]
                );
                assert_eq!(
                    option["long"].as_str().unwrap(),
                    format!("--no-{}", positive.trim_start_matches("--"))
                );
            }
        }
    }
    assert!(pairs > 0, "no --x/--no-x pair was found");
}

fn option<'a>(command: &'a Value, long: &str) -> &'a Value {
    command["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["long"] == long)
        .unwrap_or_else(|| panic!("{long} is missing from {}", command["path"]))
}

#[test]
fn a_command_flag_does_not_negate_a_global_option_with_a_value() {
    let doc = reference();
    let commands = commands_by_path(&doc);
    let no_config = option(&commands["rule-pack init"], "--no-config");
    assert!(no_config["negates"].is_null(), "{no_config}");
    let no_ignore_imports = option(&commands["dupes"], "--no-ignore-imports");
    assert_eq!(no_ignore_imports["negates"], "--ignore-imports");
    for command in commands.values() {
        for option in command["options"].as_array().unwrap() {
            if option["negates"].is_string() || option["negated_by"].is_string() {
                assert_eq!(option["takes_value"], false, "{option}");
            }
        }
    }
}

#[test]
fn hidden_commands_stay_in_the_reference() {
    let mut root = Cli::command();
    root.build();
    let hidden: Vec<&str> = root
        .get_subcommands()
        .filter(|sub| sub.is_hide_set())
        .map(clap::Command::get_name)
        .collect();
    let doc = reference();
    let commands = commands_by_path(&doc);
    for name in hidden {
        assert_eq!(commands[name]["hidden"], true, "{name}");
    }
}

#[test]
fn exit_codes_have_unique_codes_and_names() {
    let doc = reference();
    let codes = doc["exit_codes"].as_array().unwrap();
    let numbers: FxHashSet<u64> = codes.iter().map(|c| c["code"].as_u64().unwrap()).collect();
    let names: FxHashSet<&str> = codes.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(numbers.len(), codes.len());
    assert_eq!(names.len(), codes.len());
    for code in [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13] {
        assert!(numbers.contains(&code), "exit code {code} is missing");
    }
}

#[test]
fn command_exit_codes_name_existing_commands() {
    let doc = reference();
    let commands = commands_by_path(&doc);
    for entry in COMMAND_EXIT_CODES {
        let command = commands
            .get(entry.path)
            .unwrap_or_else(|| panic!("exit codes name the unknown command `{}`", entry.path));
        assert!(!command["exit_codes"].as_array().unwrap().is_empty());
    }
    let mutation = &commands["mutation"]["exit_codes"];
    assert!(
        mutation
            .as_array()
            .unwrap()
            .iter()
            .any(|code| code["name"] == "min-score" && code["code"] == 1)
    );
}

#[test]
fn every_issue_type_carries_its_explanation() {
    let doc = reference();
    let rows = doc["issue_types"].as_array().unwrap();
    let Value::Array(registry) = super::super::issue_types_schema() else {
        panic!("issue types schema is not an array");
    };
    assert_eq!(rows.len(), registry.len());
    for row in rows {
        let explanation = &row["explanation"];
        assert!(explanation.is_object(), "{} has no explanation", row["id"]);
        for field in ["summary", "rationale", "docs"] {
            assert!(
                explanation[field]
                    .as_str()
                    .is_some_and(|text| !text.is_empty()),
                "{} has no {field}",
                row["id"]
            );
        }
    }
    let unused_export = rows
        .iter()
        .find(|row| row["id"] == "unused-export")
        .unwrap();
    assert!(unused_export["explanation"]["example"].is_string());
    assert!(unused_export["explanation"]["how_to_fix"].is_string());
}

#[test]
fn always_json_marks_the_json_only_commands() {
    let doc = reference();
    let commands = commands_by_path(&doc);
    assert_eq!(commands["config-schema"]["always_json"], true);
    assert_eq!(commands["dead-code"]["always_json"], false);
}

#[test]
fn output_formats_list_every_format_value() {
    let doc = reference();
    let names: Vec<&str> = doc["output_formats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|format| format["name"].as_str().unwrap())
        .collect();
    for expected in [
        "human",
        "json",
        "sarif",
        "markdown",
        "codeclimate",
        "github-summary",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} is missing: {names:?}"
        );
    }
}

#[test]
fn reference_has_a_schema_version_and_the_binary_version() {
    let doc = reference();
    assert_eq!(doc["schema_version"], CLI_REFERENCE_SCHEMA_VERSION);
    assert_eq!(doc["version"], env!("CARGO_PKG_VERSION"));
    assert!(doc["usage"].as_str().unwrap().starts_with("fallow"));
}

fn exit_code_numbers(command: &Value) -> Vec<u64> {
    command["exit_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|code| code["code"].as_u64().unwrap())
        .collect()
}

#[test]
fn source_map_upload_lists_the_codes_that_it_returns() {
    let doc = reference();
    let commands = commands_by_path(&doc);
    let upload = &commands["coverage upload-source-maps"];
    assert_eq!(
        exit_code_numbers(upload),
        vec![1],
        "{}",
        upload["exit_codes"]
    );
    assert_eq!(upload["exit_codes"][0]["name"], "upload-incomplete");
    for path in [
        "coverage upload-inventory",
        "coverage upload-static-findings",
        "security",
    ] {
        assert!(
            exit_code_numbers(&commands[path]).is_empty(),
            "`{path}` repeats shared codes: {}",
            commands[path]["exit_codes"]
        );
    }
}

#[test]
fn command_exit_tables_hold_only_command_specific_meanings() {
    let doc = reference();
    let shared = doc["exit_codes"].as_array().unwrap();
    for command in commands_by_path(&doc).values() {
        for code in command["exit_codes"].as_array().unwrap() {
            let same_text = shared.iter().any(|def| {
                def["code"] == code["code"] && def["description"] == code["description"]
            });
            assert!(
                !same_text,
                "`{}` repeats shared exit code {} with the shared text",
                command["path"], code["code"]
            );
        }
    }
    let setup = &commands_by_path(&doc)["coverage setup"];
    assert_eq!(exit_code_numbers(setup), vec![1]);
}

/// Every option of the clap tree, at each depth, with its hidden flag.
fn clap_hidden_flags() -> Vec<(String, String, bool)> {
    fn walk(cmd: &clap::Command, path: &str, out: &mut Vec<(String, String, bool)>) {
        for arg in cmd.get_arguments() {
            if let Some(long) = arg.get_long() {
                out.push((path.to_string(), format!("--{long}"), arg.is_hide_set()));
            }
        }
        for sub in cmd.get_subcommands().filter(|sub| sub.get_name() != "help") {
            let sub_path = if path.is_empty() {
                sub.get_name().to_string()
            } else {
                format!("{path} {}", sub.get_name())
            };
            walk(sub, &sub_path, out);
        }
    }
    let mut root = Cli::command();
    root.build();
    let mut out = Vec::new();
    walk(&root, "", &mut out);
    out
}

#[test]
fn hidden_marks_follow_clap_for_every_option() {
    let doc = reference();
    let commands = commands_by_path(&doc);
    let globals = &doc["global_options"];
    let mut hidden = 0;
    for (path, long, clap_hidden) in clap_hidden_flags() {
        let options = if path.is_empty() {
            globals.as_array().unwrap().clone()
        } else {
            let mut own = commands[&path]["options"].as_array().unwrap().clone();
            own.extend(globals.as_array().unwrap().iter().cloned());
            own
        };
        let Some(option) = options.iter().find(|option| option["long"] == long) else {
            continue;
        };
        hidden += usize::from(clap_hidden);
        assert_eq!(
            option["hidden"], clap_hidden,
            "{long} of `{path}` has the wrong hidden mark"
        );
    }
    assert!(hidden >= 4, "the walk found too few hidden options");
    let parent_run = globals
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["long"] == "--parent-run")
        .unwrap();
    assert_eq!(parent_run["hidden"], true);
}

#[test]
fn environment_variables_carry_a_group_and_the_code_default() {
    let doc = reference();
    let variables = doc["environment_variables"].as_array().unwrap();
    for variable in variables {
        assert!(
            variable["group"]
                .as_str()
                .is_some_and(|group| super::super::environment::ENVIRONMENT_GROUPS.contains(&group)),
            "{} has no known group",
            variable["name"]
        );
        assert!(variable.get("default").is_some(), "{}", variable["name"]);
    }
    let by_name = |name: &str| {
        variables
            .iter()
            .find(|variable| variable["name"] == name)
            .unwrap_or_else(|| panic!("{name} is missing"))
    };
    assert_eq!(by_name("FALLOW_CACHE_MAX_SIZE")["default"], "256");
    assert_eq!(by_name("FALLOW_CACHE_MAX_SIZE")["group"], "cache");
    assert_eq!(by_name("FALLOW_TIMEOUT_SECS")["group"], "mcp");
    assert_eq!(by_name("FALLOW_GATE_REPORT")["group"], "commit-gate");
    assert!(by_name("FALLOW_API_KEY")["default"].is_null());
}

#[test]
fn every_issue_type_has_a_sentence_case_display_label() {
    let doc = reference();
    for row in doc["issue_types"].as_array().unwrap() {
        let label = row["display_label"]
            .as_str()
            .unwrap_or_else(|| panic!("{} has no display_label", row["id"]));
        let first_word = label.split(' ').next().unwrap_or_default();
        assert!(
            label.chars().next().is_some_and(char::is_uppercase)
                || first_word.chars().skip(1).any(char::is_uppercase),
            "{label} starts with neither a capital nor a name such as jQuery"
        );
        for word in label.split([' ', '-', '/']).skip(1) {
            let mut chars = word.chars();
            let title_case = chars.next().is_some_and(char::is_uppercase)
                && chars.clone().count() > 0
                && chars.all(|c| !c.is_alphabetic() || c.is_lowercase());
            assert!(
                !title_case || super::super::issue_labels::PROPER_NAMES.contains(&word),
                "`{label}` of {} is not sentence case",
                row["id"]
            );
        }
    }
    let label_of = |id: &str| {
        doc["issue_types"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap()["display_label"]
            .clone()
    };
    assert_eq!(label_of("high-crap-score"), "High CRAP score");
    assert_eq!(label_of("css-token-drift"), "CSS token drift");
    assert_eq!(label_of("test-only-branch"), "Test-only branches");
    assert_eq!(label_of("unused-file"), "Unused files");
    assert_eq!(
        label_of("nextjs-open-redirect"),
        "Next.js open redirect sink"
    );
}

#[test]
fn complexity_rows_name_their_rules_key() {
    let doc = reference();
    let rows = doc["issue_types"].as_array().unwrap();
    for (id, key) in [
        ("high-cyclomatic-complexity", "complexity-cyclomatic"),
        ("high-cognitive-complexity", "complexity-cognitive"),
        ("high-crap-score", "complexity-crap"),
        ("long-file", "complexity-file-lines"),
    ] {
        let row = rows.iter().find(|row| row["id"] == id).unwrap();
        assert_eq!(row["config_key"], key, "{id}");
        assert_eq!(row["default_severity"], "error", "{id}");
        assert_eq!(row["opt_in"], false, "{id}");
    }
}

#[test]
fn mcp_tools_and_resources_are_in_the_reference() {
    let doc = reference();
    let tools: Vec<&str> = doc["mcp_tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    let expected: Vec<&str> = fallow_types::mcp_manifest::MCP_TOOLS
        .iter()
        .map(|tool| tool.name)
        .collect();
    assert_eq!(tools, expected);
    assert_eq!(
        doc["mcp_resources"].as_array().unwrap().len(),
        fallow_types::mcp_manifest::MCP_RESOURCES.len()
    );
}

#[test]
fn dupes_options_carry_their_help_heading() {
    let doc = reference();
    let commands = commands_by_path(&doc);
    let dupes = &commands["dupes"];
    for option in dupes["options"].as_array().unwrap() {
        assert!(
            option["help_heading"]
                .as_str()
                .is_some_and(|heading| ["Detection", "Output", "Gates"].contains(&heading)),
            "{} has no dupes help heading",
            option["long"]
        );
    }
    assert_eq!(option(dupes, "--mode")["help_heading"], "Detection");
    assert_eq!(option(dupes, "--threshold")["help_heading"], "Gates");
    assert_eq!(option(dupes, "--trace")["help_heading"], "Output");
}

/// The help headings that options may use. `fallow <command> --help` and the
/// option tables of the documentation group options by these names, so a
/// heading has one meaning across commands. Add a name here only for a
/// purpose that no name of the list covers.
const HELP_HEADINGS: &[&str] = &[
    "Project",
    "Scope",
    "Detection",
    "Issue types",
    "Sections",
    "Tracing",
    "Coverage",
    "Health",
    "Duplication",
    "Type-aware",
    "Input",
    "Provider",
    "Fallow Cloud",
    "Output",
    "Review",
    "Baselines",
    "Gates",
    "Reruns",
    "Run",
    "Performance",
];

/// A command with more own visible options than this groups each of them
/// under a help heading.
const MAX_UNGROUPED_OPTIONS: usize = 6;

/// The own visible options of a command, without `--help` and `--version`.
fn own_visible_options(command: &Value) -> Vec<&Value> {
    command["options"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|option| option["hidden"] == false && option["global"] == false)
        .filter(|option| option["long"] != "--help" && option["long"] != "--version")
        .collect()
}

#[test]
fn every_option_of_a_large_command_has_a_help_heading() {
    let doc = reference();
    let mut commands: Vec<(String, Value)> = commands_by_path(&doc).into_iter().collect();
    commands.push(("fallow".to_string(), doc["root_command"].clone()));
    for (path, command) in &commands {
        let options = own_visible_options(command);
        if options.len() <= MAX_UNGROUPED_OPTIONS {
            continue;
        }
        for option in options {
            assert!(
                option["help_heading"].is_string(),
                "option {} of `{path}` has no help heading",
                option["long"]
            );
        }
    }
    for option in doc["global_options"].as_array().unwrap() {
        assert!(
            option["help_heading"].is_string(),
            "global option {} has no help heading",
            option["long"]
        );
    }
}

#[test]
fn help_headings_come_from_the_vocabulary() {
    let doc = reference();
    let mut options: Vec<(String, &Value)> = Vec::new();
    let commands = commands_by_path(&doc);
    for (path, command) in &commands {
        for option in command["options"].as_array().unwrap() {
            options.push((path.clone(), option));
        }
    }
    for scope in [&doc["root_command"]["options"], &doc["global_options"]] {
        for option in scope.as_array().unwrap() {
            options.push(("fallow".to_string(), option));
        }
    }
    for (path, option) in options {
        if let Some(heading) = option["help_heading"].as_str() {
            assert!(
                HELP_HEADINGS.contains(&heading),
                "option {} of `{path}` uses the help heading `{heading}`, which is not in HELP_HEADINGS",
                option["long"]
            );
        }
    }
}

#[test]
fn options_report_an_optional_value_and_each_value_name() {
    let doc = reference();
    let commands = commands_by_path(&doc);
    let snapshot = option(&commands["health"], "--save-snapshot");
    assert_eq!(snapshot["value_optional"], true);
    assert_eq!(snapshot["value_names"], serde_json::json!(["PATH"]));
    let path = option(&commands["trace"], "--path");
    assert_eq!(path["value_names"], serde_json::json!(["FROM", "TO"]));
    assert_eq!(path["value_optional"], false);
    let flag = option(&commands["dupes"], "--near");
    assert_eq!(flag["value_names"], serde_json::json!([]));
    assert_eq!(flag["value_optional"], false);
}
