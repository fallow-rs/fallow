//! Embeds the released fallow skills (`npm/fallow/skills/<name>/`) into
//! `fallow-api`. `fallow agent install` writes a version-matched copy from
//! this table when the project has no `node_modules/fallow`, and the MCP
//! server serves the same bytes through the skills extension.
//!
//! The skill trees live outside this crate directory, so a crates.io package
//! cannot carry them. When the trees are absent at build time the generated
//! table is empty: the CLI skill step reports itself as unavailable, and the
//! MCP server declares no skills, instead of failing the build.

use std::env;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;

/// Every released skill with the files it ships, relative to the skill root,
/// and the directories whose `.md` files it ships. Kept as an explicit list so
/// an unexpected file (for example `_artifacts/`) never ends up inside the
/// binary. The order matches `RELEASED_SKILLS` in
/// `crates/cli/src/agent_install/skill.rs`.
type SkillFiles = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
);

const SKILLS: &[SkillFiles] = &[
    (
        "fallow",
        &[
            "SKILL.md",
            "agents/openai.yaml",
            "references/cli-reference.md",
            "references/gotchas.md",
            "references/issue-types.md",
            "references/mcp.md",
            "references/node-bindings.md",
            "references/patterns.md",
            "references/similar-code.md",
        ],
        // One CLI reference file per command, so that each file fits in one
        // MCP response.
        &["references/cli"],
    ),
    (
        "fallow-setup",
        &[
            "SKILL.md",
            "agents/openai.yaml",
            "references/ci-gate.md",
            "references/configure-and-install.md",
            "references/tooling-detection.md",
        ],
        &[],
    ),
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

/// Write `embedded_skill.rs` into `out_dir`.
pub fn generate(out_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let skills_root = env_path("CARGO_MANIFEST_DIR")?.join("../../npm/fallow/skills");

    let mut skills: Vec<String> = Vec::new();
    for (name, listed, dirs) in SKILLS {
        let root = skills_root.join(name);
        println!("cargo:rerun-if-changed={}", root.display());
        for dir in *dirs {
            println!("cargo:rerun-if-changed={}", root.join(dir).display());
        }
        if !root.join("SKILL.md").is_file() {
            for relative in *listed {
                println!("cargo:rerun-if-changed={}", root.join(relative).display());
            }
            continue;
        }
        let mut files: Vec<String> = listed.iter().map(|file| (*file).to_string()).collect();
        for dir in *dirs {
            files.extend(markdown_files(&root, dir)?);
        }
        for relative in &files {
            println!("cargo:rerun-if-changed={}", root.join(relative).display());
        }
        let skill_dir = out_dir.join("embedded-skill").join(name);
        fs::create_dir_all(&skill_dir)?;
        let mut entries: Vec<String> = Vec::new();
        for relative in &files {
            let bytes = fs::read(root.join(relative))?;
            let target = skill_dir.join(format!("{}.gz", relative.replace('/', "__")));
            fs::write(&target, gzip(&bytes)?)?;
            entries.push(format!(
                "            EmbeddedSkillFile {{ path: {relative:?}, raw_len: {}, gzip: include_bytes!({:?}) }},",
                readable_len(bytes.len()),
                target.display().to_string()
            ));
        }
        skills.push(format!(
            "    EmbeddedSkill {{\n        name: {name:?},\n        files: &[\n{}\n        ],\n    }},",
            entries.join("\n")
        ));
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
         /// One released skill and every file it ships.\n\
         pub struct EmbeddedSkill {{\n\
         \x20   /// Skill directory name, equal to the frontmatter `name`.\n\
         \x20   pub name: &'static str,\n\
         \x20   /// Every file of the skill.\n\
         \x20   pub files: &'static [EmbeddedSkillFile],\n\
         }}\n\n\
         /// Every released skill, or empty when the skill trees were not\n\
         /// available at build time (crates.io source builds).\n\
         pub const EMBEDDED_SKILLS: &[EmbeddedSkill] = &[\n{}\n];\n",
        skills.join("\n")
    );
    write_if_changed(&out_dir.join("embedded_skill.rs"), &generated)?;
    Ok(())
}

/// The `.md` files directly in `root/dir`, as sorted paths relative to `root`.
fn markdown_files(root: &Path, dir: &str) -> io::Result<Vec<String>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root.join(dir))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let markdown = Path::new(&name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
        if !entry.file_type()?.is_dir() && markdown {
            files.push(format!("{dir}/{name}"));
        }
    }
    files.sort();
    Ok(files)
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
