//! Builds the heritage of exported classes from the classes that the file
//! declares locally.

use fallow_types::extract::{ClassHeritageInfo, MemberAccess, SemanticFact};
use rustc_hash::FxHashSet;

use super::ModuleInfoExtractor;

impl ModuleInfoExtractor {
    /// Complete the class data of the file after the walk: the heritage of
    /// exported classes, and the base class reads of unexported classes.
    pub(super) fn finalize_local_class_heritage(&mut self) {
        self.enrich_local_class_exports();
        self.lift_unexported_local_base_classes();
        self.record_unexported_subclass_this_accesses();
    }

    fn enrich_local_class_exports(&mut self) {
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
                    binding_super_class: None,
                });
            }
        }
    }

    /// Fold each base class that the file declares but does not export into
    /// the heritage of its exported subclasses (issue #4199). The analysis
    /// finds a base class through its export key, so a class without an
    /// export name stops the walk up the `extends` chain. The subclass takes
    /// the instance bindings of the base (its own bindings win). The walk for
    /// inherited bindings continues at the base's own `extends` target, at any
    /// depth, through `binding_super_class`. The declared `super_class` stays,
    /// because the scoped `usedClassMembers` rules and the framework base
    /// checks match the declared name.
    ///
    /// A generic base stops the fold, because its bindings need the type
    /// arguments of the subclass. A name that the file also imports or
    /// exports stops the fold, because the analysis resolves that name itself.
    fn lift_unexported_local_base_classes(&mut self) {
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
            let mut next = heritage.super_class.clone();
            let mut next_type_args = heritage.super_class_type_args.clone();
            let mut folded = false;
            while let Some(base_name) = next.clone() {
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
                inherit_base_bindings(heritage, &base.instance_bindings);
                folded = true;
                next.clone_from(&base.super_class);
                next_type_args.clone_from(&base.super_class_type_args);
            }
            if folded && let Some(outer) = next {
                heritage.binding_super_class = Some((outer, next_type_args));
            }
        }
    }

    /// Record a `this` read of an unexported class also on the base class
    /// that declares the member: `this.find()?.setVip()` in
    /// `class Viewer extends Floor` gives `Floor.find().setVip`. The analysis
    /// resolves an exported class through its heritage, but an unexported
    /// class has no heritage, so the read reaches no return type without this
    /// access (issue #4312). The analysis then credits `setVip` on the
    /// declared return type of `Floor.find()`.
    ///
    /// The walk passes unexported local base classes. A class on the way that
    /// declares the member itself stops the walk, because its own declaration
    /// overrides the base. The walk ends at the first base that is not an
    /// unexported local class: an exported local class, an import, or any
    /// other name. A namespace base (`ns.Base`) gives no access.
    fn record_unexported_subclass_this_accesses(&mut self) {
        if self.local_class_exports.is_empty() {
            return;
        }
        let exported_locals: FxHashSet<&str> = self
            .exports
            .iter()
            .filter_map(|export| export.local_name.as_deref())
            .collect();
        let mut accesses = Vec::new();
        for fact in &self.semantic_facts {
            let SemanticFact::ClassThisMemberAccess(access) = fact else {
                continue;
            };
            if exported_locals.contains(access.class_local_name.as_str()) {
                continue;
            }
            let Some(path) = access.object.strip_prefix("this.") else {
                continue;
            };
            let root = path.split_once('.').map_or(path, |(root, _)| root);
            let root = root.strip_suffix("()").unwrap_or(root);
            if root.is_empty() || root.contains(['(', ')', '#']) {
                continue;
            }
            if let Some(base) =
                self.member_declaring_base(&access.class_local_name, root, &exported_locals)
            {
                accesses.push(MemberAccess {
                    object: format!("{base}.{path}"),
                    member: access.member.clone(),
                });
            }
        }
        self.member_accesses.extend(accesses);
    }

    /// The first base of `class_name` that is not an unexported local class,
    /// from which the class inherits `member`, or `None` when a local class
    /// on the way declares `member`.
    fn member_declaring_base(
        &self,
        class_name: &str,
        member: &str,
        exported_locals: &FxHashSet<&str>,
    ) -> Option<String> {
        let mut visited: FxHashSet<&str> = FxHashSet::default();
        let mut current = class_name;
        loop {
            if !visited.insert(current) {
                return None;
            }
            let class = self.local_class_exports.get(current)?;
            if class.members.iter().any(|declared| declared.name == member) {
                return None;
            }
            let base = class.super_class.as_deref()?;
            if base.contains('.') {
                return None;
            }
            if exported_locals.contains(base) || !self.local_class_exports.contains_key(base) {
                return Some(base.to_string());
            }
            current = base;
        }
    }
}

/// Add each binding of a base class that the subclass does not declare.
fn inherit_base_bindings(heritage: &mut ClassHeritageInfo, base_bindings: &[(String, String)]) {
    for (field, type_name) in base_bindings {
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
}
