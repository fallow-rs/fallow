use super::*;

#[test]
fn zero_config_is_ready_with_stable_order() {
    let root = tempfile::tempdir().expect("temp root");
    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    assert_eq!(output.status, DoctorStatus::Warn);
    assert_eq!(output.root, ".");
    assert_eq!(
        output
            .checks
            .iter()
            .map(|check| check.id)
            .collect::<Vec<_>>(),
        vec![
            DoctorCheckId::Root,
            DoctorCheckId::Config,
            DoctorCheckId::Workspaces,
            DoctorCheckId::Plugins,
            DoctorCheckId::TypeAware,
            DoctorCheckId::Dependencies,
            DoctorCheckId::Cache,
            DoctorCheckId::GraphCache,
            DoctorCheckId::MutationRunner,
        ]
    );
    assert_eq!(
        output.checks[1].message,
        "Zero-config defaults resolved successfully."
    );
    assert_eq!(output.checks[4].status, DoctorCheckStatus::Skipped);
}

#[test]
fn mutation_runner_check_reads_the_vitest_of_a_listed_toolchain() {
    let root = tempfile::tempdir().expect("temp root");
    let toolchain = root.path().join("node_modules/toolchain");
    let vitest = toolchain.join("node_modules/vitest");
    std::fs::create_dir_all(&vitest).expect("create vitest dir");
    std::fs::write(
        root.path().join("package.json"),
        r#"{"devDependencies":{"toolchain":"1.0.0"}}"#,
    )
    .expect("write root manifest");
    std::fs::write(
        toolchain.join("package.json"),
        r#"{"name":"toolchain","dependencies":{"vitest":"5.1.1"}}"#,
    )
    .expect("write toolchain manifest");
    std::fs::write(
        vitest.join("package.json"),
        r#"{"name":"vitest","version":"5.1.1"}"#,
    )
    .expect("write vitest manifest");

    let check = mutation_runner_check(root.path(), fallow_config::MutationRunner::Auto);

    assert_eq!(check.status, DoctorCheckStatus::Pass, "{}", check.message);
    assert!(
        check.message.starts_with("vitest 5.1.1 "),
        "{}",
        check.message
    );
}

/// Issue: doctor reported `pass` on a tree that had never been installed,
/// while the command exists to diagnose exactly that.
#[test]
fn missing_node_modules_warns_without_failing() {
    let root = tempfile::tempdir().expect("temp root");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    let dependencies = output
        .checks
        .iter()
        .find(|check| check.id == DoctorCheckId::Dependencies)
        .expect("dependencies check is reported");
    assert_eq!(dependencies.status, DoctorCheckStatus::Warn);
    assert!(!dependencies.required);
    assert_eq!(output.status, DoctorStatus::Warn);
    assert!(
        dependencies
            .remediation
            .as_ref()
            .is_some_and(|remediation| remediation.mutating),
        "installing dependencies mutates the project"
    );
}

fn dependency_remediation_command(root: &Path) -> String {
    run_doctor_with_discovery(
        &DoctorOptions {
            root,
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    )
    .checks
    .into_iter()
    .find(|check| check.id == DoctorCheckId::Dependencies)
    .and_then(|check| check.remediation)
    .map(|remediation| remediation.command)
    .expect("dependencies remediation is reported")
}

/// Issue: doctor told pnpm, Yarn and Bun projects to run `npm install`,
/// which writes a foreign lockfile.
#[test]
fn dependency_remediation_uses_the_project_package_manager() {
    for (lockfile, expected) in [
        ("pnpm-lock.yaml", "pnpm install"),
        ("yarn.lock", "yarn install"),
        ("bun.lock", "bun install"),
        ("package-lock.json", "npm install"),
    ] {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(root.path().join(lockfile), "").expect("write lockfile");
        assert_eq!(
            dependency_remediation_command(root.path()),
            expected,
            "{lockfile}"
        );
    }
}

#[test]
fn dependency_remediation_prefers_the_package_manager_field() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(
        root.path().join("package.json"),
        r#"{"packageManager":"yarn@4.5.0"}"#,
    )
    .expect("write package.json");
    std::fs::write(root.path().join("package-lock.json"), "").expect("write lockfile");
    assert_eq!(dependency_remediation_command(root.path()), "yarn install");
}

