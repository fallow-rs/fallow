//! Data-driven catalogue of syntactic security-sink candidate matchers.
//!
//! The catalogue is community-maintainable: every matcher lives in
//! `crates/core/data/security_matchers.toml`, embedded via `include_str!` and
//! parsed once behind a `OnceLock`. There is NO regeneration step. Adding a
//! category is a single `[[matcher]]` TOML edit plus ZERO Rust enum or
//! discriminant churn (the `tainted_sink` detector matches captured
//! category-blind `SinkSite`s against the loaded catalogue).
//!
//! Findings are CANDIDATES for downstream agent verification, NOT verified
//! vulnerabilities: fallow is deterministic and syntactic, never taint-proof.
//! Matchers default to non-literal arguments. A row can opt into narrowly
//! captured literal or context predicates when the literal itself is the signal.

use fallow_types::extract::{SinkArgKind, SinkLiteralValue, SinkObjectProperty, SinkShape};
use rustc_hash::FxHashSet;

/// Embedded catalogue source. Because it is `include_str!`-embedded at compile
/// time, a green `security_catalogue_parses` test guarantees the released
/// binary parses.
const CATALOGUE_TOML: &str = include_str!("../../../data/security_matchers.toml");

#[derive(serde::Deserialize)]
struct RawCatalogue {
    #[serde(default)]
    matcher: Vec<RawMatcher>,
    #[serde(default)]
    source: Vec<RawSource>,
}

/// A raw untrusted-source row (issue #859). Names member-access paths that carry
/// attacker-controlled input; the analyze layer matches captured tainted-binding
/// source paths against these to mark source-tainted locals.
#[derive(serde::Deserialize)]
struct RawSource {
    id: String,
    title: String,
    /// Optional framework enabler, same semantics as matcher enablers.
    #[serde(default)]
    enabler: Option<String>,
    path_patterns: Vec<String>,
    /// Optional allowlist of receiver names for leading-`*.` wildcard patterns
    /// (issue #1092). When non-empty, a wildcard pattern fires only if the
    /// matched member's receiver is one of these (case-insensitive), so
    /// `*.query` matches `req.query` but not `db.query`. Empty / absent leaves
    /// the row ungated (every receiver matches). Has no effect on exact
    /// patterns, whose receiver is fixed in the pattern itself.
    #[serde(default)]
    receiver_allowlist: Vec<String>,
}

#[derive(serde::Deserialize)]
struct RawMatcher {
    id: String,
    cwe: u32,
    title: String,
    /// Kebab-case shape string, validated into [`SinkShape`].
    sink_shape: String,
    callee_patterns: Vec<String>,
    arg_index: u32,
    evidence_template: String,
    #[serde(default)]
    import_provenance: Option<String>,
    /// Optional framework enabler: a package name that gates this row on the
    /// active framework (issue #861). The plugin system already activates on the
    /// declared dependency set, so a row carrying `enabler = "@angular/platform-browser"`
    /// fires only when that package (or, with a trailing `/`, any package under
    /// that prefix) is present in the project's declared dependencies. Lets a
    /// framework-specific idiom (`bypassSecurityTrustHtml`, `dangerouslySetInnerHTML`)
    /// be recognized with higher precision without a new enum variant. Unset means
    /// the row is global (the prior behavior).
    #[serde(default)]
    enabler: Option<String>,
    /// Optional allowlist of argument shapes. When set, the captured sink site's
    /// `arg_kind` must be one of the listed kebab-case kinds for the matcher to
    /// fire. Lets a matcher require the unsafe SQL shapes (`concat`,
    /// `template-with-subst`) and exclude the safely-parameterized forms
    /// (`object` for `.execute({ sql, args })`, the bare `sql` tag). Unset means
    /// any non-literal argument shape matches (the prior behavior).
    #[serde(default)]
    arg_kinds: Option<Vec<String>>,
    /// Optional string-literal equality predicates for literal-aware rows.
    #[serde(default)]
    literal_values: Option<Vec<String>>,
    /// Optional string-literal substring predicates for literal-aware rows.
    #[serde(default)]
    literal_contains: Option<Vec<String>>,
    /// Optional integer-literal equality predicates for literal-aware rows.
    #[serde(default)]
    literal_integers: Option<Vec<i64>>,
    /// Optional object-literal property equality predicates.
    #[serde(default)]
    object_properties: Option<Vec<RawObjectPropertyPredicate>>,
    /// Optional object-literal flags that are unsafe when missing or `false`.
    #[serde(default)]
    object_missing_or_false: Option<Vec<String>>,
    /// Optional object-literal keys that are unsafe when absent. Unlike
    /// `object_missing_or_false`, this checks key presence only and refuses
    /// incomplete object shapes.
    #[serde(default)]
    object_missing: Option<Vec<String>>,
    /// Optional context-name keywords for zero-arg sinks like `Math.random()`.
    #[serde(default)]
    context_keywords: Option<Vec<String>>,
    /// Optional precision gate: require the captured sink argument to reference
    /// a local binding that came from a configured untrusted source.
    #[serde(default)]
    requires_source: bool,
    /// Optional precision gate narrowing `requires_source` to SPECIFIC source
    /// kinds by catalogue source id (issue #890). Empty (default) admits any
    /// matched source (the prior behavior); when set, the matched source's id
    /// must be one of these. Lets `secret-to-network` fire only when backed by a
    /// SECRET source (`process-env` / `import-meta-env`), not request input
    /// (which the `ssrf` rows already cover).
    #[serde(default)]
    requires_source_kinds: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RawObjectPropertyPredicate {
    key: String,
    #[serde(default)]
    string: Option<String>,
    #[serde(default)]
    boolean: Option<bool>,
    #[serde(default)]
    integer: Option<i64>,
    #[serde(default)]
    null: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiteralPredicate {
    String(String),
    Integer(i64),
    Boolean(bool),
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectPropertyPredicate {
    key: String,
    value: LiteralPredicate,
}

/// A pre-segmented callee pattern. Matching is segment-aware (NOT substring):
/// the pattern is split on `.`, a leading `*` segment means "any object"
/// (`*.innerHTML` matches `el.innerHTML` and `this.node.innerHTML` by
/// suffix-matching the trailing non-`*` segments), and a trailing `*` segment
/// means "any member" (`child_process.*` matches `child_process.exec` by
/// prefix-matching the leading non-`*` segments). The security catalogue uses
/// exact and leading-wildcard rows; the trailing form serves the boundary
/// forbidden-call detector.
#[derive(Debug, Clone)]
pub struct CalleePattern {
    /// The literal source pattern (`"*.innerHTML"`, `"child_process.exec"`),
    /// surfaced in evidence rendering as `{pattern}`.
    raw: String,
    /// Segments between any leading and trailing `*` (e.g. `["innerHTML"]`
    /// for `*.innerHTML`, `["child_process"]` for `child_process.*`,
    /// `["child_process", "exec"]` for the exact dotted form).
    suffix_segments: Vec<String>,
    /// Whether the pattern began with a `*` wildcard object segment.
    leading_wildcard: bool,
    /// Whether the pattern ended with a `*` wildcard member segment.
    trailing_wildcard: bool,
}

impl CalleePattern {
    /// Parse a raw pattern string into its segmented form. Returns `None` for
    /// an empty or whitespace-only pattern. Public constructor for non-security
    /// reusers of the segment-aware matcher (the boundary forbidden-call
    /// detector); the catalogue's own rows go through the same parser.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        parse_callee_pattern(raw)
    }

    /// The original pattern text, for evidence templating.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Segment-aware match against a captured dotted/bare callee path.
    ///
    /// With a leading `*`, the trailing segments must equal the tail of the
    /// candidate's segments (suffix match), so `*.innerHTML` matches
    /// `el.innerHTML` but not `el.innerHTMLFoo`. With a trailing `*`, the
    /// leading segments must equal the head of the candidate's segments
    /// (prefix match), so `child_process.*` matches `child_process.exec` but
    /// not the bare `child_process`. Without either, the whole segment list
    /// must match exactly, so `fetch` matches `fetch` but not `myfetch`.
    /// Patterns carrying BOTH wildcards match nothing (rejected by the config
    /// layer; never produced by catalogue rows).
    #[must_use]
    pub fn matches(&self, callee_path: &str) -> bool {
        // With only wildcards and no concrete segments, match nothing.
        if self.suffix_segments.is_empty() || (self.leading_wildcard && self.trailing_wildcard) {
            return false;
        }
        let candidate: Vec<&str> = callee_path.split('.').collect();
        if self.leading_wildcard {
            // A leading `*.` requires at least one object segment before the
            // suffix, so the candidate must have strictly more segments than
            // the suffix (`*.innerHTML` matches `el.innerHTML`, not `innerHTML`).
            if self.suffix_segments.len() >= candidate.len() {
                return false;
            }
            let tail = &candidate[candidate.len() - self.suffix_segments.len()..];
            self.suffix_segments
                .iter()
                .zip(tail)
                .all(|(pat, seg)| pat == seg)
        } else if self.trailing_wildcard {
            // A trailing `.*` requires at least one member segment after the
            // prefix (`child_process.*` matches `child_process.exec`, not the
            // bare `child_process`).
            if self.suffix_segments.len() >= candidate.len() {
                return false;
            }
            let head = &candidate[..self.suffix_segments.len()];
            self.suffix_segments
                .iter()
                .zip(head)
                .all(|(pat, seg)| pat == seg)
        } else {
            self.suffix_segments.len() == candidate.len()
                && self
                    .suffix_segments
                    .iter()
                    .zip(&candidate)
                    .all(|(pat, seg)| pat == seg)
        }
    }

    /// The receiver segment immediately before this pattern's matched suffix,
    /// for a leading-`*.` wildcard pattern: `*.query` against `db.query` returns
    /// `Some("db")`, against `ctx.req.query` returns `Some("req")` (the segment
    /// right before `query`, which is the receiver of the matched member). Used
    /// by a source row's receiver allowlist to keep HTTP-input patterns from
    /// firing on ORM / data-access receivers (issue #1092). Returns `None` for
    /// an exact (non-wildcard) pattern, whose receiver is fixed in the pattern
    /// itself, and for any `callee_path` this pattern does not match.
    #[must_use]
    pub fn matched_receiver<'p>(&self, callee_path: &'p str) -> Option<&'p str> {
        if !self.leading_wildcard || !self.matches(callee_path) {
            return None;
        }
        let candidate: Vec<&str> = callee_path.split('.').collect();
        // `matches` guarantees `candidate.len() > suffix_segments.len()` for a
        // leading-wildcard hit, so the receiver index is always in range.
        let recv_idx = candidate.len() - self.suffix_segments.len() - 1;
        candidate.get(recv_idx).copied()
    }
}

