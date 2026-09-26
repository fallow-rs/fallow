//! Feature flag detection via lightweight Oxc AST visitor.
//!
//! Detects three patterns:
//! 1. **Environment variables**: `process.env.FEATURE_X`, `import.meta.env.VITE_FEATURE_X`
//! 2. **SDK calls**: `useFlag('name')`, `variation('name', false)`,
//!    `flag({ key: 'name' })`, etc.
//! 3. **Config objects**: `config.features.x` (opt-in, heuristic)
//!
//! Always extracted during parse (lightweight pattern matching on `MemberExpression`
//! and `CallExpression` nodes). Custom SDK patterns, env prefixes, and config
//! object heuristics from the `flags` config section apply in the same pass.

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;
use oxc_span::ContentEq;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{
    FlagConstant, FlagConstantRead, FlagKeyRegistry, FlagPatterns, FlagRegistryFacts,
    FlagRegistryRead, FlagSiteFacts, FlagUse, FlagUseKind, byte_offset_to_line_col,
};
use oxc_semantic::ScopeFlags;

/// Built-in SDK function patterns: (function_name, name_arg_index, provider_label).
const BUILTIN_SDK_PATTERNS: &[(&str, usize, &str)] = &[
    ("useFlag", 0, "LaunchDarkly"),
    ("useLDFlag", 0, "LaunchDarkly"),
    ("useFeatureFlag", 0, "LaunchDarkly"),
    ("variation", 0, "LaunchDarkly"),
    ("boolVariation", 0, "LaunchDarkly"),
    ("stringVariation", 0, "LaunchDarkly"),
    ("numberVariation", 0, "LaunchDarkly"),
    ("jsonVariation", 0, "LaunchDarkly"),
    ("useGate", 0, "Statsig"),
    ("checkGate", 0, "Statsig"),
    ("useExperiment", 0, "Statsig"),
    ("useConfig", 0, "Statsig"),
    ("isEnabled", 0, "Unleash"),
    ("getVariant", 0, "Unleash"),
    ("isOn", 0, "GrowthBook"),
    ("isOff", 0, "GrowthBook"),
    ("getFeatureValue", 0, "GrowthBook"),
    ("getTreatment", 0, "Split"),
    ("useFeatureFlagEnabled", 0, "PostHog"),
    ("useFeatureFlagPayload", 0, "PostHog"),
    ("useFeatureFlagVariantKey", 0, "PostHog"),
    ("getFeatureFlagPayload", 0, "PostHog"),
    ("getValueAsync", 0, "ConfigCat"),
    ("getValueDetailsAsync", 0, "ConfigCat"),
    ("hasFeature", 0, "Flagsmith"),
    ("useDecision", 0, "Optimizely"),
    ("getFeatureVariable", 0, "Optimizely"),
    ("getFeatureVariableBoolean", 0, "Optimizely"),
    ("getFeatureVariableString", 0, "Optimizely"),
    ("getFeatureVariableInteger", 0, "Optimizely"),
    ("getFeatureVariableDouble", 0, "Optimizely"),
    ("getFeatureVariableJson", 0, "Optimizely"),
    ("getFeatureVariableJSON", 0, "Optimizely"),
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
    ("getValue", 0, ""),
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

/// A flag read that a later guard can attach to.
#[derive(Debug, Clone, Copy)]
enum FlagRef {
    /// Index into `FlagVisitor::results`.
    Resolved(usize),
    /// Index into `FlagVisitor::registry_reads`.
    Registry(usize),
}

/// The flag-name argument of an SDK call.
enum FlagNameArg {
    /// A string literal: `useFlag('new-checkout')`.
    Literal(String),
    /// A registry member: `useFlag(FLAGS.NewCheckout)`.
    RegistryMember { registry: String, member: String },
}

/// A guarded byte range and the facts about its branches.
#[derive(Debug, Clone, Copy)]
struct Guard {
    start: u32,
    end: u32,
    facts: FlagSiteFacts,
}

/// AST visitor that detects feature flag patterns.
struct FlagVisitor<'a> {
    results: Vec<FlagUse>,
    /// Reads whose key is a member of an imported registry.
    registry_reads: Vec<FlagRegistryRead>,
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
    /// Module-level registries: binding name -> (member, flag key) pairs.
    local_registries: FxHashMap<String, Vec<(String, String)>>,
    /// Local names of the value bindings that named imports create.
    named_imports: FxHashSet<String>,
    /// Registries the module exports.
    exported_registries: Vec<FlagKeyRegistry>,
    /// Module-level literal `const` bindings with a flag-style name.
    constants: Vec<FlagConstant>,
    /// Binding name -> index into `constants`.
    literal_consts: FxHashMap<String, usize>,
    /// Guard of the test expression the visitor is in, if any.
    current_guard: Option<Guard>,
    /// End offsets of the enclosing blocks, innermost last.
    block_ends: Vec<u32>,
    /// `const` bindings per function scope, innermost last. `None` marks a
    /// binding that shadows an outer flag binding.
    binding_scopes: Vec<FxHashMap<String, Option<FlagRef>>>,
    /// Number of flag bindings recorded. Zero skips the binding bookkeeping.
    flag_binding_count: usize,
    /// Registry names that a parameter or a local binding shadows, per
    /// function scope and block, innermost last. Empty at module level.
    shadowed_registries: Vec<FxHashSet<String>>,
    /// The most recent read the visitor recorded.
    last_ref: Option<FlagRef>,
    /// Start offset of the most recent read.
    last_read_start: Option<u32>,
    /// The declarators under visit belong to a `const` declaration.
    in_const_declaration: bool,
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
            registry_reads: Vec::new(),
            line_offsets,
            extra_sdk_patterns,
            extra_env_prefixes,
            config_object_heuristics,
            vercel_flags_imports: FxHashMap::default(),
            vercel_flags_namespaces: FxHashSet::default(),
            local_registries: FxHashMap::default(),
            named_imports: FxHashSet::default(),
            exported_registries: Vec::new(),
            constants: Vec::new(),
            literal_consts: FxHashMap::default(),
            current_guard: None,
            block_ends: Vec::new(),
            binding_scopes: vec![FxHashMap::default()],
            flag_binding_count: 0,
            shadowed_registries: Vec::new(),
            last_ref: None,
            last_read_start: None,
            in_const_declaration: false,
        }
    }

    fn read_count(&self) -> usize {
        self.results.len() + self.registry_reads.len()
    }

    fn new_flag_use(
        &self,
        flag_name: String,
        kind: FlagUseKind,
        offset: u32,
        sdk_name: Option<String>,
    ) -> FlagUse {
        let (line, col) = byte_offset_to_line_col(self.line_offsets, offset);
        FlagUse {
            flag_name,
            kind,
            line,
            col,
            guard_span_start: self.current_guard.map(|guard| guard.start),
            guard_span_end: self.current_guard.map(|guard| guard.end),
            sdk_name,
            facts: self
                .current_guard
                .map_or_else(FlagSiteFacts::default, |guard| guard.facts),
        }
    }

    fn push_flag_use(
        &mut self,
        flag_name: String,
        kind: FlagUseKind,
        offset: u32,
        sdk_name: Option<String>,
    ) {
        let flag_use = self.new_flag_use(flag_name, kind, offset, sdk_name);
        self.last_ref = Some(FlagRef::Resolved(self.results.len()));
        self.last_read_start = Some(offset);
        self.results.push(flag_use);
    }

    /// Check if a member expression reads `process.env.X` or `import.meta.env.X`.
    fn check_env_var(&mut self, expr: &StaticMemberExpression<'_>) {
        if let Some(env_name) = extract_env_name(expr)
            && self.is_flag_env_name(env_name)
        {
            self.push_flag_use(
                env_name.to_string(),
                FlagUseKind::EnvVar,
                expr.span.start,
                None,
            );
        }
    }

    /// Check if a call expression matches an SDK pattern.
    fn check_sdk_call(&mut self, call: &CallExpression<'_>) {
        let func_name = match &call.callee {
            Expression::Identifier(id) => id.name.as_str(),
            Expression::StaticMemberExpression(member) => member.property.name.as_str(),
            _ => return,
        };

        if self.check_vercel_flags_call(call) {
            return;
        }

        let extra_sdk_patterns: &'a [(String, usize, String)] = self.extra_sdk_patterns;
        let pattern = BUILTIN_SDK_PATTERNS
            .iter()
            .find(|(name, _, _)| *name == func_name)
            .map(|&(_, name_arg_idx, provider)| (name_arg_idx, provider))
            .or_else(|| {
                extra_sdk_patterns
                    .iter()
                    .find(|(name, _, _)| name == func_name)
                    .map(|(_, name_arg_idx, provider)| (*name_arg_idx, provider.as_str()))
            });
        let Some((name_arg_idx, provider)) = pattern else {
            return;
        };
        let sdk_name = (!provider.is_empty()).then(|| provider.to_string());

        match extract_flag_name_arg(&call.arguments, name_arg_idx) {
            Some(FlagNameArg::Literal(flag_name)) => {
                self.push_flag_use(flag_name, FlagUseKind::SdkCall, call.span.start, sdk_name);
            }
            Some(FlagNameArg::RegistryMember { registry, member }) => {
                self.record_registry_read(registry, member, call.span.start, sdk_name);
            }
            None => {}
        }
    }

    /// Record a read whose key is `registry.member`. A module-level registry
    /// resolves at once. A named import waits for project analysis, which
    /// resolves it through the import. Any other name is not a registry.
    fn record_registry_read(
        &mut self,
        registry: String,
        member: String,
        offset: u32,
        sdk_name: Option<String>,
    ) {
        if self
            .shadowed_registries
            .iter()
            .any(|names| names.contains(&registry))
        {
            return;
        }
        if let Some(members) = self.local_registries.get(&registry) {
            let key = members
                .iter()
                .find(|(name, _)| *name == member)
                .map(|(_, key)| key.clone());
            if let Some(key) = key {
                self.push_flag_use(key, FlagUseKind::SdkCall, offset, sdk_name);
            }
            return;
        }
        if !self.named_imports.contains(&registry) {
            return;
        }
        let flag_use = self.new_flag_use(String::new(), FlagUseKind::SdkCall, offset, sdk_name);
        self.last_ref = Some(FlagRef::Registry(self.registry_reads.len()));
        self.last_read_start = Some(offset);
        self.registry_reads.push(FlagRegistryRead {
            registry,
            member,
            flag_use,
        });
    }

    fn check_vercel_flags_call(&mut self, call: &CallExpression<'_>) -> bool {
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

        self.push_flag_use(
            flag_name,
            FlagUseKind::SdkCall,
            call.span.start,
            Some(VERCEL_FLAGS_PROVIDER.to_string()),
        );
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

    fn collect_imports(&mut self, program: &Program<'_>) {
        for stmt in &program.body {
            if let Statement::ImportDeclaration(decl) = stmt {
                self.collect_vercel_flags_import(decl);
                self.collect_named_imports(decl);
            }
        }
    }

    fn collect_named_imports(&mut self, decl: &ImportDeclaration<'_>) {
        if decl.import_kind.is_type() {
            return;
        }
        for spec in decl.specifiers.iter().flatten() {
            if let ImportDeclarationSpecifier::ImportSpecifier(specifier) = spec
                && !specifier.import_kind.is_type()
            {
                self.named_imports.insert(specifier.local.name.to_string());
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

    /// Collect module-level `as const` objects and enums with string members,
    /// and note which of them the module exports.
    fn collect_flag_registries(&mut self, program: &Program<'_>) {
        let mut exports: Vec<(String, String)> = Vec::new();
        for stmt in &program.body {
            match stmt {
                Statement::VariableDeclaration(decl) => {
                    self.collect_const_object_registries(decl);
                    self.collect_literal_constants(decl);
                }
                Statement::TSEnumDeclaration(enumd) => {
                    self.collect_enum_registry(enumd);
                }
                Statement::ExportDeclaration(export) => {
                    if let Declaration::VariableDeclaration(decl) = &export.declaration {
                        self.collect_literal_constants(decl);
                    }
                    let declared = self.collect_declared_registries(&export.declaration);
                    exports.extend(declared.into_iter().map(|name| (name.clone(), name)));
                }
                Statement::ExportNamedDeclaration(export) => {
                    collect_exported_names(export, &mut exports);
                }
                _ => {}
            }
        }
        for (local, exported) in exports {
            if let Some(members) = self.local_registries.get(&local) {
                self.exported_registries.push(FlagKeyRegistry {
                    export_name: exported,
                    members: members.clone(),
                });
            }
        }
    }

    /// Record the registries that an `export <declaration>` declares, and
    /// return their names.
    fn collect_declared_registries(&mut self, declaration: &Declaration<'_>) -> Vec<String> {
        match declaration {
            Declaration::VariableDeclaration(decl) => self.collect_const_object_registries(decl),
            Declaration::TSEnumDeclaration(enumd) => {
                self.collect_enum_registry(enumd).into_iter().collect()
            }
            _ => Vec::new(),
        }
    }

    /// Record each `const NAME = { ... } as const` registry in `decl` and
    /// return the registry names.
    fn collect_const_object_registries(&mut self, decl: &VariableDeclaration<'_>) -> Vec<String> {
        let mut names = Vec::new();
        if !decl.kind.is_const() {
            return names;
        }
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                continue;
            };
            let Some(members) = declarator.init.as_ref().and_then(as_const_object_members) else {
                continue;
            };
            let name = id.name.to_string();
            self.local_registries.insert(name.clone(), members);
            names.push(name);
        }
        names
    }

    /// Record each `const NAME = <literal>` in `decl` whose name has a flag
    /// prefix. A bundler `define` replaces free identifiers only, so a
    /// declared binding keeps the value in the source.
    fn collect_literal_constants(&mut self, decl: &VariableDeclaration<'_>) {
        if !decl.kind.is_const() {
            return;
        }
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                continue;
            };
            if !self.is_flag_env_name(id.name.as_str()) {
                continue;
            }
            let Some(value) = declarator.init.as_ref().and_then(literal_source) else {
                continue;
            };
            let (line, col) = byte_offset_to_line_col(self.line_offsets, id.span.start);
            self.literal_consts
                .insert(id.name.to_string(), self.constants.len());
            self.constants.push(FlagConstant {
                name: id.name.to_string(),
                value,
                line,
                col,
                reads: Vec::new(),
            });
        }
    }

    /// Record a guard test that reads a literal constant.
    fn record_constant_read(&mut self, ident: &IdentifierReference<'_>, guard: Guard) {
        let name = ident.name.as_str();
        let Some(&index) = self.literal_consts.get(name) else {
            return;
        };
        if self
            .shadowed_registries
            .iter()
            .any(|names| names.contains(name))
        {
            return;
        }
        let (line, col) = byte_offset_to_line_col(self.line_offsets, ident.span.start);
        if let Some(constant) = self.constants.get_mut(index) {
            constant.reads.push(FlagConstantRead {
                line,
                col,
                facts: guard.facts,
            });
        }
    }

    /// Record an enum with string members as a registry and return its name.
    fn collect_enum_registry(&mut self, enumd: &TSEnumDeclaration<'_>) -> Option<String> {
        let members: Vec<(String, String)> = enumd
            .body
            .members
            .iter()
            .filter_map(|member| {
                let value = string_value(member.initializer.as_ref()?)?;
                let name = match &member.id {
                    TSEnumMemberName::Identifier(id) => id.name.to_string(),
                    TSEnumMemberName::String(name) | TSEnumMemberName::ComputedString(name) => {
                        name.value.to_string()
                    }
                    TSEnumMemberName::ComputedTemplateString(_) => return None,
                };
                Some((name, value))
            })
            .collect();
        if members.is_empty() {
            return None;
        }
        let name = enumd.id.name.to_string();
        self.local_registries.insert(name.clone(), members);
        Some(name)
    }

    /// Check if a member expression matches a config object pattern.
    fn check_config_object(&mut self, expr: &StaticMemberExpression<'_>) -> bool {
        if !self.config_object_heuristics {
            return false;
        }

        let Some((obj_name, prop_name)) = extract_config_object_access(expr) else {
            return false;
        };
        if !CONFIG_OBJECT_KEYWORDS
            .iter()
            .any(|kw| obj_name.eq_ignore_ascii_case(kw) || prop_name.eq_ignore_ascii_case(kw))
        {
            return false;
        }
        self.push_flag_use(
            format!("{obj_name}.{prop_name}"),
            FlagUseKind::ConfigObject,
            expr.span.start,
            None,
        );
        true
    }

    fn is_flag_env_name(&self, name: &str) -> bool {
        BUILTIN_ENV_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
            || self
                .extra_env_prefixes
                .iter()
                .any(|prefix| name.starts_with(prefix.as_str()))
    }

    /// Walk a test expression with `guard` as the guard of every read in it.
    fn visit_guard_test<'b>(&mut self, test: &Expression<'b>, guard: Guard)
    where
        Self: Visit<'b>,
    {
        let outer = self.current_guard.replace(guard);
        self.visit_expression(test);
        self.current_guard = outer;
    }

    fn lookup_binding(&self, name: &str) -> Option<FlagRef> {
        if self.flag_binding_count == 0 {
            return None;
        }
        self.binding_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .copied()
            .flatten()
    }

    fn bind(&mut self, name: &str, flag_ref: Option<FlagRef>) {
        if flag_ref.is_none() && self.lookup_binding(name).is_none() {
            return;
        }
        if flag_ref.is_some() {
            self.flag_binding_count += 1;
        }
        if let Some(scope) = self.binding_scopes.last_mut() {
            scope.insert(name.to_string(), flag_ref);
        }
    }

    /// Give a bound read the first guard that tests its binding.
    fn attach_guard(&mut self, flag_ref: FlagRef, guard: Guard) {
        let flag_use = match flag_ref {
            FlagRef::Resolved(index) => self.results.get_mut(index),
            FlagRef::Registry(index) => self
                .registry_reads
                .get_mut(index)
                .map(|read| &mut read.flag_use),
        };
        if let Some(flag_use) = flag_use
            && flag_use.guard_span_start.is_none()
        {
            flag_use.guard_span_start = Some(guard.start);
            flag_use.guard_span_end = Some(guard.end);
            flag_use.facts = guard.facts;
        }
    }

    fn visit_function_scope(&mut self, walk_scope: impl FnOnce(&mut Self)) {
        self.binding_scopes.push(FxHashMap::default());
        self.shadowed_registries.push(FxHashSet::default());
        walk_scope(self);
        self.shadowed_registries.pop();
        self.binding_scopes.pop();
    }

    /// Note a parameter or a local binding that has the name of a registry.
    /// Module-level bindings declare the registries, so they do not count.
    fn note_binding(&mut self, name: &str) {
        let is_registry_name = self.local_registries.contains_key(name)
            || self.named_imports.contains(name)
            || self.literal_consts.contains_key(name);
        if !is_registry_name {
            return;
        }
        if let Some(names) = self.shadowed_registries.last_mut() {
            names.insert(name.to_string());
        }
    }

    fn visit_block(&mut self, end: u32, walk_block: impl FnOnce(&mut Self)) {
        self.block_ends.push(end);
        walk_block(self);
        self.block_ends.pop();
    }
}

