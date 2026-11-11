//! Shapes of Cloudflare Durable Object stub calls.
//!
//! A Worker reads a Durable Object through a namespace binding, typed
//! `DurableObjectNamespace<C>`. A call of `get()` or `getByName()` on that
//! binding gives an RPC stub, and a method call on the stub runs the method of
//! the class `C`. The namespace type itself resolves to `C` (see
//! `MEMBER_PRESERVING_WRAPPERS`), so a stub local is an alias of the namespace
//! receiver.
//!
//! `get()` makes a stub only when its id argument comes from the same
//! namespace (`<ns>.idFromName(...)` inline, or a local that holds such a
//! call). A `get()` on a `Map` or on a repository therefore makes no alias.
//!
//! Because the namespace type resolves to `C`, a namespace method call such
//! as `env.NS.get(...)` reads like a call of `C.get`. Such a call gives no
//! credit when the same function proves that `env.NS` is a namespace: a
//! `get()` call on it takes an id of the same namespace. A call through a
//! stub local (`stub.get()`) always gives credit.

use oxc_ast::ast::*;
use rustc_hash::{FxHashMap, FxHashSet};

use super::{StructuralParamMemberCollector, unwrap_static_expr};

/// A local that reads a property path of a tracked parameter.
pub(super) struct ReceiverAlias {
    /// The tracked parameter and the property path.
    pub(super) target: (String, String),
    /// The local holds a Durable Object stub of the namespace at `target`.
    pub(super) stub: bool,
}

impl ReceiverAlias {
    pub(super) fn plain(target: (String, String)) -> Self {
        Self {
            target,
            stub: false,
        }
    }
}

/// A receiver that resolves to a property path of a tracked parameter.
pub(super) struct ResolvedReceiver {
    pub(super) param: String,
    pub(super) path: String,
    /// The receiver reads the path through a Durable Object stub local.
    pub(super) via_stub: bool,
}

/// The Durable Object calls of one function body.
#[derive(Default)]
pub(super) struct DurableObjectCalls {
    /// Locals that hold a Durable Object id, mapped to the parameter and the
    /// namespace path that made the id.
    ids: FxHashMap<String, (String, String)>,
    /// Parameter paths that a `get()` with an id of the same namespace
    /// proves to be a Durable Object namespace.
    namespaces: FxHashSet<(String, String)>,
    /// Direct accesses of a namespace method name, as (parameter, path,
    /// member). Each one becomes a member access at the end of the body,
    /// unless its path is in `namespaces`.
    method_accesses: Vec<(String, String, String)>,
}

/// Namespace methods that make a Durable Object id.
const ID_FACTORY_METHODS: &[&str] = &["idFromName", "idFromString", "newUniqueId"];

/// The methods of `DurableObjectNamespace`.
const NAMESPACE_METHODS: &[&str] = &[
    "get",
    "getByName",
    "idFromName",
    "idFromString",
    "newUniqueId",
    "jurisdiction",
];

/// `member` is a method of `DurableObjectNamespace`.
fn is_namespace_method(member: &str) -> bool {
    NAMESPACE_METHODS.contains(&member)
}

/// The receiver of `<receiver>.<method>(...)`, when `method` is one of
/// `methods`.
fn method_call_receiver<'b, 'a>(
    expr: &'b Expression<'a>,
    methods: &[&str],
) -> Option<&'b Expression<'a>> {
    let Expression::CallExpression(call) = unwrap_static_expr(expr) else {
        return None;
    };
    call_method_receiver(call, methods)
}

/// The receiver of the call `<receiver>.<method>(...)`, when `method` is one
/// of `methods`.
fn call_method_receiver<'b, 'a>(
    call: &'b CallExpression<'a>,
    methods: &[&str],
) -> Option<&'b Expression<'a>> {
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    methods
        .contains(&member.property.name.as_str())
        .then_some(&member.object)
}

/// The namespace receiver of `<ns>.idFromName(...)`, `<ns>.idFromString(...)`
/// or `<ns>.newUniqueId()`.
fn durable_object_id_namespace<'b, 'a>(expr: &'b Expression<'a>) -> Option<&'b Expression<'a>> {
    method_call_receiver(expr, ID_FACTORY_METHODS)
}

