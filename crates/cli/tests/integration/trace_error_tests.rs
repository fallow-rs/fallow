//! `fallow trace-error [FILE|-]` behavior.
//!
//! The verb invites overclaiming, so the pinned properties are the refusals: a
//! frame with several matches stays ambiguous, a frame with none stays visible
//! as not-found, a frame outside project source is neither, and the counts
//! always close over the reported frames.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::common::{CommandOutput, fallow_bin, parse_json, run_fallow_in_root};
use tempfile::tempdir;

/// Run `fallow trace-error` with the trace piped in on stdin.
fn run_trace_error_stdin(root: &Path, trace: &str, args: &[&str]) -> CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.arg("trace-error")
        .arg("--root")
        .arg(root)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for arg in args {
        cmd.arg(arg);
    }
    let mut child = cmd.spawn().expect("failed to spawn fallow binary");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(trace.as_bytes())
        .expect("failed to write the trace to stdin");
    let output = child.wait_with_output().expect("failed to run fallow");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

/// A project with one unambiguous export, one name that two definitions in the
/// same module answer to, and one module-local function that is not a
/// definition the graph knows.
fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("src/services")).unwrap();
    std::fs::create_dir_all(root.join("node_modules/vendor")).unwrap();
    std::fs::create_dir_all(root.join("dist")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"trace-error-fixture","type":"module"}"#,
    )
    .unwrap();
    std::fs::write(root.join("tsconfig.json"), r#"{"include":["src"]}"#).unwrap();
    std::fs::write(root.join(".fallowrc.json"), r#"{"entry":["src/index.ts"]}"#).unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { loadUser } from './services/user';\nimport { Task, run } from './task';\nloadUser();\nrun();\nnew Task().run();\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/services/user.ts"),
        "const helper = () => 1;\nexport const loadUser = () => helper();\n",
    )
    .unwrap();
    // `run` is both a standalone export and a method of the exported class, so
    // a `Task.run` frame legitimately names two definitions.
    std::fs::write(
        root.join("src/task.ts"),
        "export const run = (): number => 0;\nexport class Task {\n  run(): number {\n    return run();\n  }\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("node_modules/vendor/index.js"),
        "export const vendored = () => 0;\n",
    )
    .unwrap();
    std::fs::write(root.join("dist/bundle.js"), "console.log(1);\n").unwrap();
    // Two short definitions far apart, and one long one, so a frame's line can
    // be checked against the definition its identifier matched: line 6 belongs
    // to `second`, while line 26 is still inside `long`.
    let mut wide = String::from(
        "export const first = (): number => {\n  return 0;\n};\n\nexport const second = (): number => {\n  throw new Error('boom');\n};\n\nexport const long = (): number => {\n",
    );
    for index in 0..16 {
        writeln!(wide, "  const value{index} = {index};").unwrap();
    }
    wide.push_str("  throw new Error('deep');\n};\n");
    std::fs::write(root.join("src/wide.ts"), wide).unwrap();
}