impl<'a> Visit<'a> for FlagVisitor<'_> {
    fn visit_program(&mut self, program: &Program<'a>) {
        self.collect_imports(program);
        self.collect_flag_registries(program);
        self.visit_block(program.span.end, |visitor| {
            walk::walk_program(visitor, program);
        });
    }

    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        self.collect_vercel_flags_import(decl);
    }

    fn visit_if_statement(&mut self, stmt: &IfStatement<'a>) {
        // `if (!flag) return;` guards the rest of the enclosing block, not
        // only the `if` statement.
        let guard_end = if stmt.alternate.is_none()
            && is_negated_test(&stmt.test)
            && exits_block(&stmt.consequent)
        {
            self.block_ends
                .last()
                .map_or(stmt.span.end, |&end| end.max(stmt.span.end))
        } else {
            stmt.span.end
        };
        let facts = FlagSiteFacts::default()
            .with_empty_branch(
                is_empty_statement(&stmt.consequent)
                    || stmt.alternate.as_ref().is_some_and(is_empty_statement),
            )
            .with_identical_branches(
                stmt.alternate
                    .as_ref()
                    .is_some_and(|alternate| stmt.consequent.content_eq(alternate)),
            );
        self.visit_guard_test(
            &stmt.test,
            Guard {
                start: stmt.span.start,
                end: guard_end,
                facts,
            },
        );

        self.visit_statement(&stmt.consequent);
        if let Some(alt) = &stmt.alternate {
            self.visit_statement(alt);
        }
    }

    fn visit_conditional_expression(&mut self, expr: &ConditionalExpression<'a>) {
        let facts = FlagSiteFacts::default()
            .with_empty_branch(
                is_empty_value(&expr.consequent, &expr.alternate)
                    || is_empty_value(&expr.alternate, &expr.consequent),
            )
            .with_identical_branches(expr.consequent.content_eq(&expr.alternate));
        self.visit_guard_test(
            &expr.test,
            Guard {
                start: expr.span.start,
                end: expr.span.end,
                facts,
            },
        );

        self.visit_expression(&expr.consequent);
        self.visit_expression(&expr.alternate);
    }

    fn visit_logical_expression(&mut self, expr: &LogicalExpression<'a>) {
        if expr.operator == LogicalOperator::And && is_jsx(&expr.right) {
            let facts = FlagSiteFacts::default().with_empty_branch(is_empty_fragment(&expr.right));
            self.visit_guard_test(
                &expr.left,
                Guard {
                    start: expr.span.start,
                    end: expr.span.end,
                    facts,
                },
            );
            self.visit_expression(&expr.right);
            return;
        }
        walk::walk_logical_expression(self, expr);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        self.check_sdk_call(call);
        walk::walk_call_expression(self, call);
    }

    fn visit_member_expression(&mut self, expr: &MemberExpression<'a>) {
        if let MemberExpression::StaticMemberExpression(static_expr) = expr {
            self.check_env_var(static_expr);
            // The object of a matched config access (`config.features` in
            // `config.features.x`) is part of the same read.
            if self.check_config_object(static_expr) {
                return;
            }
        }
        walk::walk_member_expression(self, expr);
    }

    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        let Some(guard) = self.current_guard else {
            return;
        };
        if let Some(flag_ref) = self.lookup_binding(ident.name.as_str()) {
            self.attach_guard(flag_ref, guard);
        }
        if !self.literal_consts.is_empty() {
            self.record_constant_read(ident, guard);
        }
    }

    fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'a>) {
        let outer = std::mem::replace(&mut self.in_const_declaration, decl.kind.is_const());
        walk::walk_variable_declaration(self, decl);
        self.in_const_declaration = outer;
    }

    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'a>) {
        let before = self.read_count();
        walk::walk_variable_declarator(self, decl);
        let BindingPattern::BindingIdentifier(id) = &decl.id else {
            return;
        };
        let flag_ref = (self.in_const_declaration
            && self.read_count() == before + 1
            && decl.init.as_ref().and_then(flag_value_read_start) == self.last_read_start)
            .then_some(self.last_ref)
            .flatten();
        self.bind(id.name.as_str(), flag_ref);
    }

    fn visit_function(&mut self, func: &Function<'a>, flags: ScopeFlags) {
        self.visit_function_scope(|visitor| walk::walk_function(visitor, func, flags));
    }

    fn visit_arrow_function_expression(&mut self, func: &ArrowFunctionExpression<'a>) {
        self.visit_function_scope(|visitor| walk::walk_arrow_function_expression(visitor, func));
    }

    fn visit_function_body(&mut self, body: &FunctionBody<'a>) {
        self.visit_block(body.span.end, |visitor| {
            walk::walk_function_body(visitor, body);
        });
    }

    fn visit_block_statement(&mut self, block: &BlockStatement<'a>) {
        self.shadowed_registries.push(FxHashSet::default());
        self.visit_block(block.span.end, |visitor| {
            walk::walk_block_statement(visitor, block);
        });
        self.shadowed_registries.pop();
    }

    fn visit_binding_identifier(&mut self, ident: &BindingIdentifier<'a>) {
        self.note_binding(ident.name.as_str());
    }
}

