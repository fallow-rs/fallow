use rustc_hash::FxHashSet;

use super::super::{IgnoredCommandEntries, ScriptCatalog, analyze_commands_with_context};
use super::*;

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn analyze(root: &Path, command: &str) -> ScriptAnalysis {
    analyze_commands_with_context(
        &[command.to_string()],
        root,
        &FxHashMap::default(),
        &FxHashSet::default(),
        &ScriptCatalog::default(),
        IgnoredCommandEntries::NONE,
    )
}

fn credits(root: &Path, command: &str, package: &str) -> bool {
    analyze(root, command).used_packages.contains(package)
}

fn has_entry(result: &ScriptAnalysis, path: &str) -> bool {
    result.entry_files.iter().any(|entry| entry == path)
}

#[test]
fn shell_runs_a_helper_and_its_commands_count() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "scripts/build.sh",
        "#!/bin/sh\nreport-cli --format json\ntsx src/generate.ts\n",
    );
    let result = analyze(dir.path(), "bash scripts/build.sh");
    assert!(result.used_packages.contains("report-cli"));
    assert!(has_entry(&result, "src/generate.ts"));
}

#[test]
fn executable_helper_needs_a_shell_extension_or_shebang() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "scripts/release",
        "#!/usr/bin/env bash\nexec-cli\n",
    );
    write(dir.path(), "scripts/data", "data-cli\n");
    assert!(credits(dir.path(), "./scripts/release", "exec-cli"));
    assert!(!credits(dir.path(), "./scripts/data", "data-cli"));
}

#[test]
fn nested_helpers_are_followed_and_cycles_stop() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.sh", "a-cli\nbash b.sh\n");
    write(dir.path(), "b.sh", "b-cli\nsh a.sh\n");
    let result = analyze(dir.path(), "bash a.sh");
    assert!(result.used_packages.contains("a-cli"));
    assert!(result.used_packages.contains("b-cli"));
}

#[test]
fn nested_helpers_stop_at_the_depth_limit() {
    let dir = tempfile::tempdir().unwrap();
    for level in 0..=MAX_SCRIPT_INDIRECTION_DEPTH {
        write(
            dir.path(),
            &format!("h{level}.sh"),
            &format!("h{level}-cli\nbash h{}.sh\n", level + 1),
        );
    }
    let result = analyze(dir.path(), "bash h0.sh");
    let last = MAX_SCRIPT_INDIRECTION_DEPTH - 1;
    assert!(
        result.used_packages.contains(&format!("h{last}-cli")),
        "the deepest allowed helper is read"
    );
    assert!(
        !result
            .used_packages
            .contains(&format!("h{MAX_SCRIPT_INDIRECTION_DEPTH}-cli")),
        "a helper past the depth limit is not read"
    );
}

#[test]
fn wide_diamond_of_helpers_is_analyzed_once_per_helper() {
    let dir = tempfile::tempdir().unwrap();
    let last = MAX_SCRIPT_INDIRECTION_DEPTH - 1;
    let fan_out = 16;
    for level in 0..last {
        let calls = format!("./l{}.sh", level + 1);
        let body = vec![calls; fan_out].join("; ");
        write(dir.path(), &format!("l{level}.sh"), &format!("{body}\n"));
    }
    write(dir.path(), &format!("l{last}.sh"), "leaf-cli\n");
    let start = std::time::Instant::now();
    let result = analyze(dir.path(), "bash l0.sh");
    assert!(
        result.used_packages.contains("leaf-cli"),
        "the leaf of the diamond is credited"
    );
    // Without the visited set this is 16^7 helper analyses.
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "a helper diamond is analyzed once per helper: {:?}",
        start.elapsed()
    );
}

#[test]
fn helper_outside_the_project_or_in_node_modules_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "outside.sh", "outside-cli\n");
    write(dir.path(), "pkg/node_modules/dep/run.sh", "dep-cli\n");
    let pkg = dir.path().join("pkg");
    assert!(!credits(&pkg, "bash ../outside.sh", "outside-cli"));
    assert!(!credits(&pkg, "bash node_modules/dep/run.sh", "dep-cli"));
}

