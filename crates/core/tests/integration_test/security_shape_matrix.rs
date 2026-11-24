//! Generated shape matrix for the call rows of the security catalogue.
//!
//! The test reads every `call`, `member-call` and `new-expression` row of the
//! catalogue and writes the source text of each case itself. It adds no
//! fixture per row. For each callee pattern of a row it finds a baseline: the
//! first generated argument that gives a candidate of the row category in the
//! plain form. It then writes each equivalent form of that call and expects a
//! candidate of the same category for each form that the row accepts:
//!
//! - the plain form and the `new` form, as the row accepts them;
//! - the last member written as an optional chain, a non-null assertion or a
//!   computed key, and the callee in an indirect call sequence `(0, f)`;
//! - a local alias and a global-object prefix of a code-executing global;
//! - a `require()` namespace, a destructured `require()`, and a destructured
//!   module binding, for a `node:` pattern;
//! - a `this.field` receiver, a call-result receiver, a chain with more call
//!   results than the extractor walks, for a pattern with a leading `*.`;
//! - an extra leading literal argument, for a row that admits more than one
//!   argument position.
//!
//! A call of a member of `this` itself (`this.m(x)`) must give a candidate
//! exactly when the row sets `this_receiver`. The matrix writes that call in a
//! class with a library base, a class that declares `m`, an arrow callback in
//! an object literal, a class with a local base, a prototype function, and an
//! options object.
//!
//! A pattern with no baseline must be in [`PATTERNS_WITHOUT_BASELINE`], with a
//! reason. A candidate counts for a row only when its category and its
//! evidence text come from that row.
//! A row that must trace its import binding to the callee root gets only the
//! plain receiver.
//! A new row thus gets this coverage with no new fixture.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use fallow_config::{SecurityCategories, Severity};
use fallow_security::{LiteralPredicate, Matcher, catalogue};
use fallow_types::extract::SinkShape;

use super::common::create_config_with_rules;

/// Patterns that no generated argument can satisfy in the plain form, keyed by
/// `id:pattern`. Each one gets its candidates from another capture path.
const PATTERNS_WITHOUT_BASELINE: &[(&str, &str)] = &[
    (
        "command-injection:child_process.exec",
        "an import of `child_process` resolves the callee to `node:child_process.exec`, \
         which the `node:` row matches",
    ),
    (
        "command-injection:child_process.execSync",
        "an import of `child_process` resolves the callee to `node:child_process.execSync`, \
         which the `node:` row matches",
    ),
    (
        "insecure-randomness:Math.random",
        "the extractor builds this sink from a token-like binding name",
    ),
];

/// The argument that carries request input in a generated case.
const SOURCE_ARG: &str = "req.body.input";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Form {
    Call,
    New,
}

/// The receiver form of a generated call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Receiver {
    /// The callee as the pattern writes it.
    Plain,
    /// `a.b?.m(x)`.
    Optional,
    /// `a.b!.m(x)`.
    NonNull,
    /// `a.b["m"](x)`.
    Computed,
    /// `(0, a.b.m)(x)`.
    Sequence,
    /// `this.field.m(x)`.
    ThisField,
    /// `make().m(x)`.
    CallResult,
    /// A chain with more call results than the extractor walks.
    LongChain,
    /// `this.m(x)` in a class that extends a class from outside the file.
    InheritedMethod,
    /// `this.m(x)` in a class that declares `m` itself.
    OwnMethod,
    /// `this.m(x)` in an arrow callback inside an object literal, in a class
    /// that declares `m`.
    ObjectCallback,
    /// `this.m(x)` in a function on a prototype.
    PrototypeMethod,
    /// `this.m(x)` in a class whose base class the file declares.
    LocalBase,
    /// `this.m(x)` in a method of an options object.
    OptionsObject,
    /// `const aliasFn = eval; aliasFn(x)`.
    CodeExecAlias,
    /// `window.eval(x)`.
    GlobalPrefix,
    /// `const nodeModule = require("node:m"); nodeModule.f(x)`.
    RequireNamespace,
    /// `const { f: aliasFn } = require("node:m"); aliasFn(x)`.
    RequireDestructure,
    /// `const nodeModule = require("node:m"); const { f: aliasFn } = nodeModule;`.
    BindingDestructure,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Case {
    form: Form,
    receiver: Receiver,
    leading_literal: bool,
}

impl Case {
    /// Whether the case must give the candidate of the row. A call of a
    /// member of `this` itself (`this.m(x)`) gives one only for a row that
    /// sets `this_receiver`.
    fn expects_candidate(&self, matcher: &Matcher) -> bool {
        matcher.this_receiver
            || !matches!(
                self.receiver,
                Receiver::InheritedMethod
                    | Receiver::OwnMethod
                    | Receiver::ObjectCallback
                    | Receiver::PrototypeMethod
                    | Receiver::LocalBase
                    | Receiver::OptionsObject
            )
    }
}

