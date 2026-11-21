//! The call forms and the argument positions that a call row accepts.

use fallow_types::extract::SinkShape;

use crate::{LiteralPredicate, Matcher, ObjectPropertyPredicate, RawMatcher};

/// The call forms that a call row accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallForms {
    /// A plain call: `f(x)` or `a.f(x)`.
    pub call: bool,
    /// A `new` expression: `new F(x)` or `new a.F(x)`.
    pub new: bool,
}

/// The argument positions that a row reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgumentSelector {
    /// Only `arg_index`.
    Index,
    /// `arg_index` and every later position.
    Variadic,
    /// Every non-literal argument, at any position.
    AnyNonLiteral,
}

impl ObjectPropertyPredicate {
    /// The property key. A dotted key (`webPreferences.nodeIntegration`)
    /// names a nested property.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The literal value that the property must hold.
    #[must_use]
    pub fn value(&self) -> &LiteralPredicate {
        &self.value
    }
}

impl Matcher {
    /// Whether the row accepts a plain call (`f(x)`, `a.f(x)`).
    #[must_use]
    pub fn accepts_call_form(&self) -> bool {
        self.forms.call
    }

    /// Whether the row accepts a `new` expression (`new F(x)`).
    #[must_use]
    pub fn accepts_new_form(&self) -> bool {
        self.forms.new
    }

    /// Whether the row admits a sink site of the given shape. A call row
    /// admits a call or a `new` expression as its `forms` say. The callee
    /// pattern alone tells a bare call from a member call. Each other shape
    /// admits only itself.
    #[must_use]
    pub fn admits_sink_shape(&self, shape: SinkShape) -> bool {
        if !is_call_shape(self.sink_shape) {
            return shape == self.sink_shape;
        }
        match shape {
            SinkShape::Call | SinkShape::MemberCall => self.forms.call,
            SinkShape::NewExpression => self.forms.new,
            SinkShape::MemberAssign
            | SinkShape::TaggedTemplate
            | SinkShape::JsxAttr
            | SinkShape::SecretLiteral => false,
        }
    }

    /// Whether a captured argument position is admitted by this matcher.
    #[must_use]
    pub fn admits_arg_index(&self, arg_index: u32) -> bool {
        match self.arguments {
            ArgumentSelector::Index => arg_index == self.arg_index,
            ArgumentSelector::Variadic => arg_index >= self.arg_index,
            ArgumentSelector::AnyNonLiteral => true,
        }
    }

    /// Whether the row can admit more than one argument of one call.
    #[must_use]
    pub fn reads_several_arguments(&self) -> bool {
        self.arguments != ArgumentSelector::Index
    }

    /// Whether the row matches the callee path of a sink site of `shape`. A
    /// call of a member of `this` itself (`this.query(x)`) runs code of the
    /// class or of its base in most cases, so it matches only a call row that
    /// sets `this_receiver`. A member of a field (`this.db.query(x)`) and a
    /// member assignment (`this.innerHTML = x`) match as any other path.
    #[must_use]
    pub fn admits_callee(&self, shape: SinkShape, callee_path: &str) -> bool {
        if is_call_shape(shape) && is_this_member_path(callee_path) && !self.this_receiver {
            return false;
        }
        self.first_matching_pattern(callee_path).is_some()
    }
}

/// Whether a callee path is a member of `this` itself (`this.query`).
#[must_use]
pub fn is_this_member_path(callee_path: &str) -> bool {
    callee_path
        .strip_prefix("this.")
        .is_some_and(|member| !member.is_empty() && !member.contains('.'))
}

/// Whether a sink shape is one of the call shapes.
pub fn is_call_shape(shape: SinkShape) -> bool {
    matches!(
        shape,
        SinkShape::Call | SinkShape::MemberCall | SinkShape::NewExpression
    )
}

/// Resolve the `forms` of a row. A call row without `forms` accepts the form
/// of its `sink_shape`. Only a call row can set `forms`.
pub fn parse_call_forms(
    id: &str,
    sink_shape: SinkShape,
    raw: Option<&[String]>,
) -> Result<CallForms, String> {
    let Some(raw) = raw else {
        return Ok(CallForms {
            call: matches!(sink_shape, SinkShape::Call | SinkShape::MemberCall),
            new: sink_shape == SinkShape::NewExpression,
        });
    };
    if !is_call_shape(sink_shape) {
        return Err(format!(
            "matcher {id:?} sets forms, but only a call or new-expression row accepts forms"
        ));
    }
    if raw.is_empty() {
        return Err(format!("matcher {id:?} has an empty forms list"));
    }
    let mut forms = CallForms {
        call: false,
        new: false,
    };
    for form in raw {
        let slot = match form.as_str() {
            "call" => &mut forms.call,
            "new" => &mut forms.new,
            other => {
                return Err(format!(
                    "matcher {id:?} has unknown form {other:?}; expected call | new"
                ));
            }
        };
        if std::mem::replace(slot, true) {
            return Err(format!("matcher {id:?} lists form {form:?} twice"));
        }
    }
    Ok(forms)
}

