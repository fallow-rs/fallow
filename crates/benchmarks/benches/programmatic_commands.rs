#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use divan::Bencher;
use fallow_cli::programmatic::{
    AnalysisOptions, ComplexityOptions, DeadCodeOptions, DuplicationMode, DuplicationOptions,
    compute_health, detect_circular_dependencies, detect_dead_code, detect_duplication,
};
use tempfile::TempDir;

const BENCH_THREADS: usize = 4;

fn main() {
    divan::main();
}

struct CommandInput {
    _temp_dir: TempDir,
    root: PathBuf,
}

fn write_file(root: &Path, path: &str, source: impl AsRef<str>) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("fixture file has parent")).unwrap();
    std::fs::write(path, source.as_ref()).unwrap();
}

fn package_json(name: &str, extra: &str) -> String {
    format!(
        r#"{{
  "name": "{name}",
  "private": true,
  "type": "module",
  "dependencies": {{
    "react": "19.0.0",
    "next": "15.0.0",
    "tailwindcss": "4.0.0"{extra}
  }},
  "devDependencies": {{
    "typescript": "5.8.0"
  }}
}}"#
    )
}

fn analysis_options(root: &Path, no_cache: bool) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(root.to_path_buf()),
        no_cache,
        threads: Some(BENCH_THREADS),
        ..AnalysisOptions::default()
    }
}

fn create_library_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(&root, "package.json", package_json("bench-library", ""));
    write_file(
        &root,
        "src/index.ts",
        r#"
export { usedFeature } from "./feature";
export type { PublicOptions } from "./types";
"#,
    );
    write_file(
        &root,
        "src/feature.ts",
        r#"
import { formatLabel } from "./format";

export type PublicOptions = { label: string };

export const usedFeature = (value: string): string => formatLabel(value);
export const unusedFeature = (value: string): string => value.toUpperCase();
export const unusedConstant = 42;
"#,
    );
    write_file(
        &root,
        "src/format.ts",
        r"
export const formatLabel = (value: string): string => `item:${value}`;
export const debugLabel = (value: string): string => `debug:${value}`;
",
    );
    write_file(
        &root,
        "src/types.ts",
        r"
export type PublicOptions = { label: string };
export type InternalOptions = { retries: number };
",
    );
    write_file(
        &root,
        "src/unused-file.ts",
        r"
export const onlyInUnusedFile = true;
",
    );

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_next_app_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(&root, "package.json", package_json("bench-next-app", ""));
    write_file(
        &root,
        "app/layout.tsx",
        r#"
import "./globals.css";

export default function Layout({ children }: { children: React.ReactNode }) {
  return <html><body>{children}</body></html>;
}
"#,
    );
    write_file(
        &root,
        "app/page.tsx",
        r#"
import { Button } from "../components/button";
import { getPosts } from "../lib/posts";

export default async function Page() {
  const posts = await getPosts();
  return <main>{posts.map((post) => <Button key={post.id} label={post.title} />)}</main>;
}
"#,
    );
    write_file(
        &root,
        "app/blog/[slug]/page.tsx",
        r#"
import { getPost } from "../../../lib/posts";

export default async function BlogPost({ params }: { params: { slug: string } }) {
  const post = await getPost(params.slug);
  return <article>{post.title}</article>;
}
"#,
    );
    write_file(
        &root,
        "components/button.tsx",
        r#"
"use client";

export const Button = ({ label }: { label: string }) => {
  return <button className="button primary">{label}</button>;
};

export const DebugButton = () => <button>debug</button>;
"#,
    );
    write_file(
        &root,
        "lib/posts.ts",
        r#"
export const getPosts = async () => [{ id: "1", title: "Intro" }];
export const getPost = async (slug: string) => ({ slug, title: "Intro" });
export const unusedPostHelper = () => "unused";
"#,
    );
    write_file(
        &root,
        "app/globals.css",
        r"
.button { display: inline-flex; }
.primary { color: white; }
.unused-global { color: red; }
",
    );

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_workspace_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{
  "name": "bench-workspace",
  "private": true,
  "workspaces": ["packages/*"],
  "dependencies": {"react": "19.0.0"}
}"#,
    );
    write_file(
        &root,
        "packages/app/package.json",
        r#"{"name":"@bench/app","main":"src/index.ts","dependencies":{"@bench/shared":"workspace:*","@bench/ui":"workspace:*"}}"#,
    );
    write_file(
        &root,
        "packages/shared/package.json",
        r#"{"name":"@bench/shared","main":"src/index.ts"}"#,
    );
    write_file(
        &root,
        "packages/ui/package.json",
        r#"{"name":"@bench/ui","main":"src/index.ts","dependencies":{"react":"19.0.0"}}"#,
    );
    write_file(
        &root,
        "packages/app/src/index.ts",
        r#"
import { formatUser } from "@bench/shared";
import { Card } from "@bench/ui";

export const render = (name: string) => Card({ title: formatUser(name) });
"#,
    );
    write_file(
        &root,
        "packages/shared/src/index.ts",
        r"
export const formatUser = (name: string): string => name.trim();
export const unusedSharedHelper = (name: string): string => name.toUpperCase();
",
    );
    write_file(
        &root,
        "packages/ui/src/index.ts",
        r#"
export const Card = ({ title }: { title: string }) => `<section>${title}</section>`;
export const UnusedCard = () => "<section>unused</section>";
"#,
    );

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_duplication_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        package_json("bench-dupes-routes", ""),
    );
    let route_body = r#"
