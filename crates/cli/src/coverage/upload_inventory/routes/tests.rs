use super::*;
use crate::coverage::upload_common;
use crate::coverage::upload_inventory::{
    InventoryRequest, collect_graph_context, collect_inventory, compile_exclude_matcher,
};
use fallow_engine::session::AnalysisSession;
use fallow_engine::source::inventory::InventoryEntry;

fn pattern(plugin: &str, rel: &str) -> Option<(String, MethodRule)> {
    derive_route(plugin, rel).map(|r| (r.pattern, r.methods))
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "the helper builds the expected value of `derive_route`, which is an Option"
)]
fn any(p: &str) -> Option<(String, MethodRule)> {
    Some((p.to_owned(), MethodRule::Any))
}

#[test]
fn next_app_router_pages_and_handlers() {
    assert_eq!(pattern("nextjs", "app/page.tsx"), any("/"));
    assert_eq!(
        pattern("nextjs", "src/app/(shop)/products/[id]/page.tsx"),
        any("/products/[id]")
    );
    assert_eq!(
        pattern("nextjs", "app/@modal/login/page.tsx"),
        any("/login")
    );
    assert_eq!(
        pattern("nextjs", "app/api/checkout/route.ts"),
        Some(("/api/checkout".to_owned(), MethodRule::FromExports))
    );
}

#[test]
fn next_app_router_skips_non_owners() {
    assert_eq!(pattern("nextjs", "app/layout.tsx"), None);
    assert_eq!(pattern("nextjs", "app/_lib/page.tsx"), None);
    assert_eq!(pattern("nextjs", "app/(.)photo/page.tsx"), None);
    assert_eq!(pattern("nextjs", "app/page.module.css"), None);
    assert_eq!(pattern("nextjs", "middleware.ts"), None);
}

#[test]
fn next_pages_router() {
    assert_eq!(pattern("nextjs", "pages/index.tsx"), any("/"));
    assert_eq!(
        pattern("nextjs", "pages/blog/[slug].tsx"),
        any("/blog/[slug]")
    );
    assert_eq!(
        pattern("nextjs", "src/pages/api/users.ts"),
        any("/api/users")
    );
    assert_eq!(pattern("nextjs", "pages/_app.tsx"), None);
    assert_eq!(pattern("nextjs", "pages/about.test.tsx"), None);
}

#[test]
fn next_pages_router_catch_all_pages() {
    assert_eq!(
        pattern("nextjs", "pages/docs/[...slug].tsx"),
        any("/docs/[...slug]")
    );
    assert_eq!(
        pattern("nextjs", "pages/[[...slug]].tsx"),
        any("/[[...slug]]")
    );
    assert_eq!(
        pattern("nextjs", "app/docs/[...slug]/page.tsx"),
        any("/docs/[...slug]")
    );
    assert_eq!(pattern("nextjs", "pages/[...slug].test.tsx"), None);
}

#[test]
fn remix_flat_routes() {
    assert_eq!(pattern("remix", "app/routes/_index.tsx"), any("/"));
    assert_eq!(
        pattern("remix", "app/routes/blog.$slug.tsx"),
        any("/blog/$slug")
    );
    assert_eq!(
        pattern("remix", "app/routes/_auth.login.tsx"),
        any("/login")
    );
    assert_eq!(
        pattern("remix", "app/routes/blog_.edit.tsx"),
        any("/blog/edit")
    );
    assert_eq!(
        pattern("remix", "app/routes/sitemap[.]xml.tsx"),
        any("/sitemap.xml")
    );
    assert_eq!(
        pattern("remix", "app/routes/account.settings/route.tsx"),
        any("/account/settings")
    );
    assert_eq!(
        pattern("remix", "app/routes/account.settings/form.tsx"),
        None
    );
    assert_eq!(pattern("remix", "app/root.tsx"), None);
    assert_eq!(pattern("remix", "app/routes/_auth.tsx"), None);
}

