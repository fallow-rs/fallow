//! Embeds the user-facing fallow skill (`npm/fallow/skills/fallow/`) into the
//! CLI so `fallow agent install` can materialize a version-matched copy when
//! the project has no `node_modules/fallow`.
//!
//! The skill tree lives outside this crate directory, so a crates.io package
//! cannot carry it. When the tree is absent at build time the generated table
//! is empty and the skill step reports itself as unavailable instead of
//! failing the build.

use std::env;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;

/// Files that make up the shipped skill, relative to the skill root. Kept as
/// an explicit list so an unexpected file (for example `_artifacts/`) never
/// ends up inside the binary.
const SKILL_FILES: &[&str] = &[
    "SKILL.md",
    "agents/openai.yaml",
    "references/cli-reference.md",
    "references/gotchas.md",
    "references/mcp.md",
    "references/node-bindings.md",
    "references/patterns.md",
];

fn env_path(key: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    env::var_os(key)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{key} is not set").into())
}

fn gzip(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(bytes)?;
    encoder.finish()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = env_path("OUT_DIR")?;
    let skill_dir = out_dir.join("embedded-skill");
    fs::create_dir_all(&skill_dir)?;

    let root = env_path("CARGO_MANIFEST_DIR")?.join("../../npm/fallow/skills/fallow");
    println!("cargo:rerun-if-changed={}", root.display());
    for relative in SKILL_FILES {
        println!("cargo:rerun-if-changed={}", root.join(relative).display());
    }

    let mut entries: Vec<String> = Vec::new();
    if root.join("SKILL.md").is_file() {
        for relative in SKILL_FILES {
            let bytes = fs::read(root.join(relative))?;
            let target = skill_dir.join(format!("{}.gz", relative.replace('/', "__")));
            fs::write(&target, gzip(&bytes)?)?;
            entries.push(format!(
                "    EmbeddedSkillFile {{ path: {relative:?}, raw_len: {}, gzip: include_bytes!({:?}) }},",
                readable_len(bytes.len()),
                target.display().to_string()
            ));
        }
    }

    let generated = format!(
        "/// One shipped skill file, gzip-compressed at build time.\n\
         pub struct EmbeddedSkillFile {{\n\
         \x20   /// Path relative to the skill root, forward slashes.\n\
         \x20   pub path: &'static str,\n\
         \x20   /// Uncompressed size in bytes.\n\
         \x20   pub raw_len: usize,\n\
         \x20   /// gzip payload.\n\
         \x20   pub gzip: &'static [u8],\n\
         }}\n\n\
         /// Every file of the shipped skill, or empty when the skill tree was not\n\
         /// available at build time (crates.io source builds).\n\
         pub const EMBEDDED_SKILL: &[EmbeddedSkillFile] = &[\n{}\n];\n",
        entries.join("\n")
    );
    write_if_changed(&out_dir.join("embedded_skill.rs"), &generated)?;
    Ok(())
}

/// Render a length with `_` separators so the generated file passes the
/// workspace's `unreadable_literal` lint.
fn readable_len(len: usize) -> String {
    let digits = len.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push('_');
        }
        out.push(ch);
    }
    out
}

fn write_if_changed(path: &Path, contents: &str) -> io::Result<()> {
    if fs::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(());
    }
    fs::write(path, contents)
}