/// Parse a raw pattern string into its segmented form. Returns `None` for an
/// empty or whitespace-only pattern (rejected at parse time).
fn parse_callee_pattern(raw: &str) -> Option<CalleePattern> {
    if raw.trim().is_empty() {
        return None;
    }
    let mut segments: Vec<&str> = raw.split('.').collect();
    let leading_wildcard = segments.first() == Some(&"*");
    if leading_wildcard {
        segments.remove(0);
    }
    let trailing_wildcard = segments.last() == Some(&"*");
    if trailing_wildcard {
        segments.pop();
    }
    Some(CalleePattern {
        raw: raw.to_string(),
        suffix_segments: segments.into_iter().map(str::to_string).collect(),
        leading_wildcard,
        trailing_wildcard,
    })
}

/// A parsed, validated matcher with the sink shape resolved to the typed enum
/// and callee patterns pre-segmented for O(1)-ish matching.
#[derive(Debug, Clone)]
pub struct Matcher {
    pub id: String,
    pub cwe: u32,
    pub title: String,
    pub sink_shape: SinkShape,
    pub callee_patterns: Vec<CalleePattern>,
    pub arg_index: u32,
    pub evidence_template: String,
    pub import_provenance: Option<String>,
    /// Framework enabler package gate (issue #861). `None` = global row.
    /// `Some("pkg")` requires an exact dependency match; `Some("@scope/")`
    /// (trailing slash) requires any dependency under that prefix.
    pub enabler: Option<String>,
    /// Resolved allowlist of admitted argument shapes. `None` admits any
    /// non-literal shape; `Some` requires the captured `arg_kind` to be listed.
    pub arg_kinds: Option<Vec<SinkArgKind>>,
    /// Whether this matcher only fires when the sink argument traces to a
    /// configured untrusted source binding.
    pub requires_source: bool,
    /// When non-empty, narrows `requires_source` to these catalogue source ids
    /// (issue #890): the matched source's id must be one of these. Empty admits
    /// any matched source.
    pub requires_source_kinds: Vec<String>,
    /// String-literal values admitted by this row.
    pub literal_values: Vec<String>,
    /// String fragments admitted by this row.
    pub literal_contains: Vec<String>,
    /// Integer literal values admitted by this row.
    pub literal_integers: Vec<i64>,
    /// Required literal object properties.
    pub object_properties: Vec<ObjectPropertyPredicate>,
    /// Object properties whose absence or boolean `false` makes the row match.
    pub object_missing_or_false: Vec<String>,
    /// Object keys whose absence makes the row match.
    pub object_missing: Vec<String>,
    /// Context-name keywords admitted by this row.
    pub context_keywords: Vec<String>,
}

/// A parsed, validated untrusted-source matcher (issue #859). Its
/// `path_patterns` reuse the segment-aware [`CalleePattern`] engine: a leading
/// `*.` matches any object prefix (`*.query` matches `req.query` and
/// `ctx.req.query`); a bare path matches exactly.
#[derive(Debug, Clone)]
pub struct SourceMatcher {
    pub id: String,
    pub title: String,
    pub enabler: Option<String>,
    pub path_patterns: Vec<CalleePattern>,
    /// Lowercased receiver allowlist for leading-wildcard patterns (issue
    /// #1092). Empty leaves the row ungated.
    pub receiver_allowlist: Vec<String>,
}

impl SourceMatcher {
    /// Whether any of this source's path patterns match the given flattened
    /// member-access path, subject to the built-in receiver allowlist.
    #[cfg(test)]
    #[must_use]
    pub fn matches(&self, source_path: &str) -> bool {
        let extra_receivers = FxHashSet::default();
        self.matches_with_extra_receivers(source_path, &extra_receivers)
    }

    #[must_use]
    pub fn matches_with_extra_receivers(
        &self,
        source_path: &str,
        extra_receivers: &FxHashSet<String>,
    ) -> bool {
        self.path_patterns.iter().any(|p| {
            p.matches(source_path) && self.receiver_allowed(p, source_path, extra_receivers)
        })
    }

