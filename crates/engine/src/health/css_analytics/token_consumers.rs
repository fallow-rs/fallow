use super::*;

/// Input for the location-aware reverse index of Tailwind v4 `@theme` token
/// consumers. The index is descriptive only and sets no summary count.
pub(super) struct TokenConsumersInput<'a> {
    pub(super) tokens: &'a CssTokenSets,
    pub(super) files: &'a [fallow_types::discover::DiscoveredFile],
    pub(super) config: &'a ResolvedConfig,
    pub(super) ignore_set: &'a globset::GlobSet,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
}

fn collect_located_utility_consumers(
    input: &TokenConsumersInput<'_>,
) -> Vec<(String, String, u32)> {
    let mut located: Vec<(String, String, u32)> = Vec::new();
    for file in input.files {
        let path = &file.path;
        let extension = path.extension().and_then(|ext| ext.to_str());
        if !extension.is_some_and(|ext| THEME_USAGE_SOURCE_EXTS.contains(&ext)) {
            continue;
        }
        let relative = path.strip_prefix(&input.config.root).unwrap_or(path);
        if input.ignore_set.is_match(relative) {
            continue;
        }
        let rel = relative.to_string_lossy().replace('\\', "/");
        if let Ok(source) = std::fs::read_to_string(path) {
            collect_class_shaped_tokens_located(&source, &rel, &mut located);
        }
    }
    located
}

pub(super) fn build_token_consumers(
    input: &TokenConsumersInput<'_>,
) -> Vec<fallow_output::TokenConsumers> {
    if !should_build_token_consumers(input) {
        return Vec::new();
    }

    let candidates = token_consumer_candidates(input);
    if candidates.is_empty() {
        return Vec::new();
    }

    let utility_located = collect_located_utility_consumers(input);

    let mut out: Vec<fallow_output::TokenConsumers> = candidates
        .into_iter()
        .map(|candidate| build_token_consumer(input, candidate, &utility_located))
        .collect();

    out.sort_by(|a, b| a.token.cmp(&b.token));
    out
}

fn should_build_token_consumers(input: &TokenConsumersInput<'_>) -> bool {
    if input.changed_files.is_some() || input.ws_roots.is_some() {
        return false;
    }
    if input.tokens.theme_token_definers.is_empty() || !project_uses_tailwind(&input.config.root) {
        return false;
    }
    !project_uses_tailwind_plugin(input.tokens.any_plugin_directive, &input.config.root)
}

fn token_consumer_candidates(input: &TokenConsumersInput<'_>) -> Vec<ThemeTokenCandidate> {
    let mut summary = fallow_output::CssAnalyticsSummary::default();
    classify_theme_token_candidates(&UnusedThemeTokenScanInput {
        tokens: input.tokens,
        files: input.files,
        config: input.config,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        output_changed_files: None,
        ws_roots: input.ws_roots,
        summary: &mut summary,
    })
}

fn build_token_consumer(
    input: &TokenConsumersInput<'_>,
    candidate: ThemeTokenCandidate,
    utility_located: &[(String, String, u32)],
) -> fallow_output::TokenConsumers {
    use fallow_output::TOKEN_CONSUMER_SAMPLE_CAP;

    let mut consumers = token_consumer_locations(input, &candidate, utility_located);
    let consumer_count = saturate_len(consumers.len());
    consumers.truncate(TOKEN_CONSUMER_SAMPLE_CAP);

    fallow_output::TokenConsumers {
        token: candidate.token,
        namespace: candidate.namespace,
        definition_path: candidate.path,
        definition_line: candidate.line,
        consumer_count,
        consumers,
    }
}

