use fallow_config::{FallowConfig, OutputFormat, RulesConfig, Severity};

use crate::common::fixture_path;

/// Resolve a fixture with both route-tree rules at `warn` (their default). The
/// detectors are gated on the project declaring `next`.
fn fixture_config(name: &str) -> fallow_config::ResolvedConfig {
    FallowConfig {
        rules: RulesConfig {
            route_collision: Severity::Warn,
            dynamic_segment_name_conflict: Severity::Warn,
            ..RulesConfig::default()
        },
        ..Default::default()
    }
    .resolve(fixture_path(name), OutputFormat::Human, 4, true, true, None)
}

fn collision_urls(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    let mut urls: Vec<String> = results
        .route_collisions
        .iter()
        .map(|c| c.collision.url.clone())
        .collect();
    urls.sort();
    urls.dedup();
    urls
}

fn norm(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[test]
fn route_group_pages_collide_at_shared_url() {
    let config = fixture_config("nextjs-route-tree");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let about: Vec<&fallow_core::results::RouteCollisionFinding> = results
        .route_collisions
        .iter()
        .filter(|c| c.collision.url == "/about")
        .collect();

    // Two pages (marketing + shop groups) both own /about => one finding per file.
    assert_eq!(
        about.len(),
        2,
        "expected a /about collision finding per colliding file: {:?}",
        results
            .route_collisions
            .iter()
            .map(|c| (norm(&c.collision.path), c.collision.url.clone()))
            .collect::<Vec<_>>()
    );
    // Each finding names the OTHER colliding file in conflicting_paths.
    for finding in &about {
        assert_eq!(finding.collision.conflicting_paths.len(), 1);
        assert!(
            norm(&finding.collision.conflicting_paths[0]).ends_with("about/page.tsx"),
            "conflicting path should be the sibling about page"
        );
    }
}

#[test]
fn route_handlers_collide_via_groups() {
    // Diego's BFF case: two `route.ts` handlers under different groups both own
    // /api/health. page and route share one URL-owner namespace.
    let config = fixture_config("nextjs-route-tree");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let api = results
        .route_collisions
        .iter()
        .filter(|c| c.collision.url == "/api/health")
        .count();
    assert_eq!(
        api,
        2,
        "expected a /api/health route-handler collision per file: {:?}",
        collision_urls(&results)
    );
}

#[test]
fn parallel_slot_siblings_do_not_collide() {
    // LOAD-BEARING FALSE-POSITIVE GUARD: @team/members and @analytics/members
    // both resolve to /members but render side-by-side in different parallel
    // slots, so they must NOT be reported as a collision.
    let config = fixture_config("nextjs-route-tree");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results
            .route_collisions
            .iter()
            .any(|c| c.collision.url == "/members"),
        "parallel-slot siblings must not collide: {:?}",
        collision_urls(&results)
    );
}

#[test]
fn single_page_does_not_collide() {
    let config = fixture_config("nextjs-route-tree");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results
            .route_collisions
            .iter()
            .any(|c| c.collision.url == "/contact"),
        "a lone page must not collide: {:?}",
        collision_urls(&results)
    );
}

#[test]
fn dynamic_segment_name_conflict_at_shared_position() {
    // app/blog/[id] and app/blog/[slug] use different slug names at /blog.
    let config = fixture_config("nextjs-route-tree");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let blog: Vec<&fallow_core::results::DynamicSegmentNameConflictFinding> = results
        .dynamic_segment_name_conflicts
        .iter()
        .filter(|c| c.conflict.position == "/blog")
        .collect();

    assert_eq!(
        blog.len(),
        2,
        "expected a /blog dynamic-segment conflict per involved file: {:?}",
        results
            .dynamic_segment_name_conflicts
            .iter()
            .map(|c| (norm(&c.conflict.path), c.conflict.position.clone()))
            .collect::<Vec<_>>()
    );
    let finding = blog[0];
    assert_eq!(
        finding.conflict.conflicting_segments,
        vec!["[id]".to_string(), "[slug]".to_string()]
    );
}

#[test]
fn dynamic_segment_conflict_is_not_a_route_collision() {
    // [id] vs [slug] differ by name, so route-collision keeps them distinct
    // (the conflict is the dynamic-segment-name-conflict detector's job).
    let config = fixture_config("nextjs-route-tree");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results
            .route_collisions
            .iter()
            .any(|c| c.collision.url.contains("blog")),
        "differing dynamic names must not double-report as a route collision: {:?}",
        collision_urls(&results)
    );
}

#[test]
fn monorepo_two_apps_sharing_url_do_not_collide() {
    // CRITICAL per-app-root scoping: apps/web/app/about and apps/admin/app/about
    // both resolve to /about but are independent Next apps with separate builds,
    // so they must NOT collide.
    let config = fixture_config("nextjs-route-tree-monorepo");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.route_collisions.is_empty(),
        "two independent app-roots sharing a URL must not collide: {:?}",
        results
            .route_collisions
            .iter()
            .map(|c| (norm(&c.collision.path), c.collision.url.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn no_findings_when_next_is_absent() {
    let config = fixture_config("nextjs-route-tree-no-next");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.route_collisions.is_empty() && results.dynamic_segment_name_conflicts.is_empty(),
        "without `next` declared, neither rule fires: collisions={:?} conflicts={:?}",
        collision_urls(&results),
        results.dynamic_segment_name_conflicts.len()
    );
}
