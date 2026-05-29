//! Feature flag detection via lightweight Oxc AST visitor.
//!
//! Detects three patterns:
//! 1. **Environment variables**: `process.env.FEATURE_X`
//! 2. **SDK calls**: `useFlag('name')`, `variation('name', false)`,
//!    `flag({ key: 'name' })`, etc.
//! 3. **Config objects**: `config.features.x` (opt-in, heuristic)
//!
//! Always extracted during parse (lightweight pattern matching on `MemberExpression`
//! and `CallExpression` nodes). Custom SDK patterns and config object heuristics
//! are applied as a supplementary pass in the CLI when user config is present.

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{FlagUse, FlagUseKind, byte_offset_to_line_col};

/// Built-in SDK function patterns: (function_name, name_arg_index, provider_label).
const BUILTIN_SDK_PATTERNS: &[(&str, usize, &str)] = &[
    // LaunchDarkly
    ("useFlag", 0, "LaunchDarkly"),
    ("useLDFlag", 0, "LaunchDarkly"),
    ("useFeatureFlag", 0, "LaunchDarkly"),
    ("variation", 0, "LaunchDarkly"),
    ("boolVariation", 0, "LaunchDarkly"),
    ("stringVariation", 0, "LaunchDarkly"),
    ("numberVariation", 0, "LaunchDarkly"),
    ("jsonVariation", 0, "LaunchDarkly"),
    // Statsig
    ("useGate", 0, "Statsig"),
    ("checkGate", 0, "Statsig"),
    ("useExperiment", 0, "Statsig"),
    ("useConfig", 0, "Statsig"),
    // Unleash
    ("isEnabled", 0, "Unleash"),
    ("getVariant", 0, "Unleash"),
    // GrowthBook
    ("isOn", 0, "GrowthBook"),
    ("isOff", 0, "GrowthBook"),
    ("getFeatureValue", 0, "GrowthBook"),
    // Split
    ("getTreatment", 0, "Split"),
    // PostHog
    ("useFeatureFlagEnabled", 0, "PostHog"),
    ("useFeatureFlagPayload", 0, "PostHog"),
    ("useFeatureFlagVariantKey", 0, "PostHog"),
    ("getFeatureFlagPayload", 0, "PostHog"),
    // ConfigCat
    ("getValueAsync", 0, "ConfigCat"),
    ("getValueDetailsAsync", 0, "ConfigCat"),
    // Flagsmith
    ("hasFeature", 0, "Flagsmith"),
    // Optimizely
    ("useDecision", 0, "Optimizely"),
    ("getFeatureVariable", 0, "Optimizely"),
    ("getFeatureVariableBoolean", 0, "Optimizely"),
    ("getFeatureVariableString", 0, "Optimizely"),
    ("getFeatureVariableInteger", 0, "Optimizely"),
    ("getFeatureVariableDouble", 0, "Optimizely"),
    ("getFeatureVariableJson", 0, "Optimizely"),
    ("getFeatureVariableJSON", 0, "Optimizely"),
    // Eppo
    ("getStringAssignment", 0, "Eppo"),
    ("getBooleanAssignment", 0, "Eppo"),
    ("getNumericAssignment", 0, "Eppo"),
    ("getIntegerAssignment", 0, "Eppo"),
    ("getJSONAssignment", 0, "Eppo"),
    ("getStringAssignmentDetails", 0, "Eppo"),
    ("getBooleanAssignmentDetails", 0, "Eppo"),
    ("getNumericAssignmentDetails", 0, "Eppo"),
    ("getIntegerAssignmentDetails", 0, "Eppo"),
    ("getJSONAssignmentDetails", 0, "Eppo"),
    // Shared: getValue is used by both ConfigCat and Flagsmith.
    // Attribution is best-effort when function names collide.
    ("getValue", 0, ""),
    // Generic
    ("useFeature", 0, ""),
    ("getFeatureFlag", 0, ""),
];

const VERCEL_FLAGS_PROVIDER: &str = "Vercel Flags";
const VERCEL_FLAGS_FUNCTIONS: &[&str] = &["flag", "evaluate"];