#[test]
fn dependency_remediation_defaults_to_npm_without_a_signal() {
    let root = tempfile::tempdir().expect("temp root");
    assert_eq!(dependency_remediation_command(root.path()), "npm install");
}

/// Review finding: a workspace package root fell back to `npm install`
/// although the monorepo root declares pnpm.
#[test]
fn dependency_remediation_finds_the_monorepo_package_manager() {
    let root = tempfile::tempdir().expect("temp root");
    let package = root.path().join("packages").join("app");
    std::fs::create_dir_all(&package).expect("create package");
    std::fs::create_dir(root.path().join(".git")).expect("create .git");
    std::fs::write(
        root.path().join("package.json"),
        r#"{"private":true,"packageManager":"pnpm@10.0.0"}"#,
    )
    .expect("write root package.json");
    std::fs::write(package.join("package.json"), r#"{"name":"app"}"#).expect("write package.json");
    assert_eq!(dependency_remediation_command(&package), "pnpm install");
}

#[test]
fn type_aware_remediation_uses_the_project_package_manager() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(
        root.path().join(".fallowrc.json"),
        r#"{"typeAware":{"enabled":true}}"#,
    )
    .expect("write config");
    std::fs::write(root.path().join("pnpm-lock.yaml"), "").expect("write lockfile");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    assert_eq!(
        output.checks[4]
            .remediation
            .as_ref()
            .map(|remediation| remediation.command.as_str()),
        Some(
            format!(
                "pnpm add -D fallow-type-aware@{}",
                env!("CARGO_PKG_VERSION")
            )
            .as_str()
        )
    );
}

/// Yarn 1 refuses `yarn add -D` at a workspace root without `-W`
/// (fallow-rs/fallow#4519).
#[test]
fn type_aware_remediation_adds_the_yarn_classic_workspace_root_flag() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(
        root.path().join(".fallowrc.json"),
        r#"{"typeAware":{"enabled":true}}"#,
    )
    .expect("write config");
    std::fs::write(
        root.path().join("package.json"),
        r#"{"private":true,"workspaces":["packages/*"]}"#,
    )
    .expect("write package.json");
    std::fs::write(root.path().join("yarn.lock"), "# yarn lockfile v1\n").expect("write lockfile");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    assert_eq!(
        output.checks[4]
            .remediation
            .as_ref()
            .map(|remediation| remediation.command.as_str()),
        Some(
            format!(
                "yarn add -D -W fallow-type-aware@{}",
                env!("CARGO_PKG_VERSION")
            )
            .as_str()
        )
    );
}

#[test]
fn installed_dependencies_pass_the_dependency_check() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::create_dir(root.path().join("node_modules")).expect("create node_modules");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    let dependencies = output
        .checks
        .iter()
        .find(|check| check.id == DoctorCheckId::Dependencies)
        .expect("dependencies check is reported");
    assert_eq!(dependencies.status, DoctorCheckStatus::Pass);
    assert_eq!(output.status, DoctorStatus::Pass);
}

#[test]
fn cache_repairs_use_the_inspected_config() {
    let temp = tempfile::tempdir().expect("create fixture");
    let root = temp.path();
    let path = root.join("ci.json");
    std::fs::write(&path, r#"{"cache":{"dir":"cache-ci"}}"#).expect("write config");
    let cache = root.join("cache-ci");
    std::fs::create_dir(&cache).expect("create cache directory");
    for blob in ["cache.bin", "graph-cache.bin"] {
        std::fs::write(cache.join(blob), b"invalid cache").expect("write corrupt cache");
    }
    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root,
            config_path: Some(&path),
        },
        &|_| Err("missing companion".to_owned()),
    );
    for id in [DoctorCheckId::Cache, DoctorCheckId::GraphCache] {
        let check = output
            .checks
            .iter()
            .find(|check| check.id == id)
            .expect("cache check");
        assert_eq!(check.status, DoctorCheckStatus::Warn);
        assert_eq!(
            check
                .remediation
                .as_ref()
                .expect("cache repair command")
                .command,
            "fallow dead-code --quiet --config=ci.json"
        );
    }
}