fn is_vercel_flags_source(source: &str) -> bool {
    source == "flags"
        || source.starts_with("flags/")
        || source == "@vercel/flags"
        || source.starts_with("@vercel/flags/")
}

/// Strip parentheses and TypeScript-only wrappers that do not change a value.
fn unwrap_value<'b, 'a>(mut expr: &'b Expression<'a>) -> &'b Expression<'a> {
    loop {
        expr = match expr {
            Expression::ParenthesizedExpression(inner) => &inner.expression,
            Expression::TSAsExpression(inner) => &inner.expression,
            Expression::TSSatisfiesExpression(inner) => &inner.expression,
            Expression::TSNonNullExpression(inner) => &inner.expression,
            _ => return expr,
        };
    }
}

fn is_jsx(expr: &Expression<'_>) -> bool {
    matches!(
        unwrap_value(expr),
        Expression::JSXElement(_) | Expression::JSXFragment(_)
    )
}

/// Whether a branch statement does nothing: `;` or `{}`.
fn is_empty_statement(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::EmptyStatement(_) => true,
        Statement::BlockStatement(block) => block.body.is_empty(),
        _ => false,
    }
}

/// Whether a ternary arm renders or yields nothing: `null`, `undefined`,
/// `void 0`, `<></>`, or `false` when the other arm is JSX.
fn is_empty_value(arm: &Expression<'_>, other: &Expression<'_>) -> bool {
    match unwrap_value(arm) {
        Expression::NullLiteral(_) => true,
        Expression::Identifier(id) => id.name == "undefined",
        Expression::UnaryExpression(unary) => unary.operator == UnaryOperator::Void,
        Expression::BooleanLiteral(boolean) => !boolean.value && is_jsx(other),
        _ => is_empty_fragment(arm),
    }
}