/// Resolve the `arguments` of a row. Only a call row can read several
/// arguments.
pub fn parse_argument_selector(
    entry: &RawMatcher,
    sink_shape: SinkShape,
) -> Result<ArgumentSelector, String> {
    let Some(raw) = entry.arguments.as_deref() else {
        return Ok(ArgumentSelector::Index);
    };
    let selector = match raw {
        "index" => ArgumentSelector::Index,
        "variadic" => ArgumentSelector::Variadic,
        "any-non-literal" => ArgumentSelector::AnyNonLiteral,
        other => {
            return Err(format!(
                "matcher {:?} has unknown arguments {other:?}; expected index | variadic | any-non-literal",
                entry.id
            ));
        }
    };
    if selector != ArgumentSelector::Index && !is_call_shape(sink_shape) {
        return Err(format!(
            "matcher {:?} sets arguments {raw:?}, but only a call row reads several arguments",
            entry.id
        ));
    }
    if selector == ArgumentSelector::AnyNonLiteral && entry.arg_index != 0 {
        return Err(format!(
            "matcher {:?} uses any-non-literal, which needs arg_index 0",
            entry.id
        ));
    }
    Ok(selector)
}

/// Check the call-form fields of a parsed row.
///
/// `any-non-literal` reads every non-literal argument, so a row with a
/// literal or context predicate cannot use it. `this_receiver` needs a call
/// row with a `*.m` pattern, the only pattern that a `this.m` path can match.
pub fn check_matcher(matcher: Matcher) -> Result<Matcher, String> {
    if matcher.arguments == ArgumentSelector::AnyNonLiteral && matcher.is_literal_aware() {
        return Err(format!(
            "matcher {:?} uses any-non-literal, which needs no literal predicate",
            matcher.id
        ));
    }
    if matcher.this_receiver
        && (!is_call_shape(matcher.sink_shape)
            || !matcher
                .callee_patterns
                .iter()
                .any(|pattern| single_member_wildcard(pattern.raw())))
    {
        return Err(format!(
            "matcher {:?} sets this_receiver, which needs a call row with a `*.m` pattern",
            matcher.id
        ));
    }
    Ok(matcher)
}

/// Whether a raw pattern is `*.m`, a wildcard receiver and one member.
fn single_member_wildcard(raw: &str) -> bool {
    raw.strip_prefix("*.")
        .is_some_and(|member| !member.is_empty() && !member.contains('.'))
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "catalogue parser tests assert fixture invariants directly"
)]
mod tests {
    use super::*;
    use crate::{catalogue, parse_catalogue};

    fn call_row(extra: &str) -> String {
        format!(
            r#"
[[matcher]]
id = "x"
cwe = 79
title = "x"
effect = "unknown"
sink_shape = "member-call"
callee_patterns = ["*.m"]
arg_index = 0
evidence_template = "x"
{extra}
"#
        )
    }

    #[test]
    fn call_row_forms_default_to_the_sink_shape() {
        let cat = parse_catalogue(&call_row("")).unwrap();
        let row = &cat.matchers()[0];
        assert!(row.admits_sink_shape(SinkShape::MemberCall));
        assert!(row.admits_sink_shape(SinkShape::Call));
        assert!(!row.admits_sink_shape(SinkShape::NewExpression));
        assert!(!row.admits_sink_shape(SinkShape::MemberAssign));
    }