#[cfg(unix)]
#[test]
fn symlink_out_of_the_project_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "outside.sh", "outside-cli\n");
    let pkg = dir.path().join("pkg");
    std::fs::create_dir_all(pkg.join("scripts")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("outside.sh"), pkg.join("scripts/link.sh")).unwrap();
    assert!(!credits(&pkg, "bash scripts/link.sh", "outside-cli"));
}

#[test]
fn helper_larger_than_the_size_cap_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let padding = "#".repeat(usize::try_from(MAX_HELPER_BYTES).unwrap());
    write(dir.path(), "big.sh", &format!("{padding}\nbig-cli\n"));
    assert!(!credits(dir.path(), "bash big.sh", "big-cli"));
}

#[test]
fn non_utf8_helper_is_read_lossily() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("latin1.sh"), b"echo \xe9t\xe9\nlossy-cli\n").unwrap();
    assert!(credits(dir.path(), "bash latin1.sh", "lossy-cli"));
}

#[test]
fn unknown_directory_change_leaves_packages_only() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "run.sh",
        "tsx src/before.ts\ncd \"$TARGET\"\nlate-cli src/after.ts\n",
    );
    let result = analyze(dir.path(), "bash run.sh");
    assert!(has_entry(&result, "src/before.ts"));
    assert!(
        !has_entry(&result, "src/after.ts"),
        "{:?}",
        result.entry_files
    );
    assert!(result.used_packages.contains("late-cli"));
}

#[test]
fn literal_directory_change_moves_later_paths() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("app")).unwrap();
    write(dir.path(), "run.sh", "cd app && tsx src/inline.ts\n");
    let result = analyze(dir.path(), "bash run.sh");
    assert!(
        has_entry(&result, "app/src/inline.ts"),
        "{:?}",
        result.entry_files
    );
}

#[test]
fn helper_directory_change_resolves_from_the_helper() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "scripts/up.sh",
        "cd \"$(dirname \"$0\")/..\"\ntsx src/root.ts\n",
    );
    write(
        dir.path(),
        "scripts/here.sh",
        "cd \"${0%/*}\" || exit 1\ntsx gen.ts\n",
    );
    let up = analyze(dir.path(), "bash scripts/up.sh");
    assert!(has_entry(&up, "src/root.ts"), "{:?}", up.entry_files);
    let here = analyze(dir.path(), "bash scripts/here.sh");
    assert!(has_entry(&here, "scripts/gen.ts"), "{:?}", here.entry_files);
}

#[test]
fn directory_change_applies_per_command_segment() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "run.sh",
        "tsx src/a.ts && cd missing\ntsx src/b.ts\n",
    );
    let result = analyze(dir.path(), "bash run.sh");
    assert!(
        has_entry(&result, "src/a.ts"),
        "the command before the cd keeps its entry"
    );
    assert!(
        !has_entry(&result, "src/b.ts"),
        "a missing directory is unknown: {:?}",
        result.entry_files
    );
}

#[test]
fn top_level_directory_change_stops_helper_lookup() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "run.sh", "root-cli\n");
    assert!(!credits(dir.path(), "cd sub && bash run.sh", "root-cli"));
}

#[test]
fn script_file_commands_skip_comments_and_heredocs() {
    let commands = script_file_commands(
        "#!/bin/sh\n# note-cli\ncat <<-'EOF'\nheredoc-cli\nEOF\nfirst \\\n  --flag\n. ./env.sh\n",
    );
    assert_eq!(commands, vec!["cat <<-'EOF'", "first --flag"]);
}

#[test]
fn heredoc_delimiter_forms() {
    for (line, end) in [
        ("cat <<EOF", "EOF"),
        ("cat <<-EOF", "EOF"),
        ("cat << 'END'", "END"),
        ("cat <<\"END\"", "END"),
        ("cat <<\\END", "END"),
    ] {
        assert_eq!(heredoc_delimiter(line).as_deref(), Some(end), "{line}");
    }
    for line in [
        "x=$((1 << 3))",
        "((mask = 1 << 4))",
        "echo \"a << b\"",
        "echo 'a << b'",
        "read -r value <<< \"$input\"",
    ] {
        assert_eq!(heredoc_delimiter(line), None, "{line}");
    }
}

#[test]
fn arithmetic_shift_does_not_hide_later_commands() {
    let commands = script_file_commands("x=$((1 << 3))\nafter-cli\n");
    assert_eq!(commands, vec!["x=$((1 << 3))", "after-cli"]);
}
