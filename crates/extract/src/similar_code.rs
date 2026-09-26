//! On-demand, bounded JS and TS function extraction for similar-code providers.

use std::collections::BTreeSet;
use std::path::Path;

use fallow_types::similar_code::{
    ExtractedSimilarCodeFunction, SIMILAR_CODE_EXTRACTION_SEMANTICS_VERSION, SimilarCodeExtraction,
    SimilarCodeExtractionLimits, SimilarCodeExtractionSkip, SimilarCodeExtractionSkipReason,
    SimilarCodeFunctionKind, SimilarCodeFunctionLocation, SimilarCodeSideEffectHint,
    SimilarCodeSourceDigest,
};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionExpression, AssignmentExpression, AwaitExpression, CallExpression, Class,
    ClassElement, ComputedMemberExpression, Declaration, ExportDefaultDeclarationKind, Function,
    ImportExpression, JSXElement, JSXFragment, MethodDefinitionKind, NewExpression,
    ObjectExpression, ObjectPropertyKind, PrivateFieldExpression, Program, PropertyKind, Statement,
    StaticMemberExpression, TaggedTemplateExpression, ThrowStatement, UnaryExpression,
    UpdateExpression, VariableDeclaration, YieldExpression,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_semantic::ScopeFlags;
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};

use crate::function_body::BodyRef;

#[derive(Debug, Clone, Copy)]
struct ReviewMetadata {
    param_count: u32,
    is_async: bool,
    is_generator: bool,
    has_await: bool,
    has_throw: bool,
    side_effect_hint: SimilarCodeSideEffectHint,
}

impl ReviewMetadata {
    fn for_function(function: &Function<'_>, syntax_reliable: bool) -> Self {
        Self::from_body(
            function.body.as_deref().map(BodyRef::Block),
            function.params.items.len(),
            function.params.rest.is_some(),
            function.r#async,
            function.generator,
            syntax_reliable,
        )
    }

    fn for_arrow(arrow: &ArrowFunctionExpression<'_>, syntax_reliable: bool) -> Self {
        Self::from_body(
            Some(BodyRef::arrow(&arrow.body)),
            arrow.params.items.len(),
            arrow.params.rest.is_some(),
            arrow.r#async,
            false,
            syntax_reliable,
        )
    }

    fn from_body(
        body: Option<BodyRef<'_, '_>>,
        fixed_params: usize,
        has_rest: bool,
        is_async: bool,
        is_generator: bool,
        syntax_reliable: bool,
    ) -> Self {
        let mut visitor = ReviewMetadataVisitor::default();
        if let Some(body) = body {
            body.visit(&mut visitor);
        }
        let param_count = fixed_params.saturating_add(usize::from(has_rest));
        let side_effect_hint = if !syntax_reliable {
            SimilarCodeSideEffectHint::Unknown
        } else if visitor.may_have_side_effects {
            SimilarCodeSideEffectHint::MayHaveSideEffects
        } else {
            SimilarCodeSideEffectHint::PureLooking
        };
        Self {
            param_count: u32::try_from(param_count).unwrap_or(u32::MAX),
            is_async,
            is_generator,
            has_await: visitor.has_await,
            has_throw: visitor.has_throw,
            side_effect_hint,
        }
    }
}

#[derive(Default)]
struct ReviewMetadataVisitor {
    has_await: bool,
    has_throw: bool,
    may_have_side_effects: bool,
}

impl<'ast> Visit<'ast> for ReviewMetadataVisitor {
    fn visit_function(&mut self, _function: &Function<'ast>, _flags: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _arrow: &ArrowFunctionExpression<'ast>) {}

    fn visit_call_expression(&mut self, expression: &CallExpression<'ast>) {
        self.may_have_side_effects = true;
        walk::walk_call_expression(self, expression);
    }

    fn visit_new_expression(&mut self, expression: &NewExpression<'ast>) {
        self.may_have_side_effects = true;
        walk::walk_new_expression(self, expression);
    }

