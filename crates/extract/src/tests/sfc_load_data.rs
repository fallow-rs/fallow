use super::parse_at_path;

#[test]
fn route_component_template_data_shorthand_attr_is_whole_use() {
    let source = "<script lang=\"ts\">\n  let { data } = $props();\n</script>\n<View {data} />";
    let info = parse_at_path("+page.svelte", source);
    assert!(
        info.has_load_data_whole_use,
        "{{data}} shorthand attribute in a route component is a whole-data use"
    );

    let spaced = "<script lang=\"ts\">\n  let { data } = $props();\n</script>\n<View { data } />";
    assert!(
        parse_at_path("+page.svelte", spaced).has_load_data_whole_use,
        "{{ data }} spaced shorthand attribute is a whole-data use"
    );

    assert!(
        !parse_at_path("src/lib/Card.svelte", source).has_load_data_whole_use,
        "non-route component must not set the load-data whole-use signal"
    );

    let shadowed = "<script lang=\"ts\">\n  let { data } = $props();\n</script>\n{#each rows as data}<Row {data} />{/each}";
    assert!(
        !parse_at_path("+page.svelte", shadowed).has_load_data_whole_use,
        "shadowed each-loop local named data must not set the load-data whole-use signal"
    );

    let commented =
        "<script lang=\"ts\">\n  let { data } = $props();\n</script>\n<!-- <View {data} /> -->";
    assert!(
        !parse_at_path("+page.svelte", commented).has_load_data_whole_use,
        "commented-out shorthand attribute must not set the load-data whole-use signal"
    );
}