const validateRequest = (request: Request): string => {
  const auth = request.headers.get("authorization");
  if (!auth) {
    throw new Error("missing authorization");
  }
  const tenant = request.headers.get("x-tenant") ?? "default";
  const trace = request.headers.get("x-trace") ?? "local";
  return `${tenant}:${trace}:${auth}`;
};

const buildResponse = (value: string) => {
  return Response.json({
    ok: true,
    value,
    createdAt: new Date().toISOString(),
    source: "api",
  });
};
"#;

    for i in 0..12 {
        write_file(
            &root,
            &format!("app/api/resource{i}/route.ts"),
            format!(
                r"{route_body}
export async function GET(request: Request) {{
  const value = validateRequest(request);
  return buildResponse(`${{value}}:{i}`);
}}
"
            ),
        );
    }

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_circular_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(&root, "package.json", package_json("bench-circulars", ""));
    for i in 0..24 {
        let next = (i + 1) % 24;
        write_file(
            &root,
            &format!("src/cycle{i}.ts"),
            format!(
                r#"
import {{ value{next} }} from "./cycle{next}";

export const value{i} = value{next} + {i};
"#
            ),
        );
    }
    write_file(
        &root,
        "src/index.ts",
        r#"
import { value0 } from "./cycle0";

console.log(value0);
"#,
    );

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_health_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(&root, "package.json", package_json("bench-health", ""));
    let mut source = String::from(
        r"
export function scoreOrder(input: { status: string; amount: number; flags: string[] }): number {
  let score = 0;
",
    );
    for i in 0..40 {
        writeln!(
            &mut source,
            r#"  if (input.flags.includes("flag{i}")) {{
    score += input.amount > {i} ? {i} : -{i};
  }}"#
        )
        .unwrap();
    }
    source.push_str(
        r#"
  if (input.status === "blocked") {
    return -score;
  }
  return score;
}
"#,
    );
    write_file(&root, "src/score.ts", source);
    write_file(
        &root,
        "src/index.ts",
        r#"
import { scoreOrder } from "./score";

console.log(scoreOrder({ status: "open", amount: 10, flags: ["flag1"] }));
"#,
    );

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_css_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(&root, "package.json", package_json("bench-css-health", ""));
    write_file(
        &root,
        "src/app.tsx",
        r#"
import "./styles.css";

export const App = () => (
  <main className="layout card text-brand shadow-panel animate-fade">
    <button className="button button-primary">Save</button>
  </main>
);
"#,
    );

    let mut css = String::from(
        r"
@theme {
  --color-brand: #0055cc;
  --color-unused-accent: #ff00aa;
  --shadow-panel: 0 1px 8px rgb(0 0 0 / 20%);
  --animate-fade: fade 200ms ease-in;
}

@keyframes fade {
  from { opacity: 0; }
  to { opacity: 1; }
}

.layout { display: grid; gap: 1rem; }
.card { color: var(--color-brand); box-shadow: var(--shadow-panel); }
.button { border: 0; padding: .5rem 1rem; }
.button-primary { background: var(--color-brand); }
",
    );
    for i in 0..80 {
        writeln!(
            &mut css,
            ".unused-{i} .child .leaf:nth-child({}) {{ color: rgb({} {} {}); }}",
            (i % 9) + 1,
            i % 255,
            (i * 3) % 255,
            (i * 7) % 255
        )
        .unwrap();
    }
    write_file(&root, "src/styles.css", css);

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_warm_workspace_project() -> CommandInput {
    let input = create_workspace_project();
    let options = DeadCodeOptions {
        analysis: analysis_options(&input.root, false),
        ..DeadCodeOptions::default()
    };
    let _ = detect_dead_code(&options).expect("warm cache priming succeeds");
    input
}

