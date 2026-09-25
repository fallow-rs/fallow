use std::path::{Path, PathBuf};

use fallow_types::discover::{EntryPoint, EntryPointSource};

use super::entry_points::resolve_entry_path;
use super::parse_scripts::{extract_script_file_refs, looks_like_script_file};

/// Discover entry points from infrastructure config files (Dockerfile, Procfile, fly.toml).
///
/// These files reference source files as entry points for processes that run outside
/// the main JS/TS build pipeline (workers, migrations, cron jobs, etc.).
pub fn discover_infrastructure_entry_points(root: &Path) -> Vec<EntryPoint> {
    let _span = tracing::info_span!("discover_infrastructure_entry_points").entered();
    let file_refs = collect_infrastructure_file_refs(root);

    if file_refs.is_empty() {
        return Vec::new();
    }

    let entries = resolve_infrastructure_file_refs(root, &file_refs);
    log_infrastructure_entries(&entries);
    entries
}

fn collect_infrastructure_file_refs(root: &Path) -> Vec<String> {
    let mut file_refs = Vec::new();
    collect_dockerfile_refs(root, &mut file_refs);
    collect_procfile_refs(root, &mut file_refs);
    collect_fly_toml_refs(root, &mut file_refs);
    file_refs
}

fn collect_dockerfile_refs(root: &Path, file_refs: &mut Vec<String>) {
    for dir in infrastructure_search_dirs(root) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if is_dockerfile(&name_str)
                && let Ok(content) = std::fs::read_to_string(entry.path())
            {
                file_refs.extend(extract_dockerfile_file_refs(&content));
            }
        }
    }
}

fn infrastructure_search_dirs(root: &Path) -> Vec<PathBuf> {
    std::iter::once(root.to_path_buf())
        .chain(
            ["config", "docker", "deploy", ".docker"]
                .iter()
                .map(|d| root.join(d)),
        )
        .filter(|d| d.is_dir())
        .collect()
}

fn collect_procfile_refs(root: &Path, file_refs: &mut Vec<String>) {
    if let Ok(content) = std::fs::read_to_string(root.join("Procfile")) {
        file_refs.extend(extract_procfile_file_refs(&content));
    }
}

fn collect_fly_toml_refs(root: &Path, file_refs: &mut Vec<String>) {
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if (name_str == "fly.toml" || (name_str.starts_with("fly.") && name_str.ends_with(".toml")))
            && let Ok(content) = std::fs::read_to_string(entry.path())
        {
            file_refs.extend(extract_fly_toml_file_refs(&content));
        }
    }
}

fn resolve_infrastructure_file_refs(root: &Path, file_refs: &[String]) -> Vec<EntryPoint> {
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut entries: Vec<EntryPoint> = file_refs
        .iter()
        .filter_map(|file_ref| {
            resolve_entry_path(
                root,
                file_ref,
                &canonical_root,
                EntryPointSource::InfrastructureConfig,
            )
        })
        .collect();

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries.dedup_by(|a, b| a.path == b.path);
    entries
}

fn log_infrastructure_entries(entries: &[EntryPoint]) {
    if !entries.is_empty() {
        tracing::info!(
            count = entries.len(),
            "infrastructure entry points discovered"
        );
    }
}

/// Check if a filename is a Dockerfile.
fn is_dockerfile(name: &str) -> bool {
    name == "Dockerfile"
        || (name.starts_with("Dockerfile.") && !name.ends_with(".dockerignore"))
        || name.ends_with(".Dockerfile")
}