/// Built-in environment variable prefixes that indicate feature flags.
const BUILTIN_ENV_PREFIXES: &[&str] = &[
    "FEATURE_",
    "NEXT_PUBLIC_FEATURE_",
    "NEXT_PUBLIC_ENABLE_",
    "REACT_APP_FEATURE_",
    "REACT_APP_ENABLE_",
    "VITE_FEATURE_",
    "VITE_ENABLE_",
    "NUXT_PUBLIC_FEATURE_",
    "ENABLE_",
    "FF_",
    "FLAG_",
    "TOGGLE_",
];

/// Distinct built-in SDK provider labels, in declaration order.
///
/// Used by `fallow flags` to tell the user which SDKs the default detectors
/// cover when no flags are found. Derived from `BUILTIN_SDK_PATTERNS` (empty
/// provider labels skipped) with the import-based Vercel Flags provider appended,
/// so the surfaced list stays in sync with what is actually detected.
#[must_use]
pub fn builtin_sdk_providers() -> Vec<&'static str> {
    let mut providers: Vec<&'static str> = Vec::new();
    for &(_, _, provider) in BUILTIN_SDK_PATTERNS {
        if !provider.is_empty() && !providers.contains(&provider) {
            providers.push(provider);
        }
    }
    // Vercel Flags is detected via import resolution, not BUILTIN_SDK_PATTERNS.
    if !providers.contains(&VERCEL_FLAGS_PROVIDER) {
        providers.push(VERCEL_FLAGS_PROVIDER);
    }
    providers
}

/// Built-in environment variable prefixes treated as feature flags.
///
/// Used by `fallow flags` to surface the default env-prefix detectors in the
/// empty-result hint. Returns the source-of-truth `BUILTIN_ENV_PREFIXES`.
#[must_use]
pub fn builtin_env_prefixes() -> &'static [&'static str] {
    BUILTIN_ENV_PREFIXES
}

/// Config object names that heuristically indicate feature flag namespaces.
const CONFIG_OBJECT_KEYWORDS: &[&str] = &[
    "feature",
    "features",
    "featureFlags",
    "featureFlag",
    "flag",
    "flags",
    "toggle",
    "toggles",
];

/// AST visitor that detects feature flag patterns.
struct FlagVisitor<'a> {
    results: Vec<FlagUse>,
    line_offsets: &'a [u32],
    /// Extra SDK patterns from user config.
    extra_sdk_patterns: &'a [(String, usize, String)],
    /// Extra env prefixes from user config.
    extra_env_prefixes: &'a [String],
    /// Whether to detect config object patterns (opt-in).
    config_object_heuristics: bool,
    /// Local named imports from Vercel Flags packages: local name -> imported name.
    vercel_flags_imports: FxHashMap<String, String>,
    /// Namespace imports from Vercel Flags packages.
    vercel_flags_namespaces: FxHashSet<String>,
}

impl<'a> FlagVisitor<'a> {
    fn new(
        line_offsets: &'a [u32],
        extra_sdk_patterns: &'a [(String, usize, String)],
        extra_env_prefixes: &'a [String],
        config_object_heuristics: bool,
    ) -> Self {
        Self {
            results: Vec::new(),
            line_offsets,
            extra_sdk_patterns,
            extra_env_prefixes,
            config_object_heuristics,
            vercel_flags_imports: FxHashMap::default(),
            vercel_flags_namespaces: FxHashSet::default(),
        }
    }

    /// Check if a member expression matches `process.env.SOMETHING`.
    fn check_env_var(&mut self, expr: &MemberExpression<'_>, guard: Option<(u32, u32)>) {
        // Match: process.env.X (static member)
        if let MemberExpression::StaticMemberExpression(static_expr) = expr
            && let Some(env_name) = extract_process_env_name(static_expr)
            && self.is_flag_env_name(&env_name)
        {
            let (line, col) = byte_offset_to_line_col(self.line_offsets, static_expr.span.start);
            self.results.push(FlagUse {
                flag_name: env_name,
                kind: FlagUseKind::EnvVar,
                line,
                col,
                guard_span_start: guard.map(|(s, _)| s),
                guard_span_end: guard.map(|(_, e)| e),
                sdk_name: None,
            });
        }
    }

