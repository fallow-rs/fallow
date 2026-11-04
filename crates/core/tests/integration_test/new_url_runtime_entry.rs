//! A `new URL("./x.ts", import.meta.url)` path can name a module that a
//! runtime loader loads by path, such as a build tool or a script runner. The
//! loader calls the default export, so the analysis credits `default`. Other
//! exports stay reportable. A direct argument of a `Worker` constructor and a
//! directory URL credit no export.

use super::common::{create_config, fixture_path};

#[cfg_attr(miri, ignore)]
#[test]
fn new_url_module_path_credits_the_default_export() {
    let root = fixture_path("new-url-runtime-entry");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused: Vec<String> = results
        .unused_exports
        .iter()
        .map(|finding| {
            let path = finding.export.path.to_string_lossy().replace('\\', "/");
            let file = path.rsplit('/').next().unwrap_or_default().to_string();
            format!("{file}:{}", finding.export.export_name)
        })
        .collect();
    unused.sort();
    assert_eq!(
        unused,
        [
            "index.ts:default",
            "signer.ts:signerName",
            "worker.ts:default"
        ],
        "a new URL module path credits only `default`, and not for a worker or a directory"
    );
}