/// The id argument of a stub-making call.
enum StubId<'b, 'a> {
    /// `getByName(name)` takes a name, not an id.
    None,
    /// `get(<ns>.idFromName(...))`: the namespace receiver of the inline id.
    Inline(&'b Expression<'a>),
    /// `get(id)`: the local that must hold an id of the same namespace.
    Local(&'b str),
}

/// The namespace receiver and the id argument of `<ns>.getByName(...)` or
/// `<ns>.get(<id>)`.
fn durable_object_stub_call<'b, 'a>(
    expr: &'b Expression<'a>,
) -> Option<(&'b Expression<'a>, StubId<'b, 'a>)> {
    let Expression::CallExpression(call) = unwrap_static_expr(expr) else {
        return None;
    };
    durable_object_stub_call_parts(call)
}

fn durable_object_stub_call_parts<'b, 'a>(
    call: &'b CallExpression<'a>,
) -> Option<(&'b Expression<'a>, StubId<'b, 'a>)> {
    if let Some(namespace) = call_method_receiver(call, &["getByName"]) {
        return Some((namespace, StubId::None));
    }
    let namespace = call_method_receiver(call, &["get"])?;
    let id = match call.arguments.first()?.as_expression()? {
        Expression::Identifier(local) => StubId::Local(local.name.as_str()),
        inline => StubId::Inline(durable_object_id_namespace(inline)?),
    };
    Some((namespace, id))
}

impl StructuralParamMemberCollector {
    /// `const stub = env.NS.get(env.NS.idFromName(n))` makes `stub` an alias
    /// of the namespace path `NS` of the tracked parameter `env`. Records an
    /// id local (`const id = env.NS.idFromName(n)`) for a later `get(id)`.
    pub(super) fn durable_object_stub_alias(
        &mut self,
        declarator: &VariableDeclarator<'_>,
    ) -> Option<(String, ReceiverAlias)> {
        let BindingPattern::BindingIdentifier(local) = &declarator.id else {
            return None;
        };
        let init = declarator.init.as_ref()?;
        if let Some(namespace) = durable_object_id_namespace(init)
            .and_then(|namespace| self.resolve_receiver_path(namespace, false))
        {
            self.durable_objects
                .ids
                .insert(local.name.to_string(), namespace);
            return None;
        }
        let (namespace, id) = durable_object_stub_call(init)?;
        let (namespace, _) = self.stub_namespace(namespace, &id)?;
        let alias = ReceiverAlias {
            target: namespace,
            stub: true,
        };
        Some((local.name.to_string(), alias))
    }

    /// `env.NS.get(env.NS.idFromName(n))`, or `env.NS.get(id)` with an id
    /// local of `env.NS`, proves that `env.NS` is a namespace.
    pub(super) fn record_durable_object_namespace(&mut self, call: &CallExpression<'_>) {
        if let Some((namespace, id)) = durable_object_stub_call_parts(call)
            && let Some((namespace, true)) = self.stub_namespace(namespace, &id)
        {
            self.durable_objects.namespaces.insert(namespace);
        }
    }

    /// The parameter and the namespace path of a stub-making call, and
    /// whether its id argument comes from the same namespace. `None` when the
    /// id comes from another receiver.
    fn stub_namespace(
        &self,
        namespace: &Expression<'_>,
        id: &StubId<'_, '_>,
    ) -> Option<((String, String), bool)> {
        let namespace = self.resolve_receiver_path(namespace, false)?;
        let id_namespace = match id {
            StubId::None => return Some((namespace, false)),
            StubId::Inline(id_namespace) => self.resolve_receiver_path(id_namespace, false),
            StubId::Local(id_local) => self.durable_objects.ids.get(*id_local).cloned(),
        };
        (id_namespace.as_ref() == Some(&namespace)).then_some((namespace, true))
    }

    /// Records the access of `member` on `receiver`.
    pub(super) fn record_member_access(&mut self, receiver: ResolvedReceiver, member: String) {
        if !receiver.via_stub && is_namespace_method(&member) {
            self.durable_objects
                .method_accesses
                .push((receiver.param, receiver.path, member));
            return;
        }
        self.insert_member_access(receiver.param, receiver.path, member);
    }

    fn insert_member_access(&mut self, param: String, path: String, member: String) {
        if path.is_empty() {
            self.members.entry(param).or_default().insert(member);
        } else {
            self.property_members
                .entry(param)
                .or_default()
                .insert((path, member));
        }
    }

    /// A namespace method call (`env.NS.get(...)`) runs the namespace API, not
    /// the method of the Durable Object class. Drops each such access on a
    /// proven namespace path, and keeps the others.
    pub(super) fn commit_namespace_method_accesses(&mut self) {
        for (param, path, member) in std::mem::take(&mut self.durable_objects.method_accesses) {
            if !self
                .durable_objects
                .namespaces
                .contains(&(param.clone(), path.clone()))
            {
                self.insert_member_access(param, path, member);
            }
        }
    }
}
