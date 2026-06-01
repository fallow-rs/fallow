//! `NestJS` backend framework plugin.
//!
//! Detects `NestJS` projects and marks module, controller, service, guard,
//! interceptor, pipe, filter, middleware, gateway, and resolver files as entry points.
//!
//! Also credits the framework-dispatched lifecycle and handler methods so they
//! are not reported as `unused-class-member`. Each rule is scoped to the
//! relevant Nest interface so unrelated classes are unaffected.

use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};

use super::Plugin;

const ENABLERS: &[&str] = &["@nestjs/core"];

const ENTRY_PATTERNS: &[&str] = &[
    "src/main.ts",
    "src/**/*.module.ts",
    "src/**/*.controller.ts",
    "src/**/*.service.ts",
    "src/**/*.guard.ts",
    "src/**/*.interceptor.ts",
    "src/**/*.pipe.ts",
    "src/**/*.filter.ts",
    "src/**/*.middleware.ts",
    "src/**/*.decorator.ts",
    "src/**/*.gateway.ts",
    "src/**/*.resolver.ts",
];

const ALWAYS_USED: &[&str] = &["nest-cli.json"];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "@nestjs/core",
    "@nestjs/common",
    "@nestjs/cli",
    "@nestjs/testing",
    "@nestjs/platform-express",
    "@nestjs/platform-fastify",
    "@nestjs/swagger",
    "@nestjs/config",
    "@nestjs/typeorm",
    "@nestjs/mongoose",
    "reflect-metadata",
];

/// `NestModule.configure()` called by the framework when wiring middleware.
const NEST_MODULE_MEMBERS: &[&str] = &["configure"];

/// General module lifecycle hooks from `OnModuleInit`, `OnModuleDestroy`,
/// `OnApplicationBootstrap`, `BeforeApplicationShutdown`, and
/// `OnApplicationShutdown`. These are invoked reflectively by the Nest
/// lifecycle manager regardless of which class implements them.
const MODULE_LIFECYCLE_MEMBERS: &[&str] = &[
    "onModuleInit",
    "onModuleDestroy",
    "onApplicationBootstrap",
    "beforeApplicationShutdown",
    "onApplicationShutdown",
];

/// `CanActivate.canActivate()` the guard dispatch method.
const GUARD_MEMBERS: &[&str] = &["canActivate"];

/// `NestInterceptor.intercept()` the interceptor dispatch method.
const INTERCEPTOR_MEMBERS: &[&str] = &["intercept"];

/// `PipeTransform.transform()` the pipe dispatch method.
const PIPE_MEMBERS: &[&str] = &["transform"];

/// `ExceptionFilter.catch()` the exception-filter dispatch method.
const FILTER_MEMBERS: &[&str] = &["catch"];

/// `NestMiddleware.use()` the middleware dispatch method.
const MIDDLEWARE_MEMBERS: &[&str] = &["use"];

fn implements_rule(iface: &str, members: &[&str]) -> UsedClassMemberRule {
    UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: None,
        implements: Some(iface.to_string()),
        members: members.iter().map(|s| (*s).to_string()).collect(),
    })
}

pub struct NestJsPlugin;