/// Whether an expression is a JSX fragment without children.
fn is_empty_fragment(expr: &Expression<'_>) -> bool {
    matches!(unwrap_value(expr), Expression::JSXFragment(fragment) if fragment.children.is_empty())
}

fn is_negated_test(expr: &Expression<'_>) -> bool {
    matches!(
        unwrap_value(expr),
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::LogicalNot
    )
}

/// Whether a statement always leaves the enclosing block.
fn exits_block(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(block) => block.body.last().is_some_and(exits_block),
        _ => false,
    }
}

/// Start offset of the flag read whose value a `const` initializer is, so
/// that its binding stands for the flag. Accepts the read itself, `await`,
/// negation, and an equality test against a literal.
fn flag_value_read_start(expr: &Expression<'_>) -> Option<u32> {
    match unwrap_value(expr) {
        Expression::AwaitExpression(inner) => flag_value_read_start(&inner.argument),
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::LogicalNot => {
            flag_value_read_start(&unary.argument)
        }
        Expression::BinaryExpression(binary) if binary.operator.is_equality() => {
            if is_literal(&binary.right) {
                flag_value_read_start(&binary.left)
            } else if is_literal(&binary.left) {
                flag_value_read_start(&binary.right)
            } else {
                None
            }
        }
        Expression::CallExpression(call) => Some(call.span.start),
        Expression::StaticMemberExpression(member) => Some(member.span.start),
        _ => None,
    }
}

