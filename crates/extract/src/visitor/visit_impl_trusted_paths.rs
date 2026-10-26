//! Trusted module-relative path expressions (issue #3711).
//!
//! A path that starts from the module location (`import.meta.url`,
//! `import.meta.dirname`, `import.meta.filename`, `__dirname`, `__filename`)
//! or the process working directory (`process.cwd()`), and adds only literal
//! segments, cannot carry caller input. Security sink capture treats such an
//! argument like a literal, so it does not become a path-traversal candidate.
//!
//! The rule is narrow on purpose: `fileURLToPath(value)` and
//! `new URL(value, import.meta.url)` stay untrusted unless `value` itself is a
//! literal or a trusted path.

#[allow(clippy::wildcard_imports, reason = "many AST node types used")]
use oxc_ast::ast::*;

use fallow_types::extract::SinkLiteralValue;

use super::super::super::ModuleInfoExtractor;
use super::unwrap_static_expr;

/// `import.meta` properties that hold the module location.
const TRUSTED_IMPORT_META_PROPERTIES: &[&str] = &["url", "dirname", "filename"];

/// CommonJS module-location globals.
const TRUSTED_MODULE_GLOBALS: &[&str] = &["__dirname", "__filename"];

/// `node:path` methods that keep a trusted result when every argument is trusted.
const TRUSTED_PATH_METHODS: &[&str] = &["join", "resolve", "dirname", "normalize"];

/// `URL` members that keep a trusted result when the URL object is trusted.
const TRUSTED_URL_MEMBERS: &[&str] = &["href", "pathname"];

/// The `node:url` helper that converts a file URL to a path.
const FILE_URL_TO_PATH: &str = "fileURLToPath";

/// How a path expression relates to the trusted-path rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathTrust {
    /// The expression can carry a value that is not known.
    Untrusted,
    /// Only literal segments. The literal capture rules handle these.
    LiteralOnly,
    /// At least one trusted base, plus only literal or trusted segments.
    TrustedBase,
}

impl PathTrust {
    /// Combine the trust of two parts of one path expression.
    const fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Untrusted, _) | (_, Self::Untrusted) => Self::Untrusted,
            (Self::TrustedBase, _) | (_, Self::TrustedBase) => Self::TrustedBase,
            (Self::LiteralOnly, Self::LiteralOnly) => Self::LiteralOnly,
        }
    }
}

/// A path helper over literal-only input is not a trusted base: keep the
/// existing capture behavior for it.
const fn require_base(trust: PathTrust) -> PathTrust {
    match trust {
        PathTrust::TrustedBase => PathTrust::TrustedBase,
        PathTrust::LiteralOnly | PathTrust::Untrusted => PathTrust::Untrusted,
    }
}

impl ModuleInfoExtractor {
    /// Whether `expr` is a path built from at least one trusted module-relative
    /// base and only literal or trusted segments (issue #3711). A literal-only
    /// expression returns `false`, so the literal capture rules stay unchanged.
    pub(in crate::visitor) fn is_trusted_static_path_expr(&self, expr: &Expression<'_>) -> bool {
        self.path_trust(expr) == PathTrust::TrustedBase
    }

    fn path_trust(&self, expr: &Expression<'_>) -> PathTrust {
        crate::cost::walk_step();
        if matches!(
            self.static_sink_literal_value(expr),
            Some(SinkLiteralValue::String(_))
        ) {
            return PathTrust::LiteralOnly;
        }
        match unwrap_static_expr(expr) {
            Expression::StringLiteral(_) => PathTrust::LiteralOnly,
            Expression::Identifier(ident) => self.identifier_trust(ident.name.as_str()),
            Expression::StaticMemberExpression(member) => self.member_trust(member),
            Expression::CallExpression(call) => self.call_trust(call),
            Expression::NewExpression(new_expr) => self.module_url_trust(new_expr),
            Expression::TemplateLiteral(template) => template
                .expressions
                .iter()
                .fold(PathTrust::LiteralOnly, |trust, part| {
                    trust.and(self.path_trust(part))
                }),
            Expression::BinaryExpression(binary) if binary.operator == BinaryOperator::Addition => {
                self.path_trust(&binary.left)
                    .and(self.path_trust(&binary.right))
            }
            _ => PathTrust::Untrusted,
        }
    }

    fn identifier_trust(&self, name: &str) -> PathTrust {
        if self.nested_scope_shadows(name) {
            return PathTrust::Untrusted;
        }
        if self.module_trusted_path_bindings.contains(name)
            || (TRUSTED_MODULE_GLOBALS.contains(&name)
                && !self.local_declaration_names.contains(name))
        {
            return PathTrust::TrustedBase;
        }
        PathTrust::Untrusted
    }

    fn member_trust(&self, member: &StaticMemberExpression<'_>) -> PathTrust {
        let property = member.property.name.as_str();
        match unwrap_static_expr(&member.object) {
            Expression::ImportMeta(_) if TRUSTED_IMPORT_META_PROPERTIES.contains(&property) => {
                PathTrust::TrustedBase
            }
            object if TRUSTED_URL_MEMBERS.contains(&property) => {
                require_base(self.path_trust(object))
            }
            _ => PathTrust::Untrusted,
        }
    }

    fn call_trust(&self, call: &CallExpression<'_>) -> PathTrust {
        if self.is_process_cwd_call(call) {
            return PathTrust::TrustedBase;
        }
        let is_path_helper = (self.is_file_url_to_path_callee(&call.callee)
            && call.arguments.len() == 1)
            || (!call.arguments.is_empty()
                && TRUSTED_PATH_METHODS
                    .iter()
                    .any(|method| self.is_node_path_method_call(call, method)));
        if !is_path_helper {
            return PathTrust::Untrusted;
        }
        require_base(self.args_trust(&call.arguments))
    }

    /// `new URL(<literal or trusted>, <trusted base>)` or `new URL(<trusted>)`.
    fn module_url_trust(&self, new_expr: &NewExpression<'_>) -> PathTrust {
        let Expression::Identifier(callee) = unwrap_static_expr(&new_expr.callee) else {
            return PathTrust::Untrusted;
        };
        if callee.name != "URL"
            || self.nested_scope_shadows("URL")
            || !matches!(new_expr.arguments.len(), 1 | 2)
        {
            return PathTrust::Untrusted;
        }
        require_base(self.args_trust(&new_expr.arguments))
    }

    fn is_process_cwd_call(&self, call: &CallExpression<'_>) -> bool {
        let Expression::StaticMemberExpression(member) = unwrap_static_expr(&call.callee) else {
            return false;
        };
        let Expression::Identifier(object) = unwrap_static_expr(&member.object) else {
            return false;
        };
        call.arguments.is_empty()
            && member.property.name == "cwd"
            && object.name == "process"
            && !self.nested_scope_shadows("process")
            && !self.local_declaration_names.contains("process")
    }

    fn is_file_url_to_path_callee(&self, callee: &Expression<'_>) -> bool {
        match unwrap_static_expr(callee) {
            Expression::Identifier(ident) => {
                ident.name == FILE_URL_TO_PATH && !self.nested_scope_shadows(FILE_URL_TO_PATH)
            }
            Expression::StaticMemberExpression(member) => member.property.name == FILE_URL_TO_PATH,
            _ => false,
        }
    }

    fn args_trust(&self, arguments: &[Argument<'_>]) -> PathTrust {
        arguments.iter().fold(PathTrust::LiteralOnly, |trust, arg| {
            trust.and(
                arg.as_expression()
                    .map_or(PathTrust::Untrusted, |expr| self.path_trust(expr)),
            )
        })
    }
}
