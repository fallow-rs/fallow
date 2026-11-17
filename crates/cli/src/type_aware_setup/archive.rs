//! Strict reader of the gzip-compressed ustar bundle of the type-aware
//! companion.
//!
//! The release packaging script (`scripts/type-aware-bundle.mjs`) writes only
//! regular files and directories with relative paths. This reader accepts
//! exactly that. A link, a device, an absolute path, a `..` component or a
//! duplicate path is an error, so a bundle cannot write outside its target.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;

const BLOCK: usize = 512;
/// Upper bound of the unpacked bundle. The released bundles are near 30 MiB.
const MAX_UNPACKED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;

/// One regular file of a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BundleFile {
    pub(super) path: String,
    pub(super) data: Vec<u8>,
    pub(super) executable: bool,
}

/// Decompress and parse a bundle archive.
pub(super) fn read_bundle(archive: &[u8]) -> Result<Vec<BundleFile>, String> {
    let mut tar = Vec::new();
    GzDecoder::new(archive)
        .take(MAX_UNPACKED_BYTES + 1)
        .read_to_end(&mut tar)
        .map_err(|error| format!("the bundle is not a valid gzip archive: {error}"))?;
    if tar.len() as u64 > MAX_UNPACKED_BYTES {
        return Err("the bundle is larger than expected".to_owned());
    }
    parse_tar(&tar)
}

fn field(block: &[u8], start: usize, length: usize) -> &[u8] {
    let slice = &block[start..start + length];
    let end = slice.iter().position(|byte| *byte == 0).unwrap_or(length);
    &slice[..end]
}

fn octal(block: &[u8], start: usize, length: usize) -> Result<u64, String> {
    let text = std::str::from_utf8(field(block, start, length))
        .map_err(|_| "a bundle header has a non-ASCII number".to_owned())?
        .trim();
    if text.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(text, 8)
        .map_err(|_| format!("a bundle header has an invalid number `{text}`"))
}

fn checksum_matches(block: &[u8]) -> Result<bool, String> {
    let expected = octal(block, 148, 8)?;
    let actual: u64 = block
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                u64::from(b' ')
            } else {
                u64::from(*byte)
            }
        })
        .sum();
    Ok(expected == actual)
}

fn entry_path(block: &[u8]) -> Result<String, String> {
    let name = std::str::from_utf8(field(block, 0, 100))
        .map_err(|_| "a bundle path is not UTF-8".to_owned())?;
    let prefix = std::str::from_utf8(field(block, 345, 155))
        .map_err(|_| "a bundle path is not UTF-8".to_owned())?;
    let path = if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    };
    validate_path(&path)?;
    Ok(path.trim_end_matches('/').to_owned())
}

/// A bundle path is relative, uses `/`, and has only normal components.
fn validate_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim_end_matches('/');
    let valid = !trimmed.is_empty()
        && !trimmed.contains('\\')
        && !trimmed.contains(':')
        && trimmed
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(trimmed)
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if valid {
        Ok(())
    } else {
        Err(format!("the bundle has an unsafe path `{path}`"))
    }
}

fn parse_tar(tar: &[u8]) -> Result<Vec<BundleFile>, String> {
    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    let mut offset = 0;
    while offset + BLOCK <= tar.len() {
        let block = &tar[offset..offset + BLOCK];
        if block.iter().all(|byte| *byte == 0) {
            return Ok(files);
        }
        if seen.len() >= MAX_ENTRIES {
            return Err("the bundle has too many entries".to_owned());
        }
        if !checksum_matches(block)? {
            return Err("the bundle has a damaged header".to_owned());
        }
        let path = entry_path(block)?;
        if !seen.insert(path.clone()) {
            return Err(format!("the bundle has the path `{path}` twice"));
        }
        let size = usize::try_from(octal(block, 124, 12)?)
            .map_err(|_| "the bundle has an entry that is too large".to_owned())?;
        let data_start = offset + BLOCK;
        let data_end = data_start
            .checked_add(size)
            .filter(|end| *end <= tar.len())
            .ok_or_else(|| format!("the bundle ends inside `{path}`"))?;
        match block[156] {
            b'0' | 0 => {
                let mode = octal(block, 100, 8)?;
                files.push(BundleFile {
                    path,
                    data: tar[data_start..data_end].to_vec(),
                    executable: mode & 0o111 != 0,
                });
            }
            b'5' if size == 0 => {}
            kind => {
                return Err(format!(
                    "the bundle entry `{path}` has the unsupported type `{}`",
                    char::from(kind)
                ));
            }
        }
        offset = data_start + size.div_ceil(BLOCK) * BLOCK;
    }
    Err("the bundle has no end marker".to_owned())
}

/// Write the files of a bundle below `target`, which must be an empty or
/// absent directory.
pub(super) fn unpack(files: &[BundleFile], target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target)
        .map_err(|error| format!("cannot create {}: {error}", target.display()))?;
    for file in files {
        let path: PathBuf = target.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        std::fs::write(&path, &file.data)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        set_mode(&path, file.executable)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, executable: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = if executable { 0o755 } else { 0o644 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|error| format!("cannot set the mode of {}: {error}", path.display()))
}

