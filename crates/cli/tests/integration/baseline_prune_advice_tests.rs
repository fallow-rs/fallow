//! The warning and gate must select the saved mode and the analyzed project.

use std::fs;

use crate::common::{copy_fixture_tree, fixture_path, run_fallow_raw_in_dir};
use fallow_engine::shell_quote::shell_quote;

#[test]
fn baseline_clients_can_count_stale_entries_and_format_a_prune_command() {
    use fallow_engine::baseline::{BaselineKind, BaselineStaleness};
    use fallow_output::BaselineMode;

    let counts = BaselineStaleness {
        entries: 5,
        matched: 2,
        current_findings: 2,
        change_scoped: false,
    };
    assert_eq!(counts.stale_entries(), 3);
    let baseline = std::env::current_dir().unwrap().join("saved baseline.json");
    let mode = BaselineMode {
        production: true,
        include_entry_exports: false,
    };
    assert_eq!(
        BaselineKind::Health.prune_command(&baseline, mode),
        format!(
            "fallow baselines prune --production --health-baseline {}",
            shell_quote(baseline.to_str().unwrap())
        )
    );
}

#[test]
fn health_prune_advice_keeps_the_project_and_saved_mode() {
    check_prune_advice("health", true);
    check_prune_advice("health", false);
}

#[test]
fn dupes_prune_advice_keeps_the_project_and_saved_mode() {
    check_prune_advice("dupes", true);
    check_prune_advice("dupes", false);
}

fn check_prune_advice(command: &str, production: bool) {
    let cwd = tempfile::tempdir().unwrap();
    let cwd = dunce::canonicalize(cwd.path()).unwrap();
    let root = cwd.join("source project");
    copy_fixture_tree(&fixture_path("duplicate-code"), &root);
    fs::write(
        root.join(".fallowrc.json"),
        r#"{"health":{"maxCyclomatic":1}}"#,
    )
    .unwrap();
    let complex = fixture_path("complexity-project").join("src/complex.ts");
    let copies = ["one", "two", "three", "four"].map(|name| root.join(format!("src/{name}.ts")));
    for path in &copies {
        fs::copy(&complex, path).unwrap();
    }

    let baseline = "saved baseline.json";
    let run = |flags: &[&str]| {
        let mut args = vec![command, "--no-cache", "--root", root.to_str().unwrap()];
        args.extend_from_slice(flags);
        run_fallow_raw_in_dir(&cwd, &args)
    };
    let mut flags = vec!["--save-baseline", baseline];
    if production {
        flags.push("--production");
    }
    let saved = run(&flags);
    assert!(cwd.join(baseline).is_file(), "{}", saved.stderr);
    for path in copies {
        fs::remove_file(path).unwrap();
    }
    let loaded = run(&["--baseline", baseline, "--fail-on-stale-baseline"]);
    assert_eq!(loaded.code, 1, "{}", loaded.stderr);
    let mode = if production { " --production" } else { "" };
    let expected = format!(
        "fallow baselines prune{mode} --{command}-baseline {} --root {}",
        shell_quote(cwd.join(baseline).to_str().unwrap()),
        shell_quote(root.to_str().unwrap())
    );
    for prefix in ["Warning:", "Baseline gate failed:"] {
        let message = loaded
            .stderr
            .lines()
            .find(|line| line.starts_with(prefix) && line.contains("baselines prune"))
            .unwrap_or_else(|| panic!("{}", loaded.stderr));
        assert!(message.contains(&expected), "{message}");
    }
}