#[test]
fn sveltekit_routes() {
    assert_eq!(pattern("sveltekit", "src/routes/+page.svelte"), any("/"));
    assert_eq!(
        pattern("sveltekit", "src/routes/(app)/blog/[slug]/+page.server.ts"),
        any("/blog/[slug]")
    );
    assert_eq!(
        pattern("sveltekit", "src/routes/api/items/+server.ts"),
        Some(("/api/items".to_owned(), MethodRule::FromExports))
    );
    assert_eq!(pattern("sveltekit", "src/routes/+layout.svelte"), None);
    assert_eq!(pattern("sveltekit", "src/hooks.server.ts"), None);
}

#[test]
fn nuxt_pages_and_server_routes() {
    assert_eq!(pattern("nuxt", "pages/index.vue"), any("/"));
    assert_eq!(
        pattern("nuxt", "app/pages/users/[id].vue"),
        any("/users/[id]")
    );
    assert_eq!(
        pattern("nuxt", "server/api/users/[id].get.ts"),
        Some(("/api/users/[id]".to_owned(), MethodRule::Fixed("GET")))
    );
    assert_eq!(pattern("nuxt", "server/api/health.ts"), any("/api/health"));
    assert_eq!(
        pattern("nuxt", "server/routes/feed.xml.ts"),
        any("/feed.xml")
    );
    assert_eq!(pattern("nuxt", "server/utils/db.ts"), None);
    assert_eq!(pattern("nuxt", "layouts/default.vue"), None);
}

#[test]
fn nuxt_catch_all_pages_and_server_routes() {
    assert_eq!(pattern("nuxt", "pages/[...slug].vue"), any("/[...slug]"));
    assert_eq!(
        pattern("nuxt", "app/pages/docs/[[...slug]].vue"),
        any("/docs/[[...slug]]")
    );
    assert_eq!(pattern("nuxt", "pages/[...slug].spec.ts"), None);
    assert_eq!(
        pattern("nuxt", "server/api/[...slug].ts"),
        any("/api/[...slug]")
    );
    assert_eq!(
        pattern("nuxt", "server/api/files/[...path].get.ts"),
        Some(("/api/files/[...path]".to_owned(), MethodRule::Fixed("GET")))
    );
}

#[test]
fn unknown_plugin_gives_no_route() {
    assert_eq!(pattern("vite", "src/main.ts"), None);
}

#[test]
fn handler_splits_one_route_per_exported_method() {
    let derived = derive_route("nextjs", "app/api/checkout/route.ts").expect("route");
    let routes = expand_methods(&derived, "", "app/api/checkout/route.ts", &["POST", "GET"]);
    let methods: Vec<Option<&str>> = routes.iter().map(|r| r.method).collect();
    assert_eq!(methods, vec![Some("GET"), Some("POST")]);
    assert_ne!(routes[0].id, routes[1].id);

    let none = expand_methods(&derived, "", "app/api/checkout/route.ts", &[]);
    assert_eq!(none.len(), 1);
    assert_eq!(none[0].method, None);
}

#[test]
fn route_id_is_stable_and_prefixed() {
    let a = route_id("nextjs", "", "/", None);
    assert_eq!(a, route_id("nextjs", "", "/", None));
    assert!(a.starts_with("rt_"));
    assert_eq!(a.len(), 3 + 12);
    assert_ne!(a, route_id("nextjs", "", "/", Some("GET")));
    assert_ne!(a, route_id("nextjs", "apps/admin", "/", None));
}

fn candidate(file_index: usize, pattern: &str) -> RouteCandidate {
    candidate_at(file_index, "", pattern, &format!("app{pattern}/page.tsx"))
}

fn candidate_at(file_index: usize, root: &str, pattern: &str, file: &str) -> RouteCandidate {
    let file = file.to_owned();
    RouteCandidate {
        file_index,
        payload: RoutePayload {
            id: route_id("nextjs", root, pattern, None),
            pattern: pattern.to_owned(),
            method: None,
            files: vec![file.clone()],
            file,
            framework: "nextjs",
        },
    }
}