    /// Whether `pattern`'s match on `source_path` is admitted by the receiver
    /// allowlist. An empty allowlist admits everything. For a leading-wildcard
    /// pattern the matched receiver must be in the allowlist (case-insensitive);
    /// an exact pattern (receiver fixed in the pattern) is always admitted.
    fn receiver_allowed(
        &self,
        pattern: &CalleePattern,
        source_path: &str,
        extra_receivers: &FxHashSet<String>,
    ) -> bool {
        if self.receiver_allowlist.is_empty() {
            return true;
        }
        match pattern.matched_receiver(source_path) {
            Some(receiver) => {
                self.receiver_allowlist
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(receiver))
                    || extra_receivers.contains(&receiver.to_ascii_lowercase())
            }
            None => true,
        }
    }

    /// Whether this source row's framework enabler is satisfied by the
    /// project's declared dependency set. Unset means global.
    #[must_use]
    pub fn enabler_satisfied(&self, declared_deps: &rustc_hash::FxHashSet<String>) -> bool {
        enabler_satisfied(self.enabler.as_deref(), declared_deps)
    }
}

/// The parsed catalogue: an ordered list of sink matchers plus untrusted-source
/// matchers. Order is preserved from the TOML so the detector can break on the
/// first match deterministically.
#[derive(Debug)]
pub struct Catalogue {
    matchers: Vec<Matcher>,
    sources: Vec<SourceMatcher>,
}

impl Matcher {
    /// The first callee pattern that matches the given path, if any. The first
    /// match wins, matching the deterministic declaration order.
    #[must_use]
    pub fn first_matching_pattern(&self, callee_path: &str) -> Option<&CalleePattern> {
        self.callee_patterns.iter().find(|p| p.matches(callee_path))
    }

    /// Whether a captured argument shape is admitted by this matcher. `None`
    /// `arg_kinds` admits any shape; `Some` requires the kind to be listed.
    #[must_use]
    pub fn admits_arg_kind(&self, arg_kind: SinkArgKind) -> bool {
        self.arg_kinds
            .as_ref()
            .is_none_or(|kinds| kinds.contains(&arg_kind))
    }

    /// Whether this row has opted into matching a literal, object-property, or
    /// context-only sink that is not covered by the default non-literal model.
    #[must_use]
    pub fn is_literal_aware(&self) -> bool {
        !self.literal_values.is_empty()
            || !self.literal_contains.is_empty()
            || !self.literal_integers.is_empty()
            || !self.object_properties.is_empty()
            || !self.object_missing_or_false.is_empty()
            || !self.object_missing.is_empty()
            || !self.context_keywords.is_empty()
            || self.arg_kinds.as_ref().is_some_and(|kinds| {
                kinds
                    .iter()
                    .any(|kind| matches!(kind, SinkArgKind::Literal | SinkArgKind::NoArg))
            })
    }

    /// Whether captured literal metadata satisfies this row's literal gates.
    #[must_use]
    pub fn literal_value_satisfied(&self, literal: Option<&SinkLiteralValue>) -> bool {
        if self.literal_values.is_empty()
            && self.literal_contains.is_empty()
            && self.literal_integers.is_empty()
        {
            return true;
        }
        let string_satisfied = (self.literal_values.is_empty() && self.literal_contains.is_empty())
            || match literal {
                Some(SinkLiteralValue::String(value)) => {
                    let lower = value.to_ascii_lowercase();
                    (self.literal_values.is_empty()
                        || self
                            .literal_values
                            .iter()
                            .any(|expected| lower == expected.to_ascii_lowercase()))
                        && (self.literal_contains.is_empty()
                            || self
                                .literal_contains
                                .iter()
                                .any(|needle| lower.contains(&needle.to_ascii_lowercase())))
                }
                _ => false,
            };
        let integer_satisfied = self.literal_integers.is_empty()
            || match literal {
                Some(SinkLiteralValue::Integer(value)) => self.literal_integers.contains(value),
                _ => false,
            };
        string_satisfied && integer_satisfied
    }

    /// Whether captured object-literal metadata satisfies this row's object
    /// property gates.
    #[must_use]
    pub fn object_properties_satisfied(&self, properties: &[SinkObjectProperty]) -> bool {
        if self.object_properties.is_empty() && self.object_missing_or_false.is_empty() {
            return true;
        }
        for predicate in &self.object_properties {
            let Some(property) = properties.iter().find(|p| p.key == predicate.key) else {
                return false;
            };
            if !predicate.value.matches(&property.value) {
                return false;
            }
        }
        if self.object_missing_or_false.is_empty() {
            return true;
        }
        self.object_missing_or_false.iter().any(|key| {
            properties
                .iter()
                .find(|p| p.key == *key)
                .is_none_or(|property| matches!(property.value, SinkLiteralValue::Boolean(false)))
        })
    }

    /// Whether missing-key predicates are satisfied by complete static object
    /// key metadata.
    #[must_use]
    pub fn object_missing_satisfied(&self, keys: &[String], keys_complete: bool) -> bool {
        if self.object_missing.is_empty() {
            return true;
        }
        keys_complete && self.object_missing.iter().any(|key| !keys.contains(key))
    }

    /// Whether captured context names satisfy this row's context keyword gate.
    #[must_use]
    pub fn context_satisfied(&self, context_names: &[String]) -> bool {
        if self.context_keywords.is_empty() {
            return true;
        }
        context_names.iter().any(|name| {
            let lower = name.to_ascii_lowercase();
            self.context_keywords
                .iter()
                .any(|keyword| lower.contains(&keyword.to_ascii_lowercase()))
        })
    }

    /// Whether this matcher's framework enabler is satisfied by the project's
    /// declared dependency set (issue #861). `None` enabler is always satisfied
    /// (a global row). A `Some` enabler matches by exact package name, or, when
    /// it ends with `/`, by prefix (`@angular/` matches `@angular/platform-browser`),
    /// mirroring the plugin-system `enablers()` semantics so framework rows
    /// activate on exactly the dependency universe the plugins do.
    #[must_use]
    pub fn enabler_satisfied(&self, declared_deps: &rustc_hash::FxHashSet<String>) -> bool {
        enabler_satisfied(self.enabler.as_deref(), declared_deps)
    }
}

fn enabler_satisfied(enabler: Option<&str>, declared_deps: &rustc_hash::FxHashSet<String>) -> bool {
    let Some(enabler) = enabler else {
        return true;
    };
    if let Some(prefix) = enabler.strip_suffix('/') {
        // Trailing-slash prefix match, e.g. `@fastify/` -> `@fastify/static`.
        // Also admit the bare scope name itself (`@fastify`).
        declared_deps
            .iter()
            .any(|d| d == prefix || d.starts_with(enabler))
    } else {
        declared_deps.contains(enabler)
    }
}

impl LiteralPredicate {
    fn matches(&self, value: &SinkLiteralValue) -> bool {
        match (self, value) {
            (Self::String(expected), SinkLiteralValue::String(actual)) => {
                expected.eq_ignore_ascii_case(actual)
            }
            (Self::Integer(expected), SinkLiteralValue::Integer(actual)) => expected == actual,
            (Self::Boolean(expected), SinkLiteralValue::Boolean(actual)) => expected == actual,
            (Self::Null, SinkLiteralValue::Null) => true,
            _ => false,
        }
    }
}

impl Catalogue {
    /// All matchers in declaration order.
    #[must_use]
    pub fn matchers(&self) -> &[Matcher] {
        &self.matchers
    }

