//! Builds the heritage of exported classes from the classes that the file
//! declares locally.

use fallow_types::extract::ClassHeritageInfo;
use rustc_hash::FxHashSet;

use super::ModuleInfoExtractor;

impl ModuleInfoExtractor {
    pub(super) fn enrich_local_class_exports(&mut self) {
        if self.local_class_exports.is_empty() {
            return;
        }

        for export in &mut self.exports {
            let Some(local_name) = export.local_name.as_deref() else {
                continue;
            };
            let Some(local_class) = self.local_class_exports.get(local_name) else {
                continue;
            };

            if export.members.is_empty() {
                export.members = local_class.members.clone();
            }
            if export.super_class.is_none() {
                export.super_class = local_class.super_class.clone();
            }

            let export_name = export.name.to_string();
            let already_has_heritage = self
                .class_heritage
                .iter()
                .any(|heritage| heritage.export_name == export_name);
            if !already_has_heritage
                && (local_class.super_class.is_some()
                    || !local_class.implemented_interfaces.is_empty()
                    || !local_class.instance_bindings.is_empty())
            {
                self.class_heritage.push(ClassHeritageInfo {
                    export_name,
                    super_class: local_class.super_class.clone(),
                    implements: local_class.implemented_interfaces.clone(),
                    type_parameters: local_class.type_parameters.clone(),
                    instance_bindings: local_class.instance_bindings.clone(),
                    super_class_type_args: local_class.super_class_type_args.clone(),
                    generic_instance_bindings: local_class.generic_instance_bindings.clone(),
                });
            }
        }
    }

    /// Fold each base class that the file declares but does not export into
    /// the heritage of its exported subclasses (issue #4199). The analysis
    /// finds a base class through its export key, so a class without an
    /// export name stops the walk up the `extends` chain. The subclass takes
    /// the instance bindings of the base (its own bindings win) and continues
    /// at the base's own `extends` target, at any depth.
    ///
    /// A generic base stops the fold, because its bindings need the type
    /// arguments of the subclass. A name that the file also imports or
    /// exports stops the fold, because the analysis resolves that name itself.
    pub(super) fn lift_unexported_local_base_classes(&mut self) {
        if self.local_class_exports.is_empty() || self.class_heritage.is_empty() {
            return;
        }
        let exported_locals: FxHashSet<&str> = self
            .exports
            .iter()
            .filter_map(|export| export.local_name.as_deref())
            .collect();
        let imported_locals: FxHashSet<&str> = self
            .imports
            .iter()
            .map(|import| import.local_name.as_str())
            .collect();

        for heritage in &mut self.class_heritage {
            let mut visited: FxHashSet<String> = FxHashSet::default();
            while let Some(base_name) = heritage.super_class.clone() {
                if exported_locals.contains(base_name.as_str())
                    || imported_locals.contains(base_name.as_str())
                    || !visited.insert(base_name.clone())
                {
                    break;
                }
                let Some(base) = self.local_class_exports.get(&base_name) else {
                    break;
                };
                if !base.type_parameters.is_empty() {
                    break;
                }
                for (field, type_name) in &base.instance_bindings {
                    let shadowed = heritage
                        .instance_bindings
                        .iter()
                        .any(|(existing, _)| existing == field)
                        || heritage
                            .generic_instance_bindings
                            .iter()
                            .any(|(existing, _)| existing == field);
                    if !shadowed {
                        heritage
                            .instance_bindings
                            .push((field.clone(), type_name.clone()));
                    }
                }
                heritage.super_class.clone_from(&base.super_class);
                heritage
                    .super_class_type_args
                    .clone_from(&base.super_class_type_args);
            }
        }
    }
}
