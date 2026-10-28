use super::{
    COVERAGE_DOCS_URL, CoverageSetupContext, FrameworkKind, PackageManager, SetupArgs,
    build_setup_json, detect_coverage_artifact, detect_framework, detect_package_manager,
    handle_license_step, load_setup_state, recipe_contents, recipe_state_is_current,
    record_recipe_state, record_sidecar_state, render_setup_json, run_setup,
    setup_context_fingerprint, setup_state_path, sidecar_state_is_current,
    upload_inventory_hint_lines, write_recipe,
};

#[test]
fn upload_inventory_hint_fits_eighty_columns_with_the_docs_url_on_its_own_line() {
    let lines = upload_inventory_hint_lines();
    let url_line = format!("  {COVERAGE_DOCS_URL}");
    assert_eq!(lines.last(), Some(&url_line));
    for line in &lines[..lines.len() - 1] {
        assert!(
            line.chars().count() <= 80,
            "wider than 80 columns: {line:?}"
        );
        assert!(
            !line.contains(COVERAGE_DOCS_URL),
            "URL not on its own line: {line:?}"
        );
    }
}
use fallow_config::PackageJson;
use fallow_license::LicenseStatus;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn setup_continues_without_license_for_single_local_capture() {
    let dir = tempdir().expect("tempdir should be created");
    let args = SetupArgs {
        non_interactive: true,
        ..SetupArgs::default()
    };
    let status = Ok(LicenseStatus::Missing);

    assert!(
        handle_license_step(dir.path(), args, &status).is_none(),
        "missing license must not stop setup; single local captures are free"
    );
}

#[test]
fn detect_framework_recognizes_nuxt_projects() {
    let package_json: PackageJson =
        serde_json::from_str(r#"{"name":"demo","dependencies":{"nuxt":"^3.0.0"}}"#)
            .expect("package.json should parse");

    assert_eq!(detect_framework(Some(&package_json)), FrameworkKind::Nuxt);
}

#[test]
fn detect_framework_recognizes_vite_browser_projects() {
    let package_json: PackageJson =
        serde_json::from_str(r#"{"name":"demo","devDependencies":{"vite":"^6.0.0"}}"#)
            .expect("package.json should parse");

    assert_eq!(
        detect_framework(Some(&package_json)),
        FrameworkKind::ViteBrowser
    );
}

#[test]
fn detect_framework_prefers_node_server_frameworks_over_vite() {
    let package_json: PackageJson = serde_json::from_str(
        r#"{"name":"api","dependencies":{"elysia":"^1.0.0"},"devDependencies":{"vite":"^6.0.0"}}"#,
    )
    .expect("package.json should parse");

    assert_eq!(
        detect_framework(Some(&package_json)),
        FrameworkKind::PlainNode
    );
}

#[test]
fn detect_package_manager_prefers_package_manager_field() {
    let dir = tempdir().expect("tempdir should be created");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"demo","packageManager":"bun@1.2.0"}"#,
    )
    .expect("package.json should be written");
    std::fs::write(dir.path().join("pnpm-lock.yaml"), "lockfileVersion: '9.0'")
        .expect("lockfile should be written");

    assert_eq!(
        detect_package_manager(dir.path()),
        Some(PackageManager::Bun)
    );
}

