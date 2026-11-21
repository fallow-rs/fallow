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
//! - a `this.field` receiver, a local alias of that receiver, and a
//!   call-result receiver, for a pattern with a leading `*.`;
//! - an extra leading literal argument, for a row that admits more than one
//!   argument position.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Receiver {
    Plain,
    ThisField,
    Alias,
    CallResult,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Case {
    form: Form,
    receiver: Receiver,
    leading_literal: bool,
}

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

fn case_source(matcher: &Matcher, pattern: &str, argument: &Argument, case: &Case) -> String {
    let (import, plain) = plain_callee(matcher, pattern);
    let rest = pattern.strip_prefix("*.").unwrap_or(pattern);
    let callee = match case.receiver {
        Receiver::Plain => plain,
        Receiver::ThisField => format!("this.field.{rest}"),
        Receiver::Alias => format!("alias.{rest}"),
        // `new make().m(x)` parses as `(new make()).m(x)`, so the `new` form
        // wraps the callee in parentheses.
        Receiver::CallResult if case.form == Form::New => format!("(make().{rest})"),
        Receiver::CallResult => format!("make().{rest}"),
    };
    let new = if case.form == Form::New { "new " } else { "" };
    let args = argument_list(matcher, argument, case.leading_literal);
    let prelude = &argument.prelude;
    let obj_param = if import.contains("import obj ") {
        ""
    } else {
        "obj: any, "
    };
    match case.receiver {
        Receiver::Plain | Receiver::CallResult => format!(
            "{import}export function run(req: any, {obj_param}make: any) {{\n\
             {prelude}  return {new}{callee}({args});\n}}\n"
        ),
        Receiver::ThisField | Receiver::Alias => {
            let alias = if case.receiver == Receiver::Alias {
                "  const alias = this.field;\n"
            } else {
                ""
            };
            format!(
                "{import}export class Holder {{\n  field: any;\n  run(req: any) {{\n\
                 {prelude}{alias}    return {new}{callee}({args});\n  }}\n}}\n"
            )
        }
    }
}

/// The cases of a pattern beyond the plain baseline.
fn equivalent_cases(matcher: &Matcher, pattern: &str) -> Vec<Case> {
    // A binding-trace row needs the import binding as the receiver root, so
    // another receiver is not an equivalent form of that row.
    let receivers: &[Receiver] = if pattern.starts_with("*.") && !matcher.requires_binding_trace() {
        &[
            Receiver::Plain,
            Receiver::ThisField,
            Receiver::Alias,
            Receiver::CallResult,
        ]
    } else {
        &[Receiver::Plain]
    };
    let mut cases = Vec::new();
    for form in accepted_forms(matcher) {
        for receiver in receivers {
            cases.push(Case {
                form,
                receiver: *receiver,
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
            expectations.push((relative.clone(), *matcher, source.clone()));
            form_files.push((relative, source));
        }
    }
    let forms = analyze_cases(&form_files);
    let missing: Vec<String> = expectations
        .iter()
        .filter(|(relative, matcher, _)| !has_row_candidate(&forms, relative, matcher))
        .map(|(_, matcher, source)| format!("{}:\n{source}", matcher.id))
        .collect();
    assert!(
        missing.is_empty(),
        "{} equivalent forms gave no candidate:\n\n{}",
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
