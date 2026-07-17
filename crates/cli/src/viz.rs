//! `fallow viz`: generate a self-contained interactive HTML map of the
//! codebase (treemap + import graph with dead-code, duplication, boundaries,
//! and complexity lenses), or emit the import graph as DOT / Mermaid text.
//!
//! The command is read-only: it runs one engine-owned project analysis
//! (dead code + duplication + complexity, graph retained) and embeds the
//! typed [`fallow_engine::viz::VizData`] payload into an HTML shell that
//! carries the prebuilt `viz-frontend/` bundle inline. No server, no
//! external assets.

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use fallow_config::OutputFormat;
use fallow_engine::project_analysis::ProjectAnalysisArtifactOptions;
use fallow_engine::session::AnalysisSession;
use fallow_engine::viz::{VizBuildInput, VizData, VizFileStatus, build_viz_data};

use crate::error::emit_error;
use crate::runtime_support::{LoadConfigArgs, load_config};

// ── Embedded viz assets ─────────────────────────────────────────

const VIZ_JS: &str = include_str!("../viz-assets/viz.js");
const VIZ_CSS: &str = include_str!("../viz-assets/viz.css");

// ── CLI types ───────────────────────────────────────────────────

/// Output format for the `viz` command (`--viz-format`).
#[derive(Clone, clap::ValueEnum)]
pub enum VizFormat {
    /// Interactive HTML map (default)
    Html,
    /// Graphviz DOT format
    Dot,
    /// Mermaid diagram format
    Mermaid,
}

/// Options threaded from the CLI dispatch into [`run_viz`].
pub struct VizOptions<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<PathBuf>,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub production: bool,
    pub allow_remote_extends: bool,
    pub output_path: Option<&'a Path>,
    pub no_open: bool,
    pub format: VizFormat,
}

// ── Entry point ─────────────────────────────────────────────────

/// Run the `viz` command: analyze, build the payload, and emit HTML/DOT/Mermaid.
pub fn run_viz(opts: &VizOptions<'_>) -> ExitCode {
    let start = Instant::now();

    let config = match load_config(
        opts.root,
        opts.config_path,
        LoadConfigArgs {
            output: OutputFormat::Human,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production: opts.production,
            quiet: opts.quiet,
            allow_remote_extends: opts.allow_remote_extends,
        },
    ) {
        Ok(c) => c,
        Err(code) => return code,
    };

    let session = AnalysisSession::from_resolved_config(config);
    let duplicates_config = session.config().duplicates.clone();
    let artifacts = match session.analyze_project_with_artifacts(
        &duplicates_config,
        ProjectAnalysisArtifactOptions {
            retain_complexity_artifacts: true,
            retain_graph: true,
            ..ProjectAnalysisArtifactOptions::default()
        },
    ) {
        Ok(artifacts) => artifacts,
        Err(e) => {
            return emit_error(&format!("Analysis error: {e}"), 2, OutputFormat::Human);
        }
    };

    let Some(graph) = artifacts.dead_code.graph.as_ref() else {
        return emit_error("Graph not available", 2, OutputFormat::Human);
    };

    let data = build_viz_data(&VizBuildInput {
        results: &artifacts.dead_code.results,
        graph,
        modules: artifacts.dead_code.modules.as_deref(),
        files: session.files(),
        duplication: &artifacts.duplication,
        workspaces: session.workspaces(),
        config: session.config(),
    });
    let elapsed = start.elapsed();

    match opts.format {
        VizFormat::Html => write_html(opts, &data, elapsed),
        VizFormat::Dot => write_text_format(opts, &generate_dot(&data)),
        VizFormat::Mermaid => write_text_format(opts, &generate_mermaid(&data)),
    }
}

/// Emit a DOT/Mermaid document: to `--out` when given (symlink-safe,
/// parent-creating, like the HTML path), otherwise to stdout.
fn write_text_format(opts: &VizOptions<'_>, content: &str) -> ExitCode {
    let Some(path) = opts.output_path else {
        println!("{content}");
        return ExitCode::SUCCESS;
    };
    if let Err(message) = write_output(path, content) {
        return emit_error(&message, 2, OutputFormat::Human);
    }
    if !opts.quiet {
        eprintln!("  → {}", path.display());
    }
    ExitCode::SUCCESS
}

// ── HTML generation ─────────────────────────────────────────────

