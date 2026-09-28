use std::process::Command;

#[test]
fn version_flag_exits_without_starting_stdio_server() {
    let output = Command::new(env!("CARGO_BIN_EXE_fallow-mcp"))
        .arg("--version")
        .output()
        .unwrap_or_else(|err| panic!("failed to run fallow-mcp --version: {err}"));

    assert!(
        output.status.success(),
        "version command failed: {output:?}"
    );

    let stdout =
        String::from_utf8(output.stdout).unwrap_or_else(|err| panic!("invalid stdout: {err}"));
    assert!(
        stdout.starts_with("fallow-mcp "),
        "unexpected version stdout: {stdout:?}"
    );
    assert!(
        output.stderr.is_empty(),
        "version probe should not initialize tracing or stdio server: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}
