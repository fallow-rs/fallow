//! Database write calls that the side-effect prune treats as sinks in a
//! module with database reach.

use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Expression, TaggedTemplateExpression,
    VariableDeclarator,
};
use oxc_ast_visit::Visit;
use rustc_hash::FxHashSet;

use super::layout::Layout;

/// Method names of ORM and query builder calls that write to a database:
/// `db.insert(table)`, `db.insertInto("person")`, `prisma.user.create(...)`.
/// The security catalogue matches only injection shapes, so a parameterized
/// write matches no catalogue row. These names count as a database sink
/// only in a module that reaches a database client package.
const DATABASE_WRITE_METHODS: [&str; 33] = [
    "$executeRaw",
    "$executeRawUnsafe",
    "$transaction",
    "batch",
    "bulkCreate",
    "bulkWrite",
    "create",
    "createMany",
    "del",
    "delete",
    "deleteFrom",
    "deleteMany",
    "deleteOne",
    "destroy",
    "findByIdAndDelete",
    "findByIdAndUpdate",
    "findOneAndDelete",
    "findOneAndReplace",
    "findOneAndUpdate",
    "insert",
    "insertInto",
    "insertMany",
    "insertOne",
    "remove",
    "replaceOne",
    "save",
    "transaction",
    "truncate",
    "update",
    "updateMany",
    "updateOne",
    "updateTable",
    "upsert",
];

/// Method names that run a prepared statement or a SQL script, such as
/// `insert.run(value)` and `lite.exec("DELETE FROM t")` in `better-sqlite3`.
/// The names are common outside databases (`deadline.run(task)`,
/// `pattern.exec(text)`), so they count only in a module that loads a
/// database client package itself. A regular expression receiver, such as
/// `/(\d+)/.exec(text)`, does not count.
const STATEMENT_RUN_METHODS: [&str; 2] = ["exec", "run"];

/// Method names that run a prepared statement: `stmt.run(value)` and
/// `db.prepare(sql).all()`. In a module with database reach, these count when
/// the receiver is a `.prepare(...)` call, or a name whose declarator
/// initializes it with one. A prepared statement whose static SQL text starts
/// with `select` is a read and does not count.
const PREPARED_RUN_METHODS: [&str; 5] = ["all", "each", "get", "iterate", "run"];

/// The method name that creates a prepared statement.
const PREPARE_METHOD: &str = "prepare";

/// The SQL verb of a prepared statement that only reads.
const SQL_READ_VERB: &str = "select";

/// SQL verbs that start a write statement. In a module with database reach,
/// a tagged template or a call whose first static SQL text starts with one of
/// these verbs is a sink: ``sql`insert into ...` ``,
/// `pool.query("DELETE FROM t")`, `lite.prepare("INSERT ...")`.
const SQL_WRITE_VERBS: [&str; 10] = [
    "alter", "create", "delete", "drop", "insert", "merge", "replace", "truncate", "update",
    "upsert",
];

/// Receivers of the language runtime. `Object.create(proto)` makes no write.
const RUNTIME_RECEIVERS: [&str; 2] = ["Object", "Reflect"];

/// The start offsets of the database write calls and tagged templates. A
/// write has a member callee or tag with a database write method name, such
/// as ``prisma.$executeRaw`DELETE ...` `` or `db["insert"](table)`, or SQL
/// text that starts with a write verb, such as ``sql`insert ...` `` or
/// `pool.query("INSERT ...")`. A call whose first argument is a path, such as
/// the route `app.delete("/items/:id", handler)`, is not a write method call.
/// A prepared statement run, such as `stmt.run(value)` after
/// `const stmt = db.prepare("INSERT ...")`, is a write too. When `direct` is
/// true, the module loads a database client package itself, and
/// `STATEMENT_RUN_METHODS` count too. The parse reads all calls, also calls
/// with only literal arguments, which the security capture does not record.
pub(super) fn starts(source: &str, layout: Layout, direct: bool) -> Vec<u32> {
    let (text, kind) = layout.parse_view(source);
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, &text, kind.source_type()).parse();
    let mut writes = Writes {
        direct,
        ..Writes::default()
    };
    writes.visit_program(&parsed.program);
    writes.finish()
}

fn sql_verb(text: &str) -> &str {
    let text = text.trim_start();
    let verb_end = text
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(text.len());
    &text[..verb_end]
}

fn is_sql_write(text: &str) -> bool {
    let verb = sql_verb(text);
    SQL_WRITE_VERBS
        .iter()
        .any(|write| write.eq_ignore_ascii_case(verb))
}

fn first_static_text<'b>(call: &'b CallExpression<'_>) -> Option<&'b str> {
    match call.arguments.first() {
        Some(Argument::StringLiteral(literal)) => Some(literal.value.as_str()),
        Some(Argument::TemplateLiteral(template)) => template
            .quasis
            .first()
            .map(|quasi| quasi.value.raw.as_str()),
        Some(Argument::TaggedTemplateExpression(tagged)) => tagged
            .quasi
            .quasis
            .first()
            .map(|quasi| quasi.value.raw.as_str()),
        _ => None,
    }
}

