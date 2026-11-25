//! Install-time rendering of the gate script.

use std::path::Path;

use super::HOOK_SCRIPT_MARKER;

/// Template content of `.claude/hooks/fallow-gate.sh`. Contains the
/// `@@FALLOW_INSTALLER_VERSION@@` placeholder; use [`rendered_gate_script`] to
/// obtain the install-ready bytes.
pub const FALLOW_GATE_SCRIPT: &str = include_str!("setup_hooks/fallow-gate.sh");

/// Marker substituted at install time by [`rendered_gate_script`]. Follows the
/// autotools `@@NAME@@` convention: grep-friendly, bash-safe, unambiguously
/// "already rendered" when read in an installed script.
pub(super) const INSTALLER_VERSION_PLACEHOLDER: &str = "@@FALLOW_INSTALLER_VERSION@@";

/// Render the gate script with install-time substitutions applied. Stamps the
/// fallow version that invoked `setup-hooks` into a header comment for
/// forensics; the enforced `FALLOW_GATE_MIN_VERSION` default stays hand-bumped
/// by maintainers when a correctness-affecting filtering fix lands, and is
/// intentionally NOT substituted here.
#[must_use]
pub fn rendered_gate_script() -> String {
    render_gate_script(false, None)
}

/// True when `script` is a gate script that fallow wrote.
pub fn is_generated_gate_script(script: &str) -> bool {
    script.contains(HOOK_SCRIPT_MARKER)
}

/// True when `script` is a gate script that fallow rendered for the user
/// scope.
pub fn is_user_scope_gate_script(script: &str) -> bool {
    script.lines().any(|line| line == "GATE_SCOPE='user'")
}

/// Marker for the install scope (`project` or `user`) in the gate script.
const GATE_SCOPE_PLACEHOLDER: &str = "@@FALLOW_GATE_SCOPE@@";
/// Marker for the absolute fallow path that a user-scope gate runs first.
const GATE_BIN_PLACEHOLDER: &str = "@@FALLOW_GATE_BIN@@";

/// Render the gate script for one install scope. A project-scope gate runs
/// only the launcher of its install root or the fallow on PATH. A user-scope
/// gate runs only `fallow_bin` (the fallow that installs it) or the fallow on
/// PATH. Neither runs a launcher from the tree that it audits.
#[must_use]
pub fn render_gate_script(user: bool, fallow_bin: Option<&str>) -> String {
    let bin = if user {
        fallow_bin
            .filter(|bin| is_recordable_path(bin))
            .unwrap_or("")
    } else {
        ""
    };
    FALLOW_GATE_SCRIPT
        .replace(INSTALLER_VERSION_PLACEHOLDER, env!("CARGO_PKG_VERSION"))
        .replace(
            GATE_SCOPE_PLACEHOLDER,
            if user { "user" } else { "project" },
        )
        .replace(GATE_BIN_PLACEHOLDER, bin)
}

/// A path that fits in a single-quoted shell word on one line.
fn is_recordable_path(path: &str) -> bool {
    !path.is_empty() && !path.contains(['\'', '\n', '\r'])
}

/// The fallow that a user-scope gate records: the first `fallow` on `PATH`
/// that [`crate::setup_hooks::user_launcher::is_user_launcher`] accepts, as found, else the
/// running binary when it is a fallow CLI outside the project. `None` leaves
/// the gate with the `fallow` on `PATH` at run time.
pub(super) fn user_fallow_bin(root: &Path) -> Option<String> {
    let project = crate::setup_hooks::user_launcher::project_dirs(root);
    let found = crate::setup_hooks::user_launcher::find_user_launcher(
        "fallow",
        crate::setup_hooks::user_launcher::search_dirs(),
        &project,
    )
    .or_else(|| {
        let exe = std::env::current_exe().ok()?;
        (exe.file_stem()? == "fallow"
            && crate::setup_hooks::user_launcher::is_user_launcher(&exe, &project))
        .then_some(exe)
    })?;
    found
        .to_str()
        .filter(|path| is_recordable_path(path))
        .map(|path| gate_path_text(path, cfg!(windows)))
}

/// The text of a recorded path for the gate script. The gate splits a path
/// only on `/`, so on Windows each `\` becomes `/` (`C:/tools/fallow.exe`).
fn gate_path_text(path: &str, windows: bool) -> String {
    if windows {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    }
}

/// A POSIX shell word for `text`: single quotes, with an inner `'` written
/// as `'\''`.
pub(super) fn posix_single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(test)]
mod gate_path_tests {
    use super::gate_path_text;

    #[test]
    fn a_windows_path_uses_forward_slashes_in_the_gate() {
        assert_eq!(
            gate_path_text("C:\\Users\\u\\.cargo\\bin\\fallow.exe", true),
            "C:/Users/u/.cargo/bin/fallow.exe"
        );
        assert_eq!(
            gate_path_text("/usr/local/bin/fallow", false),
            "/usr/local/bin/fallow"
        );
    }
}