#[cfg(unix)]
#[test]
fn remediation_omits_a_config_with_a_literal_backslash() {
    let temp = tempfile::tempdir().expect("temp root");
    let config = temp.path().join(r"ci\settings.json");
    std::fs::write(&config, "{}").expect("write config");
    assert!(remediation_with_config("fallow dead-code", temp.path(), Some(&config)).is_none());
}

#[test]
fn a_cache_repair_keeps_manual_cache_and_config_inputs() {
    let temp = tempfile::tempdir().expect("create fixture");
    let root = temp.path();
    let config = root.join("CI checks.json");
    std::fs::write(&config, r#"{"cache":{"dir":"configured"}}"#).expect("write config");
    for override_dir in [None, Some(root.join("override"))] {
        let cache = override_dir
            .clone()
            .unwrap_or_else(|| root.join("configured"));
        std::fs::create_dir_all(&cache).expect("create cache directory");
        std::fs::write(cache.join("cache.bin"), b"invalid cache").expect("write corrupt cache");
        let output = run_doctor_with_cache_dir_and_discovery(
            &DoctorOptions {
                root,
                config_path: Some(&config),
            },
            override_dir.as_deref(),
            &|_| Err("missing companion".to_owned()),
        );
        let check = output
            .checks
            .iter()
            .find(|check| check.id == DoctorCheckId::Cache)
            .expect("cache check");
        assert_eq!(check.status, DoctorCheckStatus::Warn);
        assert!(check.remediation.is_none());
        assert!(check.message.contains("--config"));
        if override_dir.is_some() {
            assert!(check.message.contains("FALLOW_CACHE_DIR"));
        }
    }
}

/// A project with no cache yet is not a problem; only a cache that exists
/// and would be thrown away is worth a warning.
#[test]
fn absent_cache_passes_the_cache_check() {
    let root = tempfile::tempdir().expect("temp root");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    let cache = output
        .checks
        .iter()
        .find(|check| check.id == DoctorCheckId::Cache)
        .expect("cache check is reported");
    assert_eq!(cache.status, DoctorCheckStatus::Pass);
    assert!(!cache.required);
}

/// Framing this build never wrote is corruption, and must not be reported
/// as a format bump.
///
/// "cache format version changed" sends the reader to look for an upgrade;
/// the fix for a blob with no fallow framing is to delete it. The upgrade
/// message has its own test below, on a blob that keeps the framing and
/// moves only the declared version.
#[test]
fn a_corrupt_cache_warns_as_undecodable_with_a_size_that_shows() {
    let root = tempfile::tempdir().expect("temp root");
    let cache_dir = root.path().join(".fallow");
    std::fs::create_dir_all(&cache_dir).expect("create cache dir");
    std::fs::write(
        cache_dir.join("cache.bin"),
        b"not-a-payload-this-build-wrote",
    )
    .expect("write foreign cache");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    let cache = output
        .checks
        .iter()
        .find(|check| check.id == DoctorCheckId::Cache)
        .expect("cache check is reported");
    assert_eq!(cache.status, DoctorCheckStatus::Warn);
    assert!(!cache.required);
    assert!(
        cache.message.contains("could not be decoded"),
        "a blob without fallow's framing is corrupt, not stale: {}",
        cache.message
    );
    assert!(
        !cache.message.contains("cache format version changed"),
        "corruption must not send the reader hunting for an upgrade: {}",
        cache.message
    );
    assert!(
        cache.message.contains("30 bytes"),
        "a small blob must report a size that shows it exists, not 0.0 MB: {}",
        cache.message
    );
    assert_ne!(
        output.status,
        DoctorStatus::Fail,
        "a refused cache costs time, not correctness"
    );
}

/// The message a user reads after upgrading. A blob that keeps fallow's
/// framing and declares a version this build does not write is stale, and
/// costs exactly one rebuild.
#[test]
fn a_cache_from_an_older_format_version_warns_as_a_format_change() {
    let root = tempfile::tempdir().expect("temp root");
    let cache_dir = root.path().join(".fallow");
    std::fs::create_dir_all(&cache_dir).expect("create cache dir");
    // `FLWX` plus a little-endian version, the framing `fallow-extract`
    // writes. Version 1 is far below anything this build produces, so the
    // blob is stale rather than foreign.
    let mut framed = b"FLWX".to_vec();
    framed.extend_from_slice(&1_u32.to_le_bytes());
    framed.extend_from_slice(b"payload-from-an-older-release");
    std::fs::write(cache_dir.join("cache.bin"), framed).expect("write stale cache");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    let cache = output
        .checks
        .iter()
        .find(|check| check.id == DoctorCheckId::Cache)
        .expect("cache check is reported");
    assert_eq!(cache.status, DoctorCheckStatus::Warn);
    assert!(
        cache.message.contains("cache format version changed"),
        "{}",
        cache.message
    );
    assert!(
        !cache.message.contains("could not be decoded"),
        "an upgrade must not be reported as corruption: {}",
        cache.message
    );
}

/// The graph blob is the larger of the two persisted caches and is reused
/// independently of the extraction blob, so a doctor that only looked at
/// the extraction cache called a project healthy while the expensive half
/// was discarded on every run.
#[test]
fn absent_graph_cache_passes_its_own_check() {
    let root = tempfile::tempdir().expect("temp root");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    let graph_cache = output
        .checks
        .iter()
        .find(|check| check.id == DoctorCheckId::GraphCache)
        .expect("graph cache check is reported");
    assert_eq!(graph_cache.status, DoctorCheckStatus::Pass);
    assert!(!graph_cache.required);
}

#[test]
fn a_corrupt_graph_cache_warns_as_undecodable_with_a_size_that_shows() {
    let root = tempfile::tempdir().expect("temp root");
    let cache_dir = root.path().join(".fallow");
    std::fs::create_dir_all(&cache_dir).expect("create cache dir");
    std::fs::write(
        cache_dir.join("graph-cache.bin"),
        b"not-a-payload-this-build-wrote",
    )
    .expect("write foreign graph cache");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    let graph_cache = output
        .checks
        .iter()
        .find(|check| check.id == DoctorCheckId::GraphCache)
        .expect("graph cache check is reported");
    assert_eq!(graph_cache.status, DoctorCheckStatus::Warn);
    assert!(!graph_cache.required);
    assert!(
        graph_cache.message.contains("could not be decoded"),
        "a blob without fallow's framing is corrupt, not stale: {}",
        graph_cache.message
    );
    assert!(
        graph_cache.message.contains("30 bytes"),
        "a small blob must report a size that shows it exists, not 0.0 MB: {}",
        graph_cache.message
    );
    assert_ne!(
        output.status,
        DoctorStatus::Fail,
        "a refused cache costs time, not correctness"
    );
}

#[test]
fn invalid_config_returns_complete_failed_report() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(root.path().join(".fallowrc.json"), "{").expect("write invalid config");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    // Look checks up by id, not by position: the report's completeness is
    // the contract, the order in which the rows happen to be pushed is not.
    let check = |id: DoctorCheckId| {
        output
            .checks
            .iter()
            .find(|check| check.id == id)
            .unwrap_or_else(|| panic!("{id:?} is reported even when config fails"))
    };

    assert_eq!(output.status, DoctorStatus::Fail);
    assert_eq!(
        output.checks.len(),
        9,
        "a failing config must not truncate the report"
    );
    assert_eq!(check(DoctorCheckId::Config).status, DoctorCheckStatus::Fail);
    assert_eq!(
        check(DoctorCheckId::Workspaces).status,
        DoctorCheckStatus::Skipped
    );
    assert!(
        !check(DoctorCheckId::Config)
            .message
            .contains(&root.path().display().to_string()),
        "the failure must not echo the host path"
    );
}

