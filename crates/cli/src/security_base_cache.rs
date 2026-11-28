//! The on-disk cache of the base-side snapshot of `fallow security`.
//!
//! The cache lives in `.fallow/cache/security-base-v<N>/`. A save checks the
//! path for a symlink that leaves the cache base before any write, so a cloned
//! repository cannot redirect the write outside the project.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use fallow_types::path_guard::CacheTarget;

use super::{
    SECURITY_BASE_SNAPSHOT_CACHE_VERSION, SecurityBaseSnapshotCacheKey, SecurityKeySnapshot,
};

const MAX_SECURITY_BASE_SNAPSHOT_CACHE_SIZE: u64 = 8 * 1024 * 1024;

#[derive(bitcode::Encode, bitcode::Decode)]
struct CachedSecurityKeySnapshot {
    version: u8,
    cli_version: String,
    key_hash: u64,
    base_sha: String,
    reachable: Vec<String>,
}

fn security_base_snapshot_cache_dir(config: &fallow_config::ResolvedConfig) -> PathBuf {
    config.cache_dir.join("cache").join(format!(
        "security-base-v{SECURITY_BASE_SNAPSHOT_CACHE_VERSION}"
    ))
}

fn snapshot_file_name(key: &SecurityBaseSnapshotCacheKey) -> String {
    format!("{:016x}.bin", key.hash)
}

fn security_base_snapshot_cache_file(
    config: &fallow_config::ResolvedConfig,
    key: &SecurityBaseSnapshotCacheKey,
) -> PathBuf {
    security_base_snapshot_cache_dir(config).join(snapshot_file_name(key))
}

/// Refuse a symlink that leaves the cache base on the way to `file`, then
/// create the directory of `target` with its `.gitignore`.
fn ensure_security_base_snapshot_cache_dir(
    target: CacheTarget<'_>,
    file: &Path,
) -> std::io::Result<()> {
    target.check(file)?;
    target.write_gitignore()
}

pub fn load_cached_security_base_snapshot(
    config: &fallow_config::ResolvedConfig,
    key: &SecurityBaseSnapshotCacheKey,
) -> Option<SecurityKeySnapshot> {
    if config.no_cache {
        return None;
    }
    let dir = security_base_snapshot_cache_dir(config);
    let data = config
        .cache_target()
        .with_dir(&dir)
        .read_file(
            &snapshot_file_name(key),
            MAX_SECURITY_BASE_SNAPSHOT_CACHE_SIZE,
        )
        .ok()?;
    let cached: CachedSecurityKeySnapshot = bitcode::decode(&data).ok()?;
    if cached.version != SECURITY_BASE_SNAPSHOT_CACHE_VERSION
        || cached.cli_version != env!("CARGO_PKG_VERSION")
        || cached.key_hash != key.hash
        || cached.base_sha != key.base_sha
    {
        return None;
    }
    Some(SecurityKeySnapshot {
        reachable: cached.reachable.into_iter().collect(),
    })
}

pub fn save_cached_security_base_snapshot(
    config: &fallow_config::ResolvedConfig,
    key: &SecurityBaseSnapshotCacheKey,
    snapshot: &SecurityKeySnapshot,
) {
    if config.no_cache {
        return;
    }
    let dir = security_base_snapshot_cache_dir(config);
    let file = security_base_snapshot_cache_file(config, key);
    let target = config.cache_target().with_dir(&dir);
    if let Err(error) = ensure_security_base_snapshot_cache_dir(target, &file) {
        tracing::debug!("Skipped the security base snapshot cache save: {error}");
        return;
    }
    let mut reachable = snapshot.reachable.iter().cloned().collect::<Vec<_>>();
    reachable.sort_unstable();
    let data = bitcode::encode(&CachedSecurityKeySnapshot {
        version: SECURITY_BASE_SNAPSHOT_CACHE_VERSION,
        cli_version: env!("CARGO_PKG_VERSION").to_owned(),
        key_hash: key.hash,
        base_sha: key.base_sha.clone(),
        reachable,
    });
    let Ok(mut tmp) = tempfile::NamedTempFile::new_in(&dir) else {
        return;
    };
    if tmp.write_all(&data).is_err() {
        return;
    }
    let _ = tmp.persist(file);
}

#[cfg(all(test, unix, not(miri)))]
mod tests {
    use super::*;

    #[test]
    fn a_linked_cache_dir_gets_no_snapshot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("project");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&root).expect("root");
        std::fs::create_dir_all(&outside).expect("outside");
        std::os::unix::fs::symlink(&outside, root.join(".fallow")).expect("link");
        let config = fallow_config::FallowConfig::default().resolve(
            root,
            fallow_config::OutputFormat::Human,
            1,
            false,
            true,
            None,
        );
        let key = SecurityBaseSnapshotCacheKey {
            hash: 1,
            base_sha: "abc".to_owned(),
        };
        let snapshot = SecurityKeySnapshot {
            reachable: std::iter::once("src/a.ts".to_owned()).collect(),
        };
        save_cached_security_base_snapshot(&config, &key, &snapshot);
        assert_eq!(std::fs::read_dir(&outside).expect("outside").count(), 0);
    }

    #[test]
    fn a_snapshot_link_out_of_the_cache_dir_is_not_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolve = |root: PathBuf| {
            fallow_config::FallowConfig::default().resolve(
                root,
                fallow_config::OutputFormat::Human,
                1,
                false,
                true,
                None,
            )
        };
        let root = dir.path().join("project");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&root).expect("root");
        std::fs::create_dir_all(&outside).expect("outside");
        let key = SecurityBaseSnapshotCacheKey {
            hash: 1,
            base_sha: "abc".to_owned(),
        };
        let snapshot = SecurityKeySnapshot {
            reachable: std::iter::once("src/a.ts".to_owned()).collect(),
        };
        let outside_config = resolve(outside);
        save_cached_security_base_snapshot(&outside_config, &key, &snapshot);
        assert!(load_cached_security_base_snapshot(&outside_config, &key).is_some());

        let config = resolve(root);
        std::fs::create_dir_all(security_base_snapshot_cache_dir(&config)).expect("cache dir");
        std::os::unix::fs::symlink(
            security_base_snapshot_cache_file(&outside_config, &key),
            security_base_snapshot_cache_file(&config, &key),
        )
        .expect("link");
        assert!(load_cached_security_base_snapshot(&config, &key).is_none());
    }
}
