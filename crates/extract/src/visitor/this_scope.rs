//! Class scope qualifiers and class facts of `this` receivers (issue #1821).

use super::ModuleInfoExtractor;
use crate::{ClassThisMemberAccessFact, ClassThisWholeObjectUseFact, SemanticFact};

impl ModuleInfoExtractor {
    /// Build the class-scoped `binding_target_names` key for a `this.<suffix>`
    /// member. Inside a class body the key is qualified with the enclosing
    /// class's scope id (`this@<id>.<suffix>`) so two classes in one module that
    /// declare a same-named field do not collide in the module-flat map: without
    /// this, last-write-wins credits only the class declared last and falsely
    /// reports the other class's members unused. Module-level `this` keeps the
    /// plain `this.<suffix>` spelling. `strip_this_scope_qualifiers` removes the
    /// qualifier before a spelling reaches `ModuleInfo` (issue #1821, Fix B).
    pub(super) fn this_member_key(&self, suffix: &str) -> String {
        match self.class_scope_stack.last() {
            Some(id) => format!("this@{id}.{suffix}"),
            None => format!("this.{suffix}"),
        }
    }

    /// Qualify a `this.`-rooted access / whole-object / iteration-receiver
    /// spelling with the enclosing class scope id so it resolves against the
    /// same class's qualified `binding_target_names` keys (issue #1821). A no-op
    /// for the bare `this` object (single segment, so the per-file self-access
    /// credit keyed on `object == "this"` is untouched), any non-`this`
    /// spelling, an already-qualified `this@<id>.` spelling, and module-level
    /// `this` (no active class scope). Paired with `strip_this_scope_qualifiers`
    /// at emission.
    ///
    /// A `function` or an object-literal method inside a class method has its
    /// own `this`, which is not the class instance. There the spelling gets
    /// `INDEPENDENT_THIS_SCOPE_ID`, which no class scope uses. So the access
    /// gives no class `this` fact and resolves through no class binding key
    /// (issue #4351).
    pub(super) fn qualify_this_scope(&self, spelling: &str) -> String {
        if let Some(id) = self.class_scope_stack.last()
            && let Some(rest) = spelling.strip_prefix("this.")
        {
            let id = if self.independent_this_depth > 0 {
                INDEPENDENT_THIS_SCOPE_ID
            } else {
                *id
            };
            return format!("this@{id}.{rest}");
        }
        spelling.to_string()
    }

    /// Preserve exact enclosing-class provenance before internal `this@<id>`
    /// qualifiers are removed for legacy consumers.
    pub(super) fn record_class_this_facts(
        &mut self,
        class: &oxc_ast::ast::Class<'_>,
        scope_id: u32,
        member_access_start: usize,
        whole_object_start: usize,
    ) {
        let prefix = format!("this@{scope_id}.");
        let class_local_name = class
            .id
            .as_ref()
            .map_or_else(|| "default".to_string(), |id| id.name.to_string());

        let member_facts = self.member_accesses[member_access_start..]
            .iter()
            .filter_map(|access| {
                let rest = access.object.strip_prefix(&prefix)?;
                Some(SemanticFact::ClassThisMemberAccess(
                    ClassThisMemberAccessFact {
                        class_local_name: class_local_name.clone(),
                        object: format!("this.{rest}"),
                        member: access.member.clone(),
                    },
                ))
            })
            .collect::<Vec<_>>();
        self.semantic_facts.extend(member_facts);

        let whole_object_facts = self.whole_object_uses[whole_object_start..]
            .iter()
            .filter_map(|object| {
                let rest = object.strip_prefix(&prefix)?;
                Some(SemanticFact::ClassThisWholeObjectUse(
                    ClassThisWholeObjectUseFact {
                        class_local_name: class_local_name.clone(),
                        object: format!("this.{rest}"),
                    },
                ))
            })
            .collect::<Vec<_>>();
        self.semantic_facts.extend(whole_object_facts);
    }

    /// Give credit to the held bare `this` reads (issue #4351), then strip the
    /// scope qualifiers. Called last, before any spelling is emitted.
    pub(super) fn finish_this_reads(&mut self) {
        self.resolve_independent_this_reads();
        self.strip_this_scope_qualifiers();
    }

    /// Rewrite every internal `this@<id>.` scope qualifier (issue #1821) back to
    /// a plain `this.` across the emitted `member_accesses` and
    /// `whole_object_uses`, so no persisted spelling and no downstream consumer
    /// (core member self-access `== "this"`, heritage `!= "this"`,
    /// `unused_component_output` `this.<name>`, SFC template `starts_with("this.")`)
    /// ever sees the qualifier. Called last in `finalize_resolution_phase`, after
    /// every resolution pass that relies on the per-class keys, so the strip is
    /// invariant across the `into_module_info` and `merge_into` (SFC) paths.
    fn strip_this_scope_qualifiers(&mut self) {
        for access in &mut self.member_accesses {
            strip_this_scope_qualifier(&mut access.object);
        }
        for whole in &mut self.whole_object_uses {
            strip_this_scope_qualifier(whole);
        }
    }
}

/// The `this@<id>` scope id of a `this` receiver in a function with its own
/// `this` inside a class body. Class scope ids start at 1, so no class uses it.
const INDEPENDENT_THIS_SCOPE_ID: u32 = 0;

/// Strip a leading `this@<id>` scope qualifier back to `this` (issue #1821),
/// leaving any non-`this@` spelling untouched. `@` cannot appear in a JS
/// identifier or dotted member path, so the marker is unambiguous, and it is
/// only ever produced for multi-segment `this.<path>` spellings, so a `.`
/// always follows the id. The `else` arm is defensive and unreachable in
/// practice.
fn strip_this_scope_qualifier(spelling: &mut String) {
    let Some(rest) = spelling.strip_prefix("this@") else {
        return;
    };
    if let Some(dot) = rest.find('.') {
        let mut rebuilt = String::with_capacity("this".len() + rest.len() - dot);
        rebuilt.push_str("this");
        rebuilt.push_str(&rest[dot..]);
        *spelling = rebuilt;
    } else {
        *spelling = "this".to_string();
    }
}