#[test]
fn optional_missing_type_aware_companion_warns() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(
        root.path().join(".fallowrc.json"),
        r#"{"typeAware":{"enabled":true}}"#,
    )
    .expect("write config");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    assert_eq!(output.status, DoctorStatus::Warn);
    assert_eq!(output.checks[4].status, DoctorCheckStatus::Warn);
    assert!(!output.checks[4].required);
}

#[test]
fn required_missing_type_aware_companion_fails() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(
        root.path().join(".fallowrc.json"),
        r#"{"typeAware":{"enabled":true,"require":"complete"}}"#,
    )
    .expect("write config");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    assert_eq!(output.status, DoctorStatus::Fail);
    assert_eq!(output.checks[4].status, DoctorCheckStatus::Fail);
    assert!(output.checks[4].required);
    assert_eq!(
        output.checks[4]
            .remediation
            .as_ref()
            .map(|remediation| remediation.mutating),
        Some(true)
    );
}

#[test]
fn invalid_root_does_not_echo_the_host_path() {
    let missing = Path::new("/definitely/missing/fallow-doctor-private-root");
    let output = run_doctor(&DoctorOptions {
        root: missing,
        config_path: None,
    });

    assert_eq!(output.status, DoctorStatus::Fail);
    assert_eq!(output.root, ".");
    assert!(output.checks[0].message.contains("--root"));
    assert_eq!(
        output.checks[0].message,
        "Project root is not accessible. Set --root to an existing, readable directory."
    );
    assert!(
        output
            .checks
            .iter()
            .all(|check| !check.message.contains("fallow-doctor-private-root"))
    );
}

