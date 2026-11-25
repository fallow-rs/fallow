//! Lit custom elements and Angular component selectors, templates and members.

use super::*;

impl<'a> ModuleInfoExtractor {
    /// Record a Lit `@customElement('tag')` class as a side-effect registration
    /// candidate, keyed on the local class name or the pending anonymous default
    /// export slot.
    pub(super) fn record_lit_custom_element(&mut self, class: &Class<'a>) {
        let Some(decorator) = lit_custom_element_decorator(class) else {
            return;
        };
        // The registered tag (`@customElement('x-foo')`) and the class span anchor
        // the Lit `unrendered-component` finding at the element, not line 1.
        let tag = lit_custom_element_tag(class);
        let span_start = class.span.start;
        if let Some(id) = class.id.as_ref() {
            self.record_lit_custom_element_candidate(
                decorator,
                SideEffectRegistrationTarget::LocalClass(id.name.to_string()),
                tag,
                span_start,
            );
        } else if let Some(export) = self.exports.last()
            && matches!(export.name, crate::ExportName::Default)
            && export.local_name.is_none()
        {
            let export_index = self.exports.len() - 1;
            self.record_lit_custom_element_candidate(
                decorator,
                SideEffectRegistrationTarget::AnonymousDefaultExport(export_index),
                tag,
                span_start,
            );
        }
    }

    /// Harvest the `@Component` selector(s) + class name + span for the Angular
    /// arm of the `unrendered-component` detector. Only a `@Component` (never
    /// `@Directive`) carries a selector here. A multi-selector string is split on
    /// `,` into the list; the detector restricts first-cut scope to
    /// all-element-selector components.
    pub(super) fn record_angular_selector(
        &mut self,
        class: &Class<'a>,
        meta: &crate::visitor::helpers::AngularComponentMetadata,
    ) {
        if let Some(ref selector_raw) = meta.selector
            && let Some(id) = class.id.as_ref()
        {
            let selectors = split_angular_selectors(selector_raw);
            if !selectors.is_empty() {
                self.angular_component_selectors
                    .push(AngularComponentSelector {
                        selectors,
                        span_start: class.span.start,
                        class_name: id.name.to_string(),
                    });
            }
        }
    }

    /// Record `templateUrl` / `styleUrls` external asset references as
    /// `SideEffect` imports for an Angular component.
    pub(super) fn record_angular_template_assets(
        &mut self,
        meta: &crate::visitor::helpers::AngularComponentMetadata,
    ) {
        if let Some(ref template_url) = meta.template_url {
            self.imports.push(ImportInfo {
                source: normalize_asset_url(template_url),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            });
            self.has_angular_component_template_url = true;
        }
        for style_url in &meta.style_urls {
            self.imports.push(ImportInfo {
                source: normalize_asset_url(style_url),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            });
        }
    }

    /// Scan an Angular inline `template:` string: record used selectors, the
    /// dynamic-render abstain, template member-access refs, offset-remapped
    /// security sinks, and the inline-template complexity finding.
    pub(super) fn record_angular_inline_template(
        &mut self,
        class: &Class<'_>,
        meta: &crate::visitor::helpers::AngularComponentMetadata,
    ) {
        let Some(ref template) = meta.inline_template else {
            return;
        };
        self.angular_used_selectors
            .extend(crate::sfc_template::angular::collect_angular_used_selectors(template));
        // `*ngComponentOutlet` dynamically renders a component from a non-literal
        // class reference; abstain project-wide.
        if template.contains("ngComponentOutlet") {
            self.has_dynamic_component_render = true;
        }

        // Type `@for` / `*ngFor` loop variables over a component field typed as an
        // array of a class (`utils: Util[]`) to the element class, so template
        // member accesses on the loop item (`{{ util.getName() }}`) remap onto the
        // class instead of being dropped (issue #1712).
        let field_types = crate::visitor::helpers::collect_component_field_array_types(class);
        let refs = crate::sfc_template::angular::collect_angular_template_refs_with_field_types(
            template,
            &field_types,
        );
        for name in refs.identifiers {
            self.record_angular_template_member_fact(name);
        }
        self.member_accesses.extend(refs.member_accesses);
        let template_offset = meta
            .inline_template_offset
            .unwrap_or(meta.decorator_span.start);
        self.security_sinks
            .extend(refs.security_sinks.into_iter().map(|mut sink| {
                sink.span_start = sink.span_start.saturating_add(template_offset);
                sink.span_end = sink.span_end.saturating_add(template_offset);
                sink
            }));

        self.inline_template_findings
            .push(crate::visitor::InlineTemplateFinding {
                template_source: template.clone(),
                decorator_start: meta.decorator_span.start,
            });
    }

    pub(super) fn record_angular_component_field_array_types(&mut self, class: &Class<'_>) {
        for (field, element_class) in
            crate::visitor::helpers::collect_component_field_array_types(class)
        {
            self.semantic_facts
                .push(SemanticFact::AngularComponentFieldArrayType(
                    AngularComponentFieldArrayTypeFact {
                        field,
                        element_class,
                    },
                ));
        }
    }

    /// Record Angular `host:` binding and `inputs:` / `outputs:` member refs as
    /// typed template member facts.
    pub(super) fn record_angular_template_members(
        &mut self,
        meta: &crate::visitor::helpers::AngularComponentMetadata,
    ) {
        for name in &meta.host_member_refs {
            self.record_angular_template_member_fact(name.clone());
        }
        for name in &meta.input_output_members {
            self.record_angular_template_member_fact(name.clone());
        }
    }
}
