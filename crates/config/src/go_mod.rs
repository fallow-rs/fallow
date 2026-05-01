//! `go.mod` and `go.work` file parsers.
//!
//! These are line-oriented text formats; no external parser is needed.

use std::path::{Path, PathBuf};

// ── go.mod ────────────────────────────────────────────────────────────────────

/// Parsed contents of a `go.mod` file.
#[derive(Debug, Clone, Default)]
pub struct GoMod {
    /// The module path declared by `module <path>`.
    /// Example: `"github.com/myorg/myproject"`.
    pub module_path: String,
    /// The minimum Go version declared by `go <version>`.
    pub go_version: String,
    /// All `require` entries (module path, version).
    pub require: Vec<GoRequire>,
}

/// A single `require` entry in `go.mod`.
#[derive(Debug, Clone)]
pub struct GoRequire {
    /// The required module path, e.g. `"github.com/some/dep"`.
    pub module_path: String,
    /// The declared version, e.g. `"v1.2.3"`.
    pub version: String,
    /// Whether this is an indirect dependency (`// indirect` comment).
    pub indirect: bool,
}

impl GoMod {
    /// Parse `go.mod` source text into a [`GoMod`].
    #[must_use]
    pub fn parse(source: &str) -> Self {
        let mut result = Self::default();
        let mut in_require_block = false;

        for raw_line in source.lines() {
            let line = raw_line.trim();

            // Strip inline comments for parsing purposes.
            let (content, comment) = if let Some(idx) = line.find("//") {
                (&line[..idx], &line[idx..])
            } else {
                (line, "")
            };
            let content = content.trim();

            // Block closes.
            if content == ")" {
                in_require_block = false;
                continue;
            }

            // Inside a require block.
            if in_require_block {
                if let Some((path, ver)) = parse_module_version(content) {
                    result.require.push(GoRequire {
                        module_path: path.to_string(),
                        version: ver.to_string(),
                        indirect: comment.contains("indirect"),
                    });
                }
                continue;
            }

            // Top-level directives.
            if let Some(rest) = content.strip_prefix("module") {
                if rest.starts_with(|c: char| c.is_whitespace()) {
                    result.module_path = rest.trim().to_string();
                }
            } else if let Some(rest) = content.strip_prefix("go") {
                if rest.starts_with(|c: char| c.is_whitespace()) {
                    result.go_version = rest.trim().to_string();
                }
            } else if content.starts_with("require (") || content == "require (" {
                in_require_block = true;
            } else if let Some(rest) = content.strip_prefix("require ") {
                // Single-line require.
                let rest = rest.trim();
                if let Some((path, ver)) = parse_module_version(rest) {
                    result.require.push(GoRequire {
                        module_path: path.to_string(),
                        version: ver.to_string(),
                        indirect: comment.contains("indirect"),
                    });
                }
            }
        }

        result
    }

    /// Try to read and parse `go.mod` from a directory.
    /// Returns `None` if the file doesn't exist or can't be parsed.
    #[must_use]
    pub fn from_dir(dir: &Path) -> Option<Self> {
        let path = dir.join("go.mod");
        let source = std::fs::read_to_string(&path).ok()?;
        Some(Self::parse(&source))
    }
}

/// Parse `"<module_path> <version>"` into `(module_path, version)`.
fn parse_module_version(s: &str) -> Option<(&str, &str)> {
    let mut parts = s.split_whitespace();
    let path = parts.next()?;
    let ver = parts.next()?;
    // Validate that path looks like a module path (contains at least one `/` or no spaces).
    if path.is_empty() || ver.is_empty() {
        return None;
    }
    Some((path, ver))
}

// ── go.work ───────────────────────────────────────────────────────────────────

/// Parsed contents of a `go.work` file (Go workspace).
#[derive(Debug, Clone, Default)]
pub struct GoWork {
    /// Workspace member module directories (relative to the `go.work` file).
    pub uses: Vec<PathBuf>,
}

impl GoWork {
    /// Parse `go.work` source text.
    #[must_use]
    pub fn parse(source: &str) -> Self {
        let mut result = Self::default();
        let mut in_use_block = false;

        for raw_line in source.lines() {
            let line = raw_line.trim();
            let content = if let Some(idx) = line.find("//") {
                line[..idx].trim()
            } else {
                line
            };

            if content == ")" {
                in_use_block = false;
                continue;
            }

            if in_use_block {
                if !content.is_empty() {
                    result.uses.push(PathBuf::from(content));
                }
                continue;
            }

            if content.starts_with("use (") || content == "use (" {
                in_use_block = true;
            } else if let Some(rest) = content.strip_prefix("use ") {
                let rest = rest.trim();
                if !rest.is_empty() {
                    result.uses.push(PathBuf::from(rest));
                }
            }
        }

        result
    }

    /// Try to read and parse `go.work` from a directory.
    #[must_use]
    pub fn from_dir(dir: &Path) -> Option<Self> {
        let path = dir.join("go.work");
        let source = std::fs::read_to_string(&path).ok()?;
        Some(Self::parse(&source))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_GO_MOD: &str = r#"module github.com/myorg/myproject

go 1.25

require (
    github.com/some/dep v1.2.3
    github.com/other/dep v0.1.0 // indirect
)

require github.com/single/dep v2.0.0
"#;

    #[test]
    fn parses_module_path() {
        let m = GoMod::parse(SAMPLE_GO_MOD);
        assert_eq!(m.module_path, "github.com/myorg/myproject");
    }

    #[test]
    fn parses_go_version() {
        let m = GoMod::parse(SAMPLE_GO_MOD);
        assert_eq!(m.go_version, "1.25");
    }

    #[test]
    fn parses_require_block() {
        let m = GoMod::parse(SAMPLE_GO_MOD);
        assert!(m.require.len() >= 2);
        let dep = &m.require[0];
        assert_eq!(dep.module_path, "github.com/some/dep");
        assert_eq!(dep.version, "v1.2.3");
        assert!(!dep.indirect);
    }

    #[test]
    fn parses_indirect_flag() {
        let m = GoMod::parse(SAMPLE_GO_MOD);
        let indirect = m
            .require
            .iter()
            .find(|r| r.module_path == "github.com/other/dep");
        assert!(indirect.is_some_and(|r| r.indirect));
    }

    #[test]
    fn parses_single_line_require() {
        let m = GoMod::parse(SAMPLE_GO_MOD);
        let single = m
            .require
            .iter()
            .find(|r| r.module_path == "github.com/single/dep");
        assert!(single.is_some_and(|r| r.version == "v2.0.0"));
    }

    #[test]
    fn empty_go_mod_gives_defaults() {
        let m = GoMod::parse("");
        assert!(m.module_path.is_empty());
        assert!(m.require.is_empty());
    }

    const SAMPLE_GO_WORK: &str = r#"go 1.25

use (
    .
    ./submodule
    ./another/module
)
"#;

    #[test]
    fn parses_go_work_uses() {
        let w = GoWork::parse(SAMPLE_GO_WORK);
        assert_eq!(w.uses.len(), 3);
        assert_eq!(w.uses[0], PathBuf::from("."));
        assert_eq!(w.uses[1], PathBuf::from("./submodule"));
    }

    #[test]
    fn single_use_directive() {
        let w = GoWork::parse("go 1.25\nuse ./mymod\n");
        assert_eq!(w.uses.len(), 1);
        assert_eq!(w.uses[0], PathBuf::from("./mymod"));
    }
}
