//! Merge of template-scanned facts into a finished module record.

use rustc_hash::FxHashSet;

use crate::{MemberAccess, ModuleInfo};

/// Append template-scanned member accesses to a finished module record,
/// skipping pairs the record already holds: a dotted tag inside an Astro
/// expression region is seen by both the template tag scan and the region
/// visitor, and no consumer counts repeats. The accesses live behind an
/// `Arc<[_]>` once the extractor is consumed, so the slice is rebuilt only when
/// there is something to add.
pub fn extend_member_accesses(info: &mut ModuleInfo, extra: Vec<MemberAccess>) {
    if extra.is_empty() {
        return;
    }
    let mut accesses = std::mem::take(&mut info.member_accesses).to_vec();
    let mut seen: FxHashSet<(String, String)> = accesses
        .iter()
        .map(|access| (access.object.clone(), access.member.clone()))
        .collect();
    for access in extra {
        if seen.insert((access.object.clone(), access.member.clone())) {
            accesses.push(access);
        }
    }
    info.member_accesses = accesses.into();
}

/// Append template-scanned whole-object uses to a finished module record, the
/// `whole_object_uses` counterpart of `extend_member_accesses`: names the
/// record already holds are skipped, so the region pass, the template guard,
/// and the script guard can each report the same binding without the
/// persisted record growing duplicates.
pub fn extend_whole_object_uses(info: &mut ModuleInfo, extra: Vec<String>) {
    if extra.is_empty() {
        return;
    }
    let mut uses = std::mem::take(&mut info.whole_object_uses).to_vec();
    for name in extra {
        if !uses.contains(&name) {
            uses.push(name);
        }
    }
    info.whole_object_uses = uses.into();
}