/// The source form of a boolean, number or string literal.
fn literal_source(expr: &Expression<'_>) -> Option<String> {
    match unwrap_value(expr) {
        Expression::BooleanLiteral(boolean) => Some(boolean.value.to_string()),
        Expression::NumericLiteral(number) => Some(
            number
                .raw
                .as_ref()
                .map_or_else(|| number.value.to_string(), ToString::to_string),
        ),
        other => string_value(other).map(|value| format!("'{value}'")),
    }
}

fn is_literal(expr: &Expression<'_>) -> bool {
    matches!(
        unwrap_value(expr),
        Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::NullLiteral(_)
    )
}

/// Members of an `{ ... } as const` object with string values, in order.
fn as_const_object_members(init: &Expression<'_>) -> Option<Vec<(String, String)>> {
    let mut expr = init;
    loop {
        expr = match expr {
            Expression::ParenthesizedExpression(inner) => &inner.expression,
            Expression::TSSatisfiesExpression(inner) => &inner.expression,
            _ => break,
        };
    }
    let Expression::TSAsExpression(as_expr) = expr else {
        return None;
    };
    if !as_expr.type_annotation.is_const_type_reference() {
        return None;
    }
    let Expression::ObjectExpression(object) = unwrap_value(&as_expr.expression) else {
        return None;
    };
    let members: Vec<(String, String)> = object
        .properties
        .iter()
        .filter_map(|property| {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                return None;
            };
            if property.computed {
                return None;
            }
            let name = property.key.static_name()?.to_string();
            Some((name, string_value(&property.value)?))
        })
        .collect();
    (!members.is_empty()).then_some(members)
}

/// The value of a string literal or of a template literal with no
/// substitutions.
fn string_value(expr: &Expression<'_>) -> Option<String> {
    match unwrap_value(expr) {
        Expression::StringLiteral(lit) => Some(lit.value.to_string()),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => template
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked.as_ref())
            .map(ToString::to_string),
        _ => None,
    }
}

/// Record the local `export { local as exported }` specifiers of a module.
/// A type-only export names no runtime registry.
fn collect_exported_names(
    export: &ExportNamedDeclaration<'_>,
    exports: &mut Vec<(String, String)>,
) {
    if export.export_kind.is_type() {
        return;
    }
    for spec in &export.specifiers {
        if !spec.export_kind.is_type() {
            exports.push((
                spec.local.name().to_string(),
                spec.exported.name().to_string(),
            ));
        }
    }
}