    fn visit_assignment_expression(&mut self, expression: &AssignmentExpression<'ast>) {
        self.may_have_side_effects = true;
        walk::walk_assignment_expression(self, expression);
    }

    fn visit_update_expression(&mut self, expression: &UpdateExpression<'ast>) {
        self.may_have_side_effects = true;
        walk::walk_update_expression(self, expression);
    }

    fn visit_unary_expression(&mut self, expression: &UnaryExpression<'ast>) {
        self.may_have_side_effects |= expression.operator.is_delete();
        walk::walk_unary_expression(self, expression);
    }

    fn visit_await_expression(&mut self, expression: &AwaitExpression<'ast>) {
        self.has_await = true;
        self.may_have_side_effects = true;
        walk::walk_await_expression(self, expression);
    }

    fn visit_throw_statement(&mut self, statement: &ThrowStatement<'ast>) {
        self.has_throw = true;
        self.may_have_side_effects = true;
        walk::walk_throw_statement(self, statement);
    }

    fn visit_yield_expression(&mut self, expression: &YieldExpression<'ast>) {
        self.may_have_side_effects = true;
        walk::walk_yield_expression(self, expression);
    }

    fn visit_import_expression(&mut self, expression: &ImportExpression<'ast>) {
        self.may_have_side_effects = true;
        walk::walk_import_expression(self, expression);
    }

    fn visit_tagged_template_expression(&mut self, expression: &TaggedTemplateExpression<'ast>) {
        self.may_have_side_effects = true;
        walk::walk_tagged_template_expression(self, expression);
    }

    fn visit_computed_member_expression(&mut self, expression: &ComputedMemberExpression<'ast>) {
        self.may_have_side_effects = true;
        walk::walk_computed_member_expression(self, expression);
    }

    fn visit_static_member_expression(&mut self, expression: &StaticMemberExpression<'ast>) {
        self.may_have_side_effects = true;
        walk::walk_static_member_expression(self, expression);
    }

    fn visit_private_field_expression(&mut self, expression: &PrivateFieldExpression<'ast>) {
        self.may_have_side_effects = true;
        walk::walk_private_field_expression(self, expression);
    }

    fn visit_jsx_element(&mut self, element: &JSXElement<'ast>) {
        self.may_have_side_effects = true;
        walk::walk_jsx_element(self, element);
    }

    fn visit_jsx_fragment(&mut self, fragment: &JSXFragment<'ast>) {
        self.may_have_side_effects = true;
        walk::walk_jsx_fragment(self, fragment);
    }
}

struct ExtractionBuilder<'a> {
    file: String,
    source: &'a str,
    line_offsets: Vec<u32>,
    limits: SimilarCodeExtractionLimits,
    functions: Vec<ExtractedSimilarCodeFunction>,
    source_bytes: usize,
    syntax_reliable: bool,
    classified_spans: BTreeSet<(u32, u32)>,
    skips: FxHashMap<SimilarCodeExtractionSkipReason, usize>,
}

impl<'a> ExtractionBuilder<'a> {
    fn new(
        file: String,
        source: &'a str,
        limits: SimilarCodeExtractionLimits,
        syntax_reliable: bool,
    ) -> Self {
        Self {
            file,
            source,
            line_offsets: fallow_types::extract::compute_line_offsets(source),
            limits,
            functions: Vec::new(),
            source_bytes: 0,
            syntax_reliable,
            classified_spans: BTreeSet::new(),
            skips: FxHashMap::default(),
        }
    }

