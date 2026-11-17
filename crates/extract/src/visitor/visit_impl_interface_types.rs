//! The property map of a named type, with inherited properties and type
//! import properties, for the `type_member_types` of a module.

use oxc_ast::ast::{
    TSImportTypeQualifier, TSInterfaceDeclaration, TSSignature, TSType, TSTypeName,
};
use rustc_hash::{FxHashMap, FxHashSet};

use super::ModuleInfoExtractor;
use super::helpers::heritage_names::unwrap_member_preserving_type;
use super::visit_impl::visit_helpers::collect_object_type_property_types;
use crate::js_text::JsText;
use fallow_types::extract::TypeMemberTypeEntry;

/// Interface links that only `type_member_types` reads.
#[derive(Default)]
pub(super) struct InterfaceTypeLinks {
    /// Interface properties typed by a type import (`ROOM:
    /// DurableObjectNamespace<import("./room").Room>`), as (property, type
    /// reference).
    import_property_types: FxHashMap<String, Vec<(String, String)>>,
    /// The local interfaces that a top-level interface extends.
    extends: FxHashMap<String, Vec<String>>,
}

impl ModuleInfoExtractor {
    /// Records the property types of `iface`, its type import properties, and
    /// the local interfaces that it extends.
    pub(super) fn record_interface_property_types(&mut self, iface: &TSInterfaceDeclaration<'_>) {
        let name = iface.id.name.to_string();
        let mut properties = collect_object_type_property_types(&iface.body.body);
        for target in properties.values_mut() {
            target.owner_span = Some(iface.id.span);
        }
        // Declarations of one name merge, as TypeScript merges them. The
        // `Env` of `declare namespace Cloudflare` and the global `Env` of a
        // `wrangler types` file share the name `Env`.
        if !properties.is_empty() {
            self.interface_property_types
                .entry(name.clone())
                .or_default()
                .extend(properties);
        }
        let import_properties = import_property_types(&iface.body.body);
        if !import_properties.is_empty() {
            self.interface_type_links
                .import_property_types
                .entry(name.clone())
                .or_default()
                .extend(import_properties);
        }
        let parents: Vec<String> = iface
            .extends
            .iter()
            .filter(|heritage| heritage.type_arguments.is_none())
            .filter_map(|heritage| parent_interface_name(&heritage.type_name))
            .collect();
        if !parents.is_empty() {
            self.interface_type_links
                .extends
                .entry(name)
                .or_default()
                .extend(parents);
        }
    }

    /// Flatten this file's named-type property maps into the persisted
    /// `ModuleInfo.type_member_types` entries, sorted for deterministic
    /// output. Names stay local to this module; resolution is deferred to
    /// the analyze-layer join. See issue #1785.
    pub(super) fn collect_type_member_types(&self) -> Vec<TypeMemberTypeEntry> {
        let type_names: FxHashSet<&String> = self
            .interface_property_types
            .keys()
            .chain(self.interface_type_links.import_property_types.keys())
            .chain(self.interface_type_links.extends.keys())
            .collect();
        let mut entries: Vec<TypeMemberTypeEntry> = type_names
            .into_iter()
            .flat_map(|type_name| {
                self.own_and_inherited_property_types(type_name)
                    .into_iter()
                    .map(|(property, property_type)| TypeMemberTypeEntry {
                        type_name: type_name.clone(),
                        property,
                        property_type,
                    })
            })
            .collect();
        entries
            .sort_unstable_by(|a, b| (&a.type_name, &a.property).cmp(&(&b.type_name, &b.property)));
        entries
    }

    /// The property types of `type_name`, and of each local interface that it
    /// extends. A declared property wins over an inherited one of the same
    /// name. `wrangler types` writes `interface Env extends __BaseEnv_Env {}`,
    /// so the bindings of `Env` are inherited properties.
    fn own_and_inherited_property_types(&self, type_name: &str) -> FxHashMap<String, String> {
        let mut properties = FxHashMap::default();
        let mut visited = FxHashSet::default();
        let mut pending = vec![type_name];
        while let Some(current) = pending.pop() {
            if !visited.insert(current) {
                continue;
            }
            for (property, target) in self
                .interface_property_types
                .get(current)
                .into_iter()
                .flatten()
            {
                properties
                    .entry(property.clone())
                    .or_insert_with(|| target.name.clone());
            }
            for (property, reference) in self
                .interface_type_links
                .import_property_types
                .get(current)
                .into_iter()
                .flatten()
            {
                properties
                    .entry(property.clone())
                    .or_insert_with(|| reference.clone());
            }
            if let Some(parents) = self.interface_type_links.extends.get(current) {
                pending.extend(parents.iter().rev().map(String::as_str));
            }
        }
        properties
    }
}

/// Collect `property -> type import` mappings from an object-type member
/// list, for a property typed `import("./room").Room`, also inside a wrapper
/// such as `DurableObjectNamespace<import("./room").Room>`.
fn import_property_types(members: &[TSSignature<'_>]) -> Vec<(String, String)> {
    members
        .iter()
        .filter_map(|member| {
            let TSSignature::TSPropertySignature(prop) = member else {
                return None;
            };
            let property_name = prop.key.static_name()?;
            let annotation = prop.type_annotation.as_deref()?;
            let TSType::TSImportType(import_type) =
                unwrap_member_preserving_type(&annotation.type_annotation)
            else {
                return None;
            };
            let Some(TSImportTypeQualifier::Identifier(name)) = &import_type.qualifier else {
                return None;
            };
            Some((
                property_name.text().into_owned(),
                fallow_types::extract::import_type_reference(
                    import_type.source.value.text().as_ref(),
                    name.name.as_str(),
                ),
            ))
        })
        .collect()
}

/// The local name of an extended interface. A qualified name such as
/// `Cloudflare.Env` gives its last part, because an interface inside a
/// non-exported namespace has its bare name in the type map.
fn parent_interface_name(name: &TSTypeName<'_>) -> Option<String> {
    match name {
        TSTypeName::IdentifierReference(parent) => Some(parent.name.to_string()),
        TSTypeName::QualifiedName(qualified) => Some(qualified.right.name.to_string()),
        TSTypeName::ThisExpression(_) => None,
    }
}