impl Plugin for NestJsPlugin {
    fn name(&self) -> &'static str {
        "nestjs"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn used_class_member_rules(&self) -> Vec<UsedClassMemberRule> {
        vec![
            // NestModule: middleware configuration
            implements_rule("NestModule", NEST_MODULE_MEMBERS),
            // Lifecycle hooks: all five interfaces share the same method names;
            // scope each individually so a class that only implements one of them
            // still gets the right credit.
            implements_rule("OnModuleInit", MODULE_LIFECYCLE_MEMBERS),
            implements_rule("OnModuleDestroy", MODULE_LIFECYCLE_MEMBERS),
            implements_rule("OnApplicationBootstrap", MODULE_LIFECYCLE_MEMBERS),
            implements_rule("BeforeApplicationShutdown", MODULE_LIFECYCLE_MEMBERS),
            implements_rule("OnApplicationShutdown", MODULE_LIFECYCLE_MEMBERS),
            // Handler dispatch methods
            implements_rule("CanActivate", GUARD_MEMBERS),
            implements_rule("NestInterceptor", INTERCEPTOR_MEMBERS),
            implements_rule("PipeTransform", PIPE_MEMBERS),
            implements_rule("ExceptionFilter", FILTER_MEMBERS),
            implements_rule("NestMiddleware", MIDDLEWARE_MEMBERS),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enablers_contain_nestjs_core() {
        assert!(NestJsPlugin.enablers().contains(&"@nestjs/core"));
    }

    #[test]
    fn tooling_dependencies_cover_common_nest_packages() {
        let deps = NestJsPlugin.tooling_dependencies();
        assert!(deps.contains(&"@nestjs/core"));
        assert!(deps.contains(&"@nestjs/common"));
        assert!(deps.contains(&"@nestjs/cli"));
        assert!(deps.contains(&"reflect-metadata"));
    }

    /// `configure` on a class that implements `NestModule` must be credited as
    /// used. A genuinely-unused method on the same class (`unusedHelper`) must
    /// NOT be credited.
    #[test]
    fn configure_on_nest_module_is_credited_as_used() {
        let rules = NestJsPlugin.used_class_member_rules();

        // Simulate a class: `class AppModule implements NestModule`
        let implemented = vec!["NestModule".to_string()];
        let super_class: Option<&str> = None;

        let configure_credited = rules.iter().any(|r| match r {
            UsedClassMemberRule::Scoped(s) => {
                s.matches_heritage(super_class, &implemented)
                    && s.members.iter().any(|m| m == "configure")
            }
            UsedClassMemberRule::Name(name) => name == "configure",
        });
        assert!(
            configure_credited,
            "`configure` must be credited as used for a class implementing NestModule"
        );

        // `unusedHelper` must not be credited by any rule matched for NestModule
        let unused_helper_credited = rules.iter().any(|r| match r {
            UsedClassMemberRule::Scoped(s) => {
                s.matches_heritage(super_class, &implemented)
                    && s.members.iter().any(|m| m == "unusedHelper")
            }
            UsedClassMemberRule::Name(name) => name == "unusedHelper",
        });
        assert!(
            !unused_helper_credited,
            "`unusedHelper` must NOT be credited as used: it is a genuinely unused method"
        );
    }

    #[test]
    fn lifecycle_hooks_are_credited_for_implementing_classes() {
        let rules = NestJsPlugin.used_class_member_rules();

        for iface in [
            "OnModuleInit",
            "OnModuleDestroy",
            "OnApplicationBootstrap",
            "BeforeApplicationShutdown",
            "OnApplicationShutdown",
        ] {
            let implemented = vec![iface.to_string()];
            for method in [
                "onModuleInit",
                "onModuleDestroy",
                "onApplicationBootstrap",
                "beforeApplicationShutdown",
                "onApplicationShutdown",
            ] {
                let credited = rules.iter().any(|r| match r {
                    UsedClassMemberRule::Scoped(s) => {
                        s.matches_heritage(None, &implemented)
                            && s.members.iter().any(|m| m == method)
                    }
                    UsedClassMemberRule::Name(_) => false,
                });
                assert!(
                    credited,
                    "`{method}` must be credited for a class implementing `{iface}`"
                );
            }
        }
    }

    #[test]
    fn handler_dispatch_methods_are_credited() {
        let rules = NestJsPlugin.used_class_member_rules();
        let cases: &[(&str, &str)] = &[
            ("CanActivate", "canActivate"),
            ("NestInterceptor", "intercept"),
            ("PipeTransform", "transform"),
            ("ExceptionFilter", "catch"),
            ("NestMiddleware", "use"),
        ];
        for (iface, method) in cases {
            let implemented = vec![(*iface).to_string()];
            let credited = rules.iter().any(|r| match r {
                UsedClassMemberRule::Scoped(s) => {
                    s.matches_heritage(None, &implemented) && s.members.iter().any(|m| m == *method)
                }
                UsedClassMemberRule::Name(_) => false,
            });
            assert!(
                credited,
                "`{method}` must be credited for a class implementing `{iface}`"
            );
        }
    }

    #[test]
    fn unrelated_class_gets_no_lifecycle_credit() {
        let rules = NestJsPlugin.used_class_member_rules();
        // A plain service class that extends nothing and implements nothing
        let no_implements: Vec<String> = vec![];
        for r in &rules {
            let UsedClassMemberRule::Scoped(s) = r else {
                continue;
            };
            assert!(
                !s.matches_heritage(None, &no_implements),
                "rule {s:?} must not match a class with no heritage"
            );
        }
    }

    #[test]
    fn rules_are_all_implements_scoped_not_extends() {
        let rules = NestJsPlugin.used_class_member_rules();
        for r in &rules {
            let UsedClassMemberRule::Scoped(s) = r else {
                continue;
            };
            assert!(
                s.extends.is_none(),
                "NestJS rules should use `implements`, not `extends`; found: {s:?}"
            );
            assert!(
                s.implements.is_some(),
                "every scoped NestJS rule must have an `implements` constraint; found: {s:?}"
            );
        }
    }
}