/// Extract file path references from Dockerfile RUN/CMD/ENTRYPOINT instructions.
///
/// Handles both shell form (`CMD node file.js`) and exec form (`CMD ["node", "file.js"]`).
/// Multi-line commands with `\` continuation are joined.
fn extract_dockerfile_file_refs(content: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        if line.is_empty() || line.starts_with('#') {
            i += 1;
            continue;
        }

        let Some(instruction_end) = strip_dockerfile_instruction(line) else {
            i += 1;
            continue;
        };

        let mut full_cmd = instruction_end.to_string();
        while full_cmd.ends_with('\\') {
            full_cmd.truncate(full_cmd.len() - 1);
            i += 1;
            if i >= lines.len() {
                break;
            }
            full_cmd.push(' ');
            full_cmd.push_str(lines[i].trim());
        }

        let cmd_str = full_cmd.trim();
        let command = if cmd_str.starts_with('[') {
            parse_exec_form(cmd_str)
        } else {
            cmd_str.to_string()
        };

        refs.extend(extract_script_file_refs(&command));
        refs.extend(extract_flag_value_file_refs(&command));
        i += 1;
    }

    refs
}

/// Extract file path references from flag values like `--alias:name=./path.ts`.
///
/// Build tools (esbuild, webpack, etc.) use flag values that reference source files.
/// This extracts paths from `--key=value` patterns where the value looks like a source file.
fn extract_flag_value_file_refs(command: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for token in command.split_whitespace() {
        if !token.starts_with('-') {
            continue;
        }
        if let Some((_key, value)) = token.split_once('=')
            && looks_like_script_file(value)
        {
            refs.push(value.to_string());
        }
    }
    refs
}

/// Strip a Dockerfile instruction keyword (RUN, CMD, ENTRYPOINT) and return the rest.
fn strip_dockerfile_instruction(line: &str) -> Option<&str> {
    for keyword in &["RUN ", "CMD ", "ENTRYPOINT "] {
        // `get` returns `None` when the keyword length falls inside a
        // multi-byte character, so non-ASCII lines cannot panic here.
        if let Some(prefix) = line.get(..keyword.len())
            && prefix.eq_ignore_ascii_case(keyword)
        {
            return Some(&line[keyword.len()..]);
        }
    }
    None
}