#[test]
fn external_config_does_not_echo_its_host_path() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::create_dir(root.path().join("node_modules")).expect("create node_modules");
    let config_dir = tempfile::tempdir().expect("temp config dir");
    let config_path = config_dir.path().join("external.fallowrc.json");
    std::fs::write(&config_path, "{}").expect("write config");

    let output = run_doctor(&DoctorOptions {
        root: root.path(),
        config_path: Some(&config_path),
    });

    assert_eq!(output.status, DoctorStatus::Pass);
    assert_eq!(
        output.checks[1].message,
        "The explicitly selected configuration resolved successfully."
    );
    assert!(
        !output.checks[1]
            .message
            .contains(&config_dir.path().display().to_string())
    );
}

#[test]
fn parent_relative_external_config_stays_private() {
    let sandbox = tempfile::tempdir().expect("temp sandbox");
    let root = sandbox.path().join("project");
    std::fs::create_dir(&root).expect("create project root");
    std::fs::create_dir(root.join("node_modules")).expect("create node_modules");
    let config_path = root.join("../customer-secret.json");
    std::fs::write(&config_path, "{}").expect("write config");

    let output = run_doctor(&DoctorOptions {
        root: &root,
        config_path: Some(&config_path),
    });

    assert_eq!(output.status, DoctorStatus::Pass);
    assert_eq!(
        output.checks[1].message,
        "The explicitly selected configuration resolved successfully."
    );
    assert!(!output.checks[1].message.contains("customer-secret"));
}

#[cfg(unix)]
#[test]
fn successful_config_below_external_symlink_stays_private() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::create_dir(root.path().join("node_modules")).expect("create node_modules");
    let external = tempfile::tempdir().expect("external root");
    std::fs::write(external.path().join("config.json"), "{}").expect("write config");
    std::os::unix::fs::symlink(external.path(), root.path().join("external"))
        .expect("create external symlink");
    let config_path = root.path().join("external/config.json");

    let output = run_doctor(&DoctorOptions {
        root: root.path(),
        config_path: Some(&config_path),
    });

    assert_eq!(output.status, DoctorStatus::Pass);
    assert_eq!(
        output.checks[1].message,
        "The explicitly selected configuration resolved successfully."
    );
    assert!(!output.checks[1].message.contains("external/config.json"));
}