    /// All untrusted-source matchers in declaration order. Test-only inspection.
    #[cfg(test)]
    #[must_use]
    pub fn sources(&self) -> &[SourceMatcher] {
        &self.sources
    }

    /// The id + human title of the first untrusted-source matcher whose pattern
    /// matches the given flattened member-access path, if any (issue #859).
    #[cfg(test)]
    #[must_use]
    pub fn matching_source(&self, source_path: &str) -> Option<(&str, &str)> {
        let request_receivers = FxHashSet::default();
        self.sources
            .iter()
            .find(|s| s.matches_with_extra_receivers(source_path, &request_receivers))
            .map(|s| (s.id.as_str(), s.title.as_str()))
    }

    /// The id + human title of the first untrusted-source matcher whose pattern
    /// and optional framework enabler match the given source path.
    #[cfg(test)]
    #[must_use]
    pub fn matching_source_for_deps(
        &self,
        source_path: &str,
        declared_deps: &FxHashSet<String>,
    ) -> Option<(&str, &str)> {
        let request_receivers = FxHashSet::default();
        self.matching_source_for_deps_with_receivers(source_path, declared_deps, &request_receivers)
    }

    /// The id + human title of the first untrusted-source matcher whose pattern,
    /// optional framework enabler, and configured request-receiver extension
    /// match the given source path.
    #[must_use]
    pub fn matching_source_for_deps_with_receivers(
        &self,
        source_path: &str,
        declared_deps: &FxHashSet<String>,
        request_receivers: &FxHashSet<String>,
    ) -> Option<(&str, &str)> {
        let empty_receivers = FxHashSet::default();
        self.sources
            .iter()
            .find(|s| {
                let extra_receivers = if s.id == "http-request-input" {
                    request_receivers
                } else {
                    &empty_receivers
                };
                s.enabler_satisfied(declared_deps)
                    && s.matches_with_extra_receivers(source_path, extra_receivers)
            })
            .map(|s| (s.id.as_str(), s.title.as_str()))
    }

    /// Whether the given flattened member-access path matches any untrusted
    /// source pattern (issue #859). Test-only convenience over `matching_source`.
    #[cfg(test)]
    #[must_use]
    pub fn is_source_path(&self, source_path: &str) -> bool {
        self.matching_source(source_path).is_some()
    }

    /// The human-readable title for a category id, if any matcher declares it.
    #[must_use]
    pub fn title_for(&self, id: &str) -> Option<&str> {
        self.matchers
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.title.as_str())
    }
}

/// The human-readable title for a category id, used by the CLI renderer.
#[must_use]
pub fn catalogue_title(id: &str) -> Option<&'static str> {
    catalogue().title_for(id)
}

/// Resolve a kebab-case sink-shape string into the typed [`SinkShape`].
fn parse_sink_shape(s: &str) -> Option<SinkShape> {
    match s {
        "call" => Some(SinkShape::Call),
        "member-call" => Some(SinkShape::MemberCall),
        "member-assign" => Some(SinkShape::MemberAssign),
        "tagged-template" => Some(SinkShape::TaggedTemplate),
        "jsx-attr" => Some(SinkShape::JsxAttr),
        "new-expression" => Some(SinkShape::NewExpression),
        _ => None,
    }
}

/// Resolve a kebab-case arg-kind string into the typed [`SinkArgKind`].
fn parse_arg_kind(s: &str) -> Option<SinkArgKind> {
    match s {
        "template-with-subst" => Some(SinkArgKind::TemplateWithSubst),
        "concat" => Some(SinkArgKind::Concat),
        "object" => Some(SinkArgKind::Object),
        "call" => Some(SinkArgKind::Call),
        "literal" => Some(SinkArgKind::Literal),
        "no-arg" => Some(SinkArgKind::NoArg),
        "other" => Some(SinkArgKind::Other),
        _ => None,
    }
}

fn parse_object_property_predicates(
    id: &str,
    raw: Option<Vec<RawObjectPropertyPredicate>>,
) -> Result<Vec<ObjectPropertyPredicate>, String> {
    let Some(raw_predicates) = raw else {
        return Ok(Vec::new());
    };
    let mut predicates = Vec::with_capacity(raw_predicates.len());
    for predicate in raw_predicates {
        if predicate.key.trim().is_empty() {
            return Err(format!(
                "matcher {id:?} has an object_properties predicate with an empty key"
            ));
        }
        let value_count = usize::from(predicate.string.is_some())
            + usize::from(predicate.boolean.is_some())
            + usize::from(predicate.integer.is_some())
            + usize::from(predicate.null);
        if value_count != 1 {
            return Err(format!(
                "matcher {id:?} object_properties predicate for {:?} must set exactly one of string | boolean | integer | null",
                predicate.key
            ));
        }
        let value = if let Some(string) = predicate.string {
            LiteralPredicate::String(string)
        } else if let Some(boolean) = predicate.boolean {
            LiteralPredicate::Boolean(boolean)
        } else if let Some(integer) = predicate.integer {
            LiteralPredicate::Integer(integer)
        } else {
            LiteralPredicate::Null
        };
        predicates.push(ObjectPropertyPredicate {
            key: predicate.key,
            value,
        });
    }
    Ok(predicates)
}

