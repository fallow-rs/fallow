//! `unrendered-component` (Angular arm): an `@Component` whose element selector
//! is used in no template project-wide and that is not routed, bootstrapped, or
//! dynamically rendered. Covers the FP-safety abstains: a selector rendered via
//! `<app-used>` is NOT flagged, a routed (`component:`) and a lazy
//! (`loadComponent`) component are NOT flagged, the bootstrapped `AppComponent`
//! is NOT flagged, an attribute-selector component is out of first-cut scope, and
//! a project containing any dynamic-render API abstains entirely.

use super::common::{create_config, fixture_path};

#[test]
fn flags_orphan_angular_component_and_abstains_on_rendered_routed_bootstrapped() {
    let root = fixture_path("angular-unrendered-component");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let flagged: Vec<String> = results
        .unrendered_components
        .iter()
        .filter(|u| u.component.framework == "angular")
        .map(|u| u.component.component_name.clone())
        .collect();

    // The orphan component's selector is used in no template and it is not
    // routed/bootstrapped/dynamically-rendered: flagged.
    assert!(
        flagged.iter().any(|c| c == "OrphanComponent"),
        "a truly-unrendered Angular component should be flagged: {flagged:?}"
    );
    // Rendered via <app-used>, routed, lazy-loaded (both `.then(m => m.X)` and
    // the bare `loadComponent: () => import('./x')` default-export form),
    // bootstrapped, and the attribute-selector component (out of first-cut
    // scope) must NOT be flagged.
    for not_flagged in [
        "UsedComponent",
        "RoutedComponent",
        "LazyComponent",
        "BareLazyComponent",
        "AppComponent",
        "ShellComponent",
        "AttrComponent",
    ] {
        assert!(
            !flagged.iter().any(|c| c == not_flagged),
            "{not_flagged} must not be flagged (rendered/routed/bootstrapped/out-of-scope): {flagged:?}"
        );
    }
    assert_eq!(
        flagged.len(),
        1,
        "exactly one Angular unrendered component (OrphanComponent): {flagged:?}"
    );
}

#[test]
fn dynamic_component_render_abstains_entire_project() {
    let root = fixture_path("angular-unrendered-component-dynamic");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let flagged: Vec<String> = results
        .unrendered_components
        .iter()
        .filter(|u| u.component.framework == "angular")
        .map(|u| u.component.component_name.clone())
        .collect();

    // The project uses a dynamic component-render API (ViewContainerRef
    // .createComponent / *ngComponentOutlet), so a component could be rendered
    // by a non-literal class reference: abstain project-wide, OrphanComponent
    // is NOT flagged.
    assert!(
        flagged.is_empty(),
        "a project with dynamic component render must abstain entirely: {flagged:?}"
    );
}