#[test]
fn a_frame_naming_one_definition_resolves_to_it() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(
        dir.path(),
        "TypeError: helper is not a function\n    at loadUser (src/services/user.ts:2:32)\n",
        &["--format", "json"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(value["kind"], "trace-error");
    assert_eq!(value["schema_version"], "1");
    assert_eq!(value["source"], "stdin");
    assert_eq!(value["header"], "TypeError: helper is not a function");
    assert_eq!(value["frames"][0]["origin"], "in_project");
    assert_eq!(value["frames"][0]["resolution"], "resolved");
    assert_eq!(value["frames"][0]["function"], "loadUser");
    assert_eq!(value["frames"][0]["line"], 2);
    assert_eq!(
        value["frames"][0]["candidates"][0]["file"],
        "src/services/user.ts"
    );
    assert_eq!(value["frames"][0]["candidates"][0]["symbol"], "loadUser");
    assert_eq!(value["frames"][0]["candidates"][0]["kind"], "export");
    assert_eq!(value["frames"][0]["candidates"][0]["line"], 2);
    assert_eq!(value["counts"]["resolved"], 1);
    assert_eq!(value["counts"]["frames"], 1);
}

#[test]
fn a_frame_naming_several_definitions_says_so_instead_of_picking_one() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(
        dir.path(),
        "Error: boom\n    at Task.run (src/task.ts:4:12)\n",
        &["--format", "json"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    let frame = &value["frames"][0];
    assert_eq!(frame["resolution"], "ambiguous");
    assert_eq!(
        frame["candidates"].as_array().unwrap().len(),
        2,
        "both the standalone export and the class method are listed: {frame}"
    );
    assert_eq!(frame["candidates"][0]["symbol"], "Task");
    assert_eq!(frame["candidates"][0]["member"], "run");
    assert_eq!(frame["candidates"][0]["kind"], "class-method");
    assert_eq!(frame["candidates"][1]["symbol"], "run");
    assert!(frame["candidates"][1].get("member").is_none());
    assert_eq!(frame["candidates_omitted"], 0);
    assert_eq!(value["counts"]["ambiguous"], 1);
    assert_eq!(value["counts"]["resolved"], 0);
}

#[test]
fn a_frame_naming_a_module_local_function_is_not_found_not_dropped() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(
        dir.path(),
        "Error: boom\n    at helper (src/services/user.ts:1:16)\n",
        &["--format", "json"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    let frame = &value["frames"][0];
    assert_eq!(frame["origin"], "in_project");
    assert_eq!(frame["resolution"], "not_found");
    assert_eq!(frame["function"], "helper");
    assert_eq!(frame["candidates"].as_array().unwrap().len(), 0);
    assert_eq!(value["counts"]["not_found"], 1);
    assert_eq!(
        value["counts"]["frames"], 1,
        "a not-found frame still occupies a row"
    );
}

#[test]
fn frames_outside_project_source_are_kept_classified_and_never_asked() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(
        dir.path(),
        "Error: boom\n\
         \x20   at loadUser (src/services/user.ts:2:32)\n\
         \x20   at vendored (node_modules/vendor/index.js:1:22)\n\
         \x20   at n (dist/bundle.js:1:200)\n\
         \x20   at process.processTicksAndRejections (node:internal/process/task_queues:95:5)\n\
         \x20   at Array.forEach (<anonymous>)\n",
        &["--format", "json"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    let frames = value["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 5, "no frame is filtered out of the array");
    assert_eq!(frames[0]["origin"], "in_project");
    assert_eq!(frames[1]["origin"], "node_modules");
    assert_eq!(frames[1]["resolution"], "not_attempted");
    assert_eq!(frames[2]["origin"], "out_of_corpus");
    assert_eq!(frames[2]["resolution"], "not_attempted");
    assert!(
        frames[2]["reason"].as_str().unwrap().contains("source map"),
        "a generated-bundle frame names why it was not resolved: {}",
        frames[2]["reason"]
    );
    assert_eq!(frames[3]["origin"], "out_of_corpus");
    assert_eq!(frames[4]["origin"], "out_of_corpus");
    assert!(frames[4].get("file").is_none());

    let counts = &value["counts"];
    assert_eq!(counts["frames"], 5);
    assert_eq!(counts["in_project"], 1);
    assert_eq!(counts["node_modules"], 1);
    assert_eq!(counts["out_of_corpus"], 3);
    assert_eq!(counts["resolved"], 1);
    assert_eq!(counts["not_attempted"], 4);
    let sum = counts["resolved"].as_u64().unwrap()
        + counts["ambiguous"].as_u64().unwrap()
        + counts["not_found"].as_u64().unwrap()
        + counts["not_attempted"].as_u64().unwrap();
    assert_eq!(sum, counts["frames"].as_u64().unwrap());
}

#[test]
fn an_empty_trace_is_an_answer_not_an_error() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(dir.path(), "", &["--format", "json"]);

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(value["kind"], "trace-error");
    assert_eq!(value["frames"].as_array().unwrap().len(), 0);
    assert_eq!(value["counts"]["frames"], 0);
    assert_eq!(value["counts"]["unparsed_lines"], 0);
    assert!(value.get("header").is_none());
    assert_eq!(value["reason"], "no stack frames in the input");
}

#[test]
fn a_trace_with_no_recognisable_frames_reports_the_lines_it_could_not_read() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(
        dir.path(),
        "something went wrong\nsee the logs\nand the dashboard\n",
        &["--format", "json"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(value["counts"]["frames"], 0);
    assert_eq!(
        value["counts"]["unparsed_lines"], 2,
        "the first line is reported as the header, the rest are counted"
    );
    assert_eq!(value["header"], "something went wrong");
    // The sentence describes the INPUT, so it counts the header line back in:
    // three lines were pasted and `unparsed_lines` deliberately excludes the
    // one reported under `header`.
    assert_eq!(
        value["reason"], "no stack frames recognised in 3 non-blank input lines",
        "the count must describe the input the caller pasted"
    );
}

/// A one-line input that is not a frame reports one input line, not zero.
#[test]
fn a_single_unrecognisable_line_is_counted_as_input() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(dir.path(), "boom\n", &["--format", "json"]);

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(value["counts"]["unparsed_lines"], 0);
    assert_eq!(value["header"], "boom");
    assert_eq!(
        value["reason"],
        "no stack frames recognised in 1 non-blank input line"
    );
}

#[test]
fn a_trace_read_from_a_file_reports_the_path_as_its_source() {
    let dir = tempdir().unwrap();
    write_project(dir.path());
    let trace_path = dir.path().join("crash.txt");
    std::fs::write(
        &trace_path,
        "Error: boom\n    at loadUser (src/services/user.ts:2:32)\n",
    )
    .unwrap();

    let output = run_fallow_in_root(
        "trace-error",
        dir.path(),
        &[trace_path.to_str().unwrap(), "--format", "json"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(value["source"], trace_path.to_str().unwrap());
    assert_eq!(value["frames"][0]["resolution"], "resolved");
}

#[test]
fn an_unreadable_trace_file_exits_two_with_a_remedy() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_fallow_in_root(
        "trace-error",
        dir.path(),
        &["does-not-exist.txt", "--format", "json"],
    );

    assert_eq!(output.code, 2, "stdout:\n{}", output.stdout);
    let value: serde_json::Value = serde_json::from_str(output.stdout.trim()).unwrap();
    assert!(
        value["help"]
            .as_str()
            .is_some_and(|help| help.contains("fallow trace-error -")),
        "the failure must name the next step, including that input can be piped: {}",
        output.stdout
    );

    let human = run_fallow_in_root("trace-error", dir.path(), &["does-not-exist.txt"]);
    assert_eq!(human.code, 2);
    assert!(
        human
            .stderr
            .contains("hint: pass a stack-trace file, or pipe one"),
        "stderr was {}",
        human.stderr
    );
}

#[test]
fn the_firefox_frame_form_resolves_the_same_way() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(
        dir.path(),
        "loadUser@src/services/user.ts:2:32\n",
        &["--format", "json"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(value["frames"][0]["resolution"], "resolved");
    assert_eq!(value["frames"][0]["function"], "loadUser");
}

#[test]
fn repeated_runs_return_a_byte_identical_payload() {
    let dir = tempdir().unwrap();
    write_project(dir.path());
    let trace = "Error: boom\n    at Task.run (src/task.ts:4:12)\n    at loadUser (src/services/user.ts:2:32)\n";

    let first = run_trace_error_stdin(dir.path(), trace, &["--format", "json"]);
    let second = run_trace_error_stdin(dir.path(), trace, &["--format", "json"]);

    assert_eq!(first.code, 0, "stderr:\n{}", first.stderr);
    let strip = |value: serde_json::Value| {
        let mut value = value;
        value.as_object_mut().map(|object| object.remove("_meta"));
        serde_json::to_string(&value).unwrap()
    };
    assert_eq!(strip(parse_json(&first)), strip(parse_json(&second)));
}

#[test]
fn human_output_prints_every_candidate_of_an_ambiguous_frame() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(
        dir.path(),
        "Error: boom\n    at Task.run (src/task.ts:4:12)\n",
        &[],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    assert!(
        output.stdout.contains("in-project/ambiguous"),
        "stdout:\n{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("Task.run (class-method)"),
        "stdout:\n{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("run (export)"),
        "stdout:\n{}",
        output.stdout
    );
}

#[test]
fn a_relative_trace_path_resolves_against_the_project_root() {
    let dir = tempdir().unwrap();
    write_project(dir.path());
    std::fs::write(
        dir.path().join("crash.txt"),
        "Error: boom\n    at loadUser (src/services/user.ts:2:32)\n",
    )
    .unwrap();

    let output = run_fallow_in_root(
        "trace-error",
        dir.path(),
        &["crash.txt", "--format", "json"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(
        value["source"], "crash.txt",
        "the reported source keeps the caller's spelling, not the resolved path"
    );
    assert_eq!(value["frames"][0]["resolution"], "resolved");
}

#[test]
fn a_frame_spelled_through_a_symlinked_root_resolves_like_the_canonical_path() {
    let dir = tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::create_dir_all(&real).unwrap();
    write_project(&real);
    let linked = dir.path().join("linked");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &linked).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&real, &linked).unwrap();

    // A runtime prints the path ITS process saw, so a project reached through
    // a symlink yields absolute frame paths no module path carries verbatim.
    let frame_path = linked.join("src/services/user.ts");
    let trace = format!(
        "TypeError: helper is not a function\n    at loadUser ({}:2:32)\n",
        frame_path.display()
    );
    let output = run_trace_error_stdin(&linked, &trace, &["--format", "json"]);

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(
        value["frames"][0]["origin"], "in_project",
        "a frame naming a real project file must not be reported out of corpus \
         because the root was reached through a symlink; frame was {}",
        value["frames"][0]
    );
    assert_eq!(value["frames"][0]["resolution"], "resolved");
    assert_eq!(
        value["frames"][0]["candidates"][0]["file"],
        "src/services/user.ts"
    );
    assert_eq!(
        value["frames"][0]["file"],
        frame_path.to_string_lossy().replace('\\', "/"),
        "the frame still reports the path as the runtime spelled it"
    );
}

#[test]
fn an_absolute_frame_path_outside_the_project_stays_out_of_corpus() {
    let dir = tempdir().unwrap();
    write_project(dir.path());
    let elsewhere = tempdir().unwrap();
    let outside = elsewhere.path().join("outside.ts");
    std::fs::write(&outside, "export const loadUser = () => 0;\n").unwrap();

    let trace = format!("Error: boom\n    at loadUser ({}:1:1)\n", outside.display());
    let output = run_trace_error_stdin(dir.path(), &trace, &["--format", "json"]);

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(
        value["frames"][0]["origin"], "out_of_corpus",
        "resolving a symlinked spelling must not turn a file outside the corpus \
         into a match; frame was {}",
        value["frames"][0]
    );
    assert_eq!(value["frames"][0]["resolution"], "not_attempted");
}

#[test]
fn a_resolved_frame_whose_line_sits_at_another_definition_says_so() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(
        dir.path(),
        "Error: boom\n    at first (src/wide.ts:6:9)\n",
        &["--format", "json"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(
        value["frames"][0]["resolution"], "resolved",
        "the identifier IS one the graph knows, so the frame stays resolved"
    );
    assert_eq!(
        value["frames"][0]["line_mismatch"], true,
        "line 6 is declared by `second`, not by the matched `first`; frame was {}",
        value["frames"][0]
    );
    let reason = value["frames"][0]["reason"].as_str().unwrap();
    assert!(
        reason.contains("'second'") && reason.contains("line 5"),
        "reason was {reason}"
    );
}

#[test]
fn a_frame_deep_inside_a_long_definition_is_not_flagged() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(
        dir.path(),
        "Error: deep\n    at long (src/wide.ts:26:9)\n",
        &["--format", "json"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    assert_eq!(value["frames"][0]["resolution"], "resolved");
    assert!(
        value["frames"][0].get("line_mismatch").is_none(),
        "a line 17 rows below its own declaration is still inside it; frame was {}",
        value["frames"][0]
    );
}

/// The empty state carries its own measurement, so `--quiet`, which drops
/// prose, cannot leave an unrecognised input looking like an empty one.
#[test]
fn quiet_human_output_keeps_the_unparsed_line_count() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(
        dir.path(),
        "something went wrong\nsee the logs\nand the dashboard\n",
        &["--quiet"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    assert!(
        output
            .stdout
            .contains("No stack frames recognised (2 input lines did not parse as a frame)."),
        "--quiet must not hide the lines that were not read, or an unrecognised \
         input looks exactly like an empty trace; stdout:\n{}",
        output.stdout
    );
}

/// The empty state used to state the same fact three times (a literal notice,
/// an all-zero counts line, and the prose `reason`) and then stop, with no next
/// step and nothing saying the input can be piped.
#[test]
fn the_empty_state_states_the_fact_once_and_says_what_to_do() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(dir.path(), "", &[]);

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    assert_eq!(
        output.stdout.matches("No stack frames recognised").count(),
        1,
        "one statement of the fact, not three; stdout:\n{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("frames 0 | resolved 0"),
        "an all-zero counts line restates the sentence above it; stdout:\n{}",
        output.stdout
    );
    assert!(
        output
            .stdout
            .contains("hint: pass a stack-trace file, or pipe one"),
        "the empty state must name the next step; stdout:\n{}",
        output.stdout
    );
}

/// One reason explains a class of frame, and a stack is usually one class
/// repeated. Sixty byte-identical explanations sat between the reader and the
/// counts; each frame still carries its own `[origin/resolution]` labels.
#[test]
fn a_repeated_frame_reason_is_stated_once() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    use std::fmt::Write as _;

    let mut trace = String::from("Error: boom\n");
    for index in 0..20 {
        let _ = writeln!(
            trace,
            "    at handler{index} (node_modules/vendor/index.js:{}:1)",
            index + 1
        );
    }
    let output = run_trace_error_stdin(dir.path(), &trace, &["--quiet"]);

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    assert_eq!(
        output
            .stdout
            .matches("frame is in an installed dependency, not in project source")
            .count(),
        1,
        "one class of frame, one explanation; stdout:\n{}",
        output.stdout
    );
    assert_eq!(
        output
            .stdout
            .matches("[node-modules/not-attempted]")
            .count(),
        20,
        "every frame keeps its own classification; stdout:\n{}",
        output.stdout
    );
}

/// A reason that CHANGES prints again, so suppressing repeats never hides a
/// different answer.
#[test]
fn a_changed_frame_reason_prints_again() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(
        dir.path(),
        "Error: boom\n\
         \x20   at a (node_modules/vendor/index.js:1:1)\n\
         \x20   at b (node_modules/vendor/index.js:2:1)\n\
         \x20   at n (dist/bundle.js:1:200)\n",
        &["--quiet"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    assert_eq!(
        output
            .stdout
            .matches("frame is in an installed dependency, not in project source")
            .count(),
        1,
        "stdout:\n{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("source map"),
        "a different class of frame states its own reason; stdout:\n{}",
        output.stdout
    );
}

#[test]
fn quiet_human_output_keeps_the_omitted_frame_count() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let mut trace = String::from("Error: boom\n");
    for _ in 0..300 {
        trace.push_str("    at loadUser (src/services/user.ts:2:32)\n");
    }
    let output = run_trace_error_stdin(dir.path(), &trace, &["--quiet"]);

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    assert!(
        output.stdout.contains("frames omitted 44"),
        "--quiet must not hide the frames the cap withheld, or a truncated trace \
         looks complete; stdout:\n{}",
        output.stdout
    );
}

#[test]
fn the_counts_line_stays_clean_when_nothing_was_omitted() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_trace_error_stdin(
        dir.path(),
        "Error: boom\n    at loadUser (src/services/user.ts:2:32)\n",
        &["--quiet"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    assert!(
        output.stdout.contains("frames 1 | resolved 1"),
        "stdout:\n{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("frames omitted"),
        "a zero omission count is not a measurement worth a permanent column; stdout:\n{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("unparsed lines"),
        "stdout:\n{}",
        output.stdout
    );
}

#[cfg(unix)]
#[test]
fn a_trace_stream_file_cannot_bypass_the_byte_limit() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("trace.pipe");
    let created = Command::new("mkfifo").arg(&path).status().unwrap();
    assert!(created.success());
    let writer_path = path.clone();
    let writer = std::thread::spawn(move || {
        let mut file = std::fs::File::create(writer_path).unwrap();
        file.write_all(&vec![
            b'x';
            fallow_engine::trace_error::MAX_STACK_TRACE_BYTES
                as usize
                + 1
        ])
        .unwrap();
    });
    let output = run_fallow_in_root(
        "trace-error",
        dir.path(),
        &[path.to_str().unwrap(), "--format", "json"],
    );
    writer.join().unwrap();
    assert_eq!(output.code, 2, "{}", output.stdout);
    let json: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert!(json["message"].as_str().unwrap().contains("byte limit"));
}