/// Code-executing globals that keep their meaning behind an alias or a
/// global-object prefix.
const CODE_EXEC_GLOBALS: &[&str] = &["eval", "Function", "setTimeout", "setInterval"];

/// Call results in a [`Receiver::LongChain`] case, more than the extractor
/// walks one by one.
const LONG_CHAIN_CALLS: usize = 10;

/// One generated argument: statements that run before the call, and the
/// argument text.
#[derive(Debug, Clone)]
struct Argument {
    prelude: String,
    text: String,
}

impl Argument {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            prelude: String::new(),
            text: text.into(),
        }
    }
}

/// The forms that a row accepts: the plain call, the `new` expression, or both.
fn accepted_forms(matcher: &Matcher) -> Vec<Form> {
    let mut forms = Vec::new();
    if matcher.accepts_call_form() {
        forms.push(Form::Call);
    }
    if matcher.accepts_new_form() {
        forms.push(Form::New);
    }
    forms
}

/// Whether a row admits an argument after its first argument position.
fn admits_later_args(matcher: &Matcher) -> bool {
    matcher.admits_arg_index(matcher.arg_index + 1)
}

fn is_call_row(matcher: &Matcher) -> bool {
    matches!(
        matcher.sink_shape,
        SinkShape::Call | SinkShape::MemberCall | SinkShape::NewExpression
    )
}

fn literal_text(value: &LiteralPredicate) -> String {
    match value {
        LiteralPredicate::String(value) => format!("{value:?}"),
        LiteralPredicate::Integer(value) => value.to_string(),
        LiteralPredicate::Boolean(value) => value.to_string(),
        LiteralPredicate::Null => "null".to_string(),
    }
}

/// An object literal that sets each dotted key of `properties`.
fn object_text(properties: &[(&str, String)]) -> String {
    let mut tree: BTreeMap<String, Vec<(&str, String)>> = BTreeMap::new();
    let mut leaves: Vec<(String, String)> = Vec::new();
    for (key, value) in properties {
        match key.split_once('.') {
            Some((head, rest)) => tree
                .entry(head.to_string())
                .or_default()
                .push((rest, value.clone())),
            None => leaves.push(((*key).to_string(), value.clone())),
        }
    }
    let mut parts: Vec<String> = leaves
        .into_iter()
        .map(|(key, value)| format!("{key}: {value}"))
        .collect();
    for (head, nested) in tree {
        parts.push(format!("{head}: {}", object_text(&nested)));
    }
    format!("{{ {} }}", parts.join(", "))
}

/// The generated arguments for a row, the row's own predicates first.
fn argument_candidates(matcher: &Matcher) -> Vec<Argument> {
    let mut out = Vec::new();
    for value in &matcher.literal_values {
        out.push(Argument::plain(format!("{value:?}")));
    }
    for fragment in &matcher.literal_contains {
        out.push(Argument::plain(format!("{fragment:?}")));
        out.push(Argument::plain(format!("{:?}", format!("{fragment}x/y"))));
    }
    for value in &matcher.literal_integers {
        out.push(Argument::plain(value.to_string()));
    }
    if !matcher.object_properties.is_empty() {
        let properties: Vec<(&str, String)> = matcher
            .object_properties
            .iter()
            .map(|predicate| (predicate.key(), literal_text(predicate.value())))
            .collect();
        out.push(Argument::plain(object_text(&properties)));
    }
    if !matcher.object_missing_or_false.is_empty() || !matcher.object_missing.is_empty() {
        out.push(Argument::plain("{}"));
    }
    if let Some(keyword) = matcher.context_keywords.first() {
        out.push(Argument {
            prelude: format!("const {keyword}Value = {SOURCE_ARG};\n"),
            text: format!("{keyword}Value"),
        });
    }
    for text in [
        SOURCE_ARG.to_string(),
        format!("\"a\" + {SOURCE_ARG}"),
        format!("`a${{{SOURCE_ARG}}}`"),
        format!("String({SOURCE_ARG})"),
        format!("[{SOURCE_ARG}]"),
        format!("{{ value: {SOURCE_ARG} }}"),
        "process.env.API_TOKEN".to_string(),
        "\"code()\"".to_string(),
    ] {
        out.push(Argument::plain(text));
    }
    out
}

