use std::env;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::constants::{
    ARTIFACTS, EMBEDDING_SEMANTICS_VERSION, MODEL_ID, MODEL_REVISION, PROTOCOL_VERSION,
};

const MANIFEST_FILE: &str = "manifest.json";
const INSTALLED_MANIFEST_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug)]
pub struct ModelPaths {
    pub directory: PathBuf,
    pub model: PathBuf,
    pub tokenizer: PathBuf,
    pub config: PathBuf,
    pub manifest: PathBuf,
}

impl ModelPaths {
    pub fn from_cache_root(cache_root: &Path) -> Self {
        let directory = cache_root.join("models").join(MODEL_REVISION);
        Self {
            model: directory.join("model.safetensors"),
            tokenizer: directory.join("tokenizer.json"),
            config: directory.join("config.json"),
            manifest: directory.join(MANIFEST_FILE),
            directory,
        }
    }

    pub fn artifact(&self, name: &str) -> Option<&Path> {
        match name {
            "model.safetensors" => Some(&self.model),
            "tokenizer.json" => Some(&self.tokenizer),
            "config.json" => Some(&self.config),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledManifest {
    pub schema_version: u32,
    pub protocol_version: u32,
    pub embedding_semantics_version: u32,
    pub model_id: String,
    pub model_revision: String,
    pub artifacts: Vec<InstalledArtifact>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledArtifact {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct CacheStatus {
    pub ready: bool,
    pub problem: Option<String>,
}

pub fn cache_root() -> Result<PathBuf, String> {
    if let Some(value) = non_empty_env("FALLOW_SIMILAR_CODE_CACHE_DIR") {
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err("FALLOW_SIMILAR_CODE_CACHE_DIR must be an absolute path".to_string());
        }
        return Ok(path);
    }

    #[cfg(target_os = "windows")]
    if let Some(value) = non_empty_env("LOCALAPPDATA") {
        return Ok(PathBuf::from(value).join("fallow").join("similar-code"));
    }

    #[cfg(target_os = "macos")]
    if let Some(value) = non_empty_env("HOME") {
        return Ok(PathBuf::from(value)
            .join("Library")
            .join("Caches")
            .join("fallow")
            .join("similar-code"));
    }

    if let Some(value) = non_empty_env("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(value).join("fallow").join("similar-code"));
    }
    if let Some(value) = non_empty_env("HOME") {
        return Ok(PathBuf::from(value)
            .join(".cache")
            .join("fallow")
            .join("similar-code"));
    }
    Err("cannot determine the user cache directory; set FALLOW_SIMILAR_CODE_CACHE_DIR".to_string())
}

pub fn inspect_cache(paths: &ModelPaths, verify_hashes: bool) -> CacheStatus {
    let manifest = match read_manifest(&paths.manifest) {
        Ok(manifest) => manifest,
        Err(problem) => {
            return CacheStatus {
                ready: false,
                problem: Some(problem),
            };
        }
    };
    if manifest != expected_manifest() {
        return CacheStatus {
            ready: false,
            problem: Some("the installed model manifest does not match this sidecar".to_string()),
        };
    }
    for artifact in ARTIFACTS {
        let Some(path) = paths.artifact(artifact.path) else {
            return CacheStatus {
                ready: false,
                problem: Some("the protocol manifest contains an unsupported artifact".to_string()),
            };
        };
        if let Err(problem) = verify_artifact(path, artifact.size, artifact.sha256, verify_hashes) {
            return CacheStatus {
                ready: false,
                problem: Some(problem),
            };
        }
    }
    CacheStatus {
        ready: true,
        problem: None,
    }
}

pub fn expected_manifest() -> InstalledManifest {
    InstalledManifest {
        schema_version: INSTALLED_MANIFEST_SCHEMA_VERSION,
        protocol_version: PROTOCOL_VERSION,
        embedding_semantics_version: EMBEDDING_SEMANTICS_VERSION,
        model_id: MODEL_ID.to_string(),
        model_revision: MODEL_REVISION.to_string(),
        artifacts: ARTIFACTS
            .iter()
            .map(|artifact| InstalledArtifact {
                path: artifact.path.to_string(),
                size: artifact.size,
                sha256: artifact.sha256.to_string(),
            })
            .collect(),
    }
}

pub fn write_manifest(path: &Path) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(&expected_manifest())
        .map_err(|error| format!("failed to serialize model manifest: {error}"))?;
    let partial = path.with_extension("json.partial");
    fs::write(&partial, bytes)
        .map_err(|error| format!("failed to write model manifest: {error}"))?;
    replace_file(&partial, path)
        .map_err(|error| format!("failed to install model manifest: {error}"))
}

pub fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    let backup = destination.with_extension("replace-backup");
    let destination_metadata = match fs::symlink_metadata(destination) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("failed to inspect existing artifact: {error}")),
    };
    let Some(metadata) = destination_metadata else {
        fs::rename(source, destination)
            .map_err(|error| format!("failed to publish verified artifact: {error}"))?;
        remove_partial(&backup);
        return Ok(());
    };
    if !metadata.file_type().is_file() && !metadata.file_type().is_symlink() {
        return Err("existing model artifact is not a replaceable file".to_string());
    }
    remove_partial(&backup);
    fs::rename(destination, &backup)
        .map_err(|error| format!("failed to preserve existing artifact: {error}"))?;
    if let Err(error) = fs::rename(source, destination) {
        return match fs::rename(&backup, destination) {
            Ok(()) => Err(format!("failed to publish verified artifact: {error}")),
            Err(restore_error) => Err(format!(
                "failed to publish verified artifact: {error}; failed to restore the previous artifact: {restore_error}"
            )),
        };
    }
    remove_partial(&backup);
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("failed to open artifact: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 128 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to read artifact: {error}"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(digest_hex(hasher.finalize()))
}

fn read_manifest(path: &Path) -> Result<InstalledManifest, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| {
        "the local model is not installed; run `fallow similar-code setup --local`".to_string()
    })?;
    if !metadata.file_type().is_file() {
        return Err("the model manifest is not a regular file".to_string());
    }
    let bytes =
        fs::read(path).map_err(|error| format!("failed to read model manifest: {error}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to parse model manifest: {error}"))
}

