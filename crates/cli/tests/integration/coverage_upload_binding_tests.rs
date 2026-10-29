use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

use crate::common::{commit_all, fallow_bin, git, git_capture};
use crate::http_stub::read_request;

const TEST_TOKEN: &str = "test-upload-bearer-secret";

fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"upload-test","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/index.ts"),
        "export const main = (): number => 1;\n",
    )
    .unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    commit_all(dir.path(), "initial");
    std::fs::write(
        dir.path().join("src/index.ts"),
        "export const main = (): number => 2;\n",
    )
    .unwrap();
    commit_all(dir.path(), "change");
    dir
}

fn inventory_command(root: &Path) -> Command {
    let mut command = Command::new(fallow_bin());
    command
        .current_dir(root)
        .args([
            "coverage",
            "upload-inventory",
            "--project-id",
            "owner/repo",
            "--git-sha",
            "abcdef123456",
            "--allow-dirty",
        ])
        .env("FALLOW_API_KEY", TEST_TOKEN)
        .env_remove("VERCEL")
        .env_remove("VERCEL_DEPLOYMENT_ID");
    command
}

fn capture_upload(command: &mut Command, endpoint_flag: &str) -> (Output, String, Value) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    command.args([
        endpoint_flag,
        &format!("http://{}", listener.local_addr().unwrap()),
    ]);
    let receiver = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let request = read_request(&mut stream);
                    let response =
                        r#"{"data":{"id":"test","functionCount":1,"blobSize":1,"fileSize":1}}"#;
                    write!(stream, "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
                    return request;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "upload never reached local server"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("accept upload: {err}"),
            }
        }
    });
    let output = command.output().unwrap();
    let request = receiver.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains(TEST_TOKEN));
    let (head, body) = request.split_once("\r\n\r\n").unwrap();
    (output, head.to_owned(), serde_json::from_str(body).unwrap())
}

#[test]
fn shallow_inventory_omits_churn_on_cold_and_cached_uploads() {
    let full = project();
    let (_, head, body) = capture_upload(&mut inventory_command(full.path()), "--api-endpoint");
    assert_eq!(body["churnByPath"]["src/index.ts"]["commits"], 2);
    assert!(
        !head
            .to_ascii_lowercase()
            .contains("x-fallow-vercel-deployment-id")
    );
    let destination = TempDir::new().unwrap();
    git(
        destination.path(),
        &[
            "clone",
            "--no-local",
            "--depth",
            "1",
            full.path().to_str().unwrap(),
            "shallow",
        ],
    );
    let shallow = destination.path().join("shallow");
    assert_eq!(
        git_capture(&shallow, &["rev-parse", "--is-shallow-repository"]),
        "true"
    );
    for _ in 0..2 {
        let (_, _, body) = capture_upload(&mut inventory_command(&shallow), "--api-endpoint");
        assert_eq!(body["functions"][0]["filePath"], "src/index.ts");
        assert!(
            body.get("churnByPath").is_none(),
            "partial shallow history must not be uploaded: {body}"
        );
    }
}

#[test]
fn inventory_upload_binds_explicit_and_vercel_build_deployments() {
    let root = project();
    for explicit in [true, false] {
        let mut command = inventory_command(root.path());
        command.env("VERCEL_DEPLOYMENT_ID", "dpl_Env123");
        if explicit {
            command.args(["--vercel-deployment-id", "dpl_Explicit123"]);
        } else {
            command.env("VERCEL", "1");
        }
        let (_, head, body) = capture_upload(&mut command, "--api-endpoint");
        let expected = if explicit {
            "dpl_Explicit123"
        } else {
            "dpl_Env123"
        };
        assert!(
            head.lines().any(|line| line
                .eq_ignore_ascii_case(&format!("x-fallow-vercel-deployment-id: {expected}"))),
            "missing deployment binding: {head}"
        );
        assert_eq!(body["gitSha"], "abcdef123456");
        assert_eq!(body["churnByPath"]["src/index.ts"]["commits"], 2);
    }
    let mut command = inventory_command(root.path());
    command.env("VERCEL_DEPLOYMENT_ID", "dpl_Env123");
    let (_, head, body) = capture_upload(&mut command, "--api-endpoint");
    assert!(
        !head
            .to_ascii_lowercase()
            .contains("x-fallow-vercel-deployment-id")
    );
    assert_eq!(body["functions"][0]["filePath"], "src/index.ts");
}

#[test]
fn inventory_rejects_invalid_deployment_bindings_without_exposing_bearer() {
    let root = project();
    for id in [
        "dpl_".to_owned(),
        "project_123".to_owned(),
        "dpl_bad-id".to_owned(),
        format!("dpl_{}", "a".repeat(125)),
    ] {
        let output = inventory_command(root.path())
            .args([
                "--vercel-deployment-id",
                &id,
                "--api-endpoint",
                "http://127.0.0.1:1",
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(10));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("invalid Vercel deployment ID"), "{stderr}");
        assert!(!stderr.contains(TEST_TOKEN));
    }
}

#[test]
fn source_map_upload_uses_the_same_deployment_binding() {
    let root = project();
    std::fs::create_dir(root.path().join("dist")).unwrap();
    std::fs::write(root.path().join("dist/app.js.map"), r#"{"version":3,"file":"app.js","sources":["../src/index.ts"],"sourcesContent":["export const main = (): number => 2;"],"names":[],"mappings":"AAAA"}"#).unwrap();
    let mut command = Command::new(fallow_bin());
    command
        .current_dir(root.path())
        .args([
            "coverage",
            "upload-source-maps",
            "--repo",
            "owner/repo",
            "--git-sha",
            "abcdef123456",
            "--vercel-deployment-id",
            "dpl_SourceMap123",
        ])
        .env("FALLOW_API_KEY", TEST_TOKEN)
        .env_remove("VERCEL")
        .env_remove("VERCEL_DEPLOYMENT_ID");
    let (_, head, body) = capture_upload(&mut command, "--endpoint");
    assert!(
        head.to_ascii_lowercase()
            .contains("x-fallow-vercel-deployment-id: dpl_sourcemap123")
    );
    assert_eq!(body["gitSha"], "abcdef123456");
    assert_eq!(body["fileName"], "app.js.map");
    assert_eq!(body["sourceMap"]["sources"][0], "../src/index.ts");
}