/// Parse + validate the catalogue source. Returns a `Result` (NOT a panic) so
/// the validation tests can assert on error messages; `catalogue()` unwraps it.
///
/// Validates: non-empty id; cwe > 0; sink_shape resolves; callee_patterns
/// non-empty and every pattern non-empty/non-whitespace; non-empty
/// evidence_template.
fn parse_catalogue(src: &str) -> Result<Catalogue, String> {
    let raw: RawCatalogue =
        toml::from_str(src).map_err(|e| format!("security_matchers.toml parse error: {e}"))?;

    let mut matchers = Vec::with_capacity(raw.matcher.len());
    for entry in raw.matcher {
        if entry.id.trim().is_empty() {
            return Err("matcher id must be non-empty / non-whitespace".to_string());
        }
        if entry.cwe == 0 {
            return Err(format!("matcher {:?} has cwe 0; cwe must be > 0", entry.id));
        }
        let sink_shape = parse_sink_shape(&entry.sink_shape).ok_or_else(|| {
            format!(
                "matcher {:?} has unknown sink_shape {:?}; expected one of \
                 call | member-call | member-assign | tagged-template | jsx-attr | new-expression",
                entry.id, entry.sink_shape
            )
        })?;
        if entry.callee_patterns.is_empty() {
            return Err(format!(
                "matcher {:?} has no callee_patterns; at least one is required",
                entry.id
            ));
        }
        if entry.evidence_template.trim().is_empty() {
            return Err(format!(
                "matcher {:?} has an empty evidence_template",
                entry.id
            ));
        }
        let mut callee_patterns = Vec::with_capacity(entry.callee_patterns.len());
        for pat in &entry.callee_patterns {
            let parsed = parse_callee_pattern(pat).ok_or_else(|| {
                format!(
                    "matcher {:?} has an empty / whitespace callee_pattern {pat:?}",
                    entry.id
                )
            })?;
            callee_patterns.push(parsed);
        }
        let arg_kinds = match &entry.arg_kinds {
            None => None,
            Some(raw_kinds) => {
                if raw_kinds.is_empty() {
                    return Err(format!(
                        "matcher {:?} has an empty arg_kinds list; omit the key to admit any shape",
                        entry.id
                    ));
                }
                let mut kinds = Vec::with_capacity(raw_kinds.len());
                for raw in raw_kinds {
                    let kind = parse_arg_kind(raw).ok_or_else(|| {
                        format!(
                            "matcher {:?} has unknown arg_kind {raw:?}; expected one of \
                             template-with-subst | concat | object | call | literal | no-arg | other",
                            entry.id
                        )
                    })?;
                    kinds.push(kind);
                }
                Some(kinds)
            }
        };
        let enabler = match entry.enabler {
            Some(e) if e.trim().is_empty() => {
                return Err(format!(
                    "matcher {:?} has an empty / whitespace enabler; omit the key for a global row",
                    entry.id
                ));
            }
            other => other,
        };
        let object_properties =
            parse_object_property_predicates(&entry.id, entry.object_properties)?;
        matchers.push(Matcher {
            id: entry.id,
            cwe: entry.cwe,
            title: entry.title,
            sink_shape,
            callee_patterns,
            arg_index: entry.arg_index,
            evidence_template: entry.evidence_template,
            import_provenance: entry.import_provenance,
            enabler,
            arg_kinds,
            requires_source: entry.requires_source,
            requires_source_kinds: entry.requires_source_kinds,
            literal_values: entry.literal_values.unwrap_or_default(),
            literal_contains: entry.literal_contains.unwrap_or_default(),
            literal_integers: entry.literal_integers.unwrap_or_default(),
            object_properties,
            object_missing_or_false: entry.object_missing_or_false.unwrap_or_default(),
            object_missing: entry.object_missing.unwrap_or_default(),
            context_keywords: entry.context_keywords.unwrap_or_default(),
        });
    }

    if matchers.is_empty() {
        return Err("security_matchers.toml has no [[matcher]] entries".to_string());
    }

    let mut sources = Vec::with_capacity(raw.source.len());
    for entry in raw.source {
        if entry.id.trim().is_empty() {
            return Err("source id must be non-empty / non-whitespace".to_string());
        }
        if entry.path_patterns.is_empty() {
            return Err(format!(
                "source {:?} has no path_patterns; at least one is required",
                entry.id
            ));
        }
        let mut path_patterns = Vec::with_capacity(entry.path_patterns.len());
        for pat in &entry.path_patterns {
            let parsed = parse_callee_pattern(pat).ok_or_else(|| {
                format!(
                    "source {:?} has an empty / whitespace path_pattern {pat:?}",
                    entry.id
                )
            })?;
            path_patterns.push(parsed);
        }
        let enabler = match entry.enabler {
            Some(e) if e.trim().is_empty() => {
                return Err(format!(
                    "source {:?} has an empty / whitespace enabler; omit the key for a global row",
                    entry.id
                ));
            }
            other => other,
        };
        let mut receiver_allowlist = Vec::with_capacity(entry.receiver_allowlist.len());
        for receiver in entry.receiver_allowlist {
            if receiver.trim().is_empty() {
                return Err(format!(
                    "source {:?} has an empty / whitespace receiver_allowlist entry; omit the key for an ungated row",
                    entry.id
                ));
            }
            receiver_allowlist.push(receiver.to_ascii_lowercase());
        }
        sources.push(SourceMatcher {
            id: entry.id,
            title: entry.title,
            enabler,
            path_patterns,
            receiver_allowlist,
        });
    }

    Ok(Catalogue { matchers, sources })
}