/// The import line and the callee text of a pattern in the plain form.
///
/// A `node:` pattern binds its first member through a named import, so the
/// extractor resolves the local to the `node:` path. A row with an import
/// provenance imports the root of the pattern from that module, so the
/// provenance gate sees the binding.
fn plain_callee(matcher: &Matcher, pattern: &str) -> (String, String) {
    if let Some(rest) = pattern.strip_prefix("node:") {
        let (module, members) = rest.split_once('.').unwrap_or((rest, ""));
        let (first, tail) = members.split_once('.').unwrap_or((members, ""));
        let import = format!("import {{ {first} as nodeMember }} from \"node:{module}\";\n");
        let callee = if tail.is_empty() {
            "nodeMember".to_string()
        } else {
            format!("nodeMember.{tail}")
        };
        return (import, callee);
    }
    let provenance = matcher.import_provenance.as_deref();
    if let Some(rest) = pattern.strip_prefix("*.") {
        let import = provenance
            .map(|module| format!("import obj from \"{module}\";\n"))
            .unwrap_or_default();
        return (import, format!("obj.{rest}"));
    }
    let import = match (provenance, pattern.split_once('.')) {
        (None, _) => String::new(),
        (Some(module), Some((root, _))) => format!("import * as {root} from \"{module}\";\n"),
        (Some(module), None) => format!("import {{ {pattern} }} from \"{module}\";\n"),
    };
    (import, pattern.to_string())
}

/// The argument list: literal fillers before the row index, the generated
/// argument, and a shell option for rows that read call options.
fn argument_list(matcher: &Matcher, argument: &Argument, leading_literal: bool) -> String {
    let index = usize::try_from(matcher.arg_index).unwrap_or(0);
    let mut args: Vec<String> = vec!["\"f\"".to_string(); index];
    if leading_literal {
        args.push("\"label\"".to_string());
    }
    args.push(argument.text.clone());
    let reads_shell = matcher
        .options_may_enable
        .iter()
        .chain(&matcher.options_enabled)
        .any(|option| option == "shell");
    if reads_shell {
        args.push("{ shell: true }".to_string());
    }
    args.join(", ")
}

/// The callee text of a node pattern behind a `require()` binding, with the
/// statements that bind it.
fn node_require_callee(pattern: &str, receiver: Receiver) -> Option<(String, String)> {
    let rest = pattern.strip_prefix("node:")?;
    let (module, members) = rest.split_once('.')?;
    let (first, tail) = members.split_once('.').unwrap_or((members, ""));
    let tail = if tail.is_empty() {
        String::new()
    } else {
        format!(".{tail}")
    };
    let require = format!("const nodeModule = require(\"node:{module}\");\n");
    Some(match receiver {
        Receiver::RequireNamespace => (require, format!("nodeModule.{members}")),
        Receiver::RequireDestructure => (
            format!("const {{ {first}: aliasFn }} = require(\"node:{module}\");\n"),
            format!("aliasFn{tail}"),
        ),
        _ => (
            format!("{require}const {{ {first}: aliasFn }} = nodeModule;\n"),
            format!("aliasFn{tail}"),
        ),
    })
}

/// `callee` with its last member written in another syntax.
fn rewrite_last_member(callee: &str, receiver: Receiver) -> String {
    let Some((object, member)) = callee.rsplit_once('.') else {
        return callee.to_string();
    };
    match receiver {
        Receiver::Optional => format!("{object}?.{member}"),
        Receiver::NonNull => format!("{object}!.{member}"),
        _ => format!("{object}[\"{member}\"]"),
    }
}

/// The statements before the call and the callee text of a function-body case.
fn function_callee(matcher: &Matcher, pattern: &str, receiver: Receiver) -> (String, String) {
    let (import, plain) = plain_callee(matcher, pattern);
    let rest = pattern.strip_prefix("*.").unwrap_or(pattern);
    match receiver {
        Receiver::Optional | Receiver::NonNull | Receiver::Computed => {
            (import, rewrite_last_member(&plain, receiver))
        }
        Receiver::Sequence => (import, format!("(0, {plain})")),
        Receiver::CallResult => (import, format!("make().{rest}")),
        Receiver::LongChain => {
            let mut chain = String::new();
            for step in 0..LONG_CHAIN_CALLS {
                let _ = write!(chain, ".step{step}()");
            }
            (import, format!("make(){chain}.{rest}"))
        }
        Receiver::CodeExecAlias => (format!("const aliasFn = {pattern};\n"), "aliasFn".into()),
        Receiver::GlobalPrefix => (import, format!("window.{pattern}")),
        Receiver::RequireNamespace
        | Receiver::RequireDestructure
        | Receiver::BindingDestructure => {
            node_require_callee(pattern, receiver).unwrap_or((import, plain))
        }
        _ => (import, plain),
    }
}

