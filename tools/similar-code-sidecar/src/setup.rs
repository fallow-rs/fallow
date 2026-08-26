use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;
use std::time::Duration;

use crate::cache::{
    ModelPaths, copy_and_hash, inspect_cache, remove_partial, replace_file, sha256_file,
    write_manifest,
};
use crate::constants::{ARTIFACTS, ArtifactSpec, MODEL_HUB_URL, MODEL_REVISION};

pub trait ArtifactDownloader {
    fn open(&self, url: &str) -> Result<Box<dyn Read>, String>;
}

const DNS_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const RESPONSE_HEADER_TIMEOUT: Duration = Duration::from_secs(30);
const RESPONSE_BODY_TIMEOUT: Duration = Duration::from_mins(15);
const ARTIFACT_TIMEOUT: Duration = Duration::from_mins(16);

#[derive(Clone, Copy)]
struct DownloadTimeouts {
    resolve: Duration,
    connect: Duration,
    response_header: Duration,
    response_body: Duration,
    artifact: Duration,
}

impl Default for DownloadTimeouts {
    fn default() -> Self {
        Self {
            resolve: DNS_TIMEOUT,
            connect: CONNECT_TIMEOUT,
            response_header: RESPONSE_HEADER_TIMEOUT,
            response_body: RESPONSE_BODY_TIMEOUT,
            artifact: ARTIFACT_TIMEOUT,
        }
    }
}

pub struct HttpDownloader {
    agent: ureq::Agent,
}

impl Default for HttpDownloader {
    fn default() -> Self {
        Self {
            agent: download_agent(DownloadTimeouts::default(), true),
        }
    }
}

impl ArtifactDownloader for HttpDownloader {
    fn open(&self, url: &str) -> Result<Box<dyn Read>, String> {
        let response = self
            .agent
            .get(url)
            .header(
                "User-Agent",
                concat!("fallow-similar-code/", env!("CARGO_PKG_VERSION")),
            )
            .call()
            .map_err(|error| format_download_error(&error))?;
        Ok(Box::new(ActionableBodyReader {
            inner: response.into_body().into_reader(),
        }))
    }
}

struct ActionableBodyReader<R> {
    inner: R,
}

impl<R: Read> Read for ActionableBodyReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buffer).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("model download response-body phase failed: {error}"),
            )
        })
    }
}

fn download_agent(timeouts: DownloadTimeouts, https_only: bool) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(https_only)
        .timeout_resolve(Some(timeouts.resolve))
        .timeout_connect(Some(timeouts.connect))
        .timeout_recv_response(Some(timeouts.response_header))
        .timeout_recv_body(Some(timeouts.response_body))
        .timeout_global(Some(timeouts.artifact))
        .build()
        .into()
}

fn format_download_error(error: &ureq::Error) -> String {
    let phase = match error {
        ureq::Error::Timeout(ureq::Timeout::Resolve) => "DNS resolution",
        ureq::Error::Timeout(ureq::Timeout::Connect) => "connect/TLS",
        ureq::Error::Timeout(ureq::Timeout::RecvResponse) => "response-header",
        ureq::Error::Timeout(ureq::Timeout::RecvBody) => "response-body",
        ureq::Error::Timeout(ureq::Timeout::Global) => "per-artifact",
        ureq::Error::Timeout(_) => "network",
        _ => return format!("model download failed: {error}"),
    };
    format!("model download {phase} phase timed out: {error}")
}

#[derive(Clone, Debug)]
pub struct SetupResult {
    pub downloaded: bool,
}

pub fn install(paths: &ModelPaths) -> Result<SetupResult, String> {
    install_with(paths, ARTIFACTS, &HttpDownloader::default())
}

pub fn install_with(
    paths: &ModelPaths,
    artifacts: &[ArtifactSpec],
    downloader: &dyn ArtifactDownloader,
) -> Result<SetupResult, String> {
    if artifacts == ARTIFACTS && inspect_cache(paths, true).ready {
        return Ok(SetupResult { downloaded: false });
    }

    fs::create_dir_all(&paths.directory)
        .map_err(|error| format!("failed to create the model cache: {error}"))?;
    let mut network_used = false;
    for artifact in artifacts {
        network_used |= install_artifact(paths, *artifact, downloader)?;
    }
    write_manifest(&paths.manifest)?;
    Ok(SetupResult {
        downloaded: network_used,
    })
}