#[test]
fn relative_explicit_config_is_preserved_in_remediation() {
    let root = tempfile::tempdir().expect("temp root");
    let config_path = root.path().join("custom.json");
    std::fs::write(&config_path, "{").expect("write invalid config");

    let output = run_doctor(&DoctorOptions {
        root: root.path(),
        config_path: Some(&config_path),
    });

    assert_eq!(output.status, DoctorStatus::Fail);
    assert_eq!(
        output.checks[1]
            .remediation
            .as_ref()
            .map(|remediation| remediation.command.as_str()),
        Some("fallow config --config=custom.json")
    );
}

#[test]
fn nested_missing_project_relative_config_is_preserved_in_remediation() {
    let root = tempfile::tempdir().expect("temp root");
    let config_path = root.path().join("missing-dir/missing.json");

    let output = run_doctor(&DoctorOptions {
        root: root.path(),
        config_path: Some(&config_path),
    });

    assert_eq!(output.status, DoctorStatus::Fail);
    assert_eq!(
        output.checks[1]
            .remediation
            .as_ref()
            .map(|remediation| remediation.command.as_str()),
        Some("fallow config --config=missing-dir/missing.json")
    );
    assert!(
        !output.checks[1]
            .message
            .contains("same explicit --config value")
    );
}

#[test]
fn leading_dash_config_name_is_bound_to_its_option() {
    let root = tempfile::tempdir().expect("temp root");
    let config_path = root.path().join("-missing.json");

    let output = run_doctor(&DoctorOptions {
        root: root.path(),
        config_path: Some(&config_path),
    });

    assert_eq!(
        output.checks[1]
            .remediation
            .as_ref()
            .map(|remediation| remediation.command.as_str()),
        Some("fallow config --config=-missing.json")
    );
}

#[test]
fn config_path_equal_to_root_never_renders_an_empty_argument() {
    let root = tempfile::tempdir().expect("temp root");

    let output = run_doctor(&DoctorOptions {
        root: root.path(),
        config_path: Some(root.path()),
    });

    let command = output.checks[1]
        .remediation
        .as_ref()
        .map(|remediation| remediation.command.as_str());
    assert_eq!(command, Some("fallow config --config=."));
    assert_ne!(command, Some("fallow config --config="));
}

#[test]
fn missing_config_traversal_outside_root_stays_private() {
    let sandbox = tempfile::tempdir().expect("temp sandbox");
    let root = sandbox.path().join("project");
    std::fs::create_dir(&root).expect("create project root");
    let config_path = root.join("../missing.json");

    let output = run_doctor(&DoctorOptions {
        root: &root,
        config_path: Some(&config_path),
    });

    assert_eq!(output.status, DoctorStatus::Fail);
    assert!(output.checks[1].remediation.is_none());
    assert!(
        output.checks[1]
            .message
            .contains("same explicit --config value")
    );
    assert!(!output.checks[1].message.contains("missing.json"));
}

#[cfg(unix)]
#[test]
fn missing_config_below_external_symlink_stays_private() {
    let root = tempfile::tempdir().expect("temp root");
    let external = tempfile::tempdir().expect("external root");
    std::os::unix::fs::symlink(external.path(), root.path().join("external"))
        .expect("create external symlink");
    let config_path = root.path().join("external/missing-dir/missing.json");

    let output = run_doctor(&DoctorOptions {
        root: root.path(),
        config_path: Some(&config_path),
    });

    assert_eq!(output.status, DoctorStatus::Fail);
    assert!(output.checks[1].remediation.is_none());
    assert!(
        output.checks[1]
            .message
            .contains("same explicit --config value")
    );
    assert!(!output.checks[1].message.contains("missing-dir"));
}

#[test]
fn failed_external_config_requires_reusing_the_private_value() {
    let root = tempfile::tempdir().expect("temp root");
    let config_dir = tempfile::tempdir().expect("temp config dir");
    let config_path = config_dir.path().join("external.json");
    std::fs::write(&config_path, "{").expect("write invalid config");

    let output = run_doctor(&DoctorOptions {
        root: root.path(),
        config_path: Some(&config_path),
    });

    assert_eq!(output.status, DoctorStatus::Fail);
    assert!(output.checks[1].remediation.is_none());
    assert!(
        output.checks[1]
            .message
            .contains("same explicit --config value")
    );
    assert!(
        !output.checks[1]
            .message
            .contains(&config_dir.path().display().to_string())
    );
}

