//! Security control classifier and declarative validation capture helpers.

#[allow(
    clippy::wildcard_imports,
    reason = "many declarative validation AST types used"
)]
use oxc_ast::ast::*;
use oxc_span::Span;

use fallow_types::extract::{SecurityControlKind, SecurityControlSite};

use super::{
    ModuleInfoExtractor, callee_leaf_name, expression_has_boundary_validation_keys,
    expression_has_fastify_schema, flatten_callee_path,
};

pub(super) fn security_control_kind_for_callee(callee_path: &str) -> Option<SecurityControlKind> {
    let lower = callee_path.to_ascii_lowercase();
    let leaf = lower.rsplit('.').next().unwrap_or(lower.as_str());
    if is_validation_control(&lower, leaf) {
        return Some(SecurityControlKind::Validation);
    }
    if is_authorization_control(leaf) {
        return Some(SecurityControlKind::Authorization);
    }
    if is_authentication_control(leaf) {
        return Some(SecurityControlKind::Authentication);
    }
    if is_sanitization_control(&lower, leaf) {
        return Some(SecurityControlKind::Sanitization);
    }
    None
}

fn is_validation_control(callee_path: &str, leaf: &str) -> bool {
    (matches!(
        leaf,
        "parse" | "safeparse" | "validate" | "validateasync" | "assert" | "check" | "is"
    ) && matches!(
        control_object(callee_path),
        Some("z")
            | Some("zod")
            | Some("joi")
            | Some("yup")
            | Some("valibot")
            | Some("v")
            | Some("superstruct")
            | Some("schema")
    )) || matches!(leaf, "validatesync" | "parseasync" | "safeparseasync")
}

fn is_authentication_control(leaf: &str) -> bool {
    leaf == "authenticate"
        || leaf == "ensureauthenticated"
        || leaf == "requireauth"
        || leaf == "requireuser"
        || leaf == "authguard"
        || leaf == "verifytoken"
        || leaf == "auth"
}

fn is_authorization_control(leaf: &str) -> bool {
    leaf == "authorize"
        || leaf == "requirepermission"
        || leaf == "requirepermissions"
        || leaf == "requirerole"
        || leaf == "can"
        || leaf == "permit"
        || leaf == "enforce"
}

fn is_sanitization_control(callee_path: &str, leaf: &str) -> bool {
    leaf == "sanitize"
        || leaf == "escape"
        || leaf == "escaperegexp"
        || callee_path.ends_with(".sanitize")
}

fn control_object(callee_path: &str) -> Option<&str> {
    callee_path
        .rsplit_once('.')
        .map(|(object, _)| object.rsplit('.').next().unwrap_or(object))
}

pub(super) fn import_source_matches_package(source: &str, package: &str) -> bool {
    source == package
        || source
            .strip_prefix(package)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub(super) fn route_method_leaf(callee_path: &str) -> Option<&str> {
    let leaf = callee_path.rsplit('.').next()?;
    matches!(
        leaf,
        "route" | "get" | "post" | "put" | "patch" | "delete" | "all" | "head" | "options"
    )
    .then_some(leaf)
}

impl ModuleInfoExtractor {
    fn record_validation_control_site(&mut self, callee_path: &str, span: Span) {
        self.security_control_sites.push(SecurityControlSite {
            kind: SecurityControlKind::Validation,
            callee_path: callee_path.to_string(),
            span_start: span.start,
            span_end: span.end,
        });
    }

    fn has_package_evidence(&self, package: &str) -> bool {
        self.imports.iter().any(|import| {
            !import.is_type_only && import_source_matches_package(&import.source, package)
        }) || self
            .require_calls
            .iter()
            .any(|require| import_source_matches_package(&require.source, package))
    }

    pub(super) fn capture_declarative_validation_control(&mut self, expr: &CallExpression<'_>) {
        let Some(callee_path) =
            flatten_callee_path(&expr.callee).or_else(|| callee_leaf_name(&expr.callee))
        else {
            return;
        };

        if self.is_elysia_validation_route(&callee_path, expr) {
            self.record_validation_control_site("elysia.route.validation", expr.span);
            return;
        }

        if self.is_fastify_validation_route(&callee_path, expr) {
            self.record_validation_control_site("fastify.route.schema", expr.span);
            return;
        }

        if self.is_trpc_input_control(&callee_path, expr) {
            self.record_validation_control_site("trpc.procedure.input", expr.span);
            return;
        }

        if self.is_hono_validation_middleware(&callee_path, expr) {
            self.record_validation_control_site("hono.validator", expr.span);
            return;
        }

        if self.is_nest_validation_pipe_call(&callee_path) {
            self.record_validation_control_site("nestjs.validation-pipe", expr.span);
            return;
        }

        if self.is_express_validator_control(&callee_path, expr) {
            self.record_validation_control_site("express-validator.middleware", expr.span);
        }
    }

    pub(super) fn capture_declarative_validation_new_expression(
        &mut self,
        expr: &NewExpression<'_>,
    ) {
        let Some(callee_path) = flatten_callee_path(&expr.callee) else {
            return;
        };
        if self.has_package_evidence("@nestjs/common")
            && callee_path.rsplit('.').next() == Some("ValidationPipe")
        {
            self.record_validation_control_site("nestjs.validation-pipe", expr.span);
        }
    }

    fn is_elysia_validation_route(&self, callee_path: &str, expr: &CallExpression<'_>) -> bool {
        self.has_package_evidence("elysia")
            && route_method_leaf(callee_path).is_some()
            && expr
                .arguments
                .iter()
                .skip(1)
                .filter_map(Argument::as_expression)
                .any(expression_has_boundary_validation_keys)
    }

    fn is_fastify_validation_route(&self, callee_path: &str, expr: &CallExpression<'_>) -> bool {
        self.has_package_evidence("fastify")
            && route_method_leaf(callee_path).is_some()
            && expr
                .arguments
                .iter()
                .filter_map(Argument::as_expression)
                .any(expression_has_fastify_schema)
    }

    fn is_trpc_input_control(&self, callee_path: &str, expr: &CallExpression<'_>) -> bool {
        let lower = callee_path.to_ascii_lowercase();
        self.has_package_evidence("@trpc/server")
            && callee_path
                .rsplit('.')
                .next()
                .is_some_and(|leaf| leaf.eq_ignore_ascii_case("input"))
            && lower.contains("procedure")
            && !expr.arguments.is_empty()
    }

    fn is_hono_validation_middleware(&self, callee_path: &str, expr: &CallExpression<'_>) -> bool {
        (self.has_package_evidence("hono") || self.has_package_evidence("@hono/zod-validator"))
            && matches!(
                callee_path.rsplit('.').next(),
                Some("validator" | "zValidator")
            )
            && !expr.arguments.is_empty()
    }

    fn is_nest_validation_pipe_call(&self, callee_path: &str) -> bool {
        self.has_package_evidence("@nestjs/common")
            && matches!(
                callee_path.rsplit('.').next(),
                Some("UsePipes" | "ValidationPipe")
            )
    }

    fn is_express_validator_control(&self, callee_path: &str, expr: &CallExpression<'_>) -> bool {
        self.has_package_evidence("express-validator")
            && matches!(
                callee_path.rsplit('.').next(),
                Some("body" | "param" | "query" | "check")
            )
            && !expr.arguments.is_empty()
    }
}