    fn collect_program(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    self.collect_function_declaration(function, None);
                }
                Statement::VariableDeclaration(declaration) => {
                    self.collect_variable_declaration(declaration);
                }
                Statement::ClassDeclaration(class) => {
                    self.collect_class_declaration(class, None);
                }
                Statement::ExportDeclaration(export) => {
                    self.collect_declaration(&export.declaration);
                }
                Statement::ExportDefaultDeclaration(export) => {
                    self.collect_default_export(&export.declaration);
                }
                _ => {}
            }
        }
    }

    fn collect_declaration(&mut self, declaration: &Declaration<'_>) {
        match declaration {
            Declaration::FunctionDeclaration(function) => {
                self.collect_function_declaration(function, None);
            }
            Declaration::VariableDeclaration(declaration) => {
                self.collect_variable_declaration(declaration);
            }
            Declaration::ClassDeclaration(class) => {
                self.collect_class_declaration(class, None);
            }
            _ => {}
        }
    }

    fn collect_default_export(&mut self, declaration: &ExportDefaultDeclarationKind<'_>) {
        match declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                self.collect_function_declaration(function, Some("default"));
            }
            ExportDefaultDeclarationKind::FunctionExpression(function) => {
                self.collect_function_expression(function, "default");
            }
            ExportDefaultDeclarationKind::ArrowFunctionExpression(arrow) => {
                self.collect_arrow_function(arrow, "default");
            }
            ExportDefaultDeclarationKind::ClassDeclaration(class)
            | ExportDefaultDeclarationKind::ClassExpression(class) => {
                self.collect_class_declaration(class, Some("default"));
            }
            ExportDefaultDeclarationKind::ObjectExpression(object) => {
                self.collect_object_methods(object, "default");
            }
            _ => {}
        }
    }

    fn collect_variable_declaration(&mut self, declaration: &VariableDeclaration<'_>) {
        for declarator in &declaration.declarations {
            let Some(initializer) = &declarator.init else {
                continue;
            };
            let is_supported_container = matches!(
                initializer,
                oxc_ast::ast::Expression::FunctionExpression(_)
                    | oxc_ast::ast::Expression::ArrowFunctionExpression(_)
                    | oxc_ast::ast::Expression::ClassExpression(_)
                    | oxc_ast::ast::Expression::ObjectExpression(_)
            );
            if !is_supported_container {
                continue;
            }

            let Some(binding) = declarator.id.get_binding_identifier() else {
                if matches!(
                    initializer,
                    oxc_ast::ast::Expression::FunctionExpression(_)
                        | oxc_ast::ast::Expression::ArrowFunctionExpression(_)
                ) {
                    self.classified_spans
                        .insert((initializer.span().start, initializer.span().end));
                    self.record_skip(SimilarCodeExtractionSkipReason::UnsupportedFunctionForm, 1);
                }
                continue;
            };
            match initializer {
                oxc_ast::ast::Expression::FunctionExpression(function) => {
                    self.collect_function_expression(function, binding.name.as_str());
                }
                oxc_ast::ast::Expression::ArrowFunctionExpression(arrow) => {
                    self.collect_arrow_function(arrow, binding.name.as_str());
                }
                oxc_ast::ast::Expression::ClassExpression(class) => {
                    self.collect_class(class, binding.name.as_str());
                }
                oxc_ast::ast::Expression::ObjectExpression(object) => {
                    self.collect_object_methods(object, binding.name.as_str());
                }
                _ => {}
            }
        }
    }

    fn collect_class_declaration(&mut self, class: &Class<'_>, fallback: Option<&str>) {
        let Some(name) = class
            .id
            .as_ref()
            .map(|identifier| identifier.name.as_str())
            .or(fallback)
        else {
            return;
        };
        self.collect_class(class, name);
    }

    fn collect_class(&mut self, class: &Class<'_>, class_name: &str) {
        for element in &class.body.body {
            let ClassElement::MethodDefinition(method) = element else {
                continue;
            };
            self.classified_spans
                .insert((method.value.span.start, method.value.span.end));
            if method.value.body.is_none() {
                self.record_skip(SimilarCodeExtractionSkipReason::DeclarationWithoutBody, 1);
                continue;
            }
            if method.kind != MethodDefinitionKind::Method || method.computed {
                self.record_skip(SimilarCodeExtractionSkipReason::UnsupportedFunctionForm, 1);
                continue;
            }
            let Some(method_name) = method.key.static_name() else {
                self.record_skip(SimilarCodeExtractionSkipReason::UnsupportedFunctionForm, 1);
                continue;
            };
            self.retain_function(
                &format!("{class_name}.{method_name}"),
                SimilarCodeFunctionKind::ClassMethod,
                method.span,
                ReviewMetadata::for_function(&method.value, self.syntax_reliable),
            );
        }
    }

    fn collect_object_methods(&mut self, object: &ObjectExpression<'_>, binding_name: &str) {
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            if !property.method {
                continue;
            }
            let oxc_ast::ast::Expression::FunctionExpression(function) = &property.value else {
                continue;
            };
            self.classified_spans
                .insert((function.span.start, function.span.end));
            if function.body.is_none() {
                self.record_skip(SimilarCodeExtractionSkipReason::DeclarationWithoutBody, 1);
                continue;
            }
            if property.kind != PropertyKind::Init || property.computed {
                self.record_skip(SimilarCodeExtractionSkipReason::UnsupportedFunctionForm, 1);
                continue;
            }
            let Some(method_name) = property.key.static_name() else {
                self.record_skip(SimilarCodeExtractionSkipReason::UnsupportedFunctionForm, 1);
                continue;
            };
            self.retain_function(
                &format!("{binding_name}.{method_name}"),
                SimilarCodeFunctionKind::ObjectMethod,
                property.span,
                ReviewMetadata::for_function(function, self.syntax_reliable),
            );
        }
    }

    fn collect_function_declaration(&mut self, function: &Function<'_>, fallback: Option<&str>) {
        self.classified_spans
            .insert((function.span.start, function.span.end));
        let Some(_body) = &function.body else {
            self.record_skip(SimilarCodeExtractionSkipReason::DeclarationWithoutBody, 1);
            return;
        };
        let Some(name) = function
            .id
            .as_ref()
            .map(|identifier| identifier.name.as_str())
            .or(fallback)
        else {
            self.record_skip(SimilarCodeExtractionSkipReason::UnsupportedFunctionForm, 1);
            return;
        };
        self.retain_function(
            name,
            SimilarCodeFunctionKind::FunctionDeclaration,
            function.span,
            ReviewMetadata::for_function(function, self.syntax_reliable),
        );
    }

    fn collect_function_expression(&mut self, function: &Function<'_>, name: &str) {
        self.classified_spans
            .insert((function.span.start, function.span.end));
        if function.body.is_none() {
            self.record_skip(SimilarCodeExtractionSkipReason::DeclarationWithoutBody, 1);
            return;
        }
        self.retain_function(
            name,
            SimilarCodeFunctionKind::FunctionExpression,
            function.span,
            ReviewMetadata::for_function(function, self.syntax_reliable),
        );
    }

    fn collect_arrow_function(&mut self, arrow: &ArrowFunctionExpression<'_>, name: &str) {
        self.classified_spans
            .insert((arrow.span.start, arrow.span.end));
        self.retain_function(
            name,
            SimilarCodeFunctionKind::ArrowFunction,
            arrow.span,
            ReviewMetadata::for_arrow(arrow, self.syntax_reliable),
        );
    }

    fn retain_function(
        &mut self,
        name: &str,
        kind: SimilarCodeFunctionKind,
        span: Span,
        metadata: ReviewMetadata,
    ) {
        let start = span.start as usize;
        let end = span.end as usize;
        let Some(source) = self.source.get(start..end) else {
            self.record_skip(SimilarCodeExtractionSkipReason::InvalidSourceSpan, 1);
            return;
        };
        if source.len() > self.limits.max_source_bytes_per_function {
            self.record_skip(
                SimilarCodeExtractionSkipReason::SourceBytesPerFunctionLimit,
                1,
            );
            return;
        }
        if self.functions.len() >= self.limits.max_functions {
            self.record_skip(SimilarCodeExtractionSkipReason::FunctionLimit, 1);
            return;
        }
        if self.source_bytes.saturating_add(source.len()) > self.limits.max_total_source_bytes {
            self.record_skip(SimilarCodeExtractionSkipReason::TotalSourceBytesLimit, 1);
            return;
        }
        let Some((start_line, start_column_utf8)) =
            utf8_line_col(self.source, &self.line_offsets, span.start)
        else {
            self.record_skip(SimilarCodeExtractionSkipReason::InvalidSourceSpan, 1);
            return;
        };
        let Some((end_line, end_column_utf8)) =
            utf8_line_col(self.source, &self.line_offsets, span.end)
        else {
            self.record_skip(SimilarCodeExtractionSkipReason::InvalidSourceSpan, 1);
            return;
        };

        let source_sha256 = SimilarCodeSourceDigest::new(Sha256::digest(source.as_bytes()).into());
        self.source_bytes = self.source_bytes.saturating_add(source.len());
        self.functions.push(ExtractedSimilarCodeFunction {
            name: name.to_string(),
            kind,
            location: SimilarCodeFunctionLocation {
                file: self.file.clone(),
                start_byte: span.start,
                end_byte: span.end,
                start_line,
                start_column_utf8,
                end_line,
                end_column_utf8,
            },
            source_sha256,
            source: source.to_string(),
            param_count: metadata.param_count,
            is_async: metadata.is_async,
            is_generator: metadata.is_generator,
            has_await: metadata.has_await,
            has_throw: metadata.has_throw,
            side_effect_hint: metadata.side_effect_hint,
        });
    }

    fn record_skip(&mut self, reason: SimilarCodeExtractionSkipReason, count: usize) {
        let value = self.skips.entry(reason).or_default();
        *value = value.saturating_add(count);
    }

    fn finish(mut self, program: &Program<'_>) -> SimilarCodeExtraction {
        let mut visitor = UnsupportedFunctionVisitor::new(&self.classified_spans);
        visitor.visit_program(program);
        for (reason, count) in visitor.skips {
            self.record_skip(reason, count);
        }
        build_result(self.functions, self.source_bytes, self.skips)
    }
}