    #[test]
    fn call_row_with_both_forms_admits_a_new_expression() {
        let cat = parse_catalogue(&call_row(r#"forms = ["call", "new"]"#)).unwrap();
        let row = &cat.matchers()[0];
        assert!(row.admits_sink_shape(SinkShape::MemberCall));
        assert!(row.admits_sink_shape(SinkShape::NewExpression));
    }

    #[test]
    fn template_escape_bypass_accepts_the_constructor_form() {
        let row = catalogue()
            .matchers()
            .iter()
            .find(|m| m.id == "template-escape-bypass")
            .unwrap();
        assert!(row.accepts_call_form() && row.accepts_new_form());
    }

    #[test]
    fn parse_rejects_bad_forms() {
        for (forms, message) in [
            ("forms = []", "empty forms"),
            (r#"forms = ["call", "call"]"#, "twice"),
            (r#"forms = ["apply"]"#, "unknown form"),
        ] {
            let err = parse_catalogue(&call_row(forms)).unwrap_err();
            assert!(err.contains(message), "{forms}: got {err}");
        }
        let assign = call_row(r#"forms = ["call"]"#).replace("member-call", "member-assign");
        let err = parse_catalogue(&assign).unwrap_err();
        assert!(
            err.contains("only a call or new-expression row"),
            "got: {err}"
        );
    }

    #[test]
    fn argument_selectors_admit_their_positions() {
        let index = parse_catalogue(&call_row("")).unwrap();
        let variadic = parse_catalogue(&call_row(r#"arguments = "variadic""#)).unwrap();
        let any = parse_catalogue(&call_row(r#"arguments = "any-non-literal""#)).unwrap();
        let (index, variadic, any) = (
            &index.matchers()[0],
            &variadic.matchers()[0],
            &any.matchers()[0],
        );
        assert!(index.admits_arg_index(0) && !index.admits_arg_index(1));
        assert!(!index.reads_several_arguments());
        assert!(variadic.admits_arg_index(0) && variadic.admits_arg_index(3));
        assert!(any.admits_arg_index(0) && any.admits_arg_index(2));
        assert!(any.reads_several_arguments());
    }

    #[test]
    fn parse_rejects_bad_argument_selectors() {
        let err = parse_catalogue(&call_row(r#"arguments = "all""#)).unwrap_err();
        assert!(err.contains("unknown arguments"), "got: {err}");
        for predicate in [
            "literal_values = [\"*\"]",
            "context_keywords = [\"token\"]",
            "arg_kinds = [\"literal\"]",
        ] {
            let row = call_row(&format!("arguments = \"any-non-literal\"\n{predicate}"));
            let err = parse_catalogue(&row).unwrap_err();
            assert!(
                err.contains("no literal predicate"),
                "{predicate}: got {err}"
            );
        }
        let late =
            call_row(r#"arguments = "any-non-literal""#).replace("arg_index = 0", "arg_index = 1");
        let err = parse_catalogue(&late).unwrap_err();
        assert!(err.contains("arg_index 0"), "got: {err}");
    }

    #[test]
    fn log_rows_read_every_non_literal_argument() {
        let row = catalogue()
            .matchers()
            .iter()
            .find(|m| m.id == "secret-pii-log")
            .unwrap();
        assert_eq!(row.arguments, ArgumentSelector::AnyNonLiteral);
    }

    /// The rows that set `this_receiver` name exactly the library methods
    /// that a `this` root reached before the canonical call: the markup
    /// parsers and the raw SQL methods.
    #[test]
    fn this_receiver_rows_name_the_library_methods() {
        let names: std::collections::BTreeSet<&str> = catalogue()
            .matchers()
            .iter()
            .filter(|m| m.this_receiver)
            .flat_map(|m| m.callee_patterns.iter())
            .filter_map(|pattern| pattern.raw().strip_prefix("*."))
            .filter(|member| !member.contains('.'))
            .collect();
        let expected = std::collections::BTreeSet::from([
            "createContextualFragment",
            "insertAdjacentHTML",
            "whereRaw",
            "havingRaw",
            "orderByRaw",
            "$queryRawUnsafe",
            "$executeRawUnsafe",
        ]);
        assert_eq!(names, expected);
    }

    #[test]
    fn this_member_path_needs_the_row_flag() {
        let plain = parse_catalogue(&call_row("")).unwrap();
        let flagged = parse_catalogue(&call_row("this_receiver = true")).unwrap();
        let (plain, flagged) = (&plain.matchers()[0], &flagged.matchers()[0]);
        assert!(!plain.admits_callee(SinkShape::MemberCall, "this.m"));
        assert!(flagged.admits_callee(SinkShape::MemberCall, "this.m"));
        assert!(plain.admits_callee(SinkShape::MemberCall, "this.db.m"));
        assert!(plain.admits_callee(SinkShape::MemberCall, "db.m"));
        assert!(!plain.admits_callee(SinkShape::NewExpression, "this.m"));
    }

    #[test]
    fn parse_rejects_this_receiver_without_a_member_pattern() {
        let exact = call_row("this_receiver = true").replace("\"*.m\"", "\"db.m\"");
        let err = parse_catalogue(&exact).unwrap_err();
        assert!(err.contains("this_receiver"), "got: {err}");
        let assign = call_row("this_receiver = true").replace("member-call", "member-assign");
        let err = parse_catalogue(&assign).unwrap_err();
        assert!(err.contains("this_receiver"), "got: {err}");
    }
}
