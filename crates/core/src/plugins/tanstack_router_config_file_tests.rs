//! Tests of the virtual route config reads of the TanStack Router plugin.

use std::io::Write as _;

use super::*;

/// A virtual route config that links out of the project is not read.
#[test]
#[cfg(unix)]
fn a_virtual_route_config_that_links_out_of_the_project_is_not_read() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().join("project");
    let outside = dir.path().join("elsewhere");
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::create_dir_all(&outside).expect("create outside");
    std::fs::write(outside.join("routes.ts"), "import { rootRoute } from \"@tanstack/virtual-file-routes\";\nexport default rootRoute(\"root.tsx\", []);\n").expect("write");
    std::os::unix::fs::symlink(outside.join("routes.ts"), root.join("routes.ts")).expect("link");
    let mut config = VirtualRouteConfig::default();
    add_virtual_route_config_file(&mut config, &root, "routes.ts");
    assert!(config.route_files.is_empty());
}

#[cfg_attr(miri, ignore)]
#[test]
fn add_virtual_route_config_file_reads_and_parses_virtual_route_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let config_file = "src/routes.ts";
    let routes_path = root.join("src");
    std::fs::create_dir_all(&routes_path).expect("create dir");
    let full_path = routes_path.join("routes.ts");
    let mut f = std::fs::File::create(&full_path).expect("create file");
    writeln!(
        f,
        r#"import {{ rootRoute, index }} from "@tanstack/virtual-file-routes";
export default rootRoute("root.tsx", [index("index.tsx")]);"#
    )
    .expect("write");

    let mut config = VirtualRouteConfig::default();
    add_virtual_route_config_file(&mut config, root, config_file);

    assert!(
        config.config_files.contains(&config_file.to_string()),
        "config file not registered: {:?}",
        config.config_files
    );
    let files: Vec<String> = config
        .route_files
        .iter()
        .map(|s| s.replace('\\', "/"))
        .collect();
    assert!(
        files.iter().any(|f| f.contains("root.tsx")),
        "root.tsx missing: {files:?}"
    );
    assert!(
        files.iter().any(|f| f.contains("index.tsx")),
        "index.tsx missing: {files:?}"
    );
}

#[cfg_attr(miri, ignore)]
#[test]
fn add_virtual_route_config_file_returns_early_when_file_missing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let mut config = VirtualRouteConfig::default();
    // File does not exist on disk; should record config_file but not panic.
    add_virtual_route_config_file(&mut config, root, "nonexistent/routes.ts");
    assert!(
        config
            .config_files
            .contains(&"nonexistent/routes.ts".to_string()),
        "config file not registered even when missing from disk"
    );
    assert!(config.route_files.is_empty());
}
