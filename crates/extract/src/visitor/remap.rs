//! Moves the spans of an extractor from a parsed body to its host file.
//!
//! A `.vue`, `.svelte`, `.astro` or `.mdx` file parses only part of its source.
//! Each fact that a detector reads as a byte offset must pass through this
//! remap, or the detector reports a position in the body as a position in the
//! file.

use oxc_span::Span;

use super::{ModuleInfoExtractor, PendingBindingForward};

impl ModuleInfoExtractor {
    pub(crate) fn remap_spans_with(&mut self, mut remap: impl FnMut(Span) -> Span) {
        self.remap_graph_spans(&mut remap);
        self.remap_type_and_signature_spans(&mut remap);
        self.remap_handled_spans(&mut remap);
        self.remap_security_spans(&mut remap);
        self.remap_site_offsets(&mut remap);
        self.remap_shared_state(&mut remap);
    }

    /// Remap the facts that store only the start offset of a site.
    fn remap_site_offsets(&mut self, remap: &mut impl FnMut(Span) -> Span) {
        // `remap` keeps the empty span at offset 0 unchanged, but a site is
        // never empty, so a one-byte span maps every site start.
        let mut remap_start = |start: u32| remap(Span::new(start, start + 1)).start;
        for call in &mut self.imported_call_sites {
            call.span_start = remap_start(call.span_start);
        }
        for reference in &mut self.import_binding_references {
            reference.span_start = remap_start(reference.span_start);
        }
        for start in self
            .callee_uses
            .iter_mut()
            .flat_map(|callee| callee.span_starts.iter_mut())
        {
            *start = remap_start(*start);
        }
        for invocation in &mut self.command_invocations {
            invocation.span_start = remap_start(invocation.span_start);
        }
        for site in &mut self.di_key_sites {
            site.span_start = remap_start(site.span_start);
        }
        for site in &mut self.misplaced_directives {
            site.span_start = remap_start(site.span_start);
        }
        for start in &mut self.client_only_dynamic_import_spans {
            *start = remap_start(*start);
        }
        for binding in &mut self.tainted_bindings {
            // `0` means that the binding has no source read.
            if binding.source_span_start != 0 {
                binding.source_span_start = remap_start(binding.source_span_start);
            }
        }
    }

    /// Remap import/export/re-export/dynamic-import/require graph spans.
    fn remap_graph_spans(&mut self, remap: &mut impl FnMut(Span) -> Span) {
        for export in &mut self.exports {
            export.span = remap(export.span);
            for member in &mut export.members {
                member.span = remap(member.span);
            }
        }
        for import in &mut self.imports {
            import.span = remap(import.span);
            import.source_span = remap(import.source_span);
        }
        for re_export in &mut self.re_exports {
            re_export.span = remap(re_export.span);
            re_export.statement_span = remap(re_export.statement_span);
            re_export.source_span = remap(re_export.source_span);
        }
        for dynamic_import in &mut self.dynamic_imports {
            dynamic_import.span = remap(dynamic_import.span);
        }
        for pattern in &mut self.dynamic_import_patterns {
            pattern.span = remap(pattern.span);
        }
        for (span, _) in &mut self.import_load_kind_marks {
            *span = remap(*span);
        }
        for require_call in &mut self.require_calls {
            require_call.span = remap(require_call.span);
        }
        for (_, span) in &mut self.package_resolve_sites {
            *span = remap(*span);
        }
    }

    /// Remap type declarations, signature references, pending specifiers, and
    /// local class member spans.
    fn remap_type_and_signature_spans(&mut self, remap: &mut impl FnMut(Span) -> Span) {
        for declaration in &mut self.local_type_declarations {
            declaration.span = remap(declaration.span);
        }
        for reference in &mut self.public_signature_type_references {
            reference.span = remap(reference.span);
        }
        for reference in &mut self.local_signature_type_references {
            reference.span = remap(reference.span);
        }
        for specifier in &mut self.pending_local_export_specifiers {
            specifier.span = remap(specifier.span);
        }
        for forward in &mut self.pending_binding_forwards {
            if let PendingBindingForward::ExportAssignment { span, .. } = forward {
                *span = remap(*span);
            }
        }
        for class in self.local_class_exports.values_mut() {
            for member in &mut class.members {
                member.span = remap(member.span);
            }
        }
    }

    /// Remap the deduped handled-require / handled-import span sets.
    fn remap_handled_spans(&mut self, remap: &mut impl FnMut(Span) -> Span) {
        self.handled_require_spans = self
            .handled_require_spans
            .iter()
            .map(|span| remap(*span))
            .collect();
        self.handled_import_spans = self
            .handled_import_spans
            .iter()
            .map(|span| remap(*span))
            .collect();
    }

    /// Remap inline-template, security sink, and security control site spans.
    fn remap_security_spans(&mut self, remap: &mut impl FnMut(Span) -> Span) {
        for finding in &mut self.inline_template_findings {
            finding.decorator_start =
                remap(Span::new(finding.decorator_start, finding.decorator_start)).start;
        }
        for sink in &mut self.security_sinks {
            let span = remap(Span::new(sink.span_start, sink.span_end));
            sink.span_start = span.start;
            sink.span_end = span.end;
        }
        for skipped in &mut self.security_unresolved_callee_sites {
            let span = remap(Span::new(skipped.span_start, skipped.span_end));
            skipped.span_start = span.start;
            skipped.span_end = span.end;
        }
        for arg in &mut self.sanitized_sink_args {
            arg.span_start = remap(Span::new(arg.span_start, arg.span_start)).start;
        }
        for control in &mut self.security_control_sites {
            let span = remap(Span::new(control.span_start, control.span_end));
            control.span_start = span.start;
            control.span_end = span.end;
        }
    }
}