#[test]
fn package_manager_commands_match_each_tool() {
    assert_eq!(PackageManager::Npm.label(), "npm");
    assert_eq!(
        PackageManager::Npm.add_runtime_package_command("pkg"),
        "npm install pkg"
    );
    assert_eq!(
        PackageManager::Npm.install_command(),
        "npm install --save-dev @fallow-cli/fallow-cov"
    );
    assert_eq!(PackageManager::Npm.run_script("build"), "npm run build");
    assert_eq!(PackageManager::Npm.exec_binary("vite", &[]), "npx vite");

    assert_eq!(PackageManager::Pnpm.label(), "pnpm");
    assert_eq!(
        PackageManager::Pnpm.add_runtime_package_command("pkg"),
        "pnpm add pkg"
    );
    assert_eq!(
        PackageManager::Pnpm.install_command(),
        "pnpm add -D @fallow-cli/fallow-cov"
    );
    assert_eq!(PackageManager::Pnpm.run_script("build"), "pnpm build");
    assert_eq!(
        PackageManager::Pnpm.exec_binary("vite", &["preview"]),
        "pnpm exec vite preview"
    );

    assert_eq!(PackageManager::Yarn.label(), "yarn");
    assert_eq!(
        PackageManager::Yarn.add_runtime_package_command("pkg"),
        "yarn add pkg"
    );
    assert_eq!(
        PackageManager::Yarn.install_command(),
        "yarn add -D @fallow-cli/fallow-cov"
    );
    assert_eq!(PackageManager::Yarn.run_script("build"), "yarn build");
    assert_eq!(
        PackageManager::Yarn.exec_binary("vite", &["build"]),
        "yarn vite build"
    );

    assert_eq!(PackageManager::Bun.label(), "bun");
    assert_eq!(
        PackageManager::Bun.add_runtime_package_command("pkg"),
        "bun add pkg"
    );
    assert_eq!(
        PackageManager::Bun.install_command(),
        "bun add -d @fallow-cli/fallow-cov"
    );
    assert_eq!(PackageManager::Bun.run_script("build"), "bun run build");
    assert_eq!(
        PackageManager::Bun.exec_binary("vite", &["preview"]),
        "bunx vite preview"
    );
}

#[test]
fn setup_context_commands_cover_framework_defaults() {
    let context = |framework, package_manager| CoverageSetupContext {
        framework,
        package_manager,
        has_build_script: false,
        has_start_script: false,
        has_preview_script: false,
        node_entry_path: "server.ts".to_owned(),
    };

    let next = context(FrameworkKind::NextJs, Some(PackageManager::Pnpm));
    assert_eq!(
        next.build_command().as_deref(),
        Some("pnpm exec next build")
    );
    assert_eq!(next.run_command(), "pnpm exec next start");

    let nuxt = context(FrameworkKind::Nuxt, Some(PackageManager::Yarn));
    assert_eq!(nuxt.build_command().as_deref(), Some("yarn nuxi build"));
    assert_eq!(nuxt.run_command(), "yarn nuxi preview");

    let astro = context(FrameworkKind::Astro, Some(PackageManager::Bun));
    assert_eq!(astro.build_command().as_deref(), Some("bunx astro build"));
    assert_eq!(astro.run_command(), "bunx astro preview");

    let remix = context(FrameworkKind::Remix, Some(PackageManager::Npm));
    assert_eq!(remix.build_command().as_deref(), Some("npx remix build"));
    assert_eq!(remix.run_command(), "node ./build/index.js");

    let svelte = context(FrameworkKind::SvelteKit, None);
    assert_eq!(svelte.build_command().as_deref(), Some("npx vite build"));
    assert_eq!(svelte.run_command(), "npx vite preview");

    let vite = context(FrameworkKind::ViteBrowser, Some(PackageManager::Pnpm));
    assert_eq!(
        vite.build_command().as_deref(),
        Some("pnpm exec vite build")
    );
    assert_eq!(vite.run_command(), "pnpm exec vite preview");

    let nest = context(FrameworkKind::NestJs, Some(PackageManager::Pnpm));
    assert_eq!(nest.build_command(), None);
    assert_eq!(nest.run_command(), "node dist/main.js");

    let plain = context(FrameworkKind::PlainNode, None);
    assert_eq!(plain.build_command(), None);
    assert_eq!(plain.run_command(), "node dist/server.js");

    let other = context(FrameworkKind::Other, Some(PackageManager::Pnpm));
    assert_eq!(other.build_command(), None);
    assert_eq!(other.run_command(), "node dist/server.js");
    assert_eq!(other.framework.label(), "custom project");
}