/// Escape HTML special characters to prevent injection in text content.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Escape every `<` so no script-content sequence (`</script`, `<!--`,
/// `<script`) can terminate or re-enter the script element; the `<`
/// escape is transparent to the JS value the browser materializes.
fn escape_payload_json(json: &str) -> String {
    json.replace('<', "\\u003c")
}

/// Write the rendered HTML, refusing to follow a planted symlink and
/// creating missing `--out` parent directories.
fn write_output(path: &Path, content: &str) -> Result<(), String> {
    if let Ok(meta) = path.symlink_metadata()
        && meta.file_type().is_symlink()
    {
        return Err(format!(
            "Refusing to write through symlink: {}",
            path.display()
        ));
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Failed to create output directory {}: {e}",
                parent.display()
            )
        })?;
    }
    std::fs::write(path, content).map_err(|e| format!("Failed to write output file: {e}"))
}

fn write_html(opts: &VizOptions<'_>, data: &VizData, elapsed: std::time::Duration) -> ExitCode {
    let json = match serde_json::to_string(data) {
        Ok(j) => j,
        Err(e) => {
            return emit_error(
                &format!("Failed to serialize viz data: {e}"),
                2,
                OutputFormat::Human,
            );
        }
    };

    let json_safe = escape_payload_json(&json);
    let title = html_escape(&data.root);

    let css = VIZ_CSS;
    let js = VIZ_JS;
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="dark light">
<title>fallow map: {title}</title>
<style>{css}</style>
</head>
<body>
<script>window.__FALLOW_DATA__={json_safe};</script>
<script>{js}</script>
</body>
</html>"#,
    );

    let output_path = opts
        .output_path
        .map_or_else(|| opts.root.join("fallow-viz.html"), Path::to_path_buf);

    if let Err(message) = write_output(&output_path, &html) {
        return emit_error(&message, 2, OutputFormat::Human);
    }

    if !opts.quiet {
        let file_count = data.summary.total_files;
        let issue_count = data.summary.unused_files
            + data.summary.unused_exports
            + data.summary.unused_deps
            + data.summary.unresolved_imports
            + data.summary.circular_deps
            + data.summary.clone_groups
            + data.summary.boundary_violations;
        eprintln!(
            "Visualization generated in {:.0}ms ({file_count} files, {issue_count} findings)",
            elapsed.as_secs_f64() * 1000.0,
        );
        eprintln!("  → {}", output_path.display());
    }

    if !opts.no_open
        && let Err(e) = open::that(&output_path)
    {
        eprintln!("Could not open browser: {e}");
        eprintln!("Open manually: {}", output_path.display());
    }

    ExitCode::SUCCESS
}

// ── DOT generation ──────────────────────────────────────────────

const fn status_color(status: VizFileStatus) -> &'static str {
    match status {
        VizFileStatus::Unused => "#e5484d",
        VizFileStatus::HasUnusedExports => "#ffc53d",
        VizFileStatus::EntryPoint => "#30a46c",
        VizFileStatus::Clean => "#62605b",
    }
}

/// Strip control characters so a hostile file name cannot split a node
/// definition across lines in the line-oriented DOT and Mermaid outputs.
fn sanitize_label(path: &str) -> String {
    path.chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .filter(|c| !c.is_control())
        .collect()
}

fn generate_dot(data: &VizData) -> String {
    let mut out =
        String::from("digraph fallow {\n  rankdir=LR;\n  node [shape=box, style=filled];\n\n");

    for (i, f) in data.files.iter().enumerate() {
        let color = status_color(f.status);
        let font = match f.status {
            VizFileStatus::HasUnusedExports => "#111110",
            _ => "#eeeeec",
        };
        let escaped_path = sanitize_label(&f.path)
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let _ = writeln!(
            out,
            "  n{i} [label=\"{escaped_path}\", fillcolor=\"{color}\", fontcolor=\"{font}\"];",
        );
    }

    out.push('\n');

    for [src, tgt, _flags] in &data.edges {
        let _ = writeln!(out, "  n{src} -> n{tgt};");
    }

    out.push_str("}\n");
    out
}

// ── Mermaid generation ──────────────────────────────────────────