fn is_route_path(call: &CallExpression<'_>) -> bool {
    first_static_text(call).is_some_and(|text| text.starts_with('/'))
}

fn is_runtime_receiver(object: &Expression<'_>) -> bool {
    matches!(object, Expression::Identifier(receiver)
        if RUNTIME_RECEIVERS.contains(&receiver.name.as_str()))
}

/// Whether the expression is a regular expression: a literal or a
/// `RegExp` construction.
fn is_regex(expression: &Expression<'_>) -> bool {
    match expression.without_parentheses() {
        Expression::RegExpLiteral(_) => true,
        Expression::NewExpression(new) => {
            matches!(&new.callee, Expression::Identifier(callee) if callee.name == "RegExp")
        }
        Expression::CallExpression(call) => {
            matches!(&call.callee, Expression::Identifier(callee) if callee.name == "RegExp")
        }
        _ => false,
    }
}

/// Whether the expression is a `.prepare(...)` call whose statement can
/// write. Only static SQL text that starts with `select` is a read.
fn is_prepared_write(expression: &Expression<'_>) -> bool {
    let Expression::CallExpression(call) = expression.without_parentheses() else {
        return false;
    };
    let is_prepare = call
        .callee
        .as_member_expression()
        .and_then(|member| member.static_property_name())
        .is_some_and(|name| name == PREPARE_METHOD);
    is_prepare
        && !first_static_text(call)
            .is_some_and(|text| sql_verb(text).eq_ignore_ascii_case(SQL_READ_VERB))
}

/// A statement run whose receiver is a name. The declarator of the name
/// can come after the call in source order, so the check waits for the
/// end of the walk.
struct NamedRun {
    start: u32,
    receiver: String,
    prepared: bool,
}

#[derive(Default)]
struct Writes {
    direct: bool,
    starts: Vec<u32>,
    prepared_names: FxHashSet<String>,
    regex_names: FxHashSet<String>,
    named_runs: Vec<NamedRun>,
}

impl Writes {
    fn check_member_call(&mut self, call: &CallExpression<'_>) {
        let Some(member) = call.callee.as_member_expression() else {
            return;
        };
        let Some(name) = member.static_property_name() else {
            return;
        };
        let object = member.object().without_parentheses();
        let start = call.span.start;
        if DATABASE_WRITE_METHODS.contains(&name)
            && !is_runtime_receiver(object)
            && !is_route_path(call)
        {
            self.starts.push(start);
        }
        if PREPARED_RUN_METHODS.contains(&name) {
            if is_prepared_write(object) {
                self.starts.push(start);
            } else if let Expression::Identifier(receiver) = object {
                self.named_runs.push(NamedRun {
                    start,
                    receiver: receiver.name.to_string(),
                    prepared: true,
                });
            }
        }
        if self.direct && STATEMENT_RUN_METHODS.contains(&name) && !is_regex(object) {
            match object {
                Expression::Identifier(receiver) => self.named_runs.push(NamedRun {
                    start,
                    receiver: receiver.name.to_string(),
                    prepared: false,
                }),
                _ => self.starts.push(start),
            }
        }
    }

    fn finish(mut self) -> Vec<u32> {
        for run in &self.named_runs {
            let write = if run.prepared {
                self.prepared_names.contains(&run.receiver)
            } else {
                !self.regex_names.contains(&run.receiver)
            };
            if write {
                self.starts.push(run.start);
            }
        }
        self.starts
    }
}

impl<'a> Visit<'a> for Writes {
    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let (BindingPattern::BindingIdentifier(id), Some(init)) = (&it.id, &it.init) {
            if is_prepared_write(init) {
                self.prepared_names.insert(id.name.to_string());
            } else if is_regex(init) {
                self.regex_names.insert(id.name.to_string());
            }
        }
        oxc_ast_visit::walk::walk_variable_declarator(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        self.check_member_call(it);
        if first_static_text(it).is_some_and(is_sql_write) {
            self.starts.push(it.span.start);
        }
        oxc_ast_visit::walk::walk_call_expression(self, it);
    }

    fn visit_tagged_template_expression(&mut self, it: &TaggedTemplateExpression<'a>) {
        let write_tag = it.tag.as_member_expression().is_some_and(|member| {
            member.static_property_name().is_some_and(|name| {
                DATABASE_WRITE_METHODS.contains(&name)
                    || (self.direct && STATEMENT_RUN_METHODS.contains(&name))
            }) && !is_runtime_receiver(member.object())
        });
        let sql_write = it
            .quasi
            .quasis
            .first()
            .is_some_and(|quasi| is_sql_write(&quasi.value.raw));
        if write_tag || sql_write {
            self.starts.push(it.span.start);
        }
        oxc_ast_visit::walk::walk_tagged_template_expression(self, it);
    }
}