fn function_in(file: &str, name: &str) -> InventoryFunction {
    let entry = InventoryEntry {
        name: name.to_owned(),
        line: 1,
        start_column: 1,
        end_line: 2,
        end_column: 1,
        source_hash: None,
        is_callback: false,
    };
    InventoryFunction::from_entry(file, file, entry, None)
}

#[test]
fn reach_follows_runtime_imports_only_forward() {
    // 0: route /a imports 2; 1: route /b imports nothing; 2 imports 3.
    let forward = vec![vec![2], vec![], vec![3], vec![]];
    let paths: FxHashMap<String, usize> = [("lib.ts", 2), ("deep.ts", 3), ("b.tsx", 1)]
        .into_iter()
        .map(|(p, i)| (p.to_owned(), i))
        .collect();
    let mut functions = vec![
        function_in("lib.ts", "lib"),
        function_in("deep.ts", "deep"),
        function_in("b.tsx", "b"),
        function_in("orphan.ts", "orphan"),
    ];
    let routes = attach_routes(
        vec![candidate(1, "/b"), candidate(0, "/a")],
        &forward,
        &paths,
        &mut functions,
    );
    let id_of = |p: &str| {
        routes
            .routes
            .iter()
            .find(|r| r.pattern == p)
            .expect("route")
            .id
            .clone()
    };
    assert_eq!(functions[0].route_ids, vec![id_of("/a")]);
    assert_eq!(functions[1].route_ids, vec![id_of("/a")]);
    assert_eq!(functions[2].route_ids, vec![id_of("/b")]);
    assert!(functions[3].route_ids.is_empty());
    // Sorted by pattern on the wire.
    assert_eq!(routes.routes[0].pattern, "/a");
}

#[test]
fn reach_terminates_on_import_cycles() {
    let forward = vec![vec![1], vec![0]];
    let paths: FxHashMap<String, usize> = std::iter::once(("x.ts".to_owned(), 1)).collect();
    let mut functions = vec![function_in("x.ts", "x")];
    let routes = attach_routes(vec![candidate(0, "/c")], &forward, &paths, &mut functions);
    assert_eq!(functions[0].route_ids.len(), 1);
    assert_eq!(routes.truncated_functions, 0);
}

#[test]
fn route_caps_drop_routes_and_ids_and_report_them() {
    let total = MAX_ROUTES + 7;
    // Every route file imports one shared file at index `total`.
    let mut forward: Vec<Vec<usize>> = std::iter::repeat_with(|| vec![total]).take(total).collect();
    forward.push(Vec::new());
    let candidates: Vec<RouteCandidate> = (0..total)
        .map(|i| candidate(i, &format!("/r{i:04}")))
        .collect();
    let paths: FxHashMap<String, usize> =
        std::iter::once(("shared.ts".to_owned(), total)).collect();
    let mut functions = vec![function_in("shared.ts", "shared")];

    let routes = attach_routes(candidates, &forward, &paths, &mut functions);

    assert_eq!(routes.routes.len(), MAX_ROUTES);
    assert_eq!(routes.dropped_routes, 7);
    assert_eq!(functions[0].route_ids.len(), MAX_ROUTES_PER_FUNCTION);
    assert_eq!(functions[0].route_ids[0], routes.routes[0].id);
    assert_eq!(routes.truncated_functions, 1);
    let limits = routes.limits().expect("limits");
    assert_eq!(limits.dropped_routes, 7);
    assert_eq!(limits.truncated_functions, 1);
}

#[test]
fn function_at_the_per_function_cap_is_not_truncated() {
    let total = MAX_ROUTES_PER_FUNCTION;
    let mut forward: Vec<Vec<usize>> = std::iter::repeat_with(|| vec![total]).take(total).collect();
    forward.push(Vec::new());
    let candidates: Vec<RouteCandidate> = (0..total)
        .map(|i| candidate(i, &format!("/r{i:04}")))
        .collect();
    let paths: FxHashMap<String, usize> =
        std::iter::once(("shared.ts".to_owned(), total)).collect();
    let mut functions = vec![function_in("shared.ts", "shared")];
    let routes = attach_routes(candidates, &forward, &paths, &mut functions);
    assert_eq!(functions[0].route_ids.len(), total);
    assert_eq!(routes.truncated_functions, 0);
}