struct UnsupportedFunctionVisitor<'a> {
    classified_spans: &'a BTreeSet<(u32, u32)>,
    function_depth: usize,
    skips: FxHashMap<SimilarCodeExtractionSkipReason, usize>,
}

impl<'a> UnsupportedFunctionVisitor<'a> {
    fn new(classified_spans: &'a BTreeSet<(u32, u32)>) -> Self {
        Self {
            classified_spans,
            function_depth: 0,
            skips: FxHashMap::default(),
        }
    }

    fn record_unclassified(&mut self, span: Span, has_body: bool) {
        if self.classified_spans.contains(&(span.start, span.end)) {
            return;
        }
        let reason = if !has_body {
            SimilarCodeExtractionSkipReason::DeclarationWithoutBody
        } else if self.function_depth > 0 {
            SimilarCodeExtractionSkipReason::NestedFunction
        } else {
            SimilarCodeExtractionSkipReason::UnsupportedFunctionForm
        };
        let value = self.skips.entry(reason).or_default();
        *value = value.saturating_add(1);
    }
}

impl<'ast> Visit<'ast> for UnsupportedFunctionVisitor<'_> {
    fn visit_function(&mut self, function: &Function<'ast>, flags: ScopeFlags) {
        self.record_unclassified(function.span, function.body.is_some());
        self.function_depth = self.function_depth.saturating_add(1);
        walk::walk_function(self, function, flags);
        self.function_depth = self.function_depth.saturating_sub(1);
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'ast>) {
        self.record_unclassified(arrow.span, true);
        self.function_depth = self.function_depth.saturating_add(1);
        walk::walk_arrow_function_expression(self, arrow);
        self.function_depth = self.function_depth.saturating_sub(1);
    }
}