#[test]
fn setup_json_emits_workspace_members_and_union_runtime_targets() {
    let dir = tempdir().expect("tempdir should be created");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{
  "name": "@demo/api",
  "private": true,
  "packageManager": "pnpm@9.0.0",
  "workspaces": ["apps/*", "packages/*"],
  "scripts": { "start": "node dist/server.js" },
  "dependencies": { "elysia": "^1.0.0" },
  "devDependencies": { "vite": "^6.0.0" }
}"#,
    )
    .expect("root package.json should be written");
    std::fs::create_dir_all(dir.path().join("src")).expect("root src dir should be created");
    std::fs::write(
        dir.path().join("src/index.ts"),
        "export const api = true;\n",
    )
    .expect("root entry should be written");

    for (path, name) in [
        ("apps/admin", "@demo/admin"),
        ("apps/marketing", "@demo/marketing"),
    ] {
        let workspace_dir = dir.path().join(path);
        std::fs::create_dir_all(&workspace_dir).expect("workspace dir should be created");
        std::fs::write(
            workspace_dir.join("package.json"),
            format!(
                r#"{{"name":"{name}","scripts":{{"dev":"vite","build":"vite build"}},"devDependencies":{{"vite":"^6.0.0"}}}}"#
            ),
        )
        .expect("workspace package.json should be written");
    }
    let library_dir = dir.path().join("packages/shared");
    std::fs::create_dir_all(&library_dir).expect("library dir should be created");
    std::fs::write(
        library_dir.join("package.json"),
        r#"{"name":"@demo/shared","scripts":{"build":"tsc"},"devDependencies":{"typescript":"^6.0.0"}}"#,
    )
    .expect("library package.json should be written");

    let payload = build_setup_json(dir.path(), false);

    assert_eq!(payload["framework_detected"], "plain_node");
    assert_eq!(
        payload["runtime_targets"],
        serde_json::json!(["node", "browser"])
    );
    assert_eq!(payload["files_to_edit"][0]["path"], "src/index.ts");
    assert_eq!(
        payload["members"].as_array().map(Vec::len),
        Some(3),
        "root plus two workspace members should be emitted: {payload:#}"
    );

    let members = payload["members"].as_array().expect("members array");
    assert!(
        members
            .iter()
            .all(|member| member["path"] != "packages/shared"),
        "build-only library workspaces should not receive runtime setup recipes: {payload:#}"
    );
    let root_member = members
        .iter()
        .find(|member| member["path"] == ".")
        .expect("root member should be present");
    assert_eq!(root_member["name"], "@demo/api");
    assert_eq!(root_member["framework_detected"], "plain_node");
    assert_eq!(root_member["runtime_targets"], serde_json::json!(["node"]));
    assert_eq!(root_member["snippets"][0]["path"], "src/index.ts");

    for path in ["apps/admin", "apps/marketing"] {
        let member = members
            .iter()
            .find(|member| member["path"] == path)
            .unwrap_or_else(|| panic!("missing workspace member {path}: {payload:#}"));
        assert_eq!(member["framework_detected"], "vite");
        assert_eq!(member["package_manager"], "pnpm");
        assert_eq!(member["runtime_targets"], serde_json::json!(["browser"]));
        assert_eq!(member["snippets"][0]["path"], format!("{path}/src/main.ts"));
    }
}

/// Snapshot test that locks the full `coverage setup --json` workspace
/// shape. Adding a new field to the payload (e.g. a future `_meta` block)
/// is an intentional contract change, so the snapshot must be reviewed
/// with `cargo insta review`. Reverting the workspace-aware fix or the
/// Vite-app heuristic regenerates a different snapshot.
#[test]
fn setup_json_workspace_payload_matches_snapshot() {
    let dir = tempdir().expect("tempdir should be created");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{
  "name": "@demo/api",
  "private": true,
  "packageManager": "pnpm@9.0.0",
  "workspaces": ["apps/*", "packages/*"],
  "scripts": { "start": "node dist/server.js" },
  "dependencies": { "elysia": "^1.0.0" },
  "devDependencies": { "vite": "^6.0.0" }
}"#,
    )
    .expect("root package.json should be written");
    std::fs::create_dir_all(dir.path().join("src")).expect("root src dir should be created");
    std::fs::write(
        dir.path().join("src/index.ts"),
        "export const api = true;\n",
    )
    .expect("root entry should be written");

    for (path, name) in [
        ("apps/admin", "@demo/admin"),
        ("apps/marketing", "@demo/marketing"),
    ] {
        let workspace_dir = dir.path().join(path);
        std::fs::create_dir_all(workspace_dir.join("src"))
            .expect("workspace src dir should be created");
        std::fs::write(
            workspace_dir.join("package.json"),
            format!(
                r#"{{"name":"{name}","scripts":{{"dev":"vite","build":"vite build"}},"devDependencies":{{"vite":"^6.0.0"}}}}"#
            ),
        )
        .expect("workspace package.json should be written");
        std::fs::write(
            workspace_dir.join("src/main.ts"),
            "export const app = true;\n",
        )
        .expect("workspace entry should be written");
    }
    let library_dir = dir.path().join("packages/shared");
    std::fs::create_dir_all(&library_dir).expect("library dir should be created");
    std::fs::write(
        library_dir.join("package.json"),
        r#"{"name":"@demo/shared","scripts":{"build":"tsc"},"devDependencies":{"typescript":"^6.0.0"}}"#,
    )
    .expect("library package.json should be written");

    let payload = build_setup_json(dir.path(), false);

    insta::assert_yaml_snapshot!("coverage_setup_json_workspace", payload);
}