#[test]
fn files_of_one_pattern_merge_into_one_route() {
    // Parallel slots: two files serve `/activities`; each imports its own helper.
    let forward = vec![vec![2], vec![3], vec![], vec![]];
    let paths: FxHashMap<String, usize> = [("a.ts".to_owned(), 2), ("b.ts".to_owned(), 3)]
        .into_iter()
        .collect();
    let mut functions = vec![function_in("a.ts", "a"), function_in("b.ts", "b")];
    let routes = attach_routes(
        vec![
            candidate_at(0, "", "/activities", "app/activities/@list/page.tsx"),
            candidate_at(1, "", "/activities", "app/activities/page.tsx"),
        ],
        &forward,
        &paths,
        &mut functions,
    );
    assert_eq!(routes.routes.len(), 1);
    let route = &routes.routes[0];
    assert_eq!(
        route.file, "app/activities/page.tsx",
        "file outside a slot wins"
    );
    assert_eq!(
        route.files,
        vec!["app/activities/page.tsx", "app/activities/@list/page.tsx"]
    );
    assert_eq!(functions[0].route_ids, vec![route.id.clone()]);
    assert_eq!(functions[1].route_ids, vec![route.id.clone()]);
}

#[test]
fn one_pattern_in_two_apps_stays_two_routes() {
    let forward = vec![vec![], vec![]];
    let mut functions = Vec::new();
    let routes = attach_routes(
        vec![
            candidate_at(0, "apps/web", "/about", "apps/web/app/about/page.tsx"),
            candidate_at(1, "apps/admin", "/about", "apps/admin/app/about/page.tsx"),
        ],
        &forward,
        &FxHashMap::default(),
        &mut functions,
    );
    assert_eq!(routes.routes.len(), 2);
    assert_ne!(routes.routes[0].id, routes.routes[1].id);
}

#[test]
fn total_budget_lowers_the_per_function_cap_and_reports_it() {
    // Ten routes all reach one shared file that holds three functions.
    let total = 10;
    let mut forward: Vec<Vec<usize>> = std::iter::repeat_with(|| vec![total]).take(total).collect();
    forward.push(Vec::new());
    let candidates: Vec<RouteCandidate> = (0..total)
        .map(|i| candidate(i, &format!("/r{i:02}")))
        .collect();
    let paths: FxHashMap<String, usize> =
        std::iter::once(("shared.ts".to_owned(), total)).collect();
    let mut functions = vec![
        function_in("shared.ts", "a"),
        function_in("shared.ts", "b"),
        function_in("shared.ts", "c"),
    ];
    let routes = attach_routes_with_budget(candidates, &forward, &paths, &mut functions, 20);
    assert!(functions.iter().all(|f| f.route_ids.len() == 6));
    assert_eq!(routes.truncated_functions, 3);
    let limits = routes.limits().expect("limits");
    assert_eq!(limits.max_routes_per_function, 6);
}

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, body).expect("write");
}

/// A Next.js project: a page and a route handler share a helper, and a second
/// helper only the handler imports. A type-only import does not reach.
fn next_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"shop","dependencies":{"next":"15.0.0","react":"19.0.0"}}"#,
    );
    write(
        root,
        "app/page.tsx",
        "import { price } from '../lib/price';\nexport default function Home() {\n  return price(1);\n}\n",
    );
    write(
        root,
        "app/api/checkout/route.ts",
        "import { price } from '../../../lib/price';\nimport { charge } from '../../../lib/charge';\nimport type { Cart } from '../../../lib/cart';\nexport async function POST() {\n  return charge(price(2));\n}\nexport async function GET() {\n  return price(3);\n}\n",
    );
    write(
        root,
        "lib/price.ts",
        "export function price(n: number) {\n  return n * 2;\n}\n",
    );
    write(
        root,
        "lib/charge.ts",
        "export function charge(n: number) {\n  return n;\n}\n",
    );
    write(
        root,
        "lib/cart.ts",
        "export type Cart = { n: number };\nexport function emptyCart() {\n  return { n: 0 };\n}\n",
    );
    dir
}