/// Parse and cache the embedded catalogue once. Unwraps the parse `Result`; in
/// a released binary this is unreachable because the bytes are compile-time
/// embedded and gated by `security_catalogue_parses`.
#[expect(
    clippy::expect_used,
    reason = "compile-time-embedded catalogue pinned by security_catalogue_parses"
)]
pub fn catalogue() -> &'static Catalogue {
    static CATALOGUE: std::sync::OnceLock<Catalogue> = std::sync::OnceLock::new();
    CATALOGUE.get_or_init(|| {
        parse_catalogue(CATALOGUE_TOML).expect(
            "embedded crates/core/data/security_matchers.toml must parse; run \
             `cargo test -p fallow-core security_catalogue_parses` to see the error",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet;

    #[test]
    fn security_catalogue_parses() {
        let cat = catalogue();
        assert!(!cat.matchers().is_empty(), "catalogue must have matchers");
        assert!(
            cat.matchers().iter().any(|m| m.id == "dangerous-html"),
            "catalogue must contain the dangerous-html seed"
        );
    }

    #[test]
    fn catalogue_rows_are_unique() {
        // Multiple rows legitimately share an `id` (dangerous-html spans three
        // shapes), so uniqueness is keyed on the FULL row: id + sink_shape +
        // callee_patterns + gates. No two identical matcher rows. Keyed off the
        // raw source so the test does not require `SinkShape: Hash`.
        let raw: RawCatalogue = toml::from_str(CATALOGUE_TOML).unwrap();
        let mut seen = FxHashSet::default();
        for m in &raw.matcher {
            let pats = m.callee_patterns.join("|");
            // Uniqueness includes the enabler: framework-scoped rows (#861) may
            // legitimately share id + shape + patterns and differ only by their
            // framework gate (e.g. one `route-send-file` row per framework).
            let enabler = m.enabler.as_deref().unwrap_or("");
            let import_provenance = m.import_provenance.as_deref().unwrap_or("");
            let arg_kinds = m
                .arg_kinds
                .as_ref()
                .map_or_else(String::new, |kinds| kinds.join("|"));
            let literal_values = m
                .literal_values
                .as_ref()
                .map_or_else(String::new, |values| values.join("|"));
            let literal_contains = m
                .literal_contains
                .as_ref()
                .map_or_else(String::new, |values| values.join("|"));
            let literal_integers = m
                .literal_integers
                .as_ref()
                .map_or_else(String::new, |values| {
                    values
                        .iter()
                        .map(i64::to_string)
                        .collect::<Vec<_>>()
                        .join("|")
                });
            let object_properties = format!("{:?}", m.object_properties);
            let object_missing_or_false = m
                .object_missing_or_false
                .as_ref()
                .map_or_else(String::new, |keys| keys.join("|"));
            let object_missing = m
                .object_missing
                .as_ref()
                .map_or_else(String::new, |keys| keys.join("|"));
            let context_keywords = m
                .context_keywords
                .as_ref()
                .map_or_else(String::new, |keywords| keywords.join("|"));
            let key = format!(
                "{}::{}::{pats}::{enabler}::{import_provenance}::{}::{arg_kinds}::{literal_values}::{literal_contains}::{literal_integers}::{object_properties}::{object_missing_or_false}::{object_missing}::{context_keywords}",
                m.id, m.sink_shape, m.requires_source
            );
            assert!(seen.insert(key.clone()), "duplicate matcher row: {key}");
        }
    }

    #[test]
    fn catalogue_ids_non_empty() {
        for m in catalogue().matchers() {
            assert!(
                !m.id.trim().is_empty(),
                "matcher id must be non-empty / non-whitespace"
            );
        }
    }

    #[test]
    fn catalogue_cwe_valid() {
        for m in catalogue().matchers() {
            assert!(m.cwe > 0, "matcher {:?} has cwe 0", m.id);
        }
    }

    #[test]
    fn catalogue_sink_shapes_known() {
        // Every parsed matcher already carries a typed SinkShape, so re-parse
        // the raw source to assert the kebab strings all resolve.
        let raw: RawCatalogue = toml::from_str(CATALOGUE_TOML).unwrap();
        for m in &raw.matcher {
            assert!(
                parse_sink_shape(&m.sink_shape).is_some(),
                "matcher {:?} has unknown sink_shape {:?}",
                m.id,
                m.sink_shape
            );
        }
    }

    #[test]
    fn catalogue_callee_patterns_non_empty() {
        for m in catalogue().matchers() {
            assert!(
                !m.callee_patterns.is_empty(),
                "matcher {:?} has no callee_patterns",
                m.id
            );
            for p in &m.callee_patterns {
                assert!(
                    !p.raw().trim().is_empty(),
                    "matcher {:?} has an empty callee_pattern",
                    m.id
                );
            }
        }
    }

    #[test]
    fn catalogue_evidence_templates_non_empty() {
        for m in catalogue().matchers() {
            assert!(
                !m.evidence_template.trim().is_empty(),
                "matcher {:?} has an empty evidence_template",
                m.id
            );
        }
    }

    #[test]
    fn parse_rejects_empty_id() {
        let toml = r#"
[[matcher]]
id = ""
cwe = 79
title = "x"
sink_shape = "member-assign"
callee_patterns = ["*.innerHTML"]
arg_index = 0
evidence_template = "x"
"#;
        let err = parse_catalogue(toml).unwrap_err();
        assert!(err.contains("id must be non-empty"), "got: {err}");
    }

    #[test]
    fn parse_rejects_zero_cwe() {
        let toml = r#"
[[matcher]]
id = "x"
cwe = 0
title = "x"
sink_shape = "member-assign"
callee_patterns = ["*.innerHTML"]
arg_index = 0
evidence_template = "x"
"#;
        let err = parse_catalogue(toml).unwrap_err();
        assert!(err.contains("cwe"), "got: {err}");
    }

    #[test]
    fn parse_rejects_unknown_sink_shape() {
        let toml = r#"
[[matcher]]
id = "x"
cwe = 79
title = "x"
sink_shape = "not-a-shape"
callee_patterns = ["*.innerHTML"]
arg_index = 0
evidence_template = "x"
"#;
        let err = parse_catalogue(toml).unwrap_err();
        assert!(err.contains("unknown sink_shape"), "got: {err}");
    }

    #[test]
    fn parse_rejects_empty_callee_patterns() {
        let toml = r#"
[[matcher]]
id = "x"
cwe = 79
title = "x"
sink_shape = "member-assign"
callee_patterns = []
arg_index = 0
evidence_template = "x"
"#;
        let err = parse_catalogue(toml).unwrap_err();
        assert!(err.contains("callee_patterns"), "got: {err}");
    }

    #[test]
    fn parse_rejects_empty_pattern_string() {
        let toml = r#"
[[matcher]]
id = "x"
cwe = 79
title = "x"
sink_shape = "member-assign"
callee_patterns = ["   "]
arg_index = 0
evidence_template = "x"
"#;
        let err = parse_catalogue(toml).unwrap_err();
        assert!(err.contains("empty"), "got: {err}");
    }

    #[test]
    fn parse_rejects_empty_evidence_template() {
        let toml = r#"
[[matcher]]
id = "x"
cwe = 79
title = "x"
sink_shape = "member-assign"
callee_patterns = ["*.innerHTML"]
arg_index = 0
evidence_template = "   "
"#;
        let err = parse_catalogue(toml).unwrap_err();
        assert!(err.contains("evidence_template"), "got: {err}");
    }

    #[test]
    fn parse_rejects_no_matchers() {
        let err = parse_catalogue("").unwrap_err();
        assert!(err.contains("no [[matcher]]"), "got: {err}");
    }

    #[test]
    fn segment_match_is_not_substring() {
        let bare = parse_callee_pattern("fetch").unwrap();
        assert!(bare.matches("fetch"));
        assert!(!bare.matches("myfetch"));
        assert!(!bare.matches("fetcher"));

        let wildcard = parse_callee_pattern("*.innerHTML").unwrap();
        assert!(wildcard.matches("el.innerHTML"));
        assert!(wildcard.matches("this.node.innerHTML"));
        assert!(!wildcard.matches("el.innerHTMLFoo"));
        assert!(!wildcard.matches("innerHTML")); // wildcard requires an object

        let dotted = parse_callee_pattern("child_process.exec").unwrap();
        assert!(dotted.matches("child_process.exec"));
        assert!(!dotted.matches("exec"));
        assert!(!dotted.matches("child_process.execSync"));
        assert!(!dotted.matches("my_child_process.exec"));
    }

    #[test]
    fn wildcard_only_pattern_matches_nothing() {
        // Guard against a degenerate `*` pattern matching every callee.
        let star = parse_callee_pattern("*").unwrap();
        assert!(!star.matches("el.innerHTML"));
        assert!(!star.matches("anything"));
    }

    #[test]
    fn trailing_wildcard_prefix_matches() {
        let trailing = parse_callee_pattern("child_process.*").unwrap();
        assert!(trailing.matches("child_process.exec"));
        assert!(trailing.matches("child_process.exec.call"));
        assert!(!trailing.matches("child_process")); // requires a member
        assert!(!trailing.matches("my_child_process.exec"));
        assert!(!trailing.matches("exec"));

        let console = parse_callee_pattern("console.*").unwrap();
        assert!(console.matches("console.log"));
        assert!(!console.matches("myconsole.log"));
    }

    #[test]
    fn double_wildcard_pattern_matches_nothing() {
        // `*.x.*` and `*.*` are rejected by config validation; the matcher
        // guards against them anyway.
        let both = parse_callee_pattern("*.query.*").unwrap();
        assert!(!both.matches("db.query.run"));
        let stars = parse_callee_pattern("*.*").unwrap();
        assert!(!stars.matches("a.b"));
    }

    #[test]
    fn arg_kinds_unset_admits_any_shape() {
        // A matcher with no arg_kinds (e.g. dangerous-html) admits every shape.
        let html = catalogue()
            .matchers()
            .iter()
            .find(|m| m.id == "dangerous-html")
            .expect("dangerous-html present");
        for kind in [
            SinkArgKind::TemplateWithSubst,
            SinkArgKind::Concat,
            SinkArgKind::Object,
            SinkArgKind::Call,
            SinkArgKind::Literal,
            SinkArgKind::NoArg,
            SinkArgKind::Other,
        ] {
            assert!(html.admits_arg_kind(kind), "html admits {kind:?}");
        }
    }

    #[test]
    fn sql_injection_query_execute_excludes_object_arg_kind() {
        // The `.query` / `.execute` matchers must require unsafe shapes (concat /
        // interpolated template) and reject the parameterized object-literal form
        // (`.execute({ sql, args })`). The separate `sql.raw` escape-hatch row is
        // intentionally shape-agnostic and is excluded from this check.
        let query_matchers: Vec<&Matcher> = catalogue()
            .matchers()
            .iter()
            .filter(|m| {
                m.id == "sql-injection"
                    && m.callee_patterns
                        .iter()
                        .any(|p| p.raw() == "*.query" || p.raw() == "*.execute")
            })
            .collect();
        assert!(
            !query_matchers.is_empty(),
            "sql-injection .query/.execute rows present"
        );
        for m in query_matchers {
            let kinds = m
                .arg_kinds
                .as_ref()
                .unwrap_or_else(|| panic!("sql-injection query/execute must constrain arg_kinds"));
            assert!(
                !kinds.contains(&SinkArgKind::Object),
                "sql-injection .query/.execute must not admit the object (parameterized) form"
            );
            assert!(
                !m.admits_arg_kind(SinkArgKind::Object),
                "admits_arg_kind agrees: object excluded"
            );
            assert!(
                m.admits_arg_kind(SinkArgKind::Concat),
                "sql-injection .query/.execute admits the concat (unsafe) form"
            );
        }
    }

    #[test]
    fn source_required_matchers_are_explicit() {
        let mass_assignment = catalogue()
            .matchers()
            .iter()
            .find(|m| m.id == "mass-assignment")
            .expect("mass-assignment row present");
        assert!(
            mass_assignment.requires_source,
            "mass-assignment should only fire for source-backed arguments"
        );
    }

    #[test]
    fn literal_integer_predicate_matches_integer_literals() {
        let chmod = catalogue()
            .matchers()
            .iter()
            .find(|m| m.id == "world-writable-permission" && m.sink_shape == SinkShape::MemberCall)
            .expect("world-writable permission row present");

        assert!(chmod.literal_value_satisfied(Some(&SinkLiteralValue::Integer(511))));
        assert!(!chmod.literal_value_satisfied(Some(&SinkLiteralValue::Integer(420))));
        assert!(
            !chmod.literal_value_satisfied(Some(&SinkLiteralValue::String("0o777".to_string())))
        );
    }

    #[test]
    fn object_property_predicate_matches_nested_integer_values() {
        let toml = r#"
[[matcher]]
id = "x"
cwe = 732
title = "x"
sink_shape = "member-call"
callee_patterns = ["fs.chmod"]
arg_index = 0
arg_kinds = ["object"]
object_properties = [{ key = "mode.value", integer = 511 }]
evidence_template = "x"
"#;
        let cat = parse_catalogue(toml).expect("catalogue parses");
        let matcher = cat.matchers().first().expect("matcher present");
        let properties = vec![SinkObjectProperty {
            key: "mode.value".to_string(),
            value: SinkLiteralValue::Integer(511),
        }];

        assert!(matcher.object_properties_satisfied(&properties));
    }

    #[test]
    fn object_missing_requires_complete_key_metadata() {
        let jwt_verify = catalogue()
            .matchers()
            .iter()
            .find(|m| m.id == "jwt-verify-missing-algorithms")
            .expect("jwt verify missing algorithms row present");

        assert!(
            jwt_verify.is_literal_aware(),
            "object_missing rows opt into literal-aware matching"
        );
        assert!(jwt_verify.object_missing_satisfied(&[], true));
        assert!(jwt_verify.object_missing_satisfied(&["audience".to_string()], true));
        assert!(!jwt_verify.object_missing_satisfied(&["algorithms".to_string()], true));
        assert!(!jwt_verify.object_missing_satisfied(&["audience".to_string()], false));
    }

    #[test]
    fn parse_rejects_unknown_arg_kind() {
        let toml = r#"
[[matcher]]
id = "x"
cwe = 89
title = "x"
sink_shape = "member-call"
callee_patterns = ["*.query"]
arg_index = 0
arg_kinds = ["not-a-kind"]
evidence_template = "x"
"#;
        let err = parse_catalogue(toml).unwrap_err();
        assert!(err.contains("unknown arg_kind"), "got: {err}");
    }

    #[test]
    fn enabler_unset_is_global() {
        // A matcher with no enabler is satisfied by ANY (even empty) dep set.
        let html = catalogue()
            .matchers()
            .iter()
            .find(|m| m.id == "dangerous-html")
            .expect("dangerous-html present");
        assert!(html.enabler.is_none(), "dangerous-html is a global row");
        assert!(html.enabler_satisfied(&FxHashSet::default()));
    }

    #[test]
    fn enabler_satisfied_exact_and_prefix() {
        let mut m = catalogue()
            .matchers()
            .iter()
            .find(|m| m.id == "dangerous-html")
            .cloned()
            .expect("dangerous-html present");

        // Exact match.
        m.enabler = Some("jquery".to_string());
        let mut deps = FxHashSet::default();
        assert!(!m.enabler_satisfied(&deps), "absent dep is not satisfied");
        deps.insert("jquery".to_string());
        assert!(m.enabler_satisfied(&deps), "present exact dep satisfies");

        // Trailing-slash prefix match, plus the bare scope name.
        m.enabler = Some("@angular/".to_string());
        let mut scoped = FxHashSet::default();
        assert!(!m.enabler_satisfied(&scoped));
        scoped.insert("@angular/platform-browser".to_string());
        assert!(m.enabler_satisfied(&scoped), "prefix dep satisfies");
        let mut bare_scope = FxHashSet::default();
        bare_scope.insert("@angular".to_string());
        assert!(
            m.enabler_satisfied(&bare_scope),
            "bare scope name satisfies the prefix form"
        );

        // A near-miss exact name does not satisfy a prefix-less enabler.
        m.enabler = Some("react".to_string());
        let mut reactish = FxHashSet::default();
        reactish.insert("react-dom".to_string());
        assert!(
            !m.enabler_satisfied(&reactish),
            "exact enabler must not prefix-match"
        );
    }

    #[test]
    fn framework_scoped_rows_are_present() {
        // The framework-scoped rows added in #861 carry an enabler.
        let cat = catalogue();
        let angular = cat
            .matchers()
            .iter()
            .find(|m| m.id == "angular-trusted-html")
            .expect("angular-trusted-html present");
        assert_eq!(
            angular.enabler.as_deref(),
            Some("@angular/platform-browser")
        );
        assert!(
            cat.matchers().iter().any(|m| m.id == "jquery-html"),
            "jquery-html present"
        );
        assert!(
            cat.matchers().iter().any(|m| m.id == "dom-document-write"),
            "dom-document-write present"
        );
    }

    #[test]
    fn parse_rejects_empty_enabler() {
        let toml = r#"
[[matcher]]
id = "x"
cwe = 79
title = "x"
sink_shape = "member-call"
callee_patterns = ["*.html"]
arg_index = 0
enabler = "   "
evidence_template = "x"
"#;
        let err = parse_catalogue(toml).unwrap_err();
        assert!(err.contains("empty / whitespace enabler"), "got: {err}");
    }

    #[test]
    fn catalogue_has_untrusted_sources() {
        // Issue #859: the embedded catalogue ships at least one [[source]] row,
        // each with a non-empty id, title, and path_patterns.
        let cat = catalogue();
        assert!(
            !cat.sources().is_empty(),
            "catalogue must ship untrusted-source rows"
        );
        for s in cat.sources() {
            assert!(!s.id.trim().is_empty(), "source id non-empty");
            assert!(!s.title.trim().is_empty(), "source title non-empty");
            assert!(!s.path_patterns.is_empty(), "source has path patterns");
        }
    }

    #[test]
    fn source_paths_match_expected_request_inputs() {
        let cat = catalogue();
        // Wildcard object prefix matches common framework request accessors.
        assert!(cat.is_source_path("req.query"));
        assert!(cat.is_source_path("ctx.req.query"));
        assert!(cat.is_source_path("request.body"));
        assert!(cat.is_source_path("req.params"));
        assert!(cat.is_source_path("process.argv"));
        assert!(cat.is_source_path("event.data"));
        assert!(cat.is_source_path("request.rawBody"));
        assert!(cat.is_source_path("document.referrer"));
        assert!(cat.is_source_path("window.name"));
        assert!(cat.is_source_path("document.cookie"));
        // A plain object path that is not an untrusted source does not match.
        assert!(!cat.is_source_path("config.value"));
        assert!(!cat.is_source_path("user.name"));
        assert!(!cat.is_source_path("profile.name"));
        assert!(!cat.is_source_path("jar.cookie"));
    }

    #[test]
    fn source_matcher_matches_helper() {
        let cat = catalogue();
        let http = cat
            .sources()
            .iter()
            .find(|s| s.id == "http-request-input")
            .expect("http-request-input source present");
        assert!(http.matches("req.query"));
        assert!(!http.matches("process.argv"));
    }

    #[test]
    fn matched_receiver_returns_segment_before_suffix() {
        // Leading-wildcard `*.query`: the receiver is the segment right before
        // the matched `query`, regardless of how many object segments precede.
        let pat = parse_callee_pattern("*.query").expect("pattern parses");
        assert_eq!(pat.matched_receiver("db.query"), Some("db"));
        assert_eq!(pat.matched_receiver("req.query"), Some("req"));
        // Hono `c.req.query` flattens so the receiver of `.query` is `req`.
        assert_eq!(pat.matched_receiver("ctx.req.query"), Some("req"));
        // A non-matching path has no receiver.
        assert_eq!(pat.matched_receiver("req.body"), None);
        // An exact (non-wildcard) pattern's receiver is fixed in the pattern, so
        // `matched_receiver` returns None even on a match.
        let exact = parse_callee_pattern("process.env").expect("pattern parses");
        assert_eq!(exact.matched_receiver("process.env"), None);
    }

    #[test]
    fn receiver_allowlist_rejects_orm_query_builders_keeps_request_objects() {
        // Issue #1092: the global HTTP-input row is receiver-gated. ORM /
        // data-access receivers no longer classify their module as a source...
        let cat = catalogue();
        assert!(!cat.is_source_path("db.query"), "Drizzle db.query");
        assert!(!cat.is_source_path("prisma.query"), "Prisma prisma.query");
        assert!(!cat.is_source_path("drizzle.query"));
        assert!(!cat.is_source_path("knex.body"));
        assert!(!cat.is_source_path("client.query"));
        // ...nor do non-request receivers that merely happen to have a `.query`
        // member (a sibling-collision check: `dbConn` is not `db`).
        assert!(!cat.is_source_path("dbConn.query"));
        assert!(!cat.is_source_path("database.params"));
        // A genuine request receiver still classifies as a source.
        assert!(cat.is_source_path("req.query"), "Express req.query");
        assert!(cat.is_source_path("request.body"));
        assert!(cat.is_source_path("ctx.params"), "Koa/Elysia ctx.params");
        assert!(cat.is_source_path("context.body"));
        assert!(cat.is_source_path("event.query"), "SvelteKit event.query");
        // Hono `c.req.query`: the matched receiver is `req`, which is allowed.
        assert!(cat.is_source_path("ctx.req.query"));
        // The allowlist is case-insensitive.
        assert!(cat.is_source_path("Req.query"));
    }

    #[test]
    fn configured_request_receivers_extend_http_request_source_allowlist() {
        let cat = catalogue();
        let deps = FxHashSet::default();
        let receivers = FxHashSet::from_iter(["h".to_string(), "httpreq".to_string()]);

        assert!(
            cat.matching_source_for_deps_with_receivers("h.query", &deps, &receivers)
                .is_some()
        );
        assert!(
            cat.matching_source_for_deps_with_receivers("HttpReq.body", &deps, &receivers)
                .is_some()
        );
        assert!(
            cat.matching_source_for_deps_with_receivers("req.params", &deps, &receivers)
                .is_some()
        );
        assert!(
            cat.matching_source_for_deps_with_receivers("db.query", &deps, &receivers)
                .is_none()
        );
    }

    #[test]
    fn search_params_source_stays_ungated() {
        // Issue #1092: `*.searchParams` is intentionally NOT receiver-gated, so a
        // `new URL(...).searchParams` binding on an arbitrary local still counts.
        let cat = catalogue();
        assert!(cat.is_source_path("u.searchParams"));
        assert!(cat.is_source_path("url.searchParams"));
        assert!(cat.is_source_path("params.searchParams"));
    }

    #[test]
    fn parse_rejects_empty_receiver_allowlist_entry() {
        let toml = r#"
[[matcher]]
id = "x"
cwe = 79
title = "x"
sink_shape = "member-assign"
callee_patterns = ["*.innerHTML"]
arg_index = 0
evidence_template = "x"

[[source]]
id = "http"
title = "HTTP"
path_patterns = ["*.query"]
receiver_allowlist = ["req", "  "]
"#;
        let err = parse_catalogue(toml).unwrap_err();
        assert!(err.contains("receiver_allowlist"), "got: {err}");
    }

    #[test]
    fn source_enabler_gates_framework_param_sources() {
        let cat = catalogue();
        let source = cat
            .sources()
            .iter()
            .find(|s| s.id == "framework-handler-input" && s.enabler.as_deref() == Some("express"))
            .expect("express handler source present");
        assert!(source.matches("framework.request"));

        let empty = FxHashSet::default();
        assert!(!source.enabler_satisfied(&empty));
        assert!(
            cat.matching_source_for_deps("framework.request", &empty)
                .is_none(),
            "framework handler params require an enabler"
        );

        let mut deps = FxHashSet::default();
        deps.insert("express".to_string());
        assert!(source.enabler_satisfied(&deps));
        assert_eq!(
            cat.matching_source_for_deps("framework.request", &deps),
            Some(("framework-handler-input", "Framework handler input"))
        );
    }

    #[test]
    fn source_enabler_gates_graphql_and_trpc_param_sources() {
        let cat = catalogue();
        let empty = FxHashSet::default();
        assert!(
            cat.matching_source_for_deps("graphql.args", &empty)
                .is_none(),
            "GraphQL resolver args require a matching package"
        );
        assert!(
            cat.matching_source_for_deps("trpc.input", &empty).is_none(),
            "tRPC procedure input requires a matching package"
        );

        let mut graphql_deps = FxHashSet::default();
        graphql_deps.insert("@apollo/server".to_string());
        assert_eq!(
            cat.matching_source_for_deps("graphql.args", &graphql_deps),
            Some(("graphql-resolver-args", "GraphQL resolver args"))
        );

        let mut trpc_deps = FxHashSet::default();
        trpc_deps.insert("@trpc/server".to_string());
        assert_eq!(
            cat.matching_source_for_deps("trpc.input", &trpc_deps),
            Some(("trpc-procedure-input", "tRPC procedure input"))
        );
    }

    #[test]
    fn parse_rejects_source_without_patterns() {
        let toml = r#"
[[matcher]]
id = "x"
cwe = 79
title = "x"
sink_shape = "member-assign"
callee_patterns = ["*.innerHTML"]
arg_index = 0
evidence_template = "x"

[[source]]
id = "bad"
title = "bad"
path_patterns = []
"#;
        let err = parse_catalogue(toml).unwrap_err();
        assert!(err.contains("path_patterns"), "got: {err}");
    }

    #[test]
    fn parse_rejects_empty_arg_kinds() {
        let toml = r#"
[[matcher]]
id = "x"
cwe = 89
title = "x"
sink_shape = "member-call"
callee_patterns = ["*.query"]
arg_index = 0
arg_kinds = []
evidence_template = "x"
"#;
        let err = parse_catalogue(toml).unwrap_err();
        assert!(err.contains("empty arg_kinds"), "got: {err}");
    }
}
