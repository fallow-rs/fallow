use super::common::{create_config, fixture_path};

fn unused_export_names(fixture: &str) -> Vec<String> {
    let config = create_config(fixture_path(fixture));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut names: Vec<String> = results
        .unused_exports
        .iter()
        .map(|export| export.export.export_name.clone())
        .collect();
    names.sort_unstable();
    names
}

/// Issue #4672: Vue compiles `<script>` and `<script setup>` into one module.
/// `<script setup>` and the template can therefore use a binding that the
/// plain `<script>` block imports. An import that no block and no template
/// uses stays unused, and a parameter in `<script setup>` that shadows the
/// import does not use it.
#[test]
fn vue_plain_script_import_is_credited_from_script_setup_and_template() {
    assert_eq!(
        unused_export_names("issue-4672-vue-plain-script-imports"),
        ["importedButUnused", "shadowedInScriptSetup"],
    );
}

/// The Svelte module script (`<script module>`) and the instance script also
/// form one module. The instance script and the markup can use a binding that
/// the module script imports.
#[test]
fn svelte_module_script_import_is_credited_from_instance_script_and_markup() {
    assert_eq!(
        unused_export_names("issue-4672-svelte-module-script-imports"),
        ["importedButUnused", "shadowedInInstanceScript"],
    );
}