/// Parse Docker/TOML exec form `["cmd", "arg1", "arg2"]` into a shell-like command string.
///
/// Handles commas inside quoted strings correctly.
fn parse_exec_form(s: &str) -> String {
    let inner = s.trim().trim_start_matches('[').trim_end_matches(']');
    let mut parts = Vec::new();
    let mut in_quotes = false;
    let mut current = String::new();
    for ch in inner.chars() {
        match ch {
            '"' | '\'' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                let t = current.trim().to_string();
                if !t.is_empty() {
                    parts.push(t);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let t = current.trim().to_string();
    if !t.is_empty() {
        parts.push(t);
    }
    parts.join(" ")
}

/// Extract file path references from a Procfile.
///
/// Format: `process_type: command`
fn extract_procfile_file_refs(content: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((_process_type, command)) = line.split_once(':') {
            refs.extend(extract_script_file_refs(command.trim()));
        }
    }
    refs
}

/// Extract file path references from fly.toml.
///
/// Parses `release_command`, `cmd` at any level, and all keys under `[processes]`.
fn extract_fly_toml_file_refs(content: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut in_processes_section = false;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') {
            in_processes_section =
                line.trim_start_matches('[').trim_end_matches(']').trim() == "processes";
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');

            if matches!(key, "release_command" | "cmd") || in_processes_section {
                let command = if value.starts_with('[') {
                    parse_exec_form(value)
                } else {
                    value.to_string()
                };
                refs.extend(extract_script_file_refs(&command));
            }
        }
    }

    refs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dockerfile_detection() {
        assert!(is_dockerfile("Dockerfile"));
        assert!(is_dockerfile("Dockerfile.worker"));
        assert!(is_dockerfile("Dockerfile.dev"));
        assert!(is_dockerfile("app.Dockerfile"));
        assert!(!is_dockerfile("Dockerfile.dockerignore"));
        assert!(!is_dockerfile("README.md"));
        assert!(!is_dockerfile("docker-compose.yml"));
    }

    #[test]
    fn dockerfile_run_node() {
        let refs = extract_dockerfile_file_refs("RUN node scripts/db-migrate.mjs");
        assert_eq!(refs, vec!["scripts/db-migrate.mjs"]);
    }

    #[test]
    fn dockerfile_cmd_shell_form() {
        let refs = extract_dockerfile_file_refs("CMD node dist/server.js");
        assert_eq!(refs, vec!["dist/server.js"]);
    }

    #[test]
    fn dockerfile_cmd_exec_form() {
        let refs = extract_dockerfile_file_refs(r#"CMD ["node", "scripts/server.js"]"#);
        assert_eq!(refs, vec!["scripts/server.js"]);
    }

    #[test]
    fn dockerfile_entrypoint_exec_form() {
        let refs = extract_dockerfile_file_refs(r#"ENTRYPOINT ["node", "src/index.ts"]"#);
        assert_eq!(refs, vec!["src/index.ts"]);
    }

    #[test]
    fn dockerfile_run_esbuild() {
        let refs = extract_dockerfile_file_refs(
            "RUN npx esbuild src/server/jobs/worker.ts --outfile=dist-worker/worker.mjs --bundle",
        );
        assert_eq!(
            refs,
            vec!["src/server/jobs/worker.ts", "dist-worker/worker.mjs"]
        );
    }

    #[test]
    fn dockerfile_multiline_run() {
        let refs =
            extract_dockerfile_file_refs("RUN node \\\n  scripts/db-migrate.mjs \\\n  --verbose");
        assert_eq!(refs, vec!["scripts/db-migrate.mjs"]);
    }

    #[test]
    fn dockerfile_skips_comments_and_other_instructions() {
        let content =
            "FROM node:20\n# This is a comment\nCOPY . .\nRUN node scripts/seed.ts\nEXPOSE 3000";
        let refs = extract_dockerfile_file_refs(content);
        assert_eq!(refs, vec!["scripts/seed.ts"]);
    }

    #[test]
    fn dockerfile_case_insensitive() {
        let refs = extract_dockerfile_file_refs("run node scripts/migrate.ts");
        assert_eq!(refs, vec!["scripts/migrate.ts"]);
    }

    #[test]
    fn dockerfile_non_ascii_after_short_prefix_does_not_panic() {
        // Regression for #2896: a multi-byte char that straddles the keyword
        // length must not split a UTF-8 boundary.
        let content = "      \\'あいうえお\\',\nRUN node scripts/seed.ts\nCMDé\nRUNあ";
        let refs = extract_dockerfile_file_refs(content);
        assert_eq!(refs, vec!["scripts/seed.ts"]);
    }

    #[test]
    fn dockerfile_run_tsx_runner() {
        let refs = extract_dockerfile_file_refs("RUN tsx src/worker.ts");
        assert_eq!(refs, vec!["src/worker.ts"]);
    }

    #[test]
    fn dockerfile_no_file_refs() {
        let content = "FROM node:20\nRUN npm install\nRUN npm run build\nCMD [\"npm\", \"start\"]";
        let refs = extract_dockerfile_file_refs(content);
        assert!(refs.is_empty());
    }

    #[test]
    fn procfile_basic() {
        let refs = extract_procfile_file_refs("web: node server.js\nworker: node worker.js");
        assert_eq!(refs, vec!["server.js", "worker.js"]);
    }

    #[test]
    fn procfile_with_comments() {
        let refs = extract_procfile_file_refs("# comment\nweb: node src/index.ts");
        assert_eq!(refs, vec!["src/index.ts"]);
    }

    #[test]
    fn procfile_empty() {
        let refs = extract_procfile_file_refs("");
        assert!(refs.is_empty());
    }

    #[test]
    fn fly_toml_release_command() {
        let refs = extract_fly_toml_file_refs(r#"release_command = "node scripts/db-migrate.mjs""#);
        assert_eq!(refs, vec!["scripts/db-migrate.mjs"]);
    }

    #[test]
    fn fly_toml_process_commands() {
        let content = "[processes]\nweb = \"node dist/server.js\"\nworker = \"node src/worker.ts\"";
        let refs = extract_fly_toml_file_refs(content);
        assert_eq!(refs, vec!["dist/server.js", "src/worker.ts"]);
    }

    #[test]
    fn fly_toml_cmd() {
        let refs = extract_fly_toml_file_refs(r#"cmd = "node src/index.js""#);
        assert_eq!(refs, vec!["src/index.js"]);
    }

    #[test]
    fn fly_toml_ignores_non_process_keys() {
        let refs = extract_fly_toml_file_refs(r#"app = "my-app""#);
        assert!(refs.is_empty());
    }

    #[test]
    fn fly_toml_comments_and_sections() {
        let content = "# deploy config\n[deploy]\nrelease_command = \"node scripts/migrate.mjs\"";
        let refs = extract_fly_toml_file_refs(content);
        assert_eq!(refs, vec!["scripts/migrate.mjs"]);
    }

    #[test]
    fn exec_form_basic() {
        assert_eq!(
            parse_exec_form(r#"["node", "server.js"]"#),
            "node server.js"
        );
    }

    #[test]
    fn exec_form_with_flags() {
        assert_eq!(
            parse_exec_form(r#"["node", "--max-old-space-size=4096", "server.js"]"#),
            "node --max-old-space-size=4096 server.js"
        );
    }

    #[test]
    fn exec_form_with_commas_in_args() {
        assert_eq!(
            parse_exec_form(r#"["node", "--require=a,b", "server.js"]"#),
            "node --require=a,b server.js"
        );
    }

    #[test]
    fn fly_toml_arbitrary_process_name() {
        let content = "[processes]\nmigrations = \"node scripts/migrate.mjs\"";
        let refs = extract_fly_toml_file_refs(content);
        assert_eq!(refs, vec!["scripts/migrate.mjs"]);
    }

    #[test]
    fn fly_toml_exec_form_array() {
        let content = r#"cmd = ["node", "src/index.js"]"#;
        let refs = extract_fly_toml_file_refs(content);
        assert_eq!(refs, vec!["src/index.js"]);
    }

    #[test]
    fn fly_toml_section_switching() {
        let content =
            "[processes]\nworker = \"node src/worker.ts\"\n[env]\nNODE_ENV = \"production\"";
        let refs = extract_fly_toml_file_refs(content);
        assert_eq!(refs, vec!["src/worker.ts"]);
    }

    #[test]
    fn strip_instruction_run() {
        assert_eq!(
            strip_dockerfile_instruction("RUN node server.js"),
            Some("node server.js")
        );
    }

    #[test]
    fn strip_instruction_cmd() {
        assert_eq!(
            strip_dockerfile_instruction("CMD node server.js"),
            Some("node server.js")
        );
    }

    #[test]
    fn strip_instruction_entrypoint() {
        assert_eq!(
            strip_dockerfile_instruction("ENTRYPOINT node server.js"),
            Some("node server.js")
        );
    }

    #[test]
    fn strip_instruction_case_insensitive() {
        assert_eq!(
            strip_dockerfile_instruction("run node server.js"),
            Some("node server.js")
        );
        assert_eq!(
            strip_dockerfile_instruction("cmd node server.js"),
            Some("node server.js")
        );
    }

    #[test]
    fn strip_instruction_non_matching() {
        assert_eq!(strip_dockerfile_instruction("FROM node:20"), None);
        assert_eq!(strip_dockerfile_instruction("COPY . ."), None);
        assert_eq!(strip_dockerfile_instruction("EXPOSE 3000"), None);
        assert_eq!(strip_dockerfile_instruction("ENV FOO=bar"), None);
    }

    #[test]
    fn flag_value_file_refs_esbuild_outfile() {
        let refs = extract_flag_value_file_refs("npx esbuild src/entry.ts --outfile=dist/out.js");
        assert_eq!(refs, vec!["dist/out.js"]);
    }

    #[test]
    fn flag_value_file_refs_alias() {
        let refs = extract_flag_value_file_refs("node --alias:helper=./src/helper.ts app.js");
        assert_eq!(refs, vec!["./src/helper.ts"]);
    }

    #[test]
    fn flag_value_file_refs_no_flags() {
        let refs = extract_flag_value_file_refs("node src/server.js");
        assert!(refs.is_empty(), "non-flag tokens should not match");
    }

    #[test]
    fn flag_value_file_refs_flag_without_file() {
        let refs = extract_flag_value_file_refs("node --max-old-space-size=4096 server.js");
        assert!(
            refs.is_empty(),
            "flag values that are not file paths should not match"
        );
    }

    #[test]
    fn exec_form_single_element() {
        assert_eq!(parse_exec_form(r#"["node"]"#), "node");
    }

    #[test]
    fn exec_form_empty() {
        assert_eq!(parse_exec_form("[]"), "");
    }

    #[test]
    fn exec_form_single_quotes() {
        assert_eq!(parse_exec_form("['node', 'server.js']"), "node server.js");
    }

    mod integration {
        use super::*;

        #[test]
        fn discovers_dockerfile_cmd_entry_point() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("server.ts"), "export const s = 1;").unwrap();

            let dockerfile = "FROM node:20\nCOPY . .\nCMD node src/server.ts";
            std::fs::write(dir.path().join("Dockerfile"), dockerfile).unwrap();

            let entries = discover_infrastructure_entry_points(dir.path());
            assert_eq!(entries.len(), 1);
            assert!(entries[0].path.ends_with("src/server.ts"));
            assert!(matches!(
                entries[0].source,
                EntryPointSource::InfrastructureConfig
            ));
        }

        #[test]
        fn discovers_procfile_entry_points() {
            let dir = tempfile::tempdir().expect("create temp dir");
            std::fs::write(dir.path().join("server.js"), "// server").unwrap();
            std::fs::write(dir.path().join("worker.js"), "// worker").unwrap();

            let procfile = "web: node server.js\nworker: node worker.js";
            std::fs::write(dir.path().join("Procfile"), procfile).unwrap();

            let entries = discover_infrastructure_entry_points(dir.path());
            assert_eq!(entries.len(), 2);

            let paths: Vec<String> = entries
                .iter()
                .map(|e| e.path.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            assert!(paths.contains(&"server.js".to_string()));
            assert!(paths.contains(&"worker.js".to_string()));
        }

        #[test]
        fn no_infrastructure_files_returns_empty() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let entries = discover_infrastructure_entry_points(dir.path());
            assert!(entries.is_empty());
        }

        #[test]
        fn discovers_variant_dockerfile_names() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let scripts = dir.path().join("scripts");
            std::fs::create_dir_all(&scripts).unwrap();
            std::fs::write(scripts.join("migrate.ts"), "// migrate").unwrap();

            let dockerfile = "FROM node:20\nRUN node scripts/migrate.ts";
            std::fs::write(dir.path().join("Dockerfile.worker"), dockerfile).unwrap();

            let entries = discover_infrastructure_entry_points(dir.path());
            assert_eq!(entries.len(), 1);
            assert!(entries[0].path.ends_with("scripts/migrate.ts"));
        }

        #[test]
        fn deduplicates_entry_points() {
            let dir = tempfile::tempdir().expect("create temp dir");
            std::fs::write(dir.path().join("server.js"), "// server").unwrap();

            std::fs::write(
                dir.path().join("Dockerfile"),
                "FROM node:20\nCMD node server.js",
            )
            .unwrap();
            std::fs::write(dir.path().join("Procfile"), "web: node server.js").unwrap();

            let entries = discover_infrastructure_entry_points(dir.path());
            assert_eq!(
                entries.len(),
                1,
                "duplicate entry points should be deduplicated"
            );
        }
    }
}