/// The source of a case whose callee is a member of `this`, or `None` for
/// another receiver.
fn this_member_source(
    matcher: &Matcher,
    pattern: &str,
    args: &str,
    prelude: &str,
    case: &Case,
) -> Option<String> {
    let rest = pattern.strip_prefix("*.").unwrap_or(pattern);
    let (import, _) = plain_callee(matcher, pattern);
    let new = if case.form == Form::New { "new " } else { "" };
    let own = format!("  {rest}(...values: any[]) {{ return values; }}\n");
    let call = format!("{new}this.{rest}({args})");
    let class = |heritage: &str, member: &str, body: &str| {
        format!(
            "{import}declare const Base: any;\ndeclare const retry: any;\n\
             export class Holder{heritage} {{\n  field: any;\n{member}\
             \x20 run(req: any) {{\n{prelude}    return {body};\n  }}\n}}\n"
        )
    };
    Some(match case.receiver {
        Receiver::ThisField => class("", "", &format!("{new}this.field.{rest}({args})")),
        Receiver::InheritedMethod => class(" extends Base", "", &call),
        Receiver::OwnMethod => class("", &own, &call),
        Receiver::ObjectCallback => class("", &own, &format!("retry({{ task: () => {call} }})")),
        Receiver::LocalBase => format!(
            "{}class LocalBase {{\n{own}}}\n",
            class(" extends LocalBase", "", &call)
        ),
        Receiver::PrototypeMethod => format!(
            "{import}export function Holder() {{}}\n\
             Holder.prototype.run = function (this: any, req: any) {{\n{prelude}  return {call};\n}};\n"
        ),
        Receiver::OptionsObject => format!(
            "{import}export default {{\n  methods: {{\n  {own}  }},\n\
             \x20 run(this: any, req: any) {{\n{prelude}    return {call};\n  }},\n}};\n"
        ),
        _ => return None,
    })
}

fn case_source(matcher: &Matcher, pattern: &str, argument: &Argument, case: &Case) -> String {
    let args = argument_list(matcher, argument, case.leading_literal);
    let prelude = &argument.prelude;
    let new = if case.form == Form::New { "new " } else { "" };
    if let Some(source) = this_member_source(matcher, pattern, &args, prelude, case) {
        return source;
    }
    let (import, callee) = function_callee(matcher, pattern, case.receiver);
    // `new make().m(x)` parses as `(new make()).m(x)`, so a `new` form of a
    // call-result callee gets parentheses.
    let callee = if case.form == Form::New && callee.contains("()") {
        format!("({callee})")
    } else {
        callee
    };
    let obj_param = if import.contains("import obj ") {
        ""
    } else {
        "obj: any, "
    };
    format!(
        "{import}export function run(req: any, {obj_param}make: any) {{\n\
         {prelude}  return {new}{callee}({args});\n}}\n"
    )
}

/// The receiver forms that apply to a pattern of a row.
fn receivers_for(matcher: &Matcher, pattern: &str, form: Form) -> Vec<Receiver> {
    let mut receivers = vec![Receiver::Plain, Receiver::Sequence];
    let is_node = pattern.starts_with("node:");
    let (_, plain) = plain_callee(matcher, pattern);
    if plain.contains('.') {
        receivers.extend([Receiver::NonNull, Receiver::Computed]);
        // An optional chain cannot be the callee of `new`.
        if form == Form::Call {
            receivers.push(Receiver::Optional);
        }
    }
    if CODE_EXEC_GLOBALS.contains(&pattern) {
        receivers.extend([Receiver::CodeExecAlias, Receiver::GlobalPrefix]);
    }
    if is_node && pattern.matches('.').count() >= 1 && !matcher.requires_binding_trace() {
        receivers.extend([
            Receiver::RequireNamespace,
            Receiver::RequireDestructure,
            Receiver::BindingDestructure,
        ]);
    }
    // A binding-trace row needs the import binding as the receiver root, so
    // another receiver is not an equivalent form of that row.
    if let Some(rest) = pattern.strip_prefix("*.")
        && !matcher.requires_binding_trace()
    {
        receivers.extend([
            Receiver::ThisField,
            Receiver::CallResult,
            Receiver::LongChain,
        ]);
        if !rest.contains('.') && form == Form::Call {
            receivers.extend([
                Receiver::InheritedMethod,
                Receiver::OwnMethod,
                Receiver::ObjectCallback,
                Receiver::PrototypeMethod,
                Receiver::LocalBase,
                Receiver::OptionsObject,
            ]);
        }
    }
    receivers
}

/// The cases of a pattern beyond the plain baseline.
fn equivalent_cases(matcher: &Matcher, pattern: &str) -> Vec<Case> {
    let mut cases = Vec::new();
    for form in accepted_forms(matcher) {
        for receiver in receivers_for(matcher, pattern, form) {
            cases.push(Case {
                form,
                receiver,
                leading_literal: false,
            });
        }
        if admits_later_args(matcher) {
            cases.push(Case {
                form,
                receiver: Receiver::Plain,
                leading_literal: true,
            });
        }
    }
    cases
}

