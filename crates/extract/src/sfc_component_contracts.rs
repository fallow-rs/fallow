//! Component contracts of the template of a Vue or Svelte single-file component.

use super::*;

/// Collect the component contracts of the template and merge them into the
/// contracts of the script blocks.
pub(super) fn apply_component_contracts(kind: SfcKind, source: &str, combined: &mut ModuleInfo) {
    let framework = match kind {
        SfcKind::Vue => fallow_types::extract::ComponentFramework::Vue,
        SfcKind::Svelte => fallow_types::extract::ComponentFramework::Svelte,
    };
    let callers = crate::sfc_template::component_contracts::collect(
        &mask_non_markup_regions(source),
        &combined.imports,
        framework,
        0,
        combined
            .component_contracts
            .as_deref()
            .map_or(&[], |facts| facts.spread_bindings.as_slice()),
        combined
            .component_contracts
            .as_deref()
            .map_or(&[], |facts| facts.aliases.as_slice()),
    );
    crate::component_contracts::merge(&mut combined.component_contracts, callers, 0, None);
    if let Some(facts) = &mut combined.component_contracts {
        let credited: FxHashSet<String> = facts
            .declarations
            .iter()
            .flat_map(|prop| [prop.local.clone(), prop.name.clone()])
            .collect();
        let usage = collect_template_usage_with_bound_targets(
            kind,
            source,
            &credited,
            &FxHashMap::default(),
            &FxHashMap::default(),
        );
        for declaration in &mut facts.declarations {
            declaration.is_used |= usage.used_bindings.contains(&declaration.local)
                || usage.member_accesses.iter().any(|access| {
                    access.object == declaration.local && access.member == declaration.name
                });
        }
    }
    if let Some(facts) = &mut combined.component_contracts {
        for declaration in &mut facts.declarations {
            if let Some(prop) = combined
                .component_props
                .iter()
                .find(|prop| prop.name == declaration.name)
            {
                declaration.is_used |= prop.used_in_template;
            }
            declaration.incomplete |= combined.has_props_attrs_fallthrough
                || combined.has_define_expose
                || combined.has_define_model;
        }
    }

    crate::component_contracts::attach_sfc_default_export(combined);
}