fn install_artifact(
    paths: &ModelPaths,
    artifact: ArtifactSpec,
    downloader: &dyn ArtifactDownloader,
) -> Result<bool, String> {
    let destination = paths
        .artifact(artifact.path)
        .ok_or_else(|| format!("unsupported model artifact `{}`", artifact.path))?;
    if artifact_is_ready(destination, artifact) {
        return Ok(false);
    }
    let partial = destination.with_extension("download-partial");
    remove_partial(&partial);
    let result = download_artifact(&partial, artifact, downloader);
    if let Err(error) = result {
        remove_partial(&partial);
        return Err(error);
    }
    replace_file(&partial, destination)
        .map_err(|error| format!("failed to install `{}`: {error}", artifact.path))?;
    Ok(true)
}

fn artifact_is_ready(path: &Path, artifact: ArtifactSpec) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.file_type().is_file()
            && metadata.len() == artifact.size
            && sha256_file(path).is_ok_and(|digest| digest == artifact.sha256)
    })
}

fn download_artifact(
    partial: &Path,
    artifact: ArtifactSpec,
    downloader: &dyn ArtifactDownloader,
) -> Result<(), String> {
    let url = format!("{MODEL_HUB_URL}/resolve/{MODEL_REVISION}/{}", artifact.path);
    let reader = downloader.open(&url)?;
    let mut reader = reader.take(artifact.size.saturating_add(1));
    let mut file = File::create(partial)
        .map_err(|error| format!("failed to create `{}`: {error}", artifact.path))?;
    let (size, sha256) = copy_and_hash(&mut reader, &mut file)?;
    if size != artifact.size {
        return Err(format!(
            "downloaded `{}` has the wrong size: expected {}, received {size}",
            artifact.path, artifact.size
        ));
    }
    if sha256 != artifact.sha256 {
        return Err(format!(
            "downloaded `{}` failed SHA-256 verification",
            artifact.path
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "test fixture construction must fail immediately"
    )]

    use std::collections::BTreeMap;
    use std::io::{Cursor, Write};
    use std::net::TcpListener;
    use std::thread;

    use sha2::{Digest, Sha256};

    use super::*;

    struct MemoryDownloader {
        files: BTreeMap<String, Vec<u8>>,
    }

    struct RejectDownloader;

    impl ArtifactDownloader for RejectDownloader {
        fn open(&self, _url: &str) -> Result<Box<dyn Read>, String> {
            Err("network must not be used".to_string())
        }
    }

    impl ArtifactDownloader for MemoryDownloader {
        fn open(&self, url: &str) -> Result<Box<dyn Read>, String> {
            let name = url.rsplit('/').next().unwrap_or_default();
            self.files
                .get(name)
                .cloned()
                .map(|bytes| Box::new(Cursor::new(bytes)) as Box<dyn Read>)
                .ok_or_else(|| "missing test artifact".to_string())
        }
    }

    fn spec(name: &'static str, bytes: &[u8]) -> ArtifactSpec {
        let sha256 = crate::cache::digest_hex(Sha256::digest(bytes));
        ArtifactSpec {
            path: name,
            size: bytes.len() as u64,
            sha256: Box::leak(sha256.into_boxed_str()),
        }
    }

    fn short_timeouts() -> DownloadTimeouts {
        DownloadTimeouts {
            resolve: Duration::from_millis(100),
            connect: Duration::from_millis(100),
            response_header: Duration::from_millis(40),
            response_body: Duration::from_millis(40),
            artifact: Duration::from_millis(250),
        }
    }

    fn local_downloader() -> HttpDownloader {
        HttpDownloader {
            agent: download_agent(short_timeouts(), false),
        }
    }

    fn local_server(handler: impl FnOnce(std::net::TcpStream) + Send + 'static) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local fixture");
        let address = listener.local_addr().expect("local fixture address");
        thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept local fixture");
            handler(stream);
        });
        format!("http://{address}/artifact")
    }

    #[test]
    fn setup_verifies_test_artifacts_without_network() {
        let directory = tempfile::tempdir().expect("tempdir");
        let paths = ModelPaths::from_cache_root(directory.path());
        let files = BTreeMap::from([
            ("model.safetensors".to_string(), b"model".to_vec()),
            ("tokenizer.json".to_string(), b"tokenizer".to_vec()),
            ("config.json".to_string(), b"config".to_vec()),
        ]);
        let artifacts = [
            spec("model.safetensors", &files["model.safetensors"]),
            spec("tokenizer.json", &files["tokenizer.json"]),
            spec("config.json", &files["config.json"]),
        ];

        let result =
            install_with(&paths, &artifacts, &MemoryDownloader { files }).expect("offline setup");
        assert!(result.downloaded);
        for artifact in artifacts {
            assert!(paths.artifact(artifact.path).is_some_and(Path::is_file));
        }
    }

    #[test]
    fn setup_rejects_a_digest_mismatch_and_removes_partial_file() {
        let directory = tempfile::tempdir().expect("tempdir");
        let paths = ModelPaths::from_cache_root(directory.path());
        let artifacts = [ArtifactSpec {
            path: "config.json",
            size: 3,
            sha256: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        }];
        let downloader = MemoryDownloader {
            files: BTreeMap::from([("config.json".to_string(), b"bad".to_vec())]),
        };

        let error = install_with(&paths, &artifacts, &downloader).expect_err("digest mismatch");
        assert!(error.contains("SHA-256"));
        assert!(!paths.config.with_extension("download-partial").exists());
    }

    #[test]
    fn setup_reuses_verified_artifacts_when_only_the_manifest_is_stale() {
        let directory = tempfile::tempdir().expect("tempdir");
        let paths = ModelPaths::from_cache_root(directory.path());
        fs::create_dir_all(&paths.directory).expect("model directory");
        let bytes = b"config";
        fs::write(&paths.config, bytes).expect("cached config");
        let artifacts = [spec("config.json", bytes)];

        let result = install_with(&paths, &artifacts, &RejectDownloader).expect("offline reuse");

        assert!(!result.downloaded);
        assert_eq!(fs::read(&paths.config).expect("reused config"), bytes);
        assert!(paths.manifest.is_file());
    }

    #[test]
    fn downloader_configures_every_network_phase_timeout() {
        let expected = DownloadTimeouts::default();
        let config = download_agent(expected, true).config().clone();
        let actual = config.timeouts();

        assert_eq!(actual.resolve, Some(expected.resolve));
        assert_eq!(actual.connect, Some(expected.connect));
        assert_eq!(actual.recv_response, Some(expected.response_header));
        assert_eq!(actual.recv_body, Some(expected.response_body));
        assert_eq!(actual.global, Some(expected.artifact));
        assert!(config.https_only());
    }

    #[test]
    fn downloader_reports_a_response_header_timeout_from_a_local_fixture() {
        let url = local_server(|_stream| thread::sleep(Duration::from_millis(100)));
        let Err(error) = local_downloader().open(&url) else {
            panic!("expected response header timeout");
        };

        assert!(error.contains("response-header phase timed out"));
    }

    #[test]
    fn downloader_reports_a_response_body_timeout_from_a_local_fixture() {
        let url = local_server(|mut stream| {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n")
                .expect("write fixture headers");
            stream.flush().expect("flush fixture headers");
            thread::sleep(Duration::from_millis(100));
            let _ = stream.write_all(b"abc");
        });
        let mut reader = local_downloader().open(&url).expect("response headers");
        let mut bytes = Vec::new();
        let error = reader
            .read_to_end(&mut bytes)
            .expect_err("response body timeout");

        assert!(error.to_string().contains("response-body phase failed"));
        assert!(error.to_string().contains("timeout"));
    }
}