fn write_file(root: &Path, relative: &str, source: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create fixture dir");
    }
    std::fs::write(path, source).expect("write fixture file");
}

/// The dependency names that satisfy each row enabler. A prefix enabler
/// (`@fastify/`) gets one package under the prefix.
fn enabler_dependencies() -> BTreeSet<String> {
    catalogue()
        .matchers()
        .iter()
        .filter_map(|matcher| matcher.enabler.clone())
        .map(|enabler| {
            if enabler.ends_with('/') {
                format!("{enabler}static")
            } else {
                enabler
            }
        })
        .collect()
}

/// Write one module per case and return the candidates of each module.
/// The category and the evidence text of each candidate, keyed by module path.
type Findings = BTreeMap<String, Vec<(String, String)>>;

fn analyze_cases(files: &[(String, String)]) -> Findings {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    let dependencies: Vec<String> = enabler_dependencies()
        .into_iter()
        .map(|name| format!("{name:?}: \"1.0.0\""))
        .collect();
    write_file(
        root,
        "package.json",
        &format!(
            "{{\"name\":\"shape-matrix\",\"private\":true,\"main\":\"src/index.ts\",\
             \"dependencies\":{{{}}}}}",
            dependencies.join(",")
        ),
    );
    let mut index = String::new();
    for (relative, source) in files {
        write_file(root, relative, source);
        let module = relative
            .strip_prefix("src/")
            .and_then(|name| name.strip_suffix(".ts"))
            .expect("generated module path");
        let _ = writeln!(index, "import \"./{module}\";");
    }
    write_file(root, "src/index.ts", &index);

    let mut config = create_config_with_rules(root.to_path_buf(), |rules| {
        rules.security_sink = Severity::Warn;
    });
    let mut categories: Vec<String> = catalogue()
        .matchers()
        .iter()
        .map(|matcher| matcher.id.clone())
        .collect();
    categories.dedup();
    config.security.categories = Some(SecurityCategories {
        include: Some(categories),
        exclude: None,
    });
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut out: Findings = BTreeMap::new();
    for finding in &results.security_findings {
        let Some(category) = &finding.category else {
            continue;
        };
        let path = finding.path.to_string_lossy().replace('\\', "/");
        let Some(start) = path.rfind("src/") else {
            continue;
        };
        out.entry(path[start..].to_string())
            .or_default()
            .push((category.clone(), finding.evidence.clone()));
    }
    out
}

/// Whether `evidence` comes from `template`: each literal part of the
/// template, between its `{placeholder}` parts, appears in order.
fn evidence_matches(template: &str, evidence: &str) -> bool {
    let mut rest = evidence;
    let mut text = template;
    while !text.is_empty() {
        let (literal, tail) = match text.find('{') {
            Some(open) => {
                let close = text[open..]
                    .find('}')
                    .map_or(text.len(), |end| open + end + 1);
                (&text[..open], &text[close..])
            }
            None => (text, ""),
        };
        if !literal.is_empty() {
            let Some(at) = rest.find(literal) else {
                return false;
            };
            rest = &rest[at + literal.len()..];
        }
        text = tail;
    }
    true
}

/// Whether a module got a candidate of this row: the row category with the
/// evidence text of the row.
fn has_row_candidate(findings: &Findings, relative: &str, matcher: &Matcher) -> bool {
    findings.get(relative).is_some_and(|found| {
        found.iter().any(|(category, evidence)| {
            *category == matcher.id && evidence_matches(&matcher.evidence_template, evidence)
        })
    })
}

/// Every call row of the catalogue, with its index in the catalogue.
fn call_rows() -> Vec<(usize, &'static Matcher)> {
    catalogue()
        .matchers()
        .iter()
        .enumerate()
        .filter(|(_, matcher)| is_call_row(matcher))
        .collect()
}