fn token_consumer_locations(
    input: &TokenConsumersInput<'_>,
    candidate: &ThemeTokenCandidate,
    utility_located: &[(String, String, u32)],
) -> Vec<fallow_output::TokenConsumerLocation> {
    let dash_name = format!("-{}", candidate.name);
    let raw = candidate.token.trim_start_matches('-');
    let mut consumers = Vec::new();

    append_exact_token_consumers(
        &mut consumers,
        &input.tokens.theme_var_reads_located,
        raw,
        fallow_output::ConsumerKind::ThemeVar,
    );
    append_exact_token_consumers(
        &mut consumers,
        &input.tokens.css_var_reads_located,
        raw,
        fallow_output::ConsumerKind::CssVar,
    );
    append_suffix_token_consumers(
        &mut consumers,
        &input.tokens.apply_uses_located,
        &dash_name,
        fallow_output::ConsumerKind::Apply,
    );
    append_suffix_token_consumers(
        &mut consumers,
        utility_located,
        &dash_name,
        fallow_output::ConsumerKind::Utility,
    );
    sort_token_consumer_locations(&mut consumers);
    consumers
}

fn append_exact_token_consumers(
    consumers: &mut Vec<fallow_output::TokenConsumerLocation>,
    located: &[(String, String, u32)],
    expected: &str,
    kind: fallow_output::ConsumerKind,
) {
    for (name, path, line) in located {
        if name == expected {
            consumers.push(fallow_output::TokenConsumerLocation {
                path: path.clone(),
                line: *line,
                kind,
            });
        }
    }
}

fn append_suffix_token_consumers(
    consumers: &mut Vec<fallow_output::TokenConsumerLocation>,
    located: &[(String, String, u32)],
    suffix: &str,
    kind: fallow_output::ConsumerKind,
) {
    for (token, path, line) in located {
        if token.len() > suffix.len() && token.ends_with(suffix) {
            consumers.push(fallow_output::TokenConsumerLocation {
                path: path.clone(),
                line: *line,
                kind,
            });
        }
    }
}

fn sort_token_consumer_locations(consumers: &mut [fallow_output::TokenConsumerLocation]) {
    consumers.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| consumer_kind_rank(a.kind).cmp(&consumer_kind_rank(b.kind)))
    });
}

/// A CSS-in-JS token-definition site discovered during the definer pass: the
/// root-relative definition file, the access binding consumers read through, and
/// its flattened leaf tokens.
pub(super) struct CssInJsDefiner {
    pub(super) rel_path: String,
    pub(super) binding: String,
    pub(super) origin: fallow_extract::CssInJsTokenOrigin,
    pub(super) leaves: Vec<fallow_extract::CssInJsToken>,
}

/// The definer-pass result: every `(file, binding)` token-definition site plus the
/// lookups the consumer pass keys on (normalized definer path + binding -> entry
/// index, and the set of normalized definer paths for relative-import resolution).
pub(super) struct CssInJsDefiners {
    pub(super) entries: Vec<CssInJsDefiner>,
    pub(super) index: rustc_hash::FxHashMap<(std::path::PathBuf, String), usize>,
    pub(super) paths: rustc_hash::FxHashSet<std::path::PathBuf>,
}

type CssInJsConsumerKey = (usize, String);
type CssInJsConsumerHit = (String, u32, fallow_output::ConsumerKind);
type CssInJsConsumerHits =
    rustc_hash::FxHashMap<CssInJsConsumerKey, rustc_hash::FxHashSet<CssInJsConsumerHit>>;
type CssInJsImportKey = (fallow_types::discover::FileId, String, String, String);
type ResolvedCssInJsImportTargets =
    rustc_hash::FxHashMap<CssInJsImportKey, fallow_types::discover::FileId>;

/// Whether a specifier names a CSS-in-JS token-DEFINITION library. `@vanilla-extract/recipes`
/// is excluded: it exports no token-definition function (`createTheme` family lives
/// in `@vanilla-extract/css`), so it is not a definer-pass pre-filter source.
fn is_css_in_js_token_lib(specifier: &str) -> bool {
    matches!(
        specifier,
        "@stylexjs/stylex" | "@vanilla-extract/css" | "@pandacss/dev"
    )
}