    /// Check if a call expression matches an SDK pattern.
    fn check_sdk_call(&mut self, call: &CallExpression<'_>, guard: Option<(u32, u32)>) {
        let func_name = match &call.callee {
            Expression::Identifier(id) => Some(id.name.as_str()),
            Expression::StaticMemberExpression(member) => Some(member.property.name.as_str()),
            _ => None,
        };

        let Some(func_name) = func_name else {
            return;
        };

        if self.check_vercel_flags_call(call, guard) {
            return;
        }

        // Check built-in patterns
        for &(pattern_name, name_arg_idx, provider) in BUILTIN_SDK_PATTERNS {
            if func_name == pattern_name {
                if let Some(flag_name) = extract_string_arg(&call.arguments, name_arg_idx) {
                    let (line, col) = byte_offset_to_line_col(self.line_offsets, call.span.start);
                    self.results.push(FlagUse {
                        flag_name,
                        kind: FlagUseKind::SdkCall,
                        line,
                        col,
                        guard_span_start: guard.map(|(s, _)| s),
                        guard_span_end: guard.map(|(_, e)| e),
                        sdk_name: if provider.is_empty() {
                            None
                        } else {
                            Some(provider.to_string())
                        },
                    });
                }
                return;
            }
        }

        // Check user-configured extra patterns
        for (pattern_name, name_arg_idx, provider) in self.extra_sdk_patterns {
            if func_name == pattern_name {
                if let Some(flag_name) = extract_string_arg(&call.arguments, *name_arg_idx) {
                    let (line, col) = byte_offset_to_line_col(self.line_offsets, call.span.start);
                    self.results.push(FlagUse {
                        flag_name,
                        kind: FlagUseKind::SdkCall,
                        line,
                        col,
                        guard_span_start: guard.map(|(s, _)| s),
                        guard_span_end: guard.map(|(_, e)| e),
                        sdk_name: if provider.is_empty() {
                            None
                        } else {
                            Some(provider.clone())
                        },
                    });
                }
                return;
            }
        }
    }

    fn check_vercel_flags_call(
        &mut self,
        call: &CallExpression<'_>,
        guard: Option<(u32, u32)>,
    ) -> bool {
        let Some(imported_name) = self.vercel_flags_imported_name(call) else {
            return false;
        };

        let flag_name = match imported_name {
            "flag" => extract_object_string_property_arg(&call.arguments, 0, "key"),
            "evaluate" => extract_string_arg(&call.arguments, 0),
            _ => None,
        };

        let Some(flag_name) = flag_name else {
            return false;
        };

        let (line, col) = byte_offset_to_line_col(self.line_offsets, call.span.start);
        self.results.push(FlagUse {
            flag_name,
            kind: FlagUseKind::SdkCall,
            line,
            col,
            guard_span_start: guard.map(|(s, _)| s),
            guard_span_end: guard.map(|(_, e)| e),
            sdk_name: Some(VERCEL_FLAGS_PROVIDER.to_string()),
        });
        true
    }