/// Extract supported named top-level functions from one standalone JS or TS source.
///
/// Supported forms are named function declarations, simple identifier bindings
/// initialized with a function expression or arrow, and equivalent default
/// exports. Statically named ordinary methods on top-level named classes and
/// bound object literals are also supported. Nested functions, callbacks,
/// constructors, accessors, computed methods, declaration files, and generated
/// sources are omitted with typed skip evidence. Source fragments are exact
/// post-BOM UTF-8 slices and are bounded before hashing or allocation.
#[must_use]
pub fn extract_similar_code_functions(
    path: &Path,
    source: &str,
    limits: SimilarCodeExtractionLimits,
) -> SimilarCodeExtraction {
    let Some(file) = path.to_str() else {
        return single_skip_result(SimilarCodeExtractionSkipReason::NonUtf8Path);
    };
    let file = file.replace('\\', "/");
    if is_declaration_file(&file) {
        return single_skip_result(SimilarCodeExtractionSkipReason::DeclarationFile);
    }
    let Some(source_type) = supported_source_type(path) else {
        return single_skip_result(SimilarCodeExtractionSkipReason::UnsupportedFileType);
    };

    let source = crate::strip_bom(source);
    if is_generated_source(&file, source) {
        return single_skip_result(SimilarCodeExtractionSkipReason::GeneratedSource);
    }

    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let syntax_reliable = parsed.diagnostics.is_empty();
    let mut builder = ExtractionBuilder::new(file, source, limits, syntax_reliable);
    builder.record_skip(
        SimilarCodeExtractionSkipReason::SyntaxDiagnostic,
        parsed.diagnostics.len(),
    );
    builder.collect_program(&parsed.program);
    builder.finish(&parsed.program)
}

