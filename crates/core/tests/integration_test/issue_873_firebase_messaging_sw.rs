use super::common::{create_config, fixture_path};
use super::framework_convention_coverage_common::collect_unused_files;

#[test]
fn firebase_messaging_service_workers_are_not_unused_files() {
    let root = fixture_path("issue-873-firebase-messaging-sw");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);

    assert!(
        !unused_files.contains(&"public/firebase-messaging-sw.js".to_string()),
        "root Firebase Messaging service worker should be framework-used, got {unused_files:?}"
    );
    assert!(
        !unused_files.contains(&"apps/web/public/firebase-messaging-sw.js".to_string()),
        "nested Firebase Messaging service worker should be framework-used, got {unused_files:?}"
    );
    assert!(
        unused_files.contains(&"public/orphan.js".to_string()),
        "unrelated root public file should still report, got {unused_files:?}"
    );
    assert!(
        unused_files.contains(&"apps/web/public/orphan.js".to_string()),
        "unrelated nested public file should still report, got {unused_files:?}"
    );

    let unused_dependencies: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    assert!(
        unused_dependencies.contains(&"firebase"),
        "service worker reachability should not credit the firebase dependency by itself, got {unused_dependencies:?}"
    );
}