#[test]
fn missing_explicit_plugin_is_a_required_failure() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(
        root.path().join(".fallowrc.json"),
        r#"{"plugins":["missing-plugin.json"]}"#,
    )
    .expect("write config");

    let output = run_doctor(&DoctorOptions {
        root: root.path(),
        config_path: None,
    });

    assert_eq!(output.status, DoctorStatus::Fail);
    assert_eq!(output.checks[3].status, DoctorCheckStatus::Fail);
    assert!(output.checks[3].required);
    assert!(
        output.checks[3]
            .message
            .contains("1 unresolved configured resource")
    );
    assert!(!output.checks[3].message.contains("missing-plugin.json"));
}

#[test]
fn malformed_explicit_plugin_is_a_required_failure() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(
        root.path().join(".fallowrc.json"),
        r#"{"plugins":["broken.json"]}"#,
    )
    .expect("write config");
    std::fs::write(root.path().join("broken.json"), "{").expect("write plugin");

    let output = run_doctor(&DoctorOptions {
        root: root.path(),
        config_path: None,
    });

    assert_eq!(output.status, DoctorStatus::Fail);
    assert_eq!(output.checks[3].status, DoctorCheckStatus::Fail);
    assert!(output.checks[3].required);
    assert!(
        output.checks[3]
            .message
            .contains("1 unresolved configured resource")
    );
    assert!(!output.checks[3].message.contains("broken.json"));
}

#[test]
fn explicit_plugin_directory_without_definitions_is_a_required_failure() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::create_dir(root.path().join("plugins")).expect("create plugin directory");
    std::fs::write(
        root.path().join(".fallowrc.json"),
        r#"{"plugins":["plugins"]}"#,
    )
    .expect("write config");

    let output = run_doctor(&DoctorOptions {
        root: root.path(),
        config_path: None,
    });

    assert_eq!(output.status, DoctorStatus::Fail);
    assert_eq!(output.checks[3].status, DoctorCheckStatus::Fail);
    assert!(output.checks[3].required);
    assert!(
        output.checks[3]
            .message
            .contains("1 unresolved configured resource")
    );
}

#[test]
fn inactive_external_plugin_warns_with_project_root_remediation() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(
        root.path().join("package.json"),
        r#"{"name":"doctor-test"}"#,
    )
    .expect("write package manifest");
    std::fs::write(
        root.path().join("fallow-plugin-doctor.json"),
        r#"{"name":"doctor-plugin","enablers":["missing-framework"]}"#,
    )
    .expect("write plugin");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    assert_eq!(output.status, DoctorStatus::Warn);
    assert_eq!(output.checks[3].status, DoctorCheckStatus::Warn);
    assert!(
        output.checks[3]
            .message
            .contains("0 active of 1 configured")
    );
    assert_eq!(
        output.checks[3]
            .remediation
            .as_ref()
            .map(|remediation| (remediation.cwd.as_str(), remediation.mutating)),
        Some((".", false))
    );
}

#[test]
fn active_external_plugin_passes() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::create_dir(root.path().join("node_modules")).expect("create node_modules");
    std::fs::write(
        root.path().join("package.json"),
        r#"{"name":"doctor-test","dependencies":{"doctor-framework":"1.0.0"}}"#,
    )
    .expect("write package manifest");
    std::fs::write(
        root.path().join("fallow-plugin-doctor.json"),
        r#"{"name":"doctor-plugin","enablers":["doctor-framework"]}"#,
    )
    .expect("write plugin");

    let output = run_doctor_with_discovery(
        &DoctorOptions {
            root: root.path(),
            config_path: None,
        },
        &|_| Err("missing companion".to_string()),
    );

    assert_eq!(output.status, DoctorStatus::Pass);
    assert_eq!(output.checks[3].status, DoctorCheckStatus::Pass);
    assert!(
        output.checks[3]
            .message
            .contains("1 active of 1 configured")
    );
}