#[test]
fn every_call_row_reports_each_equivalent_form() {
    let rows = call_rows();

    // Phase 1: the plain form of every generated argument.
    let mut plain_files = Vec::new();
    for (row, matcher) in &rows {
        let Some(form) = accepted_forms(matcher).first().copied() else {
            continue;
        };
        let case = Case {
            form,
            receiver: Receiver::Plain,
            leading_literal: false,
        };
        for (pattern_index, pattern) in matcher.callee_patterns.iter().enumerate() {
            for (arg_index, argument) in argument_candidates(matcher).iter().enumerate() {
                plain_files.push((
                    format!("src/r{row}_p{pattern_index}_a{arg_index}.ts"),
                    case_source(matcher, pattern.raw(), argument, &case),
                ));
            }
        }
    }
    let plain = analyze_cases(&plain_files);

    // Each pattern takes the first argument that gives a plain candidate.
    let mut baselines: Vec<(usize, &Matcher, usize, Argument)> = Vec::new();
    let mut patterns_without_baseline: BTreeSet<String> = BTreeSet::new();
    for (row, matcher) in &rows {
        let candidates = argument_candidates(matcher);
        for (pattern_index, pattern) in matcher.callee_patterns.iter().enumerate() {
            let found = candidates.iter().enumerate().find(|(arg_index, _)| {
                has_row_candidate(
                    &plain,
                    &format!("src/r{row}_p{pattern_index}_a{arg_index}.ts"),
                    matcher,
                )
            });
            match found {
                Some((_, argument)) => {
                    baselines.push((*row, matcher, pattern_index, argument.clone()));
                }
                None => {
                    patterns_without_baseline.insert(format!("{}:{}", matcher.id, pattern.raw()));
                }
            }
        }
    }
    let expected_without_baseline: BTreeSet<String> = PATTERNS_WITHOUT_BASELINE
        .iter()
        .map(|(key, _)| (*key).to_string())
        .collect();
    assert_eq!(
        patterns_without_baseline, expected_without_baseline,
        "every call pattern needs a generated baseline, or an entry in PATTERNS_WITHOUT_BASELINE"
    );

    // Phase 2: every equivalent form of each baseline.
    let mut form_files = Vec::new();
    let mut expectations = Vec::new();
    for (row, matcher, pattern_index, argument) in &baselines {
        let pattern = matcher.callee_patterns[*pattern_index].raw();
        for (case_index, case) in equivalent_cases(matcher, pattern).iter().enumerate() {
            let relative = format!("src/f{row}_p{pattern_index}_c{case_index}.ts");
            let source = case_source(matcher, pattern, argument, case);
            expectations.push((
                relative.clone(),
                *matcher,
                source.clone(),
                case.expects_candidate(matcher),
            ));
            form_files.push((relative, source));
        }
    }
    let forms = analyze_cases(&form_files);
    let missing: Vec<String> = expectations
        .iter()
        .filter(|(relative, matcher, _, expected)| {
            has_row_candidate(&forms, relative, matcher) != *expected
        })
        .map(|(_, matcher, source, expected)| {
            let want = if *expected {
                "a candidate"
            } else {
                "no candidate"
            };
            format!("{} (want {want}):\n{source}", matcher.id)
        })
        .collect();
    assert!(
        missing.is_empty(),
        "{} equivalent forms gave the wrong result:\n\n{}",
        missing.len(),
        missing.join("\n")
    );
}

/// Analyze one module with the given dependencies and return its categories.
fn analyze_module(dependencies: &str, source: &str) -> BTreeSet<String> {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_file(
        root,
        "package.json",
        &format!(
            "{{\"name\":\"t\",\"private\":true,\"main\":\"src/index.ts\",\
             \"dependencies\":{{{dependencies}}}}}"
        ),
    );
    write_file(root, "src/index.ts", source);
    let config = create_config_with_rules(root.to_path_buf(), |rules| {
        rules.security_sink = Severity::Warn;
    });
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .security_findings
        .iter()
        .filter_map(|finding| finding.category.clone())
        .collect()
}

#[test]
fn handlebars_safe_string_constructor_is_a_candidate() {
    let categories = analyze_module(
        "\"handlebars\": \"^4.0.0\"",
        "import Handlebars from \"handlebars\";\n\
         export const a = (bio: string) => new Handlebars.SafeString(bio);\n",
    );
    assert!(
        categories.contains("template-escape-bypass"),
        "{categories:?}"
    );
}

#[test]
fn angular_sanitizer_on_a_class_field_is_a_candidate() {
    let categories = analyze_module(
        "\"@angular/platform-browser\": \"^17.0.0\"",
        "import { DomSanitizer } from \"@angular/platform-browser\";\n\
         export class Preview {\n\
           constructor(private sanitizer: DomSanitizer) {}\n\
           show(input: string) { return this.sanitizer.bypassSecurityTrustHtml(input); }\n\
         }\n",
    );
    assert!(
        categories.contains("angular-trusted-html"),
        "{categories:?}"
    );
}

#[test]
fn jquery_html_on_a_selector_result_is_a_candidate() {
    let categories = analyze_module(
        "\"jquery\": \"^3.0.0\"",
        "import $ from \"jquery\";\n\
         export const show = (body: string) => $(\"#preview\").html(body);\n",
    );
    assert!(categories.contains("jquery-html"), "{categories:?}");
}

#[test]
fn log_call_with_a_leading_label_is_a_candidate() {
    let categories = analyze_module(
        "\"express\": \"^4.0.0\"",
        "export const login = (req: any) => { console.log(\"login\", req.body.password); };\n",
    );
    assert!(categories.contains("secret-pii-log"), "{categories:?}");
}