fn supported_source_type(path: &Path) -> Option<SourceType> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts"
    )
    .then(|| SourceType::from_path(path).unwrap_or_default())
}

fn is_declaration_file(file: &str) -> bool {
    let file = file.to_ascii_lowercase();
    file.ends_with(".d.ts") || file.ends_with(".d.mts") || file.ends_with(".d.cts")
}

fn is_generated_source(file: &str, source: &str) -> bool {
    let lower_file = file.to_ascii_lowercase();
    let generated_path = lower_file.split('/').any(|part| {
        matches!(part, "generated" | "__generated__")
            || part.contains(".generated.")
            || part.contains(".gen.")
    });
    if generated_path {
        return true;
    }

    let header = source
        .chars()
        .take(2_048)
        .collect::<String>()
        .to_ascii_lowercase();
    header.contains("@generated")
        || (header.contains("do not edit")
            && (header.contains("code generated") || header.contains("automatically generated")))
}

fn utf8_line_col(source: &str, line_offsets: &[u32], byte_offset: u32) -> Option<(u32, u32)> {
    let line_index = match line_offsets.binary_search(&byte_offset) {
        Ok(index) => index,
        Err(index) => index.saturating_sub(1),
    };
    let line_start = *line_offsets.get(line_index)? as usize;
    let column = source
        .get(line_start..byte_offset as usize)?
        .chars()
        .count();
    Some((
        u32::try_from(line_index).ok()?.saturating_add(1),
        u32::try_from(column).ok()?,
    ))
}

fn single_skip_result(reason: SimilarCodeExtractionSkipReason) -> SimilarCodeExtraction {
    build_result(Vec::new(), 0, FxHashMap::from_iter([(reason, 1)]))
}

