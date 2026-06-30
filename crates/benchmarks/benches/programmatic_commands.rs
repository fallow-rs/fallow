#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_api::{
    AnalysisOptions, ComplexityOptions, DeadCodeOptions, DuplicationMode, DuplicationOptions,
    EngineHealthRunner, run_circular_dependencies, run_dead_code, run_duplication,
    run_health_with_runner,
};
use fallow_core::{
    cache::{CacheStore, module_to_cached},
    discover::{DiscoveredFile, FileId},
    extract::{parse_all_files, parse_single_file},
};
use tempfile::TempDir;

const BENCH_THREADS: usize = 4;

struct CommandInput {
    _temp_dir: TempDir,
    root: PathBuf,
}

struct ExtractCacheInput {
    _temp_dir: TempDir,
    files: Vec<DiscoveredFile>,
    cache: CacheStore,
}

fn write_file(root: &Path, path: &str, source: impl AsRef<str>) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("fixture file has parent")).unwrap();
    std::fs::write(path, source.as_ref()).unwrap();
}

fn dependency_block(dependencies: &[(&str, &str)]) -> String {
    if dependencies.is_empty() {
        return "{}".to_string();
    }

    let mut source = String::from("{\n");
    for (index, (name, version)) in dependencies.iter().enumerate() {
        let comma = if index + 1 == dependencies.len() {
            ""
        } else {
            ","
        };
        writeln!(&mut source, r#"    "{name}": "{version}"{comma}"#).unwrap();
    }
    source.push_str("  }");
    source
}

fn package_json(name: &str, dependencies: &[(&str, &str)], extra_fields: &str) -> String {
    let dependencies = dependency_block(dependencies);
    let extra_fields = if extra_fields.is_empty() {
        String::new()
    } else {
        format!(",\n{extra_fields}")
    };

    format!(
        r#"{{
  "name": "{name}",
  "private": true,
  "type": "module",
  "dependencies": {dependencies},
  "devDependencies": {{
    "typescript": "5.8.0"
  }}{extra_fields}
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

fn is_source_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };

    matches!(extension, "css" | "js" | "jsx" | "ts" | "tsx")
}

fn collect_source_paths(dir: &Path, paths: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("benchmark fixture directory is readable") {
        let entry = entry.expect("benchmark fixture entry is readable");
        let path = entry.path();
        if path.is_dir() {
            collect_source_paths(&path, paths);
        } else if is_source_path(&path) {
            paths.push(path);
        }
    }
}

fn discovered_source_files(root: &Path) -> Vec<DiscoveredFile> {
    let mut paths = Vec::new();
    collect_source_paths(root, &mut paths);
    paths.sort();

    paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| DiscoveredFile {
            id: FileId(u32::try_from(index).expect("benchmark fixture file count fits in u32")),
            size_bytes: fs::metadata(&path)
                .expect("benchmark fixture metadata is readable")
                .len(),
            path,
        })
        .collect()
}

fn create_library_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        package_json(
            "bench-package-library",
            &[],
            r#"  "exports": {
    ".": "./src/index.ts",
    "./server": "./src/server.ts"
  },
  "files": ["src"]"#,
        ),
    );
    write_file(
        &root,
        "src/index.ts",
        r#"
export { usedFeature } from "./feature";
export { createHandler } from "./server";
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
        "src/server.ts",
        r#"
import type { PublicOptions } from "./types";

export const createHandler = (options: PublicOptions) => {
  return (request: Request): Response => {
    const label = request.headers.get("x-label") ?? options.label;
    return Response.json({ label });
  };
};

export const createDebugHandler = () => Response.json({ debug: true });
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
        "src/internal/legacy.ts",
        r"
export const onlyInUnusedFile = true;
",
    );

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the Next app router fixture keeps related generated files together"
)]
fn create_next_app_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        package_json(
            "bench-next-app-router",
            &[
                ("next", "15.0.0"),
                ("react", "19.0.0"),
                ("react-dom", "19.0.0"),
            ],
            r#"  "scripts": {
    "build": "next build"
  }"#,
        ),
    );
    write_file(
        &root,
        "next.config.ts",
        r#"
import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  experimental: {
    typedRoutes: true,
  },
};

