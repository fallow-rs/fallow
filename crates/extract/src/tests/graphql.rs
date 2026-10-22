use std::path::Path;

use fallow_types::discover::FileId;

use crate::parse_source_to_module;

#[test]
fn parse_source_dispatches_graphql_documents() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("src/story.graphql"),
        "#import \"./content.graphql\"\nfragment Story on Story { id }\n",
        0,
        false,
    );

    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./content.graphql");
}
