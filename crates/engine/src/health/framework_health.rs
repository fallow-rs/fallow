//! Framework detector coverage diagnostics for health output.

use fallow_config::{PackageJson, ResolvedConfig, RulesConfig, Severity};

#[derive(Clone, Copy, Default)]
pub(super) struct FrameworkHealthFacts {
    pub(super) unused_load_data_keys_global_abstain: bool,
}

pub(super) fn build_framework_health_diagnostics(
    config: &ResolvedConfig,
    facts: Option<FrameworkHealthFacts>,
) -> Option<fallow_output::FrameworkHealthDiagnostics> {
    let facts = facts?;
    let detected_frameworks = detect_frameworks(config);
    if detected_frameworks.is_empty() {
        return None;
    }

    let mut detectors = Vec::new();
    for framework in &detected_frameworks {
        add_framework_detectors(&mut detectors, framework, &config.rules, facts);
    }

    if detectors.is_empty() {
        return None;
    }

    Some(fallow_output::FrameworkHealthDiagnostics {
        detected_frameworks,
        detectors,
    })
}

fn detect_frameworks(config: &ResolvedConfig) -> Vec<String> {
    let mut deps = rustc_hash::FxHashSet::default();
    if let Ok(pkg) = PackageJson::load(&config.root.join("package.json")) {
        deps.extend(pkg.all_dependency_names());
    }
    for workspace in fallow_config::discover_workspaces(&config.root) {
        if let Ok(pkg) = PackageJson::load(&workspace.root.join("package.json")) {
            deps.extend(pkg.all_dependency_names());
        }
    }

    let mut frameworks = Vec::new();
    if deps.contains("react") || deps.contains("preact") || deps.contains("next") {
        frameworks.push("react".to_string());
    }
    if deps.contains("next") {
        frameworks.push("next".to_string());
    }
    if deps.contains("vue") || deps.contains("@vue/runtime-core") {
        frameworks.push("vue".to_string());
    }
    if deps.contains("nuxt") {
        frameworks.push("nuxt".to_string());
    }
    if deps.contains("svelte") || deps.contains("@sveltejs/kit") {
        frameworks.push("svelte".to_string());
    }
    if deps.contains("@sveltejs/kit") {
        frameworks.push("sveltekit".to_string());
    }
    if deps.contains("@angular/core") {
        frameworks.push("angular".to_string());
    }
    frameworks.sort_unstable();
    frameworks.dedup();
    frameworks
}

fn add_framework_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &RulesConfig,
    facts: FrameworkHealthFacts,
) {
    match framework {
        "angular" => add_angular_detectors(detectors, framework, rules),
        "next" => add_next_detectors(detectors, framework, rules),
        "nuxt" => add_nuxt_detectors(detectors, framework, rules),
        "vue" => add_vue_detectors(detectors, framework, rules),
        "react" => add_react_detectors(detectors, framework, rules),
        "svelte" => add_svelte_detectors(detectors, framework, rules),
        "sveltekit" => add_sveltekit_detectors(detectors, framework, rules, facts),
        _ => {}
    }
}

fn add_angular_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &RulesConfig,
) {
    add_detector(
        detectors,
        framework,
        "unrendered-component",
        rules.unrendered_components,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-input",
        rules.unused_component_inputs,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-output",
        rules.unused_component_outputs,
    );
    add_detector(
        detectors,
        framework,
        "unprovided-inject",
        rules.unprovided_injects,
    );
}

fn add_next_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &RulesConfig,
) {
    add_detector(
        detectors,
        framework,
        "invalid-client-export",
        rules.invalid_client_export,
    );
    add_detector(
        detectors,
        framework,
        "mixed-client-server-barrel",
        rules.mixed_client_server_barrel,
    );
    add_detector(
        detectors,
        framework,
        "misplaced-directive",
        rules.misplaced_directive,
    );
    add_detector(
        detectors,
        framework,
        "route-collision",
        rules.route_collision,
    );
    add_detector(
        detectors,
        framework,
        "dynamic-segment-name-conflict",
        rules.dynamic_segment_name_conflict,
    );
    add_detector(
        detectors,
        framework,
        "unused-server-action",
        rules.unused_server_actions,
    );
}

fn add_nuxt_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &RulesConfig,
) {
    add_detector(
        detectors,
        framework,
        "unrendered-component",
        rules.unrendered_components,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-prop",
        rules.unused_component_props,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-emit",
        rules.unused_component_emits,
    );
    add_not_checked_detector(
        detectors,
        framework,
        "unprovided-inject",
        "requires_vue_runtime_dependency",
    );
}

fn add_vue_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &RulesConfig,
) {
    add_detector(
        detectors,
        framework,
        "unrendered-component",
        rules.unrendered_components,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-prop",
        rules.unused_component_props,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-emit",
        rules.unused_component_emits,
    );
    add_detector(
        detectors,
        framework,
        "unprovided-inject",
        rules.unprovided_injects,
    );
}

fn add_react_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &RulesConfig,
) {
    add_detector(
        detectors,
        framework,
        "unused-component-prop",
        rules.unused_component_props,
    );
    add_detector(detectors, framework, "prop-drilling", rules.prop_drilling);
    add_detector(detectors, framework, "thin-wrapper", rules.thin_wrapper);
    add_detector(
        detectors,
        framework,
        "duplicate-prop-shape",
        rules.duplicate_prop_shape,
    );
}

fn add_svelte_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &RulesConfig,
) {
    add_detector(
        detectors,
        framework,
        "unrendered-component",
        rules.unrendered_components,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-prop",
        rules.unused_component_props,
    );
    add_detector(
        detectors,
        framework,
        "unused-svelte-event",
        rules.unused_svelte_events,
    );
    add_detector(
        detectors,
        framework,
        "unprovided-inject",
        rules.unprovided_injects,
    );
}

fn add_sveltekit_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &RulesConfig,
    facts: FrameworkHealthFacts,
) {
    if facts.unused_load_data_keys_global_abstain && rules.unused_load_data_keys != Severity::Off {
        detectors.push(fallow_output::FrameworkHealthDetector {
            id: "unused-load-data-key".to_string(),
            framework: framework.to_string(),
            status: fallow_output::FrameworkHealthDetectorStatus::Abstained,
            reason: Some("unused_load_data_keys_global_abstain".to_string()),
        });
    } else {
        add_detector(
            detectors,
            framework,
            "unused-load-data-key",
            rules.unused_load_data_keys,
        );
    }
}

fn add_detector(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    id: &str,
    severity: Severity,
) {
    let (status, reason) = if severity == Severity::Off {
        (
            fallow_output::FrameworkHealthDetectorStatus::DisabledByConfig,
            Some("disabled_by_config".to_string()),
        )
    } else {
        (fallow_output::FrameworkHealthDetectorStatus::Active, None)
    };
    detectors.push(fallow_output::FrameworkHealthDetector {
        id: id.to_string(),
        framework: framework.to_string(),
        status,
        reason,
    });
}

fn add_not_checked_detector(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    id: &str,
    reason: &str,
) {
    detectors.push(fallow_output::FrameworkHealthDetector {
        id: id.to_string(),
        framework: framework.to_string(),
        status: fallow_output::FrameworkHealthDetectorStatus::NotChecked,
        reason: Some(reason.to_string()),
    });
}
