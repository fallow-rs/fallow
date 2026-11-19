//! Asset imports of `` html`...` `` tagged templates.
//!
//! SSR helpers such as `hono/html`, `lit-html` and `htm` render page markup
//! from an `html` tagged template. The `src` and `href` values of its
//! `<script>` and `<link>` tags are imports, as in an `.html` file.

use oxc_ast::ast::{
    Argument, Declaration, Expression, ImportDeclarationSpecifier, Program, Statement,
    TemplateElement, VariableDeclarator,
};
use oxc_span::Span;

use crate::asset_url::normalize_asset_url;
use crate::html::collect_asset_ref_spans;
use crate::js_text::JsText;
use crate::visitor::ModuleInfoExtractor;
use crate::{ImportInfo, ImportedName};

/// The identifier of the tagged template that renders HTML markup.
const HTML_TEMPLATE_TAG: &str = "html";

/// Returns true when the tagged template's tag is the bare identifier `html`.
pub(super) fn is_html_tagged_template(tag: &Expression<'_>) -> bool {
    matches!(tag, Expression::Identifier(id) if id.name == HTML_TEMPLATE_TAG)
}

impl ModuleInfoExtractor {
    /// Records whether the module binds the `html` tag to a value of the
    /// project. An HTML template package (`lit`, `hono/html`, `htm`) gives a
    /// tag that renders markup. A tag that the module declares, or that it
    /// imports from a project file, is often `String.raw`, so its templates
    /// are plain text and their `src` and `href` values are not imports.
    pub(super) fn record_html_tag_provenance(&mut self, program: &Program<'_>) {
        self.html_tag_is_project_value = program.body.iter().any(statement_binds_project_html_tag);
    }

    /// Push the asset references of one template quasi onto `imports`, with
    /// the remote-url filter and the normalization of the HTML parser. Each
    /// import points at its quoted attribute value. When an escape makes the
    /// cooked text differ from the source text, the offsets do not match the
    /// source, so the import points at the start of the template text. The
    /// parser also turns CRLF into LF in the raw text, so a raw text that is
    /// shorter than its span gets the same fallback.
    pub(super) fn push_html_template_asset_imports(
        &mut self,
        quasi: &TemplateElement<'_>,
        text: &str,
    ) {
        if self.html_tag_is_project_value {
            return;
        }
        let raw = quasi.value.raw.as_str();
        let exact = raw == text && quasi.span.size() as usize == raw.len();
        for asset in collect_asset_ref_spans(text) {
            let (span, source_span) = if exact {
                let quoted = Span::new(
                    quasi.span.start + to_u32(asset.quoted.start),
                    quasi.span.start + to_u32(asset.quoted.end),
                );
                (quoted, quoted)
            } else {
                (Span::empty(quasi.span.start), Span::default())
            };
            self.imports.push(ImportInfo {
                source: normalize_asset_url(&asset.value),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span,
                source_span,
            });
        }
    }
}

fn statement_binds_project_html_tag(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::ImportDeclaration(import) => {
            is_project_specifier(&import.source.value.text())
                && import.specifiers.iter().flatten().any(|specifier| {
                    let local = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(named) => &named.local,
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(default) => {
                            &default.local
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                            &namespace.local
                        }
                    };
                    local.name == HTML_TEMPLATE_TAG
                })
        }
        Statement::ExportDeclaration(export) => {
            declaration_binds_project_html_tag(&export.declaration)
        }
        _ => statement
            .as_declaration()
            .is_some_and(declaration_binds_project_html_tag),
    }
}

fn declaration_binds_project_html_tag(declaration: &Declaration<'_>) -> bool {
    match declaration {
        Declaration::VariableDeclaration(variables) => variables
            .declarations
            .iter()
            .any(declarator_binds_project_html_tag),
        Declaration::FunctionDeclaration(function) => function
            .id
            .as_ref()
            .is_some_and(|id| id.name == HTML_TEMPLATE_TAG),
        Declaration::ClassDeclaration(class) => class
            .id
            .as_ref()
            .is_some_and(|id| id.name == HTML_TEMPLATE_TAG),
        _ => false,
    }
}

/// `const { html } = require("lit-html")` binds the tag of a package. Every
/// other declarator of `html` binds a value of the project.
fn declarator_binds_project_html_tag(declarator: &VariableDeclarator<'_>) -> bool {
    let binds_tag = declarator
        .id
        .get_binding_identifiers()
        .iter()
        .any(|id| id.name == HTML_TEMPLATE_TAG);
    binds_tag && !requires_package(declarator.init.as_ref())
}

fn requires_package(init: Option<&Expression<'_>>) -> bool {
    let Some(Expression::CallExpression(call)) = init else {
        return false;
    };
    let Expression::Identifier(callee) = &call.callee else {
        return false;
    };
    callee.name == "require"
        && matches!(
            call.arguments.first(),
            Some(Argument::StringLiteral(source)) if !is_project_specifier(&source.value.text())
        )
}

/// A relative or absolute path names a file of the project, not a package.
fn is_project_specifier(source: &str) -> bool {
    source.starts_with('.') || source.starts_with('/')
}

/// A template quasi is part of a source file, and a source file is shorter
/// than `u32::MAX` bytes.
fn to_u32(offset: usize) -> u32 {
    u32::try_from(offset).unwrap_or(u32::MAX)
}