#[test]
fn setup_json_skips_non_runtime_workspace_root() {
    let dir = tempdir().expect("tempdir should be created");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{
  "name": "repo",
  "private": true,
  "packageManager": "pnpm@9.0.0",
  "workspaces": ["apps/*"]
}"#,
    )
    .expect("root package.json should be written");

    let api_dir = dir.path().join("apps/api");
    std::fs::create_dir_all(api_dir.join("src")).expect("api src dir should be created");
    std::fs::write(
        api_dir.join("package.json"),
        r#"{"name":"api","dependencies":{"elysia":"^1.0.0"}}"#,
    )
    .expect("api package.json should be written");
    std::fs::write(api_dir.join("src/index.ts"), "export const api = true;\n")
        .expect("api entry should be written");

    let web_dir = dir.path().join("apps/web");
    std::fs::create_dir_all(web_dir.join("src")).expect("web src dir should be created");
    std::fs::write(
        web_dir.join("package.json"),
        r#"{"name":"web","scripts":{"dev":"vite"},"devDependencies":{"vite":"^6.0.0"}}"#,
    )
    .expect("web package.json should be written");
    std::fs::write(web_dir.join("src/main.ts"), "export const web = true;\n")
        .expect("web entry should be written");

    let payload = build_setup_json(dir.path(), false);

    assert_eq!(payload["framework_detected"], "plain_node");
    assert_eq!(
        payload["runtime_targets"],
        serde_json::json!(["node", "browser"])
    );
    assert_eq!(payload["files_to_edit"][0]["path"], "apps/api/src/index.ts");

    let members = payload["members"].as_array().expect("members array");
    assert_eq!(
        members.len(),
        2,
        "only runtime workspaces should be emitted: {payload:#}"
    );
    assert!(
        members.iter().all(|member| member["path"] != "."),
        "workspace aggregator root must not receive a runtime setup recipe: {payload:#}"
    );
    assert_eq!(members[0]["path"], "apps/api");
    assert_eq!(members[0]["snippets"][0]["path"], "apps/api/src/index.ts");
    assert_eq!(members[1]["path"], "apps/web");
    assert_eq!(members[1]["framework_detected"], "vite");
}

#[test]
fn setup_json_skips_vite_build_only_workspace_packages() {
    let dir = tempdir().expect("tempdir should be created");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{
  "name": "repo",
  "private": true,
  "packageManager": "pnpm@9.0.0",
  "workspaces": ["packages/*"],
  "scripts": { "dev": "turbo dev" },
  "devDependencies": { "vite": "^6.0.0" }
}"#,
    )
    .expect("root package.json should be written");

    let library_dir = dir.path().join("packages/ui");
    std::fs::create_dir_all(library_dir.join("src")).expect("library src dir should be created");
    std::fs::write(
        library_dir.join("package.json"),
        r#"{"name":"@repo/ui","scripts":{"build":"vite build"},"devDependencies":{"vite":"^6.0.0"}}"#,
    )
    .expect("library package.json should be written");

    let payload = build_setup_json(dir.path(), false);

    assert_eq!(payload["framework_detected"], "unknown");
    assert_eq!(payload["runtime_targets"], serde_json::json!([]));
    assert_eq!(payload["files_to_edit"], serde_json::json!([]));
    assert_eq!(payload["snippets"], serde_json::json!([]));
    assert_eq!(payload["members"], serde_json::json!([]));
    assert!(
        payload["warnings"]
            .as_array()
            .is_some_and(|warnings| warnings.iter().any(|warning| warning
                .as_str()
                .is_some_and(|warning| warning.contains("No runtime workspace members")))),
        "empty runtime workspace detection should be explicit: {payload:#}"
    );
}