/// A cheap source pre-filter: only re-parse a token-lib-importing file as a
/// potential definer if its source mentions a token-definition function, so a
/// StyleX file that only calls `stylex.create` (no `defineVars`) is not parsed.
fn source_mentions_token_definer(source: &str) -> bool {
    source.contains("defineVars")
        || source.contains("createThemeContract")
        || source.contains("createGlobalTheme")
        || source.contains("createTheme")
        || source.contains("defineTokens")
        || source.contains("defineConfig")
}

fn source_mentions_theme_definer(source: &str) -> bool {
    source.contains("theme") || source.contains("Theme")
}

fn is_theme_provider_source(specifier: &str) -> bool {
    matches!(specifier, "styled-components" | "@emotion/react")
}

fn project_imports_theme_provider(modules: &[fallow_types::extract::ModuleInfo]) -> bool {
    use fallow_types::extract::ImportedName;

    modules.iter().any(|module| {
        module.imports.iter().any(|import| {
            !import.is_type_only
                && is_theme_provider_source(&import.source)
                && matches!(&import.imported_name, ImportedName::Named(name) if name == "ThemeProvider")
        })
    })
}

/// Whether an import specifier is a relative path. The shared graph resolver
/// handles tsconfig aliases and workspace packages first; this light resolver is
/// the zero-FP local fallback for cases where a graph edge was not available.
fn is_relative_specifier(specifier: &str) -> bool {
    specifier.starts_with('.')
}

fn is_panda_generated_specifier(specifier: &str) -> bool {
    specifier
        .split(['/', '\\'])
        .any(|segment| segment == "styled-system")
}

fn is_panda_style_function(name: &str) -> bool {
    matches!(name, "css" | "cva" | "sva" | "recipe" | "styled")
}