fn generate_mermaid(data: &VizData) -> String {
    let mut out = String::from("graph LR\n");

    for (i, f) in data.files.iter().enumerate() {
        let escaped_path = sanitize_label(&f.path)
            .replace('"', "#quot;")
            .replace('[', "#91;")
            .replace(']', "#93;");
        let _ = writeln!(out, "  n{i}[\"{escaped_path}\"]");
    }

    out.push('\n');

    for [src, tgt, _flags] in &data.edges {
        let _ = writeln!(out, "  n{src} --> n{tgt}");
    }

    // Style classes for file status
    out.push('\n');
    let mut unused: Vec<usize> = Vec::new();
    let mut has_unused: Vec<usize> = Vec::new();
    let mut entry: Vec<usize> = Vec::new();

    for (i, f) in data.files.iter().enumerate() {
        match f.status {
            VizFileStatus::Unused => unused.push(i),
            VizFileStatus::HasUnusedExports => has_unused.push(i),
            VizFileStatus::EntryPoint => entry.push(i),
            VizFileStatus::Clean => {}
        }
    }

    if !unused.is_empty() {
        let nodes: Vec<String> = unused.iter().map(|i| format!("n{i}")).collect();
        let _ = writeln!(out, "  style {} fill:#e5484d,color:#fff", nodes.join(","));
    }
    if !has_unused.is_empty() {
        let nodes: Vec<String> = has_unused.iter().map(|i| format!("n{i}")).collect();
        let _ = writeln!(out, "  style {} fill:#ffc53d,color:#111", nodes.join(","));
    }
    if !entry.is_empty() {
        let nodes: Vec<String> = entry.iter().map(|i| format!("n{i}")).collect();
        let _ = writeln!(out, "  style {} fill:#30a46c,color:#fff", nodes.join(","));
    }

    out
}

#[cfg(test)]
mod tests {
    use fallow_engine::viz::{VizFile, VizSummary};

    use super::*;

    fn file(path: &str, status: VizFileStatus) -> VizFile {
        VizFile {
            path: path.to_string(),
            size: 100,
            status,
            export_count: 0,
            unused_export_count: 0,
            is_entry: false,
            importer_count: 0,
            import_count: 0,
            workspace: None,
            zone: None,
            unused_exports: Vec::new(),
            fn_count: 0,
            max_cyclomatic: 0,
            max_cognitive: 0,
            react_hooks: 0,
            jsx_depth: 0,
            functions: Vec::new(),
            dup_lines: 0,
            clone_groups: Vec::new(),
            in_cycle: false,
        }
    }

    fn sample_data() -> VizData {
        VizData {
            root: "proj".to_string(),
            files: vec![
                file("src/index.ts", VizFileStatus::EntryPoint),
                file("src/dead.ts", VizFileStatus::Unused),
                file("src/lib.ts", VizFileStatus::Clean),
            ],
            edges: vec![[0, 2, 0]],
            summary: VizSummary {
                total_files: 3,
                total_size: 300,
                total_edges: 1,
                unused_files: 1,
                unused_exports: 0,
                unused_types: 0,
                unused_deps: 0,
                unresolved_imports: 0,
                circular_deps: 0,
                clone_groups: 0,
                duplicated_lines: 0,
                boundary_violations: 0,
                hotspot_files: 0,
                clone_groups_truncated: None,
            },
            workspaces: Vec::new(),
            zones: Vec::new(),
            cycles: Vec::new(),
            clones: Vec::new(),
            violations: Vec::new(),
        }
    }

