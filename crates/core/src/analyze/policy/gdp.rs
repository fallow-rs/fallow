//! Proof factory provenance and explicitly configured producer ownership.

use fallow_types::extract::{ImportedCallSite, ImportedName, ReExportInfo};

use crate::discover::FileId;
use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    PolicyCollectionInput, PolicyRuleKind, PolicyViolation, RulePackRuleKind,
    byte_offset_to_line_col, wire_severity,
};

const GDP_PACKAGE: &str = "@gdp-ts/core";
const GDP_FACTORY: &str = "defineProof";

/// A bounded DFS prevents untrusted projects from exhausting the call stack.
const MAX_PROVENANCE_DEPTH: usize = 128;

type SymbolKey = (FileId, Vec<String>);

#[derive(Clone, PartialEq, Eq)]
enum Origin {
    External(String, Vec<String>),
    Local(FileId, Vec<String>),
    Missing,
    Unknown,
}

impl Origin {
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Missing, origin) | (origin, Self::Missing) => origin,
            (left, right) if left == right => left,
            _ => Self::Unknown,
        }
    }
}

pub(super) struct ProofOrigins<'a> {
    modules: FxHashMap<FileId, &'a ResolvedModule>,
    memo: FxHashMap<SymbolKey, Origin>,
    active: FxHashSet<SymbolKey>,
    factories: FxHashMap<SymbolKey, bool>,
}

impl<'a> ProofOrigins<'a> {
    pub(super) fn new(modules: &'a [ResolvedModule]) -> Self {
        Self {
            modules: modules
                .iter()
                .map(|module| (module.file_id, module))
                .collect(),
            memo: FxHashMap::default(),
            active: FxHashSet::default(),
            factories: FxHashMap::default(),
        }
    }

    fn is_factory(&mut self, file_id: FileId, site: &ImportedCallSite) -> bool {
        let mut path = vec![site.local_name.clone()];
        if !site.member_path.is_empty() {
            path.extend(site.member_path.split('.').map(str::to_owned));
        }
        let key = (file_id, path);
        if let Some(&matched) = self.factories.get(&key) {
            return matched;
        }
        let origin = self
            .modules
            .get(&file_id)
            .copied()
            .map_or(Origin::Unknown, |module| {
                self.local_import_origin(module, &site.local_name, &key.1[1..], 0)
            });
        let matched = matches!(origin, Origin::External(ref source, ref symbol) if source == GDP_PACKAGE && symbol.as_slice() == [GDP_FACTORY]);
        self.factories.insert(key, matched);
        matched
    }

    fn local_import_origin(
        &mut self,
        module: &ResolvedModule,
        local: &str,
        member: &[String],
        depth: usize,
    ) -> Origin {
        let mut imports = module
            .resolved_imports
            .iter()
            .filter(|import| import.info.local_name == local && !import.info.is_type_only);
        let Some(first) = imports.next() else {
            return Origin::Local(module.file_id, symbol_path(local, member));
        };
        let origin = self.import_origin(first, member, depth);
        // Embedded script blocks can repeat one written binding name. The merged
        // facts establish an origin only when every block resolves to it.
        for import in imports {
            if self.import_origin(import, member, depth) != origin {
                return Origin::Unknown;
            }
        }
        origin
    }

    fn import_origin(
        &mut self,
        import: &ResolvedImport,
        member: &[String],
        depth: usize,
    ) -> Origin {
        let symbol = match &import.info.imported_name {
            ImportedName::Named(name) => symbol_path(name, member),
            ImportedName::Default => symbol_path("default", member),
            ImportedName::Namespace if !member.is_empty() => member.to_vec(),
            ImportedName::Namespace | ImportedName::SideEffect => return Origin::Unknown,
        };
        self.target_origin(&import.info.source, &import.target, &symbol, depth)
    }

    fn target_origin(
        &mut self,
        source: &str,
        target: &ResolveResult,
        symbol: &[String],
        depth: usize,
    ) -> Origin {
        if matches!(target, ResolveResult::InternalPackageModule { package_name, .. } if source == GDP_PACKAGE && package_name == GDP_PACKAGE)
        {
            return Origin::External(source.to_owned(), symbol.to_vec());
        }
        if let Some(file_id) = target.internal_file_id() {
            return self.export_origin(file_id, symbol, depth + 1);
        }
        match target {
            ResolveResult::NpmPackage(package) if package == source => {
                Origin::External(source.to_owned(), symbol.to_vec())
            }
            // External files can be path mappings or unsupported module forms,
            // so their raw written package name alone cannot prove provenance.
            _ => Origin::Unknown,
        }
    }

    fn export_origin(&mut self, file_id: FileId, symbol: &[String], depth: usize) -> Origin {
        if depth > MAX_PROVENANCE_DEPTH {
            return Origin::Unknown;
        }
        let key = (file_id, symbol.to_vec());
        if let Some(origin) = self.memo.get(&key) {
            return origin.clone();
        }
        if !self.active.insert(key.clone()) {
            return Origin::Unknown;
        }
        let origin = self
            .modules
            .get(&file_id)
            .copied()
            .map_or(Origin::Unknown, |module| {
                self.resolve_export(module, symbol, depth)
            });
        self.active.remove(&key);
        self.memo.insert(key, origin.clone());
        origin
    }