#[test]
fn log_call_with_only_literal_arguments_is_not_a_candidate() {
    let categories = analyze_module(
        "\"express\": \"^4.0.0\"",
        "export const login = () => { console.log(\"login\", \"done\"); };\n",
    );
    assert!(!categories.contains("secret-pii-log"), "{categories:?}");
}

#[test]
fn call_of_an_own_class_method_is_not_a_candidate() {
    let categories = analyze_module(
        "\"express\": \"^4.0.0\"",
        "export class Repo {\n\
           query(text: string) { return text; }\n\
           run(req: any) { return this.query(\"select \" + req.body.q); }\n\
         }\n",
    );
    assert!(!categories.contains("sql-injection"), "{categories:?}");
}

#[test]
fn class_field_receiver_of_a_query_is_a_candidate() {
    let categories = analyze_module(
        "\"express\": \"^4.0.0\"",
        "export class Repo {\n\
           constructor(private readonly db: any) {}\n\
           run(req: any) { return this.db.query(\"select \" + req.body.q); }\n\
         }\n",
    );
    assert!(categories.contains("sql-injection"), "{categories:?}");
}

#[test]
fn constructor_form_of_a_call_only_row_is_not_a_candidate() {
    let categories = analyze_module(
        "\"jquery\": \"^3.0.0\"",
        "import $ from \"jquery\";\n\
         export const show = (el: any, body: string) => new el.html(body);\n",
    );
    assert!(!categories.contains("jquery-html"), "{categories:?}");
}

#[test]
fn leading_literal_does_not_move_an_index_row() {
    let categories = analyze_module(
        "\"jquery\": \"^3.0.0\"",
        "import $ from \"jquery\";\n\
         export const show = (body: string) => $(\"#p\").html(\"label\", body);\n",
    );
    assert!(!categories.contains("jquery-html"), "{categories:?}");
}

#[test]
fn inherited_raw_query_of_a_client_subclass_is_a_candidate() {
    let categories = analyze_module(
        "\"@prisma/client\": \"^5.0.0\"",
        "import { PrismaClient } from \"@prisma/client\";\n\
         export class Db extends PrismaClient {\n\
           run(req: any) { return this.$queryRawUnsafe(\"select \" + req.body.q); }\n\
         }\n",
    );
    assert!(categories.contains("sql-injection"), "{categories:?}");
}

#[test]
fn inherited_markup_method_of_a_custom_element_is_a_candidate() {
    let categories = analyze_module(
        "",
        "export class Widget extends HTMLElement {\n\
           show(req: any) { this.insertAdjacentHTML(\"beforeend\", req.body.html); }\n\
         }\n",
    );
    assert!(categories.contains("dangerous-html"), "{categories:?}");
}

#[test]
fn own_method_of_an_object_literal_is_not_a_candidate() {
    let categories = analyze_module(
        "",
        "export const repo = {\n\
           query(text: string) { return text; },\n\
           run(req: any) { return this.query(\"select \" + req.body.q); },\n\
         };\n",
    );
    assert!(!categories.contains("sql-injection"), "{categories:?}");
}

#[test]
fn raw_sql_at_the_end_of_a_long_builder_chain_is_a_candidate() {
    let categories = analyze_module(
        "\"knex\": \"^3.0.0\"",
        "import knex from \"knex\";\n\
         const db = knex({});\n\
         export const q = (req: any) =>\n\
           db(\"users\").select(\"*\").where(\"a\", 1).where(\"b\", 2).where(\"c\", 3)\n\
             .where(\"d\", 4).where(\"e\", 5).where(\"f\", 6).where(\"g\", 7).whereRaw(req.body.q);\n",
    );
    assert!(categories.contains("sql-injection"), "{categories:?}");
}

#[test]
fn markup_write_at_the_end_of_a_long_chain_is_a_candidate() {
    let categories = analyze_module(
        "",
        "export const w = (req: any, a: any) => {\n\
           a.b().c().d().e().f().g().h().i().j().innerHTML = req.body.x;\n\
         };\n",
    );
    assert!(categories.contains("dangerous-html"), "{categories:?}");
}

#[test]
fn destructured_member_of_a_required_module_binding_is_a_candidate() {
    let categories = analyze_module(
        "",
        "const cp = require(\"node:child_process\");\n\
         const { exec } = cp;\n\
         export const run = (req: any) => exec(req.body.cmd);\n",
    );
    assert!(categories.contains("command-injection"), "{categories:?}");
}

#[test]
fn own_method_in_an_object_literal_callback_is_not_a_candidate() {
    let categories = analyze_module(
        "",
        "declare const retry: any;\n\
         export class Repo {\n\
           query(text: string) { return text; }\n\
           run(req: any) {\n\
             return retry({ attempts: 3, task: () => this.query(\"select \" + req.body.q) });\n\
           }\n\
         }\n",
    );
    assert!(!categories.contains("sql-injection"), "{categories:?}");
}