/// Lexically normalize a path (resolve `.` / `..` without touching the
/// filesystem), so a consumer-relative join compares equal to a definer's
/// discovered absolute path regardless of `./` / `../` segments.
fn lexical_normalize(path: &std::path::Path) -> std::path::PathBuf {
    let mut out = std::path::PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve a relative import specifier from a consuming file to a known definer
/// path (extension + `/index` candidates, lexically normalized). Returns the
/// matched, normalized definer path or `None`. Zero-FP for relative imports: a
/// specifier that resolves to a non-definer path yields `None`, so an unrelated
/// `import { vars } from './other'` is never matched against a design-token `vars`.
fn resolve_relative_specifier(
    consumer_abs: &std::path::Path,
    specifier: &str,
    definer_paths: &rustc_hash::FxHashSet<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    const EXTS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"];
    let base = lexical_normalize(&consumer_abs.parent()?.join(specifier));
    // 1. Exact (specifier already carried a resolvable filename).
    if definer_paths.contains(&base) {
        return Some(base);
    }
    // 2. `<base>.<ext>` (`./tokens` -> `./tokens.ts`; `./theme.css` -> `./theme.css.ts`).
    for ext in EXTS {
        let mut candidate = base.clone().into_os_string();
        candidate.push(".");
        candidate.push(ext);
        let candidate = std::path::PathBuf::from(candidate);
        if definer_paths.contains(&candidate) {
            return Some(candidate);
        }
    }
    // 3. `<base>/index.<ext>`.
    for ext in EXTS {
        let candidate = base.join(format!("index.{ext}"));
        if definer_paths.contains(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn css_in_js_import_key(
    file_id: fallow_types::discover::FileId,
    import: &fallow_types::extract::ImportInfo,
) -> Option<CssInJsImportKey> {
    let fallow_types::extract::ImportedName::Named(imported_name) = &import.imported_name else {
        return None;
    };
    Some((
        file_id,
        import.source.clone(),
        imported_name.clone(),
        import.local_name.clone(),
    ))
}

fn resolve_css_in_js_import_targets(
    files: &[fallow_types::discover::DiscoveredFile],
    modules: &[fallow_types::extract::ModuleInfo],
    config: &ResolvedConfig,
) -> ResolvedCssInJsImportTargets {
    let workspaces = fallow_config::discover_workspaces(&config.root);
    let active_plugins: Vec<String> = Vec::new();
    let path_aliases: Vec<(String, String)> = Vec::new();
    let auto_imports: Vec<fallow_config::AutoImportRule> = Vec::new();
    let scss_include_paths: Vec<std::path::PathBuf> = Vec::new();
    let static_dir_mappings: Vec<(std::path::PathBuf, String)> = Vec::new();
    let input = fallow_graph::resolve::ResolveAllImportsInput {
        modules,
        files,
        workspaces: &workspaces,
        active_plugins: &active_plugins,
        path_aliases: &path_aliases,
        auto_imports: &auto_imports,
        scss_include_paths: &scss_include_paths,
        static_dir_mappings: &static_dir_mappings,
        root: &config.root,
        extra_conditions: &config.resolve.conditions,
    };
    let mut targets = ResolvedCssInJsImportTargets::default();
    for resolved in fallow_graph::resolve::resolve_all_imports(&input) {
        for import in resolved.resolved_imports {
            let Some(file_id) = import.target.internal_file_id() else {
                continue;
            };
            let Some(key) = css_in_js_import_key(resolved.file_id, &import.info) else {
                continue;
            };
            targets.insert(key, file_id);
        }
    }
    targets
}

fn resolve_css_in_js_definer_import(
    consumer_file_id: fallow_types::discover::FileId,
    consumer_abs: &std::path::Path,
    import: &fallow_types::extract::ImportInfo,
    definers: &CssInJsDefiners,
    path_by_id: &rustc_hash::FxHashMap<fallow_types::discover::FileId, &std::path::Path>,
    resolved_targets: &ResolvedCssInJsImportTargets,
) -> Option<usize> {
    let fallow_types::extract::ImportedName::Named(imported_name) = &import.imported_name else {
        return None;
    };
    if let Some(key) = css_in_js_import_key(consumer_file_id, import)
        && let Some(target_id) = resolved_targets.get(&key)
        && let Some(target_abs) = path_by_id.get(target_id)
    {
        let resolved = lexical_normalize(target_abs);
        if let Some(&idx) = definers.index.get(&(resolved, imported_name.clone())) {
            return Some(idx);
        }
    }
    if !is_relative_specifier(&import.source) {
        return None;
    }
    let resolved = resolve_relative_specifier(consumer_abs, &import.source, &definers.paths)?;
    definers
        .index
        .get(&(resolved, imported_name.clone()))
        .copied()
}

/// Definer pass: re-parse every token-lib-importing file that mentions a
/// token-definition function, collecting each `(file, binding)` token-definition
/// site plus the lookup structures the consumer pass needs.
pub(super) fn collect_css_in_js_definers(
    modules: &[fallow_types::extract::ModuleInfo],
    path_by_id: &rustc_hash::FxHashMap<fallow_types::discover::FileId, &std::path::Path>,
    config: &ResolvedConfig,
) -> CssInJsDefiners {
    let mut definers: Vec<CssInJsDefiner> = Vec::new();
    let mut definer_index: rustc_hash::FxHashMap<(std::path::PathBuf, String), usize> =
        rustc_hash::FxHashMap::default();
    let mut definer_paths: rustc_hash::FxHashSet<std::path::PathBuf> =
        rustc_hash::FxHashSet::default();
    let has_theme_provider = project_imports_theme_provider(modules);

    for module in modules {
        let imports_token_lib = module
            .imports
            .iter()
            .any(|i| !i.is_type_only && is_css_in_js_token_lib(&i.source));
        let Some(abs) = path_by_id.get(&module.file_id).copied() else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(abs) else {
            continue;
        };
        let mut defs = Vec::new();
        if imports_token_lib && source_mentions_token_definer(&source) {
            defs.extend(fallow_extract::css_in_js_token_defs(&source, abs));
        }
        if has_theme_provider && source_mentions_theme_definer(&source) {
            defs.extend(fallow_extract::css_in_js_theme_token_defs(&source, abs));
        }
        if defs.is_empty() {
            continue;
        }
        let Some(rel) = relative_to_root(abs, &config.root) else {
            continue;
        };
        let norm = lexical_normalize(abs);
        for def in defs {
            let idx = definers.len();
            definer_index.insert((norm.clone(), def.binding.clone()), idx);
            definer_paths.insert(norm.clone());
            definers.push(CssInJsDefiner {
                rel_path: rel.clone(),
                binding: def.binding,
                origin: def.origin,
                leaves: def.tokens,
            });
        }
    }
    CssInJsDefiners {
        entries: definers,
        index: definer_index,
        paths: definer_paths,
    }
}

/// Consumer pass: for each file whose named imports resolve to a definer binding
/// through the shared graph resolver or local relative fallback, re-parse it and
/// collect located member-access reads, deduped by `(consumer file, line)` per
/// `(definer, leaf token path)`.
fn collect_css_in_js_consumers(
    modules: &[fallow_types::extract::ModuleInfo],
    path_by_id: &rustc_hash::FxHashMap<fallow_types::discover::FileId, &std::path::Path>,
    config: &ResolvedConfig,
    definers: &CssInJsDefiners,
    resolved_targets: &ResolvedCssInJsImportTargets,
) -> CssInJsConsumerHits {
    let mut hits: CssInJsConsumerHits = rustc_hash::FxHashMap::default();

    // Precompute per-definer data ONCE. The old pass rebuilt a leaf_set per
    // (definer, alias) pair and re-parsed each consumer source once per definer
    // (and once per theme definer for EVERY module). Here every consumer file is
    // parsed exactly once and all its queries run against that single parse.
    let leaf_sets: Vec<rustc_hash::FxHashSet<String>> = definers
        .entries
        .iter()
        .map(|definer| definer.leaves.iter().map(|t| t.path.clone()).collect())
        .collect();
    let theme_definer_indices: Vec<usize> = definers
        .entries
        .iter()
        .enumerate()
        .filter(|(_, definer)| definer.origin == fallow_extract::CssInJsTokenOrigin::Theme)
        .map(|(idx, _)| idx)
        .collect();
    let panda_definer_indices: Vec<usize> = definers
        .entries
        .iter()
        .enumerate()
        .filter(|(_, definer)| definer.origin == fallow_extract::CssInJsTokenOrigin::Panda)
        .map(|(idx, _)| idx)
        .collect();
    let has_theme_definers = !theme_definer_indices.is_empty();

    for module in modules {
        let Some(consumer_abs) = path_by_id.get(&module.file_id).copied() else {
            continue;
        };
        let matches =
            css_in_js_definer_matches(module, consumer_abs, definers, path_by_id, resolved_targets);
        let has_panda_generated_alias = has_panda_generated_alias(module);
        if matches.is_empty() && !has_panda_generated_alias && !has_theme_definers {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(consumer_abs) else {
            continue;
        };
        let Some(consumer_rel) = relative_to_root(consumer_abs, &config.root) else {
            continue;
        };
        let token_aliases = module_panda_token_aliases(module);
        let style_aliases = module_panda_style_aliases(module);
        let (queries, attribution) = build_module_consumer_queries(
            &matches,
            &leaf_sets,
            &theme_definer_indices,
            &panda_definer_indices,
            &token_aliases,
            &style_aliases,
        );
        for (query_pos, hit) in
            fallow_extract::css_in_js_consumer_scan(&source, consumer_abs, &queries)
        {
            let (definer_idx, kind) = attribution[query_pos];
            hits.entry((definer_idx, hit.token_path))
                .or_default()
                .insert((consumer_rel.clone(), hit.line, kind));
        }
    }
    hits
}

fn css_in_js_definer_matches<'a>(
    module: &'a fallow_types::extract::ModuleInfo,
    consumer_abs: &std::path::Path,
    definers: &CssInJsDefiners,
    path_by_id: &rustc_hash::FxHashMap<fallow_types::discover::FileId, &std::path::Path>,
    resolved_targets: &ResolvedCssInJsImportTargets,
) -> Vec<(usize, &'a str)> {
    use fallow_types::extract::ImportedName;

    let mut matches: Vec<(usize, &str)> = Vec::new();
    for import in &module.imports {
        if import.is_type_only || !matches!(&import.imported_name, ImportedName::Named(_)) {
            continue;
        }
        if let Some(idx) = resolve_css_in_js_definer_import(
            module.file_id,
            consumer_abs,
            import,
            definers,
            path_by_id,
            resolved_targets,
        ) {
            matches.push((idx, import.local_name.as_str()));
        }
    }
    matches
}

fn has_panda_generated_alias(module: &fallow_types::extract::ModuleInfo) -> bool {
    use fallow_types::extract::ImportedName;

    module.imports.iter().any(|import| {
        !import.is_type_only
            && is_panda_generated_specifier(&import.source)
            && matches!(
                &import.imported_name,
                ImportedName::Named(name) if name == "token" || is_panda_style_function(name)
            )
    })
}

/// The local aliases a module imports PandaCSS's generated `token` helper under
/// (from a `styled-system` specifier). Borrows the module's import strings.
fn module_panda_token_aliases(module: &fallow_types::extract::ModuleInfo) -> Vec<&str> {
    use fallow_types::extract::ImportedName;

    module
        .imports
        .iter()
        .filter(|import| {
            !import.is_type_only
                && is_panda_generated_specifier(&import.source)
                && matches!(&import.imported_name, ImportedName::Named(name) if name == "token")
        })
        .map(|import| import.local_name.as_str())
        .collect()
}

/// The local aliases a module imports PandaCSS style functions (`css`, `cva`, ...)
/// under from a `styled-system` specifier.
fn module_panda_style_aliases(
    module: &fallow_types::extract::ModuleInfo,
) -> rustc_hash::FxHashSet<String> {
    use fallow_types::extract::ImportedName;

    module
        .imports
        .iter()
        .filter(|import| {
            !import.is_type_only
                && is_panda_generated_specifier(&import.source)
                && matches!(&import.imported_name, ImportedName::Named(name) if is_panda_style_function(name))
        })
        .map(|import| import.local_name.clone())
        .collect()
}

type ConsumerQueryPlan<'a> = (
    Vec<fallow_extract::ConsumerQuery<'a>>,
    Vec<(usize, fallow_output::ConsumerKind)>,
);

/// Build the per-module `ConsumerQuery` list plus a parallel attribution vector
/// mapping each query position to its `(definer index, ConsumerKind)`. The single
/// scan of the consumer source returns `(query_index, hit)` pairs the caller folds
/// back through this attribution, preserving the old per-definer kinds exactly.
fn build_module_consumer_queries<'a>(
    matches: &[(usize, &'a str)],
    leaf_sets: &'a [rustc_hash::FxHashSet<String>],
    theme_definer_indices: &[usize],
    panda_definer_indices: &[usize],
    token_aliases: &[&'a str],
    style_aliases: &'a rustc_hash::FxHashSet<String>,
) -> ConsumerQueryPlan<'a> {
    use fallow_extract::ConsumerQuery;
    use fallow_output::ConsumerKind;

    let mut queries: Vec<ConsumerQuery<'a>> = Vec::new();
    let mut attribution: Vec<(usize, ConsumerKind)> = Vec::new();

    // Named-import member reads (`vars.color.primary`): one query per matched
    // (definer, alias) pair.
    for &(idx, alias) in matches {
        queries.push(ConsumerQuery::MemberBinding {
            alias,
            leaf_paths: &leaf_sets[idx],
        });
        attribution.push((idx, ConsumerKind::JsMember));
    }

    // PandaCSS generated-token consumers: `token('a.b')` calls and style-object
    // values, scanned against every Panda definer (matching the old pass, which
    // ran once per Panda definer when the module had any Panda alias).
    if !token_aliases.is_empty() || !style_aliases.is_empty() {
        for &idx in panda_definer_indices {
            for &alias in token_aliases {
                queries.push(ConsumerQuery::PandaTokenCall {
                    alias,
                    leaf_paths: &leaf_sets[idx],
                });
                attribution.push((idx, ConsumerKind::JsCall));
            }
            if !style_aliases.is_empty() {
                queries.push(ConsumerQuery::PandaStyleValues {
                    aliases: style_aliases,
                    leaf_paths: &leaf_sets[idx],
                });
                attribution.push((idx, ConsumerKind::JsCall));
            }
        }
    }

    // styled-components / Emotion theme reads (`theme.colors.x`): unconditional
    // per theme definer for every surviving module (matching the old pass).
    for &idx in theme_definer_indices {
        queries.push(ConsumerQuery::ThemeReads {
            leaf_paths: &leaf_sets[idx],
        });
        attribution.push((idx, ConsumerKind::JsMember));
    }

    (queries, attribution)
}

/// Build the CSS-in-JS design-token blast-radius: StyleX `defineVars`,
/// vanilla-extract `createTheme`-family, PandaCSS `defineTokens`, and
/// styled-components / Emotion theme objects. Uses resolved import edges for
/// relative imports, tsconfig aliases, and workspace packages, then falls back to
/// the light relative resolver for zero-FP local cases.
pub(super) fn build_css_in_js_token_consumers(
    files: &[fallow_types::discover::DiscoveredFile],
    modules: &[fallow_types::extract::ModuleInfo],
    config: &ResolvedConfig,
    definers: Option<&CssInJsDefiners>,
) -> Vec<fallow_output::TokenConsumers> {
    use fallow_output::{TOKEN_CONSUMER_SAMPLE_CAP, TokenConsumerLocation, TokenConsumers};

    let Some(definers) = definers else {
        return Vec::new();
    };
    if definers.entries.is_empty() {
        return Vec::new();
    }
    let path_by_id: rustc_hash::FxHashMap<fallow_types::discover::FileId, &std::path::Path> =
        files.iter().map(|f| (f.id, f.path.as_path())).collect();
    let resolved_targets = resolve_css_in_js_import_targets(files, modules, config);
    let hits =
        collect_css_in_js_consumers(modules, &path_by_id, config, definers, &resolved_targets);

    let mut out: Vec<TokenConsumers> = Vec::new();
    for (idx, definer) in definers.entries.iter().enumerate() {
        for leaf in &definer.leaves {
            let mut consumers: Vec<TokenConsumerLocation> = hits
                .get(&(idx, leaf.path.clone()))
                .map(|set| {
                    set.iter()
                        .map(|(path, line, kind)| TokenConsumerLocation {
                            path: path.clone(),
                            line: *line,
                            kind: *kind,
                        })
                        .collect()
                })
                .unwrap_or_default();
            consumers.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));
            let consumer_count = saturate_len(consumers.len());
            consumers.truncate(TOKEN_CONSUMER_SAMPLE_CAP);
            out.push(TokenConsumers {
                token: format!("{}.{}", definer.binding, leaf.path),
                namespace: definer.binding.clone(),
                definition_path: definer.rel_path.clone(),
                definition_line: leaf.def_line,
                consumer_count,
                consumers,
            });
        }
    }
    // Deterministic order among the CSS-in-JS entries. The caller
    // (`compute_css_analytics_report`) applies a final sort over the COMBINED
    // Tailwind + CSS-in-JS list, so the emitted `token_consumers` is globally
    // ordered by `(token, definition_path)`.
    out.sort_by(|a, b| {
        a.token
            .cmp(&b.token)
            .then_with(|| a.definition_path.cmp(&b.definition_path))
    });
    out
}

fn consumer_kind_rank(kind: fallow_output::ConsumerKind) -> u8 {
    use fallow_output::ConsumerKind;
    match kind {
        ConsumerKind::ThemeVar => 0,
        ConsumerKind::CssVar => 1,
        ConsumerKind::Utility => 2,
        ConsumerKind::Apply => 3,
        ConsumerKind::JsMember => 4,
        ConsumerKind::JsCall => 5,
    }
}