    fn vercel_flags_imported_name<'b>(&'b self, call: &'b CallExpression<'_>) -> Option<&'b str> {
        match &call.callee {
            Expression::Identifier(id) => self
                .vercel_flags_imports
                .get(id.name.as_str())
                .map(String::as_str),
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(object) = &member.object else {
                    return None;
                };
                self.vercel_flags_namespaces
                    .contains(object.name.as_str())
                    .then_some(member.property.name.as_str())
            }
            _ => None,
        }
    }

    fn collect_vercel_flags_imports(&mut self, program: &Program<'_>) {
        for stmt in &program.body {
            if let Statement::ImportDeclaration(decl) = stmt {
                self.collect_vercel_flags_import(decl);
            }
        }
    }

    fn collect_vercel_flags_import(&mut self, decl: &ImportDeclaration<'_>) {
        if !is_vercel_flags_source(decl.source.value.as_str()) || decl.import_kind.is_type() {
            return;
        }

        let Some(specifiers) = &decl.specifiers else {
            return;
        };

        for spec in specifiers {
            match spec {
                ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                    if specifier.import_kind.is_type() {
                        continue;
                    }
                    let imported_name = specifier.imported.name();
                    if VERCEL_FLAGS_FUNCTIONS.contains(&imported_name.as_str()) {
                        self.vercel_flags_imports
                            .insert(specifier.local.name.to_string(), imported_name.to_string());
                    }
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                    self.vercel_flags_namespaces
                        .insert(specifier.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {}
            }
        }
    }

    /// Check if a member expression matches a config object pattern.
    fn check_config_object(
        &mut self,
        expr: &StaticMemberExpression<'_>,
        guard: Option<(u32, u32)>,
    ) {
        if !self.config_object_heuristics {
            return;
        }

        // Look for patterns like config.features.x, flags.enableNewFeature
        // The object must contain a keyword from CONFIG_OBJECT_KEYWORDS
        if let Some((obj_name, prop_name)) = extract_config_object_access(expr)
            && CONFIG_OBJECT_KEYWORDS
                .iter()
                .any(|kw| obj_name.eq_ignore_ascii_case(kw) || prop_name.eq_ignore_ascii_case(kw))
        {
            let (line, col) = byte_offset_to_line_col(self.line_offsets, expr.span.start);
            self.results.push(FlagUse {
                flag_name: format!("{obj_name}.{prop_name}"),
                kind: FlagUseKind::ConfigObject,
                line,
                col,
                guard_span_start: guard.map(|(s, _)| s),
                guard_span_end: guard.map(|(_, e)| e),
                sdk_name: None,
            });
        }
    }

    fn is_flag_env_name(&self, name: &str) -> bool {
        for prefix in BUILTIN_ENV_PREFIXES {
            if name.starts_with(prefix) {
                return true;
            }
        }
        for prefix in self.extra_env_prefixes {
            if name.starts_with(prefix.as_str()) {
                return true;
            }
        }
        false
    }
}

impl Visit<'_> for FlagVisitor<'_> {
    fn visit_program(&mut self, program: &Program<'_>) {
        self.collect_vercel_flags_imports(program);
        walk::walk_program(self, program);
    }

    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'_>) {
        self.collect_vercel_flags_import(decl);
    }

    fn visit_if_statement(&mut self, stmt: &IfStatement<'_>) {
        let guard = Some((stmt.span.start, stmt.span.end));

        // Check the test expression for flag patterns (with guard context)
        check_expression_for_flags(self, &stmt.test, guard);

        // Visit consequent and alternate, but NOT the test expression again
        self.visit_statement(&stmt.consequent);
        if let Some(alt) = &stmt.alternate {
            self.visit_statement(alt);
        }
    }

    fn visit_conditional_expression(&mut self, expr: &ConditionalExpression<'_>) {
        let guard = Some((expr.span.start, expr.span.end));
        check_expression_for_flags(self, &expr.test, guard);

        // Visit consequent and alternate, but NOT the test expression again
        self.visit_expression(&expr.consequent);
        self.visit_expression(&expr.alternate);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'_>) {
        self.check_sdk_call(call, None);
        walk::walk_call_expression(self, call);
    }

    fn visit_member_expression(&mut self, expr: &MemberExpression<'_>) {
        self.check_env_var(expr, None);
        if let MemberExpression::StaticMemberExpression(static_expr) = expr {
            self.check_config_object(static_expr, None);
        }
        walk::walk_member_expression(self, expr);
    }
}

fn is_vercel_flags_source(source: &str) -> bool {
    source == "flags"
        || source.starts_with("flags/")
        || source == "@vercel/flags"
        || source.starts_with("@vercel/flags/")
}

/// Check an expression (typically an if-test) for flag patterns.
fn check_expression_for_flags(
    visitor: &mut FlagVisitor<'_>,
    expr: &Expression<'_>,
    guard: Option<(u32, u32)>,
) {
    match expr {
        Expression::CallExpression(call) => {
            visitor.check_sdk_call(call, guard);
        }
        Expression::StaticMemberExpression(member) => {
            check_static_member_for_env(visitor, member, guard);
            visitor.check_config_object(member, guard);
        }
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::LogicalNot => {
            check_expression_for_flags(visitor, &unary.argument, guard);
        }
        Expression::LogicalExpression(logical) => {
            check_expression_for_flags(visitor, &logical.left, guard);
            check_expression_for_flags(visitor, &logical.right, guard);
        }
        _ => {}
    }
}