export default nextConfig;
"#,
    );
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
import { createPost } from "./actions";
import { getPosts } from "../lib/posts";

export default async function Page() {
  const posts = await getPosts();
  return <main>{posts.map((post) => <Button key={post.id} label={post.title} action={createPost} />)}</main>;
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
        "app/(marketing)/pricing/page.tsx",
        r#"
import { getPlans } from "../../../lib/plans";

export default async function PricingPage() {
  const plans = await getPlans();
  return <section>{plans.map((plan) => <p key={plan.id}>{plan.name}</p>)}</section>;
}
"#,
    );
    write_file(
        &root,
        "app/dashboard/@analytics/page.tsx",
        r#"
import { getDashboardStats } from "../../../lib/dashboard";

export default async function AnalyticsSlot() {
  const stats = await getDashboardStats();
  return <aside>{stats.visits}</aside>;
}
"#,
    );
    write_file(
        &root,
        "app/api/posts/route.ts",
        r#"
import { getPosts } from "../../../lib/posts";

export async function GET() {
  return Response.json(await getPosts());
}
"#,
    );
    write_file(
        &root,
        "app/actions.ts",
        r#"
"use server";

export const createPost = async (formData: FormData): Promise<string> => {
  return String(formData.get("title") ?? "untitled");
};

export const unusedServerAction = async (): Promise<void> => {};
"#,
    );
    write_file(
        &root,
        "components/button.tsx",
        r#"
"use client";

export const Button = ({ action, label }: { action: (formData: FormData) => Promise<string>; label: string }) => {
  return <form action={action}><button className="button primary">{label}</button></form>;
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
        "lib/plans.ts",
        r#"
export const getPlans = async () => [{ id: "starter", name: "Starter" }];
export const unusedPlanMapper = (name: string) => name.toLowerCase();
"#,
    );
    write_file(
        &root,
        "lib/dashboard.ts",
        r"
export const getDashboardStats = async () => ({ visits: 42 });
export const unusedDashboardExport = () => ({ visits: 0 });
",
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
  "packageManager": "pnpm@10.0.0",
  "workspaces": ["apps/*", "packages/*"],
  "dependencies": {}
}"#,
    );
    write_file(
        &root,
        "pnpm-workspace.yaml",
        r#"
packages:
  - "apps/*"
  - "packages/*"
"#,
    );
    write_file(
        &root,
        "apps/web/package.json",
        r#"{"name":"@bench/web","main":"src/index.ts","dependencies":{"@bench/config":"workspace:*","@bench/shared":"workspace:*","@bench/ui":"workspace:*"}}"#,
    );
    write_file(
        &root,
        "apps/admin/package.json",
        r#"{"name":"@bench/admin","main":"src/index.ts","dependencies":{"@bench/shared":"workspace:*","@bench/ui":"workspace:*"}}"#,
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
        "packages/config/package.json",
        r#"{"name":"@bench/config","main":"src/index.ts"}"#,
    );
    write_file(
        &root,
        "apps/web/src/index.ts",
        r#"
import { featureFlags } from "@bench/config";
import { formatUser } from "@bench/shared";
import { Card } from "@bench/ui";

export const render = (name: string) => Card({ title: `${formatUser(name)}:${featureFlags.checkout}` });
"#,
    );
    write_file(
        &root,
        "apps/admin/src/index.ts",
        r#"
import { formatUser } from "@bench/shared";
import { Card } from "@bench/ui";

export const renderAdmin = (name: string) => Card({ title: `admin:${formatUser(name)}` });
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
    write_file(
        &root,
        "packages/config/src/index.ts",
        r#"
export const featureFlags = { checkout: "new" } as const;
export const unusedExperiment = { search: "legacy" } as const;
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
        package_json("bench-next-route-callback-dupes", &[("next", "15.0.0")], ""),
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

    for i in 0..14 {
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
    write_file(
        &root,
        "middleware.ts",
        r#"
export const middleware = (request: Request): Response => {
  const tenant = request.headers.get("x-tenant") ?? "default";
  return Response.json({ tenant });
};
"#,
    );

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_circular_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        package_json("bench-circulars", &[], ""),
    );
    for domain in ["orders", "billing", "users"] {
        for i in 0..10 {
            let next = (i + 1) % 10;
            write_file(
                &root,
                &format!("src/domains/{domain}/node{i}.ts"),
                format!(
                    r#"
import {{ value{next} }} from "./node{next}";

export const value{i} = value{next} + {i};
"#
                ),
            );
        }
        write_file(
            &root,
            &format!("src/domains/{domain}/index.ts"),
            r#"
export { value0 } from "./node0";
"#,
        );
    }
    write_file(
        &root,
        "src/index.ts",
        r#"
import { value0 as orderValue } from "./domains/orders";
import { value0 as billingValue } from "./domains/billing";
import { value0 as userValue } from "./domains/users";

console.log(orderValue, billingValue, userValue);
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

    write_file(
        &root,
        "package.json",
        package_json("bench-health-service", &[], ""),
    );
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

    write_file(
        &root,
        "package.json",
        package_json(
            "bench-css-tailwind-design-system",
            &[("react", "19.0.0"), ("tailwindcss", "4.0.0")],
            "",
        ),
    );
    write_file(
        &root,
        "tailwind.config.ts",
        r##"
export default {
  content: ["./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        brand: "#0055cc",
      },
    },
  },
};
"##,
    );
    write_file(
        &root,
        "src/app.tsx",
        r#"
import "./styles.css";

export const App = () => (
  <main className="layout card text-brand shadow-panel animate-fade">
    <button className="button button-primary">Save</button>
    <span className="sr-only">Draft saved</span>
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
.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; }
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

fn create_warm_metadata_workspace_project() -> CommandInput {
    let input = create_workspace_project();
    let options = DeadCodeOptions {
        analysis: analysis_options(&input.root, false),
        ..DeadCodeOptions::default()
    };
    let _ = run_dead_code(&options).expect("warm cache priming succeeds");
    input
}

fn create_warm_hash_workspace_project() -> ExtractCacheInput {
    let CommandInput {
        _temp_dir: temp_dir,
        root,
    } = create_workspace_project();
    let files = discovered_source_files(&root);
    let mut cache = CacheStore::new();

    for file in &files {
        let module = parse_single_file(file).expect("benchmark fixture parses");
        let cached = module_to_cached(
            &module,
            fallow_types::source_fingerprint::SourceFingerprint::new(1, 1),
        );
        cache.insert(&file.path, cached);
    }

    ExtractCacheInput {
        _temp_dir: temp_dir,
        files,
        cache,
    }
}

fn create_warm_complexity_health_project() -> CommandInput {
    let input = create_health_project();
    let options = ComplexityOptions {
        analysis: analysis_options(&input.root, false),
        complexity: true,
        file_scores: true,
        hotspots: true,
        targets: true,
        ..ComplexityOptions::default()
    };
    let _ = run_health_with_runner(&options, &EngineHealthRunner)
        .expect("warm complexity cache priming succeeds");
    input
}

fn dead_code_package_library_exports(c: &mut Criterion) {
    c.bench_function("dead_code_package_library_exports", |bencher| {
        bencher.iter_batched_ref(
            create_library_project,
            |input| {
                let options = DeadCodeOptions {
                    analysis: analysis_options(&input.root, true),
                    ..DeadCodeOptions::default()
                };
                run_dead_code(&options)
            },
            BatchSize::LargeInput,
        );
    });
}

fn dead_code_next_app_router_segments(c: &mut Criterion) {
    c.bench_function("dead_code_next_app_router_segments", |bencher| {
        bencher.iter_batched_ref(
            create_next_app_project,
            |input| {
                let options = DeadCodeOptions {
                    analysis: analysis_options(&input.root, true),
                    ..DeadCodeOptions::default()
                };
                run_dead_code(&options)
            },
            BatchSize::LargeInput,
        );
    });
}

fn dead_code_workspace_monorepo_cross_package(c: &mut Criterion) {
    c.bench_function("dead_code_workspace_monorepo_cross_package", |bencher| {
        bencher.iter_batched_ref(
            create_workspace_project,
            |input| {
                let options = DeadCodeOptions {
                    analysis: analysis_options(&input.root, true),
                    ..DeadCodeOptions::default()
                };
                run_dead_code(&options)
            },
            BatchSize::LargeInput,
        );
    });
}

fn dead_code_workspace_monorepo_cross_package_warm_metadata_hit(c: &mut Criterion) {
    c.bench_function(
        "dead_code_workspace_monorepo_cross_package_warm_metadata_hit",
        |bencher| {
            bencher.iter_batched_ref(
                create_warm_metadata_workspace_project,
                |input| {
                    let options = DeadCodeOptions {
                        analysis: analysis_options(&input.root, false),
                        ..DeadCodeOptions::default()
                    };
                    run_dead_code(&options)
                },
                BatchSize::LargeInput,
            );
        },
    );
}

fn extract_workspace_monorepo_warm_hash_hit(c: &mut Criterion) {
    c.bench_function("extract_workspace_monorepo_warm_hash_hit", |bencher| {
        bencher.iter_batched_ref(
            create_warm_hash_workspace_project,
            |input| {
                let result = parse_all_files(&input.files, Some(&input.cache), false);
                assert_eq!(result.cache_hits, input.files.len());
                assert_eq!(result.cache_misses, 0);
                result
            },
            BatchSize::LargeInput,
        );
    });
}

fn duplication_next_route_callbacks_repeated_auth(c: &mut Criterion) {
    c.bench_function(
        "duplication_next_route_callbacks_repeated_auth",
        |bencher| {
            bencher.iter_batched_ref(
                create_duplication_project,
                |input| {
                    let options = DuplicationOptions {
                        analysis: analysis_options(&input.root, true),
                        mode: Some(DuplicationMode::Mild),
                        min_tokens: Some(35),
                        min_lines: Some(5),
                        min_occurrences: Some(2),
                        ..DuplicationOptions::default()
                    };
                    run_duplication(&options)
                },
                BatchSize::LargeInput,
            );
        },
    );
}

fn circular_dependencies_domain_graph_cycles(c: &mut Criterion) {
    c.bench_function("circular_dependencies_domain_graph_cycles", |bencher| {
        bencher.iter_batched_ref(
            create_circular_project,
            |input| {
                let options = DeadCodeOptions {
                    analysis: analysis_options(&input.root, true),
                    ..DeadCodeOptions::default()
                };
                run_circular_dependencies(&options)
            },
            BatchSize::LargeInput,
        );
    });
}

fn health_complex_service_scoring(c: &mut Criterion) {
    c.bench_function("health_complex_service_scoring", |bencher| {
        bencher.iter_batched_ref(
            create_health_project,
            |input| {
                let options = ComplexityOptions {
                    analysis: analysis_options(&input.root, true),
                    complexity: true,
                    file_scores: true,
                    hotspots: true,
                    targets: true,
                    ..ComplexityOptions::default()
                };
                run_health_with_runner(&options, &EngineHealthRunner)
            },
            BatchSize::LargeInput,
        );
    });
}

fn health_complex_service_warm_complexity_hit(c: &mut Criterion) {
    c.bench_function("health_complex_service_warm_complexity_hit", |bencher| {
        bencher.iter_batched_ref(
            create_warm_complexity_health_project,
            |input| {
                let options = ComplexityOptions {
                    analysis: analysis_options(&input.root, false),
                    complexity: true,
                    file_scores: true,
                    hotspots: true,
                    targets: true,
                    ..ComplexityOptions::default()
                };
                run_health_with_runner(&options, &EngineHealthRunner)
            },
            BatchSize::LargeInput,
        );
    });
}

fn health_css_tailwind_design_system(c: &mut Criterion) {
    c.bench_function("health_css_tailwind_design_system", |bencher| {
        bencher.iter_batched_ref(
            create_css_project,
            |input| {
                let options = ComplexityOptions {
                    analysis: analysis_options(&input.root, true),
                    css: true,
                    score: true,
                    ..ComplexityOptions::default()
                };
                run_health_with_runner(&options, &EngineHealthRunner)
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(
    benches,
    dead_code_package_library_exports,
    dead_code_next_app_router_segments,
    dead_code_workspace_monorepo_cross_package,
    dead_code_workspace_monorepo_cross_package_warm_metadata_hit,
    extract_workspace_monorepo_warm_hash_hit,
    duplication_next_route_callbacks_repeated_auth,
    circular_dependencies_domain_graph_cycles,
    health_complex_service_scoring,
    health_complex_service_warm_complexity_hit,
    health_css_tailwind_design_system
);
criterion_main!(benches);