#[test]
fn next_fixture_round_trips_routes_through_the_blob() {
    let project = next_project();
    let config = upload_common::load_resolved_config(project.path()).expect("config loads");
    let session = AnalysisSession::from_resolved_config(config).expect("session");
    let include_all = compile_exclude_matcher(&[]).expect("matcher");
    let mut functions = collect_inventory(&session, &include_all, Some("/app"));
    let (_, routes) = collect_graph_context(&session, &mut functions);
    let route_limits = routes.limits();
    let request = InventoryRequest {
        version: 4,
        git_sha: "deadbeef",
        functions: &functions,
        churn_by_path: BTreeMap::new(),
        caller_edges: BTreeMap::new(),
        caller_edge_limits: None,
        routes: &routes.routes,
        route_limits,
    };
    let json = serde_json::to_value(&request).expect("serialize");

    let wire: Vec<(String, serde_json::Value, String)> = json["routes"]
        .as_array()
        .expect("routes array")
        .iter()
        .map(|r| {
            assert_eq!(r["framework"], "nextjs");
            assert!(r["id"].as_str().expect("id").starts_with("rt_"));
            (
                r["pattern"].as_str().expect("pattern").to_owned(),
                r["method"].clone(),
                r["file"].as_str().expect("file").to_owned(),
            )
        })
        .collect();
    assert_eq!(
        wire,
        vec![
            (
                "/".to_owned(),
                serde_json::Value::Null,
                "app/page.tsx".to_owned()
            ),
            (
                "/api/checkout".to_owned(),
                serde_json::json!("GET"),
                "app/api/checkout/route.ts".to_owned()
            ),
            (
                "/api/checkout".to_owned(),
                serde_json::json!("POST"),
                "app/api/checkout/route.ts".to_owned()
            ),
        ]
    );

    let ids_of = |name: &str| -> Vec<String> {
        let function = json["functions"]
            .as_array()
            .expect("functions")
            .iter()
            .find(|f| f["functionName"] == name)
            .unwrap_or_else(|| panic!("{name} present"));
        function
            .get("routeIds")
            .and_then(serde_json::Value::as_array)
            .map(|ids| {
                ids.iter()
                    .map(|id| id.as_str().expect("id").to_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let all_ids: Vec<String> = json["routes"]
        .as_array()
        .expect("routes")
        .iter()
        .map(|r| r["id"].as_str().expect("id").to_owned())
        .collect();
    // The page and both handler methods reach the shared helper.
    assert_eq!(ids_of("price"), all_ids);
    // Only the handler reaches `charge`.
    assert_eq!(ids_of("charge"), all_ids[1..].to_vec());
    // A type-only import reaches nothing.
    assert!(ids_of("emptyCart").is_empty());
    assert_eq!(json["routeLimits"]["maxRoutes"], MAX_ROUTES);
    assert_eq!(
        json["routeLimits"]["maxRoutesPerFunction"],
        MAX_ROUTES_PER_FUNCTION
    );
    assert_eq!(json["routeLimits"]["droppedRoutes"], 0);
    assert_eq!(json["routeLimits"]["truncatedFunctions"], 0);
}

#[test]
fn project_without_route_framework_emits_no_routes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"lib","main":"src/index.ts"}"#,
    );
    write(
        root,
        "src/index.ts",
        "export function main() {\n  return 1;\n}\n",
    );
    write(
        root,
        "app/page.tsx",
        "export default function P() {\n  return 1;\n}\n",
    );
    let config = upload_common::load_resolved_config(root).expect("config loads");
    let session = AnalysisSession::from_resolved_config(config).expect("session");
    let include_all = compile_exclude_matcher(&[]).expect("matcher");
    let mut functions = collect_inventory(&session, &include_all, None);
    let (_, routes) = collect_graph_context(&session, &mut functions);
    assert!(routes.routes.is_empty());
    assert!(routes.limits().is_none());
    assert!(functions.iter().all(|f| f.route_ids.is_empty()));
}
