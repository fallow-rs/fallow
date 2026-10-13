//! `OpenTelemetry` SDK plugin.
//!
//! The `OpenTelemetry` SDK calls the methods of a sampler, a processor, an
//! exporter, a context manager or a propagator that the project registers.
//! The project never calls these methods itself, so they look unused.
//!
//! The plugin credits only the methods of the interface that a class
//! implements, or the hooks of the `MetricReader` base class that a class
//! extends. Other members of the class stay reportable.

use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};
use fallow_types::semantic::{SemanticFrameworkContract, SemanticFrameworkRelation};

use super::Plugin;

/// Any `@opentelemetry/*` dependency activates the plugin.
const ENABLERS: &[&str] = &["@opentelemetry/"];

/// The trace SDK 2.x declares the trace interfaces in `sdk-trace`.
/// `sdk-trace-base` re-exports them in 2.x and declares them in 1.x.
const TRACE_PACKAGES: &[&str] = &["@opentelemetry/sdk-trace", "@opentelemetry/sdk-trace-base"];
const METRICS_PACKAGES: &[&str] = &["@opentelemetry/sdk-metrics"];
const LOGS_PACKAGES: &[&str] = &["@opentelemetry/sdk-logs"];
const API_PACKAGES: &[&str] = &["@opentelemetry/api"];

/// `Sampler`: the tracer calls `shouldSample` and logs `toString`.
const SAMPLER_MEMBERS: &[&str] = &["shouldSample", "toString"];

/// `SpanProcessor`, with the optional `onEnding` hook.
const SPAN_PROCESSOR_MEMBERS: &[&str] = &["onStart", "onEnding", "onEnd", "shutdown", "forceFlush"];

/// `SpanExporter`, with the optional `forceFlush`.
const SPAN_EXPORTER_MEMBERS: &[&str] = &["export", "shutdown", "forceFlush"];

/// `PushMetricExporter`, with the optional aggregation selectors.
const PUSH_METRIC_EXPORTER_MEMBERS: &[&str] = &[
    "export",
    "forceFlush",
    "selectAggregationTemporality",
    "selectAggregation",
    "shutdown",
];

/// The protected hooks that the `MetricReader` base class calls.
const METRIC_READER_HOOKS: &[&str] = &["onInitialized", "onForceFlush", "onShutdown"];

/// `LogRecordProcessor`, with the optional `enabled`.
const LOG_RECORD_PROCESSOR_MEMBERS: &[&str] = &["onEmit", "enabled", "shutdown", "forceFlush"];

/// `LogRecordExporter`.
const LOG_RECORD_EXPORTER_MEMBERS: &[&str] = &["export", "shutdown", "forceFlush"];

/// `ContextManager` from `@opentelemetry/api`.
const CONTEXT_MANAGER_MEMBERS: &[&str] = &["active", "with", "bind", "enable", "disable"];

/// `TextMapPropagator` from `@opentelemetry/api`.
const TEXT_MAP_PROPAGATOR_MEMBERS: &[&str] = &["inject", "extract", "fields"];

/// `TextMapGetter` from `@opentelemetry/api`.
const TEXT_MAP_GETTER_MEMBERS: &[&str] = &["keys", "get"];

/// `TextMapSetter` from `@opentelemetry/api`.
const TEXT_MAP_SETTER_MEMBERS: &[&str] = &["set"];

/// One heritage contract: the packages that declare the symbol, the symbol,
/// the relation and the members that the SDK calls.
struct Contract {
    packages: &'static [&'static str],
    symbol: &'static str,
    relation: SemanticFrameworkRelation,
    members: &'static [&'static str],
}

const CONTRACTS: &[Contract] = &[
    Contract {
        packages: TRACE_PACKAGES,
        symbol: "Sampler",
        relation: SemanticFrameworkRelation::Implements,
        members: SAMPLER_MEMBERS,
    },
    Contract {
        packages: TRACE_PACKAGES,
        symbol: "SpanProcessor",
        relation: SemanticFrameworkRelation::Implements,
        members: SPAN_PROCESSOR_MEMBERS,
    },
    Contract {
        packages: TRACE_PACKAGES,
        symbol: "SpanExporter",
        relation: SemanticFrameworkRelation::Implements,
        members: SPAN_EXPORTER_MEMBERS,
    },
    Contract {
        packages: METRICS_PACKAGES,
        symbol: "PushMetricExporter",
        relation: SemanticFrameworkRelation::Implements,
        members: PUSH_METRIC_EXPORTER_MEMBERS,
    },
    Contract {
        packages: METRICS_PACKAGES,
        symbol: "MetricReader",
        relation: SemanticFrameworkRelation::Extends,
        members: METRIC_READER_HOOKS,
    },
    Contract {
        packages: LOGS_PACKAGES,
        symbol: "LogRecordProcessor",
        relation: SemanticFrameworkRelation::Implements,
        members: LOG_RECORD_PROCESSOR_MEMBERS,
    },
    Contract {
        packages: LOGS_PACKAGES,
        symbol: "LogRecordExporter",
        relation: SemanticFrameworkRelation::Implements,
        members: LOG_RECORD_EXPORTER_MEMBERS,
    },
    Contract {
        packages: API_PACKAGES,
        symbol: "ContextManager",
        relation: SemanticFrameworkRelation::Implements,
        members: CONTEXT_MANAGER_MEMBERS,
    },
    Contract {
        packages: API_PACKAGES,
        symbol: "TextMapPropagator",
        relation: SemanticFrameworkRelation::Implements,
        members: TEXT_MAP_PROPAGATOR_MEMBERS,
    },
    Contract {
        packages: API_PACKAGES,
        symbol: "TextMapGetter",
        relation: SemanticFrameworkRelation::Implements,
        members: TEXT_MAP_GETTER_MEMBERS,
    },
    Contract {
        packages: API_PACKAGES,
        symbol: "TextMapSetter",
        relation: SemanticFrameworkRelation::Implements,
        members: TEXT_MAP_SETTER_MEMBERS,
    },
];