fn build_result(
    functions: Vec<ExtractedSimilarCodeFunction>,
    source_bytes: usize,
    skips: FxHashMap<SimilarCodeExtractionSkipReason, usize>,
) -> SimilarCodeExtraction {
    let mut skipped = skips
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(reason, count)| SimilarCodeExtractionSkip { reason, count })
        .collect::<Vec<_>>();
    skipped.sort_by_key(|skip| skip.reason);
    SimilarCodeExtraction {
        extraction_semantics_version: SIMILAR_CODE_EXTRACTION_SEMANTICS_VERSION,
        functions,
        source_bytes,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(file: &str, source: &str) -> SimilarCodeExtraction {
        extract_similar_code_functions(
            Path::new(file),
            source,
            SimilarCodeExtractionLimits::default(),
        )
    }

    fn skip_count(
        result: &SimilarCodeExtraction,
        reason: SimilarCodeExtractionSkipReason,
    ) -> usize {
        result
            .skipped
            .iter()
            .find(|skip| skip.reason == reason)
            .map_or(0, |skip| skip.count)
    }

    #[test]
    fn extracts_supported_top_level_forms_in_source_order() {
        let source = r"
function declared(value: number) { return value + 1; }
const expression = function named(value: number) { return value + 2; };
export const arrow = (value: number) => value + 3;
export default function fallback(value: number) { return value + 4; }
";
        let result = extract("src/forms.ts", source);

        assert_eq!(
            result
                .functions
                .iter()
                .map(|function| (function.name.as_str(), function.kind))
                .collect::<Vec<_>>(),
            vec![
                ("declared", SimilarCodeFunctionKind::FunctionDeclaration),
                ("expression", SimilarCodeFunctionKind::FunctionExpression),
                ("arrow", SimilarCodeFunctionKind::ArrowFunction),
                ("fallback", SimilarCodeFunctionKind::FunctionDeclaration),
            ]
        );
        assert!(result.skipped.is_empty());
        assert_eq!(
            result.source_bytes,
            result
                .functions
                .iter()
                .map(|function| function.source.len())
                .sum::<usize>()
        );
    }

    #[test]
    fn records_exact_utf8_span_source_and_full_sha256() {
        let source = "const label = '🙂'; const naïve = (waarde: string) => {\n  return `${label}:${waarde}`;\n};\n";
        let result = extract("src/utf8.ts", source);
        let function = &result.functions[0];
        let expected_start = source.find("(waarde").unwrap();
        let expected_end = source.rfind("};\n").map_or(source.len(), |index| index + 1);
        let expected_source = &source[expected_start..expected_end];

        assert_eq!(function.name, "naïve");
        assert_eq!(function.location.start_byte as usize, expected_start);
        assert_eq!(
            function.location.start_column_utf8 as usize,
            source[..expected_start].chars().count()
        );
        assert_eq!(function.source, expected_source);
        assert_eq!(
            function.source_sha256,
            SimilarCodeSourceDigest::new(Sha256::digest(expected_source.as_bytes()).into())
        );
    }

    #[test]
    fn extracts_named_methods_and_skips_nested_or_unsupported_methods() {
        let source = r#"
function outer() {
  const nested = () => 1;
  return nested();
}
class Service {
  constructor() {}
  get value() { return 1; }
  set value(next) {}
  ["computed"]() {}
  method(value) { return value + 2; }
  static helper() { return 3; }
}
const object = {
  get value() { return 1; },
  ["computed"]() {},
  method(value) { return value + 3; },
};
"#;
        let result = extract("src/scopes.ts", source);

        assert_eq!(
            result
                .functions
                .iter()
                .map(|function| (function.name.as_str(), function.kind))
                .collect::<Vec<_>>(),
            vec![
                ("outer", SimilarCodeFunctionKind::FunctionDeclaration),
                ("Service.method", SimilarCodeFunctionKind::ClassMethod),
                ("Service.helper", SimilarCodeFunctionKind::ClassMethod),
                ("object.method", SimilarCodeFunctionKind::ObjectMethod),
            ]
        );
        assert_eq!(
            skip_count(&result, SimilarCodeExtractionSkipReason::NestedFunction),
            1
        );
        assert_eq!(
            skip_count(
                &result,
                SimilarCodeExtractionSkipReason::UnsupportedFunctionForm
            ),
            6
        );
    }

    #[test]
    fn records_conservative_review_metadata_without_nested_leakage() {
        let source = r#"
async function load(input, ...rest) {
  await fetch(input);
  if (rest.length > 0) throw new Error("unexpected");
}
function* generate(value) { yield value; }
function pure(left, right) { return left + right; }
function wrapper() {
  const nested = async () => { await fetch("nested"); throw new Error("nested"); };
  return 1;
}
"#;
        let result = extract("src/metadata.ts", source);
        let find = |name: &str| {
            result
                .functions
                .iter()
                .find(|function| function.name == name)
                .unwrap()
        };

        let load = find("load");
        assert_eq!(load.param_count, 2);
        assert!(load.is_async);
        assert!(!load.is_generator);
        assert!(load.has_await);
        assert!(load.has_throw);
        assert_eq!(
            load.side_effect_hint,
            SimilarCodeSideEffectHint::MayHaveSideEffects
        );

        let generate = find("generate");
        assert!(generate.is_generator);
        assert_eq!(
            generate.side_effect_hint,
            SimilarCodeSideEffectHint::MayHaveSideEffects
        );

        let pure = find("pure");
        assert_eq!(pure.param_count, 2);
        assert_eq!(
            pure.side_effect_hint,
            SimilarCodeSideEffectHint::PureLooking
        );

        let wrapper = find("wrapper");
        assert!(!wrapper.has_await);
        assert!(!wrapper.has_throw);
        assert_eq!(
            wrapper.side_effect_hint,
            SimilarCodeSideEffectHint::PureLooking
        );
    }

    #[test]
    fn parser_recovery_marks_review_metadata_unknown() {
        let result = extract(
            "src/recovered.ts",
            "function recovered(value) { return value; }\nreturn;",
        );

        assert_eq!(result.functions.len(), 1);
        assert_eq!(
            result.functions[0].side_effect_hint,
            SimilarCodeSideEffectHint::Unknown
        );
        assert!(skip_count(&result, SimilarCodeExtractionSkipReason::SyntaxDiagnostic) > 0);
    }

    #[test]
    fn generated_and_declaration_sources_are_excluded_before_parsing() {
        let generated = extract(
            "src/__generated__/client.ts",
            "export const call = () => 1;",
        );
        assert_eq!(
            generated.skipped,
            vec![SimilarCodeExtractionSkip {
                reason: SimilarCodeExtractionSkipReason::GeneratedSource,
                count: 1,
            }]
        );

        let declaration = extract("src/api.d.ts", "export declare function call(): void;");
        assert_eq!(
            declaration.skipped,
            vec![SimilarCodeExtractionSkip {
                reason: SimilarCodeExtractionSkipReason::DeclarationFile,
                count: 1,
            }]
        );
    }

    #[test]
    fn source_payload_limits_fail_closed_with_typed_counts() {
        let source = "const first = () => 'this payload is definitely too large';\nconst second = () => 2;\nconst third = () => 3;\n";
        let result = extract_similar_code_functions(
            Path::new("src/limits.ts"),
            source,
            SimilarCodeExtractionLimits {
                max_functions: 1,
                max_source_bytes_per_function: 22,
                max_total_source_bytes: 22,
            },
        );

        assert_eq!(result.functions.len(), 1);
        assert_eq!(
            skip_count(
                &result,
                SimilarCodeExtractionSkipReason::SourceBytesPerFunctionLimit
            ),
            1
        );
        assert_eq!(
            skip_count(&result, SimilarCodeExtractionSkipReason::FunctionLimit),
            1
        );
        assert!(result.source_bytes <= 22);
    }
}