#[test]
fn own_method_beside_an_arrow_property_is_not_a_candidate() {
    let categories = analyze_module(
        "",
        "export class Repo {\n\
           query(text: string) { return text; }\n\
           run(req: any) { return { cb: () => {}, v: this.query(\"select \" + req.body.q) }; }\n\
         }\n",
    );
    assert!(!categories.contains("sql-injection"), "{categories:?}");
}

#[test]
fn own_method_in_a_class_field_object_is_not_a_candidate() {
    let categories = analyze_module(
        "",
        "export class Repo {\n\
           query(text: string) { return text; }\n\
           handlers = { onQuery: (req: any) => this.query(\"select \" + req.body.q) };\n\
         }\n",
    );
    assert!(!categories.contains("sql-injection"), "{categories:?}");
}

#[test]
fn this_call_in_a_prototype_function_is_not_a_candidate() {
    let categories = analyze_module(
        "",
        "export function Repo() {}\n\
         Repo.prototype.run = function (req: any) {\n\
           return this.query(\"select \" + req.body.q);\n\
         };\n",
    );
    assert!(!categories.contains("sql-injection"), "{categories:?}");
}

#[test]
fn method_of_a_local_base_class_is_not_a_candidate() {
    for source in [
        "class BaseRepo { query(text: string) { return text; } }\n\
         export class UserRepo extends BaseRepo {\n\
           run(req: any) { return this.query(\"select \" + req.body.q); }\n\
         }\n",
        "export class UserRepo extends BaseRepo {\n\
           run(req: any) { return this.query(\"select \" + req.body.q); }\n\
         }\n\
         class BaseRepo { query(text: string) { return text; } }\n",
    ] {
        let categories = analyze_module("", source);
        assert!(
            !categories.contains("sql-injection"),
            "{categories:?}\n{source}"
        );
    }
}

#[test]
fn method_of_a_vue_options_object_is_not_a_candidate() {
    let categories = analyze_module(
        "\"vue\": \"^3.0.0\"",
        "export default {\n\
           methods: { query(text: string) { return text; } },\n\
           mounted(this: any, req: any) { return this.query(\"select \" + req.body.q); },\n\
         };\n",
    );
    assert!(!categories.contains("sql-injection"), "{categories:?}");
}

/// A call of a member of `this` itself matches only the rows that set
/// `this_receiver`, whatever the class extends.
#[test]
fn this_query_matches_no_broad_row_in_any_class_shape() {
    for source in [
        "import { BaseRepo } from \"./base\";\n\
         export class Repo extends BaseRepo {\n\
           run(req: any) { return this.query(\"select \" + req.body.q); }\n\
         }\n",
        "import React from \"react\";\n\
         export class Repo extends React.Component {\n\
           run(req: any) { return this.query(\"select \" + req.body.q); }\n\
         }\n",
        "declare const mixin: any;\n\
         class LocalBase {}\n\
         export class Repo extends mixin(LocalBase) {\n\
           run(req: any) { return this.query(\"select \" + req.body.q); }\n\
         }\n",
        "export class Repo {\n\
           constructor(private query: (text: string) => unknown) {}\n\
           run(req: any) { return this.query(\"select \" + req.body.q); }\n\
         }\n",
        "interface Repo { query(text: string): unknown }\n\
         export const run = function (this: Repo, req: any) {\n\
           return this.query(\"select \" + req.body.q);\n\
         };\n",
    ] {
        let categories = analyze_module("", source);
        assert!(
            !categories.contains("sql-injection"),
            "{categories:?}\n{source}"
        );
    }
}

/// A library method name that a row allows on `this` gives a candidate in
/// every class shape.
#[test]
fn this_raw_query_is_a_candidate_in_every_class_shape() {
    for source in [
        "import { PrismaClient } from \"@prisma/client\";\n\
         class Base extends PrismaClient {}\n\
         export class Db extends Base {\n\
           run(req: any) { return this.$queryRawUnsafe(\"select \" + req.body.q); }\n\
         }\n",
        "import { PrismaClient } from \"@prisma/client\";\n\
         export class Db extends PrismaClient {\n\
           run(req: any) { return this.$queryRawUnsafe(\"select \" + req.body.q); }\n\
         }\n\
         export function local() { class PrismaClient {} return PrismaClient; }\n",
        "declare class Client { $queryRawUnsafe(text: string): unknown }\n\
         export class Db extends Client {\n\
           run(req: any) { return this.$queryRawUnsafe(\"select \" + req.body.q); }\n\
         }\n",
    ] {
        let categories = analyze_module("\"@prisma/client\": \"^5.0.0\"", source);
        assert!(
            categories.contains("sql-injection"),
            "{categories:?}\n{source}"
        );
    }
}