fn to_strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

pub struct OpenTelemetryPlugin;

impl Plugin for OpenTelemetryPlugin {
    fn name(&self) -> &'static str {
        "opentelemetry"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn used_class_member_rules(&self) -> Vec<UsedClassMemberRule> {
        CONTRACTS
            .iter()
            .map(|contract| {
                let symbol = Some(contract.symbol.to_string());
                let (extends, implements) = match contract.relation {
                    SemanticFrameworkRelation::Extends => (symbol, None),
                    SemanticFrameworkRelation::Implements => (None, symbol),
                };
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends,
                    implements,
                    members: to_strings(contract.members),
                })
            })
            .collect()
    }

    fn framework_class_member_contracts(&self) -> Vec<SemanticFrameworkContract> {
        CONTRACTS
            .iter()
            .flat_map(|contract| {
                contract
                    .packages
                    .iter()
                    .map(move |package| SemanticFrameworkContract {
                        framework: self.name().to_string(),
                        package: (*package).to_string(),
                        heritage_symbol: contract.symbol.to_string(),
                        heritage_names: vec![contract.symbol.to_string()],
                        relation: contract.relation,
                        members: to_strings(contract.members),
                    })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scoped_rules() -> Vec<ScopedUsedClassMemberRule> {
        OpenTelemetryPlugin
            .used_class_member_rules()
            .into_iter()
            .map(|rule| match rule {
                UsedClassMemberRule::Scoped(scoped) => scoped,
                UsedClassMemberRule::Name(name) => {
                    panic!("rule `{name}` is not scoped to an OpenTelemetry heritage")
                }
            })
            .collect()
    }

    fn credited(super_class: Option<&str>, interfaces: &[&str]) -> Vec<String> {
        let interfaces = to_strings(interfaces);
        scoped_rules()
            .into_iter()
            .filter(|rule| rule.matches_heritage(super_class, &interfaces))
            .flat_map(|rule| rule.members)
            .collect()
    }

    #[test]
    fn any_opentelemetry_dependency_enables_the_plugin() {
        let deps = vec!["@opentelemetry/api".to_string()];
        assert!(OpenTelemetryPlugin.is_enabled_with_deps(&deps, std::path::Path::new("/")));
        let deps = vec!["opentelemetry".to_string()];
        assert!(!OpenTelemetryPlugin.is_enabled_with_deps(&deps, std::path::Path::new("/")));
    }

    #[test]
    fn sampler_credits_only_its_interface_methods() {
        let members = credited(None, &["Sampler"]);
        assert_eq!(members, vec!["shouldSample", "toString"]);
    }

    #[test]
    fn metric_reader_hooks_need_the_base_class() {
        let members = credited(Some("MetricReader"), &[]);
        assert_eq!(members, vec!["onInitialized", "onForceFlush", "onShutdown"]);
        assert!(credited(None, &["MetricReader"]).is_empty());
    }

    #[test]
    fn unrelated_heritage_gets_no_credit() {
        assert!(credited(Some("Base"), &["Disposable"]).is_empty());
        assert!(credited(None, &[]).is_empty());
    }

    #[test]
    fn trace_contracts_accept_both_trace_packages() {
        let contracts = OpenTelemetryPlugin.framework_class_member_contracts();
        for package in TRACE_PACKAGES {
            for symbol in ["Sampler", "SpanProcessor", "SpanExporter"] {
                assert!(
                    contracts.iter().any(|contract| contract.package == *package
                        && contract.heritage_symbol == symbol
                        && contract.relation == SemanticFrameworkRelation::Implements),
                    "missing {symbol} contract for {package}"
                );
            }
        }
    }
}