#[test]
fn recipe_contents_uses_detected_package_manager_scripts() {
    let context = CoverageSetupContext {
        framework: FrameworkKind::SvelteKit,
        package_manager: Some(PackageManager::Pnpm),
        has_build_script: true,
        has_start_script: false,
        has_preview_script: true,
        node_entry_path: "src/server.ts".to_owned(),
    };

    let recipe = recipe_contents(&context);

    assert!(recipe.contains("`pnpm build`"));
    assert!(recipe.contains("`NODE_V8_COVERAGE=./coverage pnpm preview`"));
}

#[test]
fn recipe_contents_mentions_upload_inventory_ci_step() {
    let context = CoverageSetupContext {
        framework: FrameworkKind::SvelteKit,
        package_manager: Some(PackageManager::Pnpm),
        has_build_script: true,
        has_start_script: false,
        has_preview_script: true,
        node_entry_path: "src/server.ts".to_owned(),
    };
    let recipe = recipe_contents(&context);
    assert!(
        recipe.contains("fallow coverage upload-inventory"),
        "recipe missing upload-inventory CI instruction:\n{recipe}"
    );
    assert!(recipe.contains("FALLOW_API_KEY"));
}

#[test]
fn recipe_contents_mentions_upload_inventory_without_build_script() {
    let context = CoverageSetupContext {
        framework: FrameworkKind::PlainNode,
        package_manager: Some(PackageManager::Npm),
        has_build_script: false,
        has_start_script: false,
        has_preview_script: false,
        node_entry_path: "src/server.ts".to_owned(),
    };
    let recipe = recipe_contents(&context);
    assert!(recipe.contains("fallow coverage upload-inventory"));
}

#[test]
fn setup_json_is_deterministic_and_does_not_write_files() {
    let dir = tempdir().expect("tempdir should be created");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"demo","packageManager":"pnpm@9.0.0","dependencies":{"next":"^16.0.0"}}"#,
    )
    .expect("package.json should be written");

    let payload = build_setup_json(dir.path(), false);

    assert_eq!(payload["schema_version"], "1");
    assert_eq!(payload["framework_detected"], "nextjs");
    assert_eq!(payload["package_manager"], "pnpm");
    assert_eq!(payload["config_written"], serde_json::Value::Null);
    assert_eq!(
        payload["runtime_targets"],
        serde_json::json!(["node", "browser"])
    );
    assert_eq!(payload["commands"][0], "pnpm add @fallow-cli/beacon");
    assert_eq!(payload["commands"][1], "pnpm add -D @fallow-cli/fallow-cov");
    assert_eq!(payload["files_to_edit"][0]["path"], "instrumentation.ts");
    assert!(
        payload["snippets"][0]["content"]
            .as_str()
            .is_some_and(|content| content.contains("createNodeBeacon"))
    );
    assert!(
        !dir.path().join("docs/collect-coverage.md").exists(),
        "JSON setup must not write the human recipe"
    );
    assert!(
        !dir.path().join(".fallow").exists(),
        "JSON setup must not write setup state or lock files"
    );
}

#[test]
fn setup_json_respects_explicit_style() {
    let payload = serde_json::json!({"kind": "coverage-setup", "commands": []});
    let compact = render_setup_json(&payload, crate::json_style::JsonStyle::Compact)
        .expect("compact coverage setup should serialize");
    let pretty = render_setup_json(&payload, crate::json_style::JsonStyle::Pretty)
        .expect("pretty coverage setup should serialize");

    assert!(
        !compact.contains('\n'),
        "compact JSON must stay on one line"
    );
    assert!(pretty.contains("\n  \""), "pretty JSON must be indented");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&compact).unwrap(),
        serde_json::from_str::<serde_json::Value>(&pretty).unwrap(),
    );
}

#[test]
fn run_setup_json_does_not_write_state_lock_or_recipe() {
    let dir = tempdir().expect("tempdir should be created");

    let exit = run_setup(
        SetupArgs {
            json: true,
            ..SetupArgs::default()
        },
        dir.path(),
        crate::json_style::JsonStyle::Compact,
    );

    assert_eq!(exit, std::process::ExitCode::SUCCESS);
    assert!(!dir.path().join(".fallow").exists());
    assert!(!dir.path().join("docs/collect-coverage.md").exists());
}

