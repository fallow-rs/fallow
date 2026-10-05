//! `nestjs-trpc` plugin.
//!
//! The `nestjs-trpc` module resolves context, error-handler, and middleware
//! classes through Nest dependency injection and calls their interface methods
//! itself. This plugin credits those methods so they are not reported as
//! `unused-class-member`. Each rule is scoped to the matching interface, so
//! other classes are not affected.

use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};

use super::Plugin;

const ENABLERS: &[&str] = &["nestjs-trpc"];

/// `TRPCContext.create()` builds the request context for each call.
const CONTEXT_MEMBERS: &[&str] = &["create"];

/// `TRPCErrorHandler.onError()` receives each procedure error.
const ERROR_HANDLER_MEMBERS: &[&str] = &["onError"];

/// `TRPCMiddleware.use()` runs for each procedure that applies the middleware.
const MIDDLEWARE_MEMBERS: &[&str] = &["use"];

fn implements_rule(iface: &str, members: &[&str]) -> UsedClassMemberRule {
    UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: None,
        implements: Some(iface.to_string()),
        members: members.iter().map(|s| (*s).to_string()).collect(),
    })
}

pub struct NestJsTrpcPlugin;

impl Plugin for NestJsTrpcPlugin {
    fn name(&self) -> &'static str {
        "nestjs-trpc"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn used_class_member_rules(&self) -> Vec<UsedClassMemberRule> {
        vec![
            implements_rule("TRPCContext", CONTEXT_MEMBERS),
            implements_rule("TRPCErrorHandler", ERROR_HANDLER_MEMBERS),
            implements_rule("TRPCMiddleware", MIDDLEWARE_MEMBERS),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credits(implemented: &[&str], member: &str) -> bool {
        let implemented: Vec<String> = implemented.iter().map(|s| (*s).to_string()).collect();
        NestJsTrpcPlugin
            .used_class_member_rules()
            .iter()
            .any(|rule| match rule {
                UsedClassMemberRule::Scoped(s) => {
                    s.matches_heritage(None, &implemented) && s.members.iter().any(|m| m == member)
                }
                UsedClassMemberRule::Name(name) => name == member,
            })
    }

    #[test]
    fn enabler_is_the_package_name() {
        assert_eq!(NestJsTrpcPlugin.enablers(), &["nestjs-trpc"]);
    }

    #[test]
    fn interface_methods_are_credited() {
        assert!(credits(&["TRPCContext"], "create"));
        assert!(credits(&["TRPCErrorHandler"], "onError"));
        assert!(credits(&["TRPCMiddleware"], "use"));
    }

    #[test]
    fn other_methods_are_not_credited() {
        assert!(!credits(&["TRPCContext"], "helper"));
        assert!(!credits(&["TRPCContext"], "onError"));
        assert!(!credits(&["TRPCMiddleware"], "create"));
    }

    #[test]
    fn class_without_interface_gets_no_credit() {
        assert!(!credits(&[], "create"));
        assert!(!credits(&[], "onError"));
        assert!(!credits(&[], "use"));
    }
}
