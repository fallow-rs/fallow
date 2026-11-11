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

use oxc_ast::ast::*;

use super::{StructuralParamMemberCollector, unwrap_static_expr};

/// Namespace methods that make a Durable Object id.
const ID_FACTORY_METHODS: &[&str] = &["idFromName", "idFromString", "newUniqueId"];

/// The receiver of `<receiver>.<method>(...)`, when `method` is one of
/// `methods`.
fn method_call_receiver<'b, 'a>(
    expr: &'b Expression<'a>,
    methods: &[&str],
) -> Option<&'b Expression<'a>> {
    let Expression::CallExpression(call) = unwrap_static_expr(expr) else {
        return None;
    };
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
    if let Some(namespace) = method_call_receiver(expr, &["getByName"]) {
        return Some((namespace, StubId::None));
    }
    let namespace = method_call_receiver(expr, &["get"])?;
    let Expression::CallExpression(call) = unwrap_static_expr(expr) else {
        return None;
    };
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
    ) -> Option<(String, (String, String))> {
        let BindingPattern::BindingIdentifier(local) = &declarator.id else {
            return None;
        };
        let init = declarator.init.as_ref()?;
        if let Some(namespace) = durable_object_id_namespace(init)
            .and_then(|namespace| self.resolve_receiver_path(namespace, false))
        {
            self.durable_object_ids
                .insert(local.name.to_string(), namespace);
            return None;
        }
        let (namespace, id) = durable_object_stub_call(init)?;
        let namespace = self.resolve_receiver_path(namespace, false)?;
        let same_namespace = match id {
            StubId::None => true,
            StubId::Inline(id_namespace) => {
                self.resolve_receiver_path(id_namespace, false).as_ref() == Some(&namespace)
            }
            StubId::Local(id_local) => self.durable_object_ids.get(id_local) == Some(&namespace),
        };
        same_namespace.then(|| (local.name.to_string(), namespace))
    }
}