    #[test]
    fn html_escape_escapes_injection_characters() {
        assert_eq!(
            html_escape(r#"<script>"a"&'b'</script>"#),
            "&lt;script&gt;&quot;a&quot;&amp;'b'&lt;/script&gt;"
        );
    }

    #[test]
    fn embedded_json_contains_no_raw_angle_bracket() {
        let mut data = sample_data();
        data.files[0].path = "</script><!--<script>alert(1)".to_string();
        let json = serde_json::to_string(&data).expect("serialize viz data");
        let escaped = escape_payload_json(&json);
        assert!(!escaped.contains('<'));
        assert!(!escaped.contains("</"));
    }

    #[test]
    fn escaped_payload_round_trips() {
        let mut data = sample_data();
        data.files[0].path = "</script><!--<script>alert(1)".to_string();
        let json = serde_json::to_string(&data).expect("serialize viz data");
        let escaped = escape_payload_json(&json);
        // The angle-bracket escape is a plain JSON string escape, so the
        // escaped payload stays valid JSON and decodes to the original text.
        let value: serde_json::Value =
            serde_json::from_str(&escaped).expect("escaped payload stays valid JSON");
        assert_eq!(value["files"][0]["path"], "</script><!--<script>alert(1)");
    }

    #[test]
    fn generate_dot_emits_nodes_edges_and_status_colors() {
        let dot = generate_dot(&sample_data());
        assert!(dot.starts_with("digraph fallow {"));
        // Entry node green, unused node red.
        assert!(dot.contains("n0 [label=\"src/index.ts\", fillcolor=\"#30a46c\""));
        assert!(dot.contains("n1 [label=\"src/dead.ts\", fillcolor=\"#e5484d\""));
        assert!(dot.contains("n0 -> n2;"));
        assert!(dot.trim_end().ends_with('}'));
    }

    #[test]
    fn generate_dot_escapes_quotes_and_backslashes_in_paths() {
        let mut data = sample_data();
        data.files[0].path = r#"a\b"c.ts"#.to_string();
        let dot = generate_dot(&data);
        assert!(dot.contains(r#"label="a\\b\"c.ts""#));
    }

    #[test]
    fn generate_mermaid_emits_graph_nodes_edges_and_styles() {
        let mermaid = generate_mermaid(&sample_data());
        assert!(mermaid.starts_with("graph LR\n"));
        assert!(mermaid.contains("n0[\"src/index.ts\"]"));
        assert!(mermaid.contains("n0 --> n2"));
        // Clean files carry no style line; unused + entry do.
        assert!(mermaid.contains("style n1 fill:#e5484d"));
        assert!(mermaid.contains("style n0 fill:#30a46c"));
    }

    #[test]
    fn generate_dot_strips_control_characters_from_paths() {
        let mut data = sample_data();
        data.files[0].path = "evil\nname\".ts".to_string();
        let dot = generate_dot(&data);
        // Newline collapses to a space so the node stays on one line, and
        // the quote still escapes.
        assert!(dot.contains(r#"n0 [label="evil name\".ts""#));
        assert!(!dot.contains("evil\n"));
    }

    #[test]
    fn generate_mermaid_strips_control_characters_from_paths() {
        let mut data = sample_data();
        data.files[0].path = "evil\r\nname\t[].ts".to_string();
        let mermaid = generate_mermaid(&data);
        // CR and LF collapse to spaces, other control characters drop, and
        // both brackets escape; the node stays on one line.
        assert!(mermaid.contains("n0[\"evil  name#91;#93;.ts\"]"));
        assert!(!mermaid.contains("evil\r"));
        assert!(!mermaid.contains("evil\n"));
    }

    #[test]
    fn mermaid_escapes_dynamic_route_brackets() {
        // Next.js/Remix/SvelteKit `[id]` segments must not reach a raw
        // `[` in the Mermaid label, which would break its parser.
        let mut data = sample_data();
        data.files[0].path = "app/routes/[id]/page.tsx".to_string();
        let mermaid = generate_mermaid(&data);
        assert!(mermaid.contains("n0[\"app/routes/#91;id#93;/page.tsx\"]"));
        assert!(!mermaid.contains("[id]"));
    }

    #[cfg(unix)]
    #[test]
    fn write_refuses_symlink_target() {
        let temp = tempfile::tempdir().expect("tempdir");
        let real = temp.path().join("real.txt");
        std::fs::write(&real, "secret").expect("write real target");
        let link = temp.path().join("fallow-viz.html");
        std::os::unix::fs::symlink(&real, &link).expect("plant symlink");

        let result = write_output(&link, "<html></html>");

        let message = result.expect_err("symlink target must be refused");
        assert!(message.contains("Refusing to write through symlink"));
        assert_eq!(
            std::fs::read_to_string(&real).expect("read real target"),
            "secret"
        );
    }

    #[test]
    fn write_creates_parent_dirs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("nested/dir/out.html");

        write_output(&target, "<html></html>").expect("write with parent creation");

        assert_eq!(
            std::fs::read_to_string(&target).expect("read output"),
            "<html></html>"
        );
    }

    #[test]
    fn text_format_honors_out_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("graph.dot");
        let config_path = None;
        let opts = VizOptions {
            root: temp.path(),
            config_path: &config_path,
            no_cache: false,
            threads: 1,
            quiet: true,
            production: false,
            allow_remote_extends: false,
            output_path: Some(target.as_path()),
            no_open: true,
            format: VizFormat::Dot,
        };

        let _ = write_text_format(&opts, "digraph fallow {}\n");

        assert_eq!(
            std::fs::read_to_string(&target).expect("--out should have been written"),
            "digraph fallow {}\n"
        );
    }
}