#[divan::bench]
fn dead_code_library_package(bencher: Bencher) {
    bencher
        .with_inputs(create_library_project)
        .bench_refs(|input| {
            let options = DeadCodeOptions {
                analysis: analysis_options(&input.root, true),
                ..DeadCodeOptions::default()
            };
            detect_dead_code(&options)
        });
}

#[divan::bench]
fn dead_code_next_app_router(bencher: Bencher) {
    bencher
        .with_inputs(create_next_app_project)
        .bench_refs(|input| {
            let options = DeadCodeOptions {
                analysis: analysis_options(&input.root, true),
                ..DeadCodeOptions::default()
            };
            detect_dead_code(&options)
        });
}

#[divan::bench]
fn dead_code_workspace_monorepo(bencher: Bencher) {
    bencher
        .with_inputs(create_workspace_project)
        .bench_refs(|input| {
            let options = DeadCodeOptions {
                analysis: analysis_options(&input.root, true),
                ..DeadCodeOptions::default()
            };
            detect_dead_code(&options)
        });
}

#[divan::bench]
fn dead_code_workspace_monorepo_warm_cache(bencher: Bencher) {
    bencher
        .with_inputs(create_warm_workspace_project)
        .bench_refs(|input| {
            let options = DeadCodeOptions {
                analysis: analysis_options(&input.root, false),
                ..DeadCodeOptions::default()
            };
            detect_dead_code(&options)
        });
}

#[divan::bench]
fn duplicates_route_callbacks(bencher: Bencher) {
    bencher
        .with_inputs(create_duplication_project)
        .bench_refs(|input| {
            let options = DuplicationOptions {
                analysis: analysis_options(&input.root, true),
                mode: DuplicationMode::Mild,
                min_tokens: 35,
                min_lines: 5,
                min_occurrences: 2,
                ..DuplicationOptions::default()
            };
            detect_duplication(&options)
        });
}

#[divan::bench]
fn circular_dense_cycles(bencher: Bencher) {
    bencher
        .with_inputs(create_circular_project)
        .bench_refs(|input| {
            let options = DeadCodeOptions {
                analysis: analysis_options(&input.root, true),
                ..DeadCodeOptions::default()
            };
            detect_circular_dependencies(&options)
        });
}

#[divan::bench]
fn health_complex_app(bencher: Bencher) {
    bencher
        .with_inputs(create_health_project)
        .bench_refs(|input| {
            let options = ComplexityOptions {
                analysis: analysis_options(&input.root, true),
                complexity: true,
                file_scores: true,
                hotspots: true,
                targets: true,
                ..ComplexityOptions::default()
            };
            compute_health(&options)
        });
}

#[divan::bench]
fn health_css_tailwind_app(bencher: Bencher) {
    bencher.with_inputs(create_css_project).bench_refs(|input| {
        let options = ComplexityOptions {
            analysis: analysis_options(&input.root, true),
            css: true,
            score: true,
            ..ComplexityOptions::default()
        };
        compute_health(&options)
    });
}