/// Extract the environment variable name from `process.env.X` or
/// `import.meta.env.X`.
fn extract_env_name<'b>(expr: &'b StaticMemberExpression<'_>) -> Option<&'b str> {
    let Expression::StaticMemberExpression(inner) = &expr.object else {
        return None;
    };
    if inner.property.name.as_str() != "env" {
        return None;
    }
    let is_env_object = match &inner.object {
        Expression::Identifier(id) => id.name.as_str() == "process",
        Expression::ImportMeta(_) => true,
        _ => false,
    };
    is_env_object.then(|| expr.property.name.as_str())
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

/// Extract the flag-name argument at `index`: a string literal, or a member
/// of a registry (`FLAGS.X` or `FLAGS['X']`).
fn extract_flag_name_arg(args: &[Argument<'_>], index: usize) -> Option<FlagNameArg> {
    match args.get(index)? {
        Argument::StringLiteral(lit) => Some(FlagNameArg::Literal(lit.value.to_string())),
        Argument::StaticMemberExpression(member) => {
            let Expression::Identifier(object) = &member.object else {
                return None;
            };
            Some(FlagNameArg::RegistryMember {
                registry: object.name.to_string(),
                member: member.property.name.to_string(),
            })
        }
        Argument::ComputedMemberExpression(member) => {
            let (Expression::Identifier(object), Expression::StringLiteral(key)) =
                (&member.object, &member.expression)
            else {
                return None;
            };
            Some(FlagNameArg::RegistryMember {
                registry: object.name.to_string(),
                member: key.value.to_string(),
            })
        }
        _ => None,
    }
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
                Some((inner.property.name.to_string(), prop_name))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Feature flag facts extracted from one parsed program.
#[derive(Debug, Default)]
pub(crate) struct ExtractedFlags {
    /// Reads with a known flag key.
    pub flag_uses: Vec<FlagUse>,
    /// Registries the module exports and reads through imported registries.
    /// `None` when the module has neither.
    pub registry_facts: Option<Box<FlagRegistryFacts>>,
}

/// Entry point: extract feature flag use sites from a parsed program.
///
/// Called unconditionally from `parse_source_to_module` for all parsed files,
/// with the user patterns of the `flags` config section. The parse cache keys
/// on those patterns, so one parse gives every flag.
pub(crate) fn extract_flags(
    program: &Program<'_>,
    line_offsets: &[u32],
    patterns: &FlagPatterns,
) -> ExtractedFlags {
    let mut visitor = FlagVisitor::new(
        line_offsets,
        &patterns.sdk_patterns,
        &patterns.env_prefixes,
        patterns.config_object_heuristics,
    );
    visitor.visit_program(program);
    let registry_facts = FlagRegistryFacts {
        registries: visitor.exported_registries,
        reads: visitor.registry_reads,
        constants: visitor
            .constants
            .into_iter()
            .filter(|constant| !constant.reads.is_empty())
            .collect(),
    };
    ExtractedFlags {
        flag_uses: visitor.results,
        registry_facts: (!registry_facts.is_empty()).then(|| Box::new(registry_facts)),
    }
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
        extract_flags(
            &parser_return.program,
            &line_offsets,
            &FlagPatterns::default(),
        )
        .flag_uses
    }

    fn extract_with_config_objects(source: &str) -> Vec<FlagUse> {
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, source, SourceType::tsx()).parse();
        let line_offsets = fallow_types::extract::compute_line_offsets(source);
        extract_flags(
            &parser_return.program,
            &line_offsets,
            &FlagPatterns {
                config_object_heuristics: true,
                ..FlagPatterns::default()
            },
        )
        .flag_uses
    }

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

    #[test]
    fn detects_custom_sdk_pattern() {
        let allocator = Allocator::default();
        let source = "isFeatureActive('my-flag');";
        let parser_return = Parser::new(&allocator, source, SourceType::tsx()).parse();
        let line_offsets = fallow_types::extract::compute_line_offsets(source);
        let custom = FlagPatterns {
            sdk_patterns: vec![("isFeatureActive".to_string(), 0, "Internal".to_string())],
            ..FlagPatterns::default()
        };
        let flags = extract_flags(&parser_return.program, &line_offsets, &custom).flag_uses;
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
        let custom = FlagPatterns {
            sdk_patterns: vec![("flag".to_string(), 0, "Internal".to_string())],
            ..FlagPatterns::default()
        };
        let flags = extract_flags(&parser_return.program, &line_offsets, &custom).flag_uses;
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "internal-flag");
        assert_eq!(flags[0].sdk_name.as_deref(), Some("Internal"));
    }

    #[test]
    fn detects_custom_env_prefix() {
        let allocator = Allocator::default();
        let source = "if (process.env.MYAPP_ENABLE_V2) {}";
        let parser_return = Parser::new(&allocator, source, SourceType::tsx()).parse();
        let line_offsets = fallow_types::extract::compute_line_offsets(source);
        let custom = FlagPatterns {
            env_prefixes: vec!["MYAPP_ENABLE_".to_string()],
            ..FlagPatterns::default()
        };
        let flags = extract_flags(&parser_return.program, &line_offsets, &custom).flag_uses;
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "MYAPP_ENABLE_V2");
    }

    fn extract_facts(source: &str) -> ExtractedFlags {
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, source, SourceType::tsx()).parse();
        let line_offsets = fallow_types::extract::compute_line_offsets(source);
        extract_flags(
            &parser_return.program,
            &line_offsets,
            &FlagPatterns::default(),
        )
    }

    fn guard_text<'s>(source: &'s str, flag: &FlagUse) -> &'s str {
        let start = flag.guard_span_start.expect("guard start") as usize;
        let end = flag.guard_span_end.expect("guard end") as usize;
        &source[start..end]
    }

    #[test]
    fn detects_import_meta_env_flag() {
        let flags = extract_from_source("if (import.meta.env.VITE_FEATURE_CHAT) { chat(); }");
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "VITE_FEATURE_CHAT");
        assert_eq!(flags[0].kind, FlagUseKind::EnvVar);
        assert!(flags[0].guard_span_start.is_some());
    }

    #[test]
    fn ignores_non_flag_import_meta_env() {
        let flags = extract_from_source(
            "const url = import.meta.env.VITE_API_URL;\nconst other = import.meta.url;",
        );
        assert!(flags.is_empty());
    }

    #[test]
    fn resolves_local_as_const_registry_member() {
        let flags = extract_from_source(
            "const FLAGS = { NewCheckout: 'new-checkout', Beta: `beta` } as const;\n\
             const a = useFlag(FLAGS.NewCheckout);\n\
             const b = useFlag(FLAGS['Beta']);",
        );
        let names: Vec<_> = flags.iter().map(|flag| flag.flag_name.as_str()).collect();
        assert_eq!(names, ["new-checkout", "beta"]);
        assert!(
            flags
                .iter()
                .all(|flag| flag.sdk_name.as_deref() == Some("LaunchDarkly"))
        );
    }

    #[test]
    fn resolves_local_enum_registry_member() {
        let flags = extract_from_source(
            "enum Gates { Beta = 'beta-gate', Count = 3 }\n\
             if (useGate(Gates.Beta)) {}\n\
             useGate(Gates.Count);",
        );
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "beta-gate");
        assert!(flags[0].guard_span_start.is_some());
    }

    #[test]
    fn mutable_object_is_not_a_local_registry() {
        let facts = extract_facts(
            "const FLAGS = { NewCheckout: 'new-checkout' };\nuseFlag(FLAGS.NewCheckout);",
        );
        assert!(facts.flag_uses.is_empty());
        assert!(facts.registry_facts.is_none());
    }

    #[test]
    fn keeps_imported_registry_read_for_project_analysis() {
        let source = "import { FLAGS } from './flags';\n\
                      if (useFlag(FLAGS.NewCheckout)) { render(); }";
        let facts = extract_facts(source);
        assert!(facts.flag_uses.is_empty());
        let reads = &facts.registry_facts.expect("registry facts").reads;
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].registry, "FLAGS");
        assert_eq!(reads[0].member, "NewCheckout");
        assert_eq!(reads[0].flag_use.sdk_name.as_deref(), Some("LaunchDarkly"));
        assert_eq!(reads[0].flag_use.line, 2);
        assert!(reads[0].flag_use.guard_span_start.is_some());
    }

    #[test]
    fn keeps_registry_reads_only_for_named_value_imports() {
        let facts = extract_facts(
            "import type { TypeFlags } from './types';\n\
             import * as all from './flags';\n\
             function View(props) {\n\
               useFlag(props.flagKey);\n\
               useFlag(TypeFlags.A);\n\
               useFlag(all.B);\n\
             }",
        );
        assert!(facts.flag_uses.is_empty());
        assert!(facts.registry_facts.is_none());
    }

    #[test]
    fn records_exported_registries_only() {
        let facts = extract_facts(
            "export const FLAGS = { A: 'a' } as const satisfies Record<string, string>;\n\
             export enum Gates { B = 'b' }\n\
             const Local = { C: 'c' } as const;\n\
             const Hidden = { D: 'd' } as const;\n\
             export { Local as Renamed };\n\
             export const Plain = { E: 'e' };",
        );
        let registries = facts.registry_facts.expect("registry facts").registries;
        let names: Vec<_> = registries
            .iter()
            .map(|registry| registry.export_name.as_str())
            .collect();
        assert_eq!(names, ["FLAGS", "Gates", "Renamed"]);
        assert_eq!(registries[2].members, [("C".to_string(), "c".to_string())]);
    }

    #[test]
    fn a_binding_that_shadows_a_registry_is_not_a_registry() {
        let facts = extract_facts(
            "import { FLAGS } from './flags';\n\
             const LOCAL = { A: 'local-a' } as const;\n\
             export function f(LOCAL) { return useFlag(LOCAL.A); }\n\
             function g(FLAGS) { return useFlag(FLAGS.Chat); }\n\
             const h = () => { const LOCAL = pick(); return useFlag(LOCAL.A); };\n\
             function k() { try { run(); } catch (FLAGS) { useFlag(FLAGS.Chat); } }\n\
             export const outer = useFlag(LOCAL.A);\n\
             export const imported = () => useFlag(FLAGS.Chat);",
        );
        let names: Vec<_> = facts
            .flag_uses
            .iter()
            .map(|flag| flag.flag_name.as_str())
            .collect();
        assert_eq!(names, ["local-a"]);
        assert_eq!(facts.flag_uses[0].line, 7);
        let reads = &facts.registry_facts.expect("registry facts").reads;
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].flag_use.line, 8);
    }

    #[test]
    fn module_without_registries_has_no_registry_facts() {
        let facts = extract_facts("const FLAGS = { A: 1 } as const;\nuseFlag('a');");
        assert!(facts.registry_facts.is_none());
        assert_eq!(facts.flag_uses.len(), 1);
    }

    #[test]
    fn jsx_logical_and_is_a_guard() {
        let source = "const View = () => <div>{useFlag('beta') && <Beta />}</div>;";
        let flags = extract_from_source(source);
        assert_eq!(flags.len(), 1);
        assert_eq!(guard_text(source, &flags[0]), "useFlag('beta') && <Beta />");
    }

    #[test]
    fn logical_and_without_jsx_is_not_a_guard() {
        let flags = extract_from_source("const run = useFlag('beta') && start();");
        assert_eq!(flags.len(), 1);
        assert!(flags[0].guard_span_start.is_none());
    }

    #[test]
    fn const_binding_takes_the_guard_of_a_jsx_ternary() {
        let source = "function View() {\n\
                        const enabled = useFlag('beta');\n\
                        return <div>{enabled ? <New /> : <Old />}</div>;\n\
                      }";
        let flags = extract_from_source(source);
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].line, 2);
        assert_eq!(guard_text(source, &flags[0]), "enabled ? <New /> : <Old />");
    }

    #[test]
    fn const_binding_takes_the_guard_of_a_jsx_logical_and() {
        let source = "function View() {\n\
                        const enabled = await getFeatureValue('beta', false) === true;\n\
                        return <div>{!enabled && <Old />}</div>;\n\
                      }";
        let flags = extract_from_source(source);
        assert_eq!(flags.len(), 1);
        assert_eq!(guard_text(source, &flags[0]), "!enabled && <Old />");
    }

    #[test]
    fn negated_early_return_guards_the_rest_of_the_block() {
        let source = "function View() {\n\
                        const enabled = useFlag('beta');\n\
                        if (!enabled) return null;\n\
                        return <New />;\n\
                      }\n\
                      after();";
        let flags = extract_from_source(source);
        assert_eq!(flags.len(), 1);
        assert_eq!(
            guard_text(source, &flags[0]),
            "if (!enabled) return null;\nreturn <New />;\n}"
        );
    }

    #[test]
    fn direct_negated_early_return_guards_the_rest_of_the_block() {
        let source = "function run() {\n\
                        if (!process.env.FEATURE_JOBS) { log(); return; }\n\
                        jobs();\n\
                      }";
        let flags = extract_from_source(source);
        assert_eq!(flags.len(), 1);
        assert!(guard_text(source, &flags[0]).ends_with("jobs();\n}"));
    }

    #[test]
    fn early_return_without_negation_keeps_the_if_span() {
        let source = "function run() {\n\
                        if (process.env.FEATURE_JOBS) return;\n\
                        jobs();\n\
                      }";
        let flags = extract_from_source(source);
        assert_eq!(flags.len(), 1);
        assert_eq!(
            guard_text(source, &flags[0]),
            "if (process.env.FEATURE_JOBS) return;"
        );
    }

    #[test]
    fn let_binding_does_not_take_a_guard() {
        let flags = extract_from_source("let enabled = useFlag('beta');\nif (enabled) { run(); }");
        assert_eq!(flags.len(), 1);
        assert!(flags[0].guard_span_start.is_none());
    }

    #[test]
    fn shadowed_binding_does_not_take_a_guard() {
        let flags = extract_from_source(
            "const enabled = useFlag('beta');\n\
             function inner() { const enabled = compute(); if (enabled) { run(); } }",
        );
        assert_eq!(flags.len(), 1);
        assert!(flags[0].guard_span_start.is_none());
    }

    #[test]
    fn binding_from_a_wrapped_call_does_not_take_a_guard() {
        let flags =
            extract_from_source("const enabled = wrap(useFlag('beta'));\nif (enabled) { run(); }");
        assert_eq!(flags.len(), 1);
        assert!(flags[0].guard_span_start.is_none());
    }

    #[test]
    fn detects_sdk_call_nested_in_an_if_test() {
        let flags = extract_from_source("if (variation('beta', false) === true) { run(); }");
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].flag_name, "beta");
        assert!(flags[0].guard_span_start.is_some());
    }

    #[test]
    fn config_object_access_is_one_read() {
        let flags = extract_with_config_objects("const on = config.features.newCheckout;");
        let names: Vec<_> = flags.iter().map(|flag| flag.flag_name.as_str()).collect();
        assert_eq!(names, ["features.newCheckout"]);
    }

    fn only_flag(source: &str) -> FlagUse {
        let mut flags = extract_from_source(source);
        assert_eq!(flags.len(), 1, "one flag read in {source}");
        flags.remove(0)
    }

    #[test]
    fn identical_if_branches_ignore_whitespace_and_comments() {
        let flag =
            only_flag("if (process.env.FEATURE_X) {\n  run(1);\n} else {\n  run( 1 ) ; // same\n}");
        assert!(flag.facts.identical_branches());
        assert!(!flag.facts.empty_branch());
    }

    #[test]
    fn a_one_token_difference_is_not_identical() {
        let flag = only_flag("if (process.env.FEATURE_X) { run(1); } else { run(2); }");
        assert!(!flag.facts.identical_branches());
    }

    #[test]
    fn identical_ternary_arms_are_identical_branches() {
        let flag = only_flag("const v = process.env.FEATURE_X ? pick('a') : pick('a');");
        assert!(flag.facts.identical_branches());
    }

    #[test]
    fn an_empty_else_or_consequent_is_an_empty_branch() {
        assert!(
            only_flag("if (process.env.FEATURE_X) { run(); } else {}")
                .facts
                .empty_branch()
        );
        assert!(
            only_flag("if (process.env.FEATURE_X) {}")
                .facts
                .empty_branch()
        );
        assert!(
            only_flag("if (process.env.FEATURE_X) ;")
                .facts
                .empty_branch()
        );
        assert!(
            !only_flag("if (process.env.FEATURE_X) { run(); }")
                .facts
                .empty_branch()
        );
    }

    #[test]
    fn null_undefined_and_jsx_false_arms_are_empty_branches() {
        for source in [
            "const v = useFlag('beta') ? <Beta /> : null;",
            "const v = useFlag('beta') ? undefined : <Beta />;",
            "const v = useFlag('beta') ? <Beta /> : false;",
            "const v = useFlag('beta') ? <Beta /> : <></>;",
        ] {
            assert!(only_flag(source).facts.empty_branch(), "{source}");
        }
        assert!(
            !only_flag("const v = useFlag('beta') ? 1 : false;")
                .facts
                .empty_branch(),
            "false is a value outside JSX"
        );
    }

    #[test]
    fn a_bound_read_takes_the_facts_of_its_guard() {
        let flag = only_flag("const on = useFlag('beta');\nif (on) { run(); } else { run(); }");
        assert!(flag.facts.identical_branches());
    }

    #[test]
    fn a_read_without_a_guard_has_no_facts() {
        let flag = only_flag("track(useFlag('beta'));");
        assert_eq!(flag.facts, FlagSiteFacts::default());
    }

    fn constants(source: &str) -> Vec<FlagConstant> {
        extract_facts(source)
            .registry_facts
            .map(|facts| facts.constants)
            .unwrap_or_default()
    }

    #[test]
    fn literal_const_flags_tested_by_a_guard_are_constants() {
        let source = "const FEATURE_NEW_UI = true;\n\
                      if (FEATURE_NEW_UI) { run(); }\n\
                      export const ENABLE_BETA = false;\n\
                      export const pick = (): number => (ENABLE_BETA ? 1 : 2);\n\
                      const FF_MODE = 'on';\n\
                      if (FF_MODE === 'on') { run(); }\n\
                      const FEATURE_BANNER = 1;\n\
                      export const View = () => <div>{FEATURE_BANNER && <Banner />}</div>;\n";
        let found = constants(source);
        let summary: Vec<(&str, &str, u32, usize)> = found
            .iter()
            .map(|c| (c.name.as_str(), c.value.as_str(), c.line, c.reads.len()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("FEATURE_NEW_UI", "true", 1, 1),
                ("ENABLE_BETA", "false", 3, 1),
                ("FF_MODE", "'on'", 5, 1),
                ("FEATURE_BANNER", "1", 7, 1),
            ]
        );
        assert_eq!(found[0].reads[0].line, 2);
        assert!(
            extract_from_source(source).is_empty(),
            "constants are not per-site flag reads"
        );
    }

    #[test]
    fn let_bindings_calls_shadows_and_plain_names_are_not_constants() {
        for source in [
            "let FEATURE_LET = true;\nif (FEATURE_LET) { run(); }",
            "let FEATURE_R = true;\nFEATURE_R = false;\nif (FEATURE_R) { run(); }",
            "const FEATURE_CALL = readFlag();\nif (FEATURE_CALL) { run(); }",
            "const FEATURE_S = true;\nfunction f(FEATURE_S: boolean) { if (FEATURE_S) { run(); } }",
            "const FEATURE_UNUSED = true;\nlog(FEATURE_UNUSED);",
            "const DEBUG = true;\nif (DEBUG) { run(); }",
        ] {
            assert!(constants(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn a_constant_read_takes_the_facts_of_its_guard() {
        let found = constants("const FEATURE_X = true;\nconst v = FEATURE_X ? <A /> : null;");
        assert!(found[0].reads[0].facts.empty_branch());
    }

    #[test]
    fn builtin_sdk_providers_are_distinct_and_ordered() {
        let providers = builtin_sdk_providers();
        assert!(!providers.is_empty());
        let mut sorted = providers.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            providers.len(),
            "providers must be distinct: {providers:?}"
        );
        assert!(
            !providers.contains(&""),
            "empty provider labels must not leak into the surfaced list"
        );
        assert_eq!(providers.first(), Some(&"LaunchDarkly"));
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
