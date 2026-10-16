#![no_main]
use fallow_config::{
    PackageJson, parse_bun_package_json_resolutions, parse_npm_package_json_overrides,
    parse_override_key, parse_package_json_catalog_data, parse_pnpm_catalog_data,
    parse_pnpm_package_json_overrides, parse_pnpm_workspace_overrides,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    let Ok(source) = std::str::from_utf8(rest) else {
        return;
    };
    match selector % 4 {
        // pnpm-workspace.yaml: the YAML parser plus the hand-written line
        // index that maps catalog and override keys back to line numbers.
        0 => {
            let _ = fallow_config::yaml::parse_documents(source);
            let _ = parse_pnpm_catalog_data(source);
            let _ = parse_pnpm_workspace_overrides(source);
        }
        // package.json: overrides, resolutions and catalogs, each with its own
        // line index over the raw JSON text.
        1 => {
            let _ = parse_package_json_catalog_data(source);
            let _ = parse_pnpm_package_json_overrides(source);
            let _ = parse_npm_package_json_overrides(source);
            let _ = parse_bun_package_json_resolutions(source);
        }
        // package.json: the typed manifest and the walks over `exports`,
        // `workspaces` and local link dependencies.
        2 => {
            if let Ok(pkg) = serde_json::from_str::<PackageJson>(source) {
                let _ = pkg.entry_points();
                let _ = pkg.exports_subdirectories();
                let _ = pkg.workspace_patterns();
                let _ = pkg.local_link_dependencies();
                let _ = pkg.all_dependency_names();
            }
        }
        // A single override key, such as `foo@1>bar@^2`.
        _ => {
            let _ = parse_override_key(source);
        }
    }
});