#[test]
fn corrupt_setup_state_falls_back_to_default() {
    let dir = tempdir().expect("tempdir should be created");
    std::fs::create_dir_all(dir.path().join(".fallow")).expect(".fallow should be created");
    std::fs::write(setup_state_path(dir.path()), "{not json")
        .expect("corrupt setup state should be written");

    let state = load_setup_state(dir.path());

    assert_eq!(state.schema_version, 1);
    assert!(state.license.is_none());
    assert!(state.sidecar.is_none());
    assert!(state.recipe.is_none());
}

#[test]
fn sidecar_state_requires_matching_file_checksum() {
    let dir = tempdir().expect("tempdir should be created");
    let sidecar = write_fake_sidecar(dir.path(), b"first");
    let mut state = super::CoverageSetupState::default();
    record_sidecar_state(&mut state, sidecar.clone()).expect("sidecar state should record");

    assert!(sidecar_state_is_current(&state, dir.path()));

    std::fs::write(&sidecar, b"second").expect("sidecar should be mutated");

    assert!(!sidecar_state_is_current(&state, dir.path()));
}

#[test]
fn recipe_state_requires_current_context_and_file_checksum() {
    let dir = tempdir().expect("tempdir should be created");
    let vite_context = CoverageSetupContext {
        framework: FrameworkKind::ViteBrowser,
        package_manager: Some(PackageManager::Pnpm),
        has_build_script: true,
        has_start_script: false,
        has_preview_script: true,
        node_entry_path: "src/main.ts".to_owned(),
    };
    let next_context = CoverageSetupContext {
        framework: FrameworkKind::NextJs,
        package_manager: Some(PackageManager::Pnpm),
        has_build_script: true,
        has_start_script: true,
        has_preview_script: false,
        node_entry_path: "src/main.ts".to_owned(),
    };
    let vite_recipe = recipe_contents(&vite_context);
    let recipe_path = write_recipe(dir.path(), &vite_recipe).expect("recipe should write");
    let mut state = super::CoverageSetupState::default();
    record_recipe_state(&mut state, recipe_path.clone(), &vite_context, &vite_recipe);

    assert!(recipe_state_is_current(
        &state,
        dir.path(),
        &vite_context,
        &vite_recipe
    ));
    assert!(!recipe_state_is_current(
        &state,
        dir.path(),
        &next_context,
        &recipe_contents(&next_context)
    ));

    std::fs::write(&recipe_path, "manual edit").expect("recipe should be mutated");

    assert!(!recipe_state_is_current(
        &state,
        dir.path(),
        &vite_context,
        &vite_recipe
    ));
}

#[test]
fn recipe_state_requires_path_under_current_root() {
    let old_dir = tempdir().expect("old tempdir should be created");
    let new_dir = tempdir().expect("new tempdir should be created");
    let context = CoverageSetupContext {
        framework: FrameworkKind::PlainNode,
        package_manager: Some(PackageManager::Npm),
        has_build_script: false,
        has_start_script: false,
        has_preview_script: false,
        node_entry_path: "src/server.ts".to_owned(),
    };
    let recipe = recipe_contents(&context);
    let old_recipe_path = write_recipe(old_dir.path(), &recipe).expect("old recipe should write");
    let mut state = super::CoverageSetupState::default();
    record_recipe_state(&mut state, old_recipe_path, &context, &recipe);

    assert!(!recipe_state_is_current(
        &state,
        new_dir.path(),
        &context,
        &recipe
    ));
}

#[test]
fn setup_context_fingerprint_changes_when_recipe_inputs_change() {
    let base = CoverageSetupContext {
        framework: FrameworkKind::PlainNode,
        package_manager: Some(PackageManager::Npm),
        has_build_script: false,
        has_start_script: false,
        has_preview_script: false,
        node_entry_path: "src/server.ts".to_owned(),
    };
    let changed = CoverageSetupContext {
        package_manager: Some(PackageManager::Bun),
        ..base.clone()
    };

    assert_ne!(
        setup_context_fingerprint(&base),
        setup_context_fingerprint(&changed)
    );
}