/// Check a static member expression directly for `process.env.X` pattern.
fn check_static_member_for_env(
    visitor: &mut FlagVisitor<'_>,
    expr: &StaticMemberExpression<'_>,
    guard: Option<(u32, u32)>,
) {
    if let Some(env_name) = extract_process_env_name(expr)
        && visitor.is_flag_env_name(&env_name)
    {
        let (line, col) = byte_offset_to_line_col(visitor.line_offsets, expr.span.start);
        visitor.results.push(FlagUse {
            flag_name: env_name,
            kind: FlagUseKind::EnvVar,
            line,
            col,
            guard_span_start: guard.map(|(s, _)| s),
            guard_span_end: guard.map(|(_, e)| e),
            sdk_name: None,
        });
    }
}

/// Extract the environment variable name from `process.env.X`.
fn extract_process_env_name(expr: &StaticMemberExpression<'_>) -> Option<String> {
    // Match: process.env.SOMETHING
    let prop_name = expr.property.name.as_str();

    if let Expression::StaticMemberExpression(inner) = &expr.object
        && inner.property.name.as_str() == "env"
        && let Expression::Identifier(id) = &inner.object
        && id.name.as_str() == "process"
    {
        return Some(prop_name.to_string());
    }

    None
}

/// Extract a string literal argument at the given index.
fn extract_string_arg(args: &[Argument<'_>], index: usize) -> Option<String> {
    args.get(index).and_then(|arg| {
        if let Argument::StringLiteral(lit) = arg {
            Some(lit.value.to_string())
        } else {
            None
        }
    })
}

/// Extract a string property from an object argument at the given index.
fn extract_object_string_property_arg(
    args: &[Argument<'_>],
    index: usize,
    property_name: &str,
) -> Option<String> {
    let Some(Argument::ObjectExpression(obj)) = args.get(index) else {
        return None;
    };

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        if prop
            .key
            .static_name()
            .is_some_and(|key| key.as_ref() == property_name)
            && let Expression::StringLiteral(lit) = &prop.value
        {
            return Some(lit.value.to_string());
        }
    }

    None
}

/// Extract config object access pattern: `obj.prop` where either name is a flag keyword.
fn extract_config_object_access(expr: &StaticMemberExpression<'_>) -> Option<(String, String)> {
    let prop_name = expr.property.name.to_string();

    match &expr.object {
        Expression::Identifier(id) => Some((id.name.to_string(), prop_name)),
        Expression::StaticMemberExpression(inner) => {
            if matches!(&inner.object, Expression::Identifier(_)) {
                // Two-level: config.features.x -> obj="features", prop="x"
                Some((inner.property.name.to_string(), prop_name))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Entry point: extract feature flag use sites from a parsed program.
///
/// Called unconditionally from `parse_source_to_module` for all parsed files.
pub fn extract_flags(
    program: &Program<'_>,
    line_offsets: &[u32],
    extra_sdk_patterns: &[(String, usize, String)],
    extra_env_prefixes: &[String],
    config_object_heuristics: bool,
) -> Vec<FlagUse> {
    let mut visitor = FlagVisitor::new(
        line_offsets,
        extra_sdk_patterns,
        extra_env_prefixes,
        config_object_heuristics,
    );
    visitor.visit_program(program);
    visitor.results
}

/// Extract feature flags from source text with custom configuration.
///
/// Higher-level convenience function that handles parsing internally.
/// Used by the CLI flags command for supplementary extraction with
/// user-configured patterns that aren't applied at parse/cache time.
pub fn extract_flags_from_source(
    source: &str,
    path: &std::path::Path,
    extra_sdk_patterns: &[(String, usize, String)],
    extra_env_prefixes: &[String],
    config_object_heuristics: bool,
) -> Vec<FlagUse> {
    let source_type = oxc_span::SourceType::from_path(path).unwrap_or_default();
    let allocator = oxc_allocator::Allocator::default();
    let parser_return = oxc_parser::Parser::new(&allocator, source, source_type).parse();
    let line_offsets = fallow_types::extract::compute_line_offsets(source);
    extract_flags(
        &parser_return.program,
        &line_offsets,
        extra_sdk_patterns,
        extra_env_prefixes,
        config_object_heuristics,
    )
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn extract_from_source(source: &str) -> Vec<FlagUse> {
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, source, SourceType::tsx()).parse();
        let line_offsets = fallow_types::extract::compute_line_offsets(source);
        extract_flags(&parser_return.program, &line_offsets, &[], &[], false)
    }

    fn extract_with_config_objects(source: &str) -> Vec<FlagUse> {
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, source, SourceType::tsx()).parse();
        let line_offsets = fallow_types::extract::compute_line_offsets(source);
        extract_flags(&parser_return.program, &line_offsets, &[], &[], true)
    }

    // ── Environment variable detection ──────────────────────────────

    #[test]
    fn detects_process_env_feature_flag() {
        let flags = extract_from_source("if (process.env.FEATURE_NEW_CHECKOUT) { doStuff(); }");
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "FEATURE_NEW_CHECKOUT");
        assert_eq!(flags[0].kind, FlagUseKind::EnvVar);
        assert!(flags[0].guard_span_start.is_some());
    }

    #[test]
    fn detects_next_public_enable_prefix() {
        let flags = extract_from_source("if (process.env.NEXT_PUBLIC_ENABLE_BETA) {}");
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "NEXT_PUBLIC_ENABLE_BETA");
    }

    #[test]
    fn ignores_non_flag_env_vars() {
        let flags = extract_from_source("const url = process.env.DATABASE_URL;");
        assert!(flags.is_empty());
    }

    #[test]
    fn detects_negated_env_flag() {
        let flags = extract_from_source("if (!process.env.FEATURE_X) { fallback(); }");
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "FEATURE_X");
    }

    // ── SDK call detection ──────────────────────────────────────────

    #[test]
    fn detects_launchdarkly_use_flag() {
        let flags = extract_from_source("const flag = useFlag('new-checkout');");
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "new-checkout");
        assert_eq!(flags[0].kind, FlagUseKind::SdkCall);
        assert_eq!(flags[0].sdk_name.as_deref(), Some("LaunchDarkly"));
    }

    #[test]
    fn detects_statsig_use_gate() {
        let flags = extract_from_source("if (useGate('beta-feature')) {}");
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "beta-feature");
        assert_eq!(flags[0].sdk_name.as_deref(), Some("Statsig"));
    }

    #[test]
    fn detects_unleash_is_enabled() {
        let flags = extract_from_source("client.isEnabled('feature-x')");
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "feature-x");
    }

    #[test]
    fn detects_growthbook_get_feature_value() {
        let flags = extract_from_source("const val = getFeatureValue('parser', false);");
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "parser");
        assert_eq!(flags[0].sdk_name.as_deref(), Some("GrowthBook"));
    }

    #[test]
    fn detects_posthog_hooks() {
        let flags = extract_from_source(
            "const enabled = useFeatureFlagEnabled('new-checkout');\n\
             const payload = useFeatureFlagPayload('checkout-copy');\n\
             const variant = useFeatureFlagVariantKey('pricing-test');",
        );

        let names: Vec<_> = flags.iter().map(|flag| flag.flag_name.as_str()).collect();
        assert_eq!(names, ["new-checkout", "checkout-copy", "pricing-test"]);
        assert!(
            flags
                .iter()
                .all(|flag| flag.sdk_name.as_deref() == Some("PostHog"))
        );
    }

    #[test]
    fn detects_vercel_flags_object_key_and_core_evaluate_from_imports() {
        let flags = extract_from_source(
            "import { flag, evaluate as evalFlag } from 'flags/next';\n\
             export const showSale = flag({ key: 'summer-sale', decide: () => false });\n\
             const value = await evalFlag('show-new-feature', false);",
        );

        let names: Vec<_> = flags.iter().map(|flag| flag.flag_name.as_str()).collect();
        assert_eq!(names, ["summer-sale", "show-new-feature"]);
        assert!(
            flags
                .iter()
                .all(|flag| flag.sdk_name.as_deref() == Some("Vercel Flags"))
        );
    }

    #[test]
    fn detects_vercel_flags_namespace_imports() {
        let flags = extract_from_source(
            "import * as vercelFlags from '@vercel/flags';\n\
             const value = await vercelFlags.evaluate('show-new-feature', false);\n\
             export const showSale = vercelFlags.flag({ key: 'summer-sale', decide: () => false });",
        );

        let names: Vec<_> = flags.iter().map(|flag| flag.flag_name.as_str()).collect();
        assert_eq!(names, ["show-new-feature", "summer-sale"]);
        assert!(
            flags
                .iter()
                .all(|flag| flag.sdk_name.as_deref() == Some("Vercel Flags"))
        );
    }

    #[test]
    fn detects_vercel_flags_calls_before_import_declaration() {
        let flags = extract_from_source(
            "export const showSale = flag({ key: 'summer-sale', decide: () => false });\n\
             import { flag } from 'flags/next';",
        );

        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "summer-sale");
        assert_eq!(flags[0].sdk_name.as_deref(), Some("Vercel Flags"));
    }

    #[test]
    fn ignores_unimported_vercel_like_function_names() {
        let flags = extract_from_source(
            "function math() { return evaluate('2 + 2'); }\n\
             function marker() { return flag({ key: 'ui-row' }); }",
        );

        assert!(flags.is_empty());
    }

    #[test]
    fn detects_configcat_detail_evaluation() {
        let flags = extract_from_source(
            "const details = await client.getValueDetailsAsync('new-checkout', false);",
        );
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "new-checkout");
        assert_eq!(flags[0].sdk_name.as_deref(), Some("ConfigCat"));
    }

    #[test]
    fn detects_optimizely_decisions_and_variables() {
        let flags = extract_from_source(
            "const [decision] = useDecision('checkout-flow');\n\
             const copy = optimizelyClient.getFeatureVariableString('checkout-flow', 'copy', userId, attrs);\n\
             const json = optimizelyClient.getFeatureVariableJson('checkout-flow', 'json', userId, attrs);",
        );

        assert_eq!(flags.len(), 3);
        assert!(flags.iter().all(|flag| flag.flag_name == "checkout-flow"));
        assert!(
            flags
                .iter()
                .all(|flag| flag.sdk_name.as_deref() == Some("Optimizely"))
        );
    }

    #[test]
    fn detects_eppo_typed_assignments() {
        let flags = extract_from_source(
            "const value = client.getBooleanAssignment('new-onboarding', subject, {}, false);\n\
             const details = client.getStringAssignmentDetails('copy-test', subject, {}, 'control');\n\
             const payload = client.getJSONAssignmentDetails('payload-test', subject, {}, {});",
        );

        let names: Vec<_> = flags.iter().map(|flag| flag.flag_name.as_str()).collect();
        assert_eq!(names, ["new-onboarding", "copy-test", "payload-test"]);
        assert!(
            flags
                .iter()
                .all(|flag| flag.sdk_name.as_deref() == Some("Eppo"))
        );
    }

    #[test]
    fn ignores_sdk_call_without_string_arg() {
        let flags = extract_from_source("useFlag(dynamicKey);");
        assert!(flags.is_empty());
    }

    // ── Config object detection (opt-in) ────────────────────────────

    #[test]
    fn config_objects_off_by_default() {
        let flags = extract_from_source("if (config.features.newCheckout) {}");
        assert!(flags.is_empty());
    }

    #[test]
    fn detects_config_features_when_enabled() {
        let flags = extract_with_config_objects("if (config.features.newCheckout) {}");
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "features.newCheckout");
        assert_eq!(flags[0].kind, FlagUseKind::ConfigObject);
    }

    #[test]
    fn detects_flags_object() {
        let flags = extract_with_config_objects("if (flags.enableV2) {}");
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "flags.enableV2");
    }

    #[test]
    fn ignores_non_flag_config_object() {
        let flags = extract_with_config_objects("const host = config.database.host;");
        assert!(flags.is_empty());
    }

    // ── Guard span detection ────────────────────────────────────────

    #[test]
    fn captures_if_guard_span() {
        let source = "if (process.env.FEATURE_X) {\n  doStuff();\n}";
        let flags = extract_from_source(source);
        assert_eq!(flags.len(), 1);
        assert!(flags[0].guard_span_start.is_some());
        assert!(flags[0].guard_span_end.is_some());
    }

    #[test]
    fn captures_ternary_guard_span() {
        let source = "const x = useFlag('beta') ? newFlow() : oldFlow();";
        let flags = extract_from_source(source);
        assert_eq!(flags.len(), 1);
        assert!(flags[0].guard_span_start.is_some());
    }

    // ── Custom SDK patterns ─────────────────────────────────────────

    #[test]
    fn detects_custom_sdk_pattern() {
        let allocator = Allocator::default();
        let source = "isFeatureActive('my-flag');";
        let parser_return = Parser::new(&allocator, source, SourceType::tsx()).parse();
        let line_offsets = fallow_types::extract::compute_line_offsets(source);
        let custom = vec![("isFeatureActive".to_string(), 0, "Internal".to_string())];
        let flags = extract_flags(&parser_return.program, &line_offsets, &custom, &[], false);
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "my-flag");
        assert_eq!(flags[0].sdk_name.as_deref(), Some("Internal"));
    }

    #[test]
    fn custom_sdk_pattern_can_use_vercel_object_function_name() {
        let allocator = Allocator::default();
        let source = "flag('internal-flag');";
        let parser_return = Parser::new(&allocator, source, SourceType::tsx()).parse();
        let line_offsets = fallow_types::extract::compute_line_offsets(source);
        let custom = vec![("flag".to_string(), 0, "Internal".to_string())];
        let flags = extract_flags(&parser_return.program, &line_offsets, &custom, &[], false);
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "internal-flag");
        assert_eq!(flags[0].sdk_name.as_deref(), Some("Internal"));
    }

    // ── Custom env prefixes ─────────────────────────────────────────

    #[test]
    fn detects_custom_env_prefix() {
        let allocator = Allocator::default();
        let source = "if (process.env.MYAPP_ENABLE_V2) {}";
        let parser_return = Parser::new(&allocator, source, SourceType::tsx()).parse();
        let line_offsets = fallow_types::extract::compute_line_offsets(source);
        let custom_prefixes = vec!["MYAPP_ENABLE_".to_string()];
        let flags = extract_flags(
            &parser_return.program,
            &line_offsets,
            &[],
            &custom_prefixes,
            false,
        );
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "MYAPP_ENABLE_V2");
    }

    // ── Built-in detector accessors (surfaced in the empty-result hint) ──

    #[test]
    fn builtin_sdk_providers_are_distinct_and_ordered() {
        let providers = builtin_sdk_providers();
        assert!(!providers.is_empty());
        // Distinct: no duplicate labels.
        let mut sorted = providers.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            providers.len(),
            "providers must be distinct: {providers:?}"
        );
        // Empty provider labels (generic patterns like `getValue`) are skipped.
        assert!(
            !providers.contains(&""),
            "empty provider labels must not leak into the surfaced list"
        );
        // Declaration order: LaunchDarkly first (first entry in BUILTIN_SDK_PATTERNS).
        assert_eq!(providers.first(), Some(&"LaunchDarkly"));
        // Vercel Flags is detected separately and appended last.
        assert_eq!(providers.last(), Some(&VERCEL_FLAGS_PROVIDER));
    }

    #[test]
    fn builtin_env_prefixes_match_source_constant() {
        let prefixes = builtin_env_prefixes();
        assert_eq!(prefixes, BUILTIN_ENV_PREFIXES);
        assert!(prefixes.contains(&"FEATURE_"));
        assert!(prefixes.contains(&"TOGGLE_"));
    }
}