fn verify_artifact(
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
    verify_hash: bool,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| format!("model artifact `{}` is missing", file_label(path)))?;
    if !metadata.file_type().is_file() {
        return Err(format!(
            "model artifact `{}` is not a regular file",
            file_label(path)
        ));
    }
    if metadata.len() != expected_size {
        return Err(format!(
            "model artifact `{}` has the wrong size",
            file_label(path)
        ));
    }
    if verify_hash && sha256_file(path)? != expected_sha256 {
        return Err(format!(
            "model artifact `{}` failed SHA-256 verification",
            file_label(path)
        ));
    }
    Ok(())
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact")
        .to_string()
}

fn non_empty_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn copy_and_hash(reader: &mut dyn Read, writer: &mut File) -> Result<(u64, String), String> {
    use std::io::Write as _;

    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 128 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("download read failed: {error}"))?;
        if count == 0 {
            break;
        }
        writer
            .write_all(&buffer[..count])
            .map_err(|error| format!("artifact write failed: {error}"))?;
        hasher.update(&buffer[..count]);
        total = total.saturating_add(count as u64);
    }
    writer
        .sync_all()
        .map_err(|error| format!("artifact sync failed: {error}"))?;
    Ok((total, digest_hex(hasher.finalize())))
}

pub(crate) fn digest_hex(digest: impl AsRef<[u8]>) -> String {
    let bytes = digest.as_ref();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

pub fn remove_partial(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "test fixture construction must fail immediately"
    )]

    use super::*;

    #[test]
    fn missing_cache_is_not_ready_without_creating_it() {
        let directory = tempfile::tempdir().expect("tempdir");
        let paths = ModelPaths::from_cache_root(directory.path());
        let status = inspect_cache(&paths, true);
        assert!(!status.ready);
        assert!(!paths.directory.exists());
    }

    #[test]
    fn expected_manifest_tracks_protocol_artifacts() {
        let manifest = expected_manifest();
        assert_eq!(manifest.schema_version, INSTALLED_MANIFEST_SCHEMA_VERSION);
        assert_eq!(manifest.protocol_version, PROTOCOL_VERSION);
        assert_eq!(
            manifest.embedding_semantics_version,
            EMBEDDING_SEMANTICS_VERSION
        );
        assert_eq!(manifest.model_revision, MODEL_REVISION);
        assert_eq!(manifest.artifacts.len(), ARTIFACTS.len());
    }

    #[test]
    fn replace_file_overwrites_an_existing_artifact_without_leaving_a_backup() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = directory.path().join("model.download-partial");
        let destination = directory.path().join("model.safetensors");
        fs::write(&source, b"new").expect("new artifact");
        fs::write(&destination, b"old").expect("old artifact");

        replace_file(&source, &destination).expect("replace artifact");

        assert_eq!(fs::read(&destination).expect("installed artifact"), b"new");
        assert!(!source.exists());
        assert!(!destination.with_extension("replace-backup").exists());
    }
}
