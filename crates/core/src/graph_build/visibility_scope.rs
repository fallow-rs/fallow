//! Apply the `keepExportTags` config key to the resolved modules before the
//! graph build.
//!
//! Extraction stores each JSDoc visibility tag as it is in the source, so the
//! extraction cache does not depend on the config. This step clears each tag
//! that the config does not keep. The graph build, the re-export credit, the
//! unused-export check and the trace then see one value per export. A graph
//! from the graph cache needs no second pass: the cache mode hash includes
//! the key, so the cached graph had the same tags cleared.

use std::sync::Arc;

use fallow_config::KeepExportTags;
use fallow_types::extract::VisibilityTag;

use crate::resolve::ResolvedModule;

/// Clear each visibility tag that `keep` does not retain.
pub fn apply(modules: &mut [ResolvedModule], keep: &KeepExportTags) {
    if keep.retains_all() {
        return;
    }
    for module in modules.iter_mut() {
        if module
            .exports
            .iter()
            .any(|export| !keep.retains(export.visibility))
        {
            let mut exports = module.exports.to_vec();
            for export in &mut exports {
                clear_unless_retained(&mut export.visibility, keep);
            }
            module.exports = Arc::from(exports);
        }
        for re_export in &mut module.re_exports {
            clear_unless_retained(&mut re_export.info.visibility, keep);
        }
    }
}

fn clear_unless_retained(tag: &mut VisibilityTag, keep: &KeepExportTags) {
    if !keep.retains(*tag) {
        *tag = VisibilityTag::None;
    }
}
