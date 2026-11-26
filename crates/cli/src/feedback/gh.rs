//! Create the `fallow feedback` issue with the GitHub CLI (`gh`).

use std::io::Write as _;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// `HOST/OWNER/REPO`, so a `GH_HOST` setting cannot send the issue to another
/// GitHub host.
const GH_REPOSITORY: &str = "github.com/fallow-rs/fallow";
/// A `gh` run that takes longer has stalled on the network or on a prompt.
const GH_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Create the issue. `Ok` holds the issue URL, `Err` the reason for the
/// person who reads the output.
pub fn create_issue(title: &str, body: &str) -> Result<String, String> {
    let child = Command::new("gh")
        .args(["issue", "create", "--repo", GH_REPOSITORY, "--title", title])
        .args(["--body-file", "-"])
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err("the GitHub CLI (`gh`) is not installed".to_owned());
        }
        Err(err) => return Err(format!("cannot start the GitHub CLI (`gh`): {err}")),
    };
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    if let Some(mut stdin) = child.stdin.take()
        && let Err(err) = stdin.write_all(body.as_bytes())
    {
        stop(&mut child);
        return Err(format!(
            "cannot send the report to the GitHub CLI (`gh`): {err}"
        ));
    }
    let success = match wait(&mut child) {
        Ok(success) => success,
        Err(reason) => {
            stop(&mut child);
            return Err(reason);
        }
    };
    gh_result(
        success,
        &stdout.join().unwrap_or_default(),
        &stderr.join().unwrap_or_default(),
    )
}

/// Read a pipe on its own thread, so a full pipe cannot block `gh`.
fn drain<R: std::io::Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    })
}

fn wait(child: &mut Child) -> Result<bool, String> {
    let deadline = Instant::now() + GH_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) if Instant::now() >= deadline => {
                return Err(format!(
                    "the GitHub CLI (`gh`) did not finish in {} seconds",
                    GH_TIMEOUT.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(err) => return Err(format!("the GitHub CLI (`gh`) did not finish: {err}")),
        }
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn gh_result(success: bool, stdout: &str, stderr: &str) -> Result<String, String> {
    let issue_url = stdout
        .lines()
        .map(str::trim)
        .rfind(|line| line.starts_with("https://"));
    match (success, issue_url) {
        (true, Some(url)) => Ok(url.to_owned()),
        (true, None) => Err("the GitHub CLI (`gh`) printed no issue URL".to_owned()),
        (false, _) => {
            let detail = stderr
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("no error output");
            Err(format!("the GitHub CLI (`gh`) failed: {detail}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gh_output_maps_to_a_result() {
        assert_eq!(
            gh_result(
                true,
                "Creating issue\nhttps://github.com/fallow-rs/fallow/issues/9\n",
                ""
            ),
            Ok("https://github.com/fallow-rs/fallow/issues/9".to_owned())
        );
        let reason = gh_result(false, "", "\nHTTP 401: Bad credentials\n").unwrap_err();
        assert!(reason.contains("Bad credentials"), "{reason}");
        assert!(gh_result(true, "", "").is_err());
    }
}