#[test]
fn setup_lock_reports_contention() {
    let dir = tempdir().expect("tempdir should be created");
    let _first =
        super::CoverageSetupLock::try_acquire(dir.path()).expect("first lock should acquire");

    let err = super::CoverageSetupLock::try_acquire(dir.path())
        .expect_err("second lock should report contention");

    assert!(err.contains("another fallow coverage setup is already running"));
    assert!(err.replace('\\', "/").contains(".fallow/setup.lock"));
}

#[cfg(unix)]
#[test]
fn run_setup_resumes_valid_recipe_state_without_rewriting() {
    use super::{hash_file, save_setup_state};
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().expect("tempdir should be created");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"api","packageManager":"npm@10.0.0","dependencies":{"elysia":"^1.0.0"}}"#,
    )
    .expect("package.json should be written");
    let sidecar = write_fake_sidecar(root, b"sidecar");
    let context = super::detect_setup_context(root);
    let recipe = recipe_contents(&context);
    let recipe_path = write_recipe(root, &recipe).expect("recipe should be written");
    let mut state = super::CoverageSetupState::default();
    record_sidecar_state(&mut state, sidecar).expect("sidecar should record");
    record_recipe_state(&mut state, recipe_path.clone(), &context, &recipe);
    save_setup_state(root, &state).expect("state should save");

    let docs_dir = root.join("docs");
    let original_mode = std::fs::metadata(&docs_dir)
        .expect("docs metadata should be readable")
        .permissions()
        .mode();
    let mut readonly = std::fs::metadata(&docs_dir)
        .expect("docs metadata should be readable")
        .permissions();
    readonly.set_mode(0o555);
    std::fs::set_permissions(&docs_dir, readonly).expect("docs should become read-only");

    let exit = run_setup(
        SetupArgs {
            non_interactive: true,
            ..SetupArgs::default()
        },
        root,
        crate::json_style::JsonStyle::Compact,
    );

    let mut restored = std::fs::metadata(&docs_dir)
        .expect("docs metadata should be readable")
        .permissions();
    restored.set_mode(original_mode & 0o7777);
    std::fs::set_permissions(&docs_dir, restored).expect("docs permissions should restore");

    assert_eq!(exit, std::process::ExitCode::SUCCESS);
    assert_eq!(
        std::fs::read_to_string(&recipe_path).expect("recipe should be readable"),
        recipe
    );
    assert_eq!(
        hash_file(&recipe_path).as_deref(),
        state.recipe.as_ref().map(|r| r.checksum.as_str())
    );
}

#[test]
fn setup_json_explain_includes_meta_without_bumping_schema_version() {
    let dir = tempdir().expect("tempdir should be created");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"demo","packageManager":"bun@1.2.0","dependencies":{"elysia":"^1.0.0"}}"#,
    )
    .expect("package.json should be written");

    let payload = build_setup_json(dir.path(), true);

    assert_eq!(payload["schema_version"], "1");
    assert_eq!(
        payload["_meta"]["docs_url"],
        "https://fallow.tools/docs/cli/coverage/#agent-readable-json"
    );
    assert!(
        payload["_meta"]["field_definitions"]
            .as_object()
            .is_some_and(|fields| fields.contains_key("members[]"))
    );
    assert!(
        payload["_meta"]["enums"]
            .as_object()
            .is_some_and(|enums| enums.contains_key("runtime_targets"))
    );
}

fn write_fake_sidecar(root: &Path, bytes: &[u8]) -> PathBuf {
    let bin_dir = root.join("node_modules").join(".bin");
    std::fs::create_dir_all(&bin_dir).expect("sidecar bin dir should be created");
    let path = if cfg!(windows) {
        bin_dir.join("fallow-cov.cmd")
    } else {
        bin_dir.join("fallow-cov")
    };
    std::fs::write(&path, bytes).expect("sidecar should be written");
    path
}

#[test]
fn detect_coverage_artifact_finds_nyc_output_istanbul_file() {
    let dir = tempdir().expect("tempdir should be created");
    let nyc_dir = dir.path().join(".nyc_output");
    std::fs::create_dir_all(&nyc_dir).expect("nyc dir should be created");
    let coverage_file = nyc_dir.join("coverage-final.json");
    std::fs::write(&coverage_file, "{}").expect("coverage file should be written");

    assert_eq!(detect_coverage_artifact(dir.path()), Some(coverage_file));
}