#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "matches the Unix signature; Windows has no mode bits"
)]
fn set_mode(_path: &Path, _executable: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use std::io::Write as _;

    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::*;

    /// One raw entry of a test archive: path, data, mode, type flag.
    pub type RawEntry<'a> = (&'a str, &'a [u8], u32, u8);

    fn header(path: &str, size: usize, mode: u32, kind: u8) -> [u8; BLOCK] {
        let mut block = [0_u8; BLOCK];
        block[..path.len()].copy_from_slice(path.as_bytes());
        block[100..107].copy_from_slice(format!("{mode:07o}").as_bytes());
        block[124..135].copy_from_slice(format!("{size:011o}").as_bytes());
        block[136..147].copy_from_slice(b"00000000000");
        block[156] = kind;
        block[257..263].copy_from_slice(b"ustar\0");
        block[148..156].fill(b' ');
        let sum: u32 = block.iter().map(|byte| u32::from(*byte)).sum();
        block[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        block
    }

    /// Build a gzip-compressed ustar archive from raw entries.
    pub fn archive(entries: &[RawEntry<'_>]) -> Vec<u8> {
        let mut tar = Vec::new();
        for (path, data, mode, kind) in entries {
            tar.extend_from_slice(&header(path, data.len(), *mode, *kind));
            tar.extend_from_slice(data);
            tar.resize(tar.len().div_ceil(BLOCK) * BLOCK, 0);
        }
        tar.extend_from_slice(&[0; BLOCK * 2]);
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&tar).expect("compress test archive");
        encoder.finish().expect("finish test archive")
    }

    #[test]
    fn reads_regular_files_and_directories() {
        let bytes = archive(&[
            ("lib/", b"", 0o755, b'5'),
            ("fallow-type-aware", b"#!/bin/sh\n", 0o755, b'0'),
            ("lib/a.mjs", b"export {};\n", 0o644, b'0'),
        ]);
        let files = read_bundle(&bytes).expect("valid archive");
        assert_eq!(
            files,
            vec![
                BundleFile {
                    path: "fallow-type-aware".to_owned(),
                    data: b"#!/bin/sh\n".to_vec(),
                    executable: true,
                },
                BundleFile {
                    path: "lib/a.mjs".to_owned(),
                    data: b"export {};\n".to_vec(),
                    executable: false,
                },
            ]
        );
    }

    #[test]
    fn rejects_links_unsafe_paths_and_duplicates() {
        for (entries, expected) in [
            (vec![("link", &b""[..], 0o777, b'2')], "unsupported type"),
            (vec![("hard", &b""[..], 0o644, b'1')], "unsupported type"),
            (vec![("../escape", &b"x"[..], 0o644, b'0')], "unsafe path"),
            (vec![("/etc/passwd", &b"x"[..], 0o644, b'0')], "unsafe path"),
            (vec![("a/./b", &b"x"[..], 0o644, b'0')], "unsafe path"),
            (vec![("c:evil", &b"x"[..], 0o644, b'0')], "unsafe path"),
            (
                vec![
                    ("same", &b"x"[..], 0o644, b'0'),
                    ("same", &b"y"[..], 0o644, b'0'),
                ],
                "twice",
            ),
        ] {
            let error = read_bundle(&archive(&entries)).expect_err("unsafe archive");
            assert!(error.contains(expected), "{expected}: {error}");
        }
    }

    #[test]
    fn rejects_a_damaged_or_truncated_archive() {
        let bytes = archive(&[("file", b"data", 0o644, b'0')]);
        assert!(read_bundle(&bytes[..bytes.len() / 2]).is_err());
        assert!(read_bundle(b"not gzip").is_err());

        let mut tar = Vec::new();
        GzDecoder::new(bytes.as_slice())
            .read_to_end(&mut tar)
            .expect("decompress");
        tar[0] = b'g';
        let error = parse_tar(&tar).expect_err("checksum mismatch");
        assert!(error.contains("damaged header"), "{error}");
    }

    #[test]
    fn unpacks_files_with_their_modes() {
        let target = tempfile::tempdir().expect("target");
        let files = read_bundle(&archive(&[
            ("fallow-type-aware", b"#!/bin/sh\n", 0o755, b'0'),
            ("lib/deep/a.mjs", b"x", 0o644, b'0'),
        ]))
        .expect("valid archive");
        unpack(&files, target.path()).expect("unpack");
        assert_eq!(
            std::fs::read(target.path().join("lib/deep/a.mjs")).expect("read file"),
            b"x"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = |path: &str| {
                std::fs::metadata(target.path().join(path))
                    .expect("metadata")
                    .permissions()
                    .mode()
                    & 0o777
            };
            assert_eq!(mode("fallow-type-aware"), 0o755);
            assert_eq!(mode("lib/deep/a.mjs"), 0o644);
        }
    }
}