    fn resolve_export(
        &mut self,
        module: &ResolvedModule,
        symbol: &[String],
        depth: usize,
    ) -> Origin {
        let Some((name, member)) = symbol.split_first() else {
            return Origin::Unknown;
        };
        // The existing edge shape cannot distinguish `export *` from
        // `export * as "*"`. Neither may invent a named-star object origin.
        if name == "*"
            && module.re_exports.iter().any(|edge| {
                !edge.info.is_type_only
                    && edge.info.exported_name == "*"
                    && is_whole_module_reexport(&edge.info)
            })
        {
            return Origin::Unknown;
        }
        let mut declared = Origin::Missing;
        let mut has_declared = false;
        for re_export in &module.re_exports {
            if re_export.info.is_type_only || re_export.info.exported_name != *name {
                continue;
            }
            has_declared = true;
            let imported = if is_whole_module_reexport(&re_export.info) {
                if member.is_empty() {
                    return Origin::Unknown;
                }
                member.to_vec()
            } else {
                symbol_path(&re_export.info.imported_name, member)
            };
            declared = declared.merge(self.target_origin(
                &re_export.info.source,
                &re_export.target,
                &imported,
                depth,
            ));
        }
        for export in module
            .exports
            .iter()
            .filter(|export| !export.is_type_only && export.name.matches_str(name))
        {
            has_declared = true;
            let local = export.local_name.as_deref().unwrap_or(name.as_str());
            // Anonymous default expressions do not name a lexically imported value.
            let origin = if name == "default" && export.local_name.is_none() {
                Origin::Local(module.file_id, symbol.to_vec())
            } else {
                self.local_import_origin(module, local, member, depth)
            };
            declared = declared.merge(origin);
        }
        if has_declared {
            return declared;
        }
        if name == "default" {
            return Origin::Missing;
        }
        module
            .re_exports
            .iter()
            .filter(|re_export| {
                !re_export.info.is_type_only
                    && is_whole_module_reexport(&re_export.info)
                    && re_export.info.exported_name == "*"
            })
            .fold(Origin::Missing, |origin, re_export| {
                origin.merge(self.target_origin(
                    &re_export.info.source,
                    &re_export.target,
                    symbol,
                    depth,
                ))
            })
    }
}

fn symbol_path(symbol: &str, members: &[String]) -> Vec<String> {
    std::iter::once(symbol.to_owned())
        .chain(members.iter().cloned())
        .collect()
}

/// Whole-module export declarations retain the declaration span; quoted named
/// `"*"` exports retain a narrower specifier span. Missing synthesized spans
/// cannot establish which syntax produced the edge.
fn is_whole_module_reexport(info: &ReExportInfo) -> bool {
    info.imported_name == "*"
        && info.statement_span.start < info.statement_span.end
        && info.span == info.statement_span
}

pub(super) fn collect_producer_violations(
    input: &mut PolicyCollectionInput<'_>,
    origins: &mut ProofOrigins<'_>,
    root: &std::path::Path,
) {
    if !input
        .in_scope
        .iter()
        .any(|(_, rule)| rule.rule.kind == RulePackRuleKind::GdpProofProducer)
    {
        return;
    }
    let Ok(relative) = input.node.path.strip_prefix(root) else {
        return;
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    for site in input.module.imported_call_sites.iter() {
        if !origins.is_factory(input.node.file_id, site) {
            continue;
        }
        for (_, rule) in input.in_scope {
            if rule.rule.kind != RulePackRuleKind::GdpProofProducer
                || rule
                    .allowed_files
                    .iter()
                    .any(|matcher| matcher.is_match(&relative))
            {
                continue;
            }
            if !rule.rule.proof_kinds.is_empty()
                && !site
                    .first_argument
                    .as_ref()
                    .is_some_and(|kind| rule.rule.proof_kinds.contains(kind))
            {
                continue;
            }
            let Some(severity) = wire_severity(rule.effective_severity(input.master)) else {
                continue;
            };
            let (line, col) = byte_offset_to_line_col(
                input.line_offsets_by_file,
                input.node.file_id,
                site.span_start,
            );
            if input.suppressions.is_policy_suppressed(
                input.node.file_id,
                line,
                rule.pack,
                &rule.rule.id,
            ) {
                continue;
            }
            let argument = site.first_argument.as_ref().map_or_else(
                || "...".to_owned(),
                |kind| serde_json::Value::String(kind.clone()).to_string(),
            );
            let message = rule.rule.message.clone().or_else(|| {
                Some(format!(
                    "Create proof {argument} only in allowed producer files: {}.",
                    rule.rule.allowed_files.join(", ")
                ))
            });
            input.violations.push(PolicyViolation {
                path: input.node.path.clone(),
                line,
                col,
                pack: rule.pack.to_owned(),
                rule_id: rule.rule.id.clone(),
                kind: PolicyRuleKind::GdpProofProducer,
                matched: format!("{GDP_PACKAGE}.{GDP_FACTORY}({argument})"),
                severity,
                message,
            });
        }
    }
}
