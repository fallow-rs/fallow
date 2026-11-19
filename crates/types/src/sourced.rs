//! Values that record their cause.
//!
//! A run decision, such as the production mode of one analysis, comes from
//! one of several inputs. [`Sourced`](crate::sourced::Sourced) keeps the
//! value together with the [`Source`](crate::sourced::Source) that gave it and
//! the [`Scope`](crate::sourced::Scope) of that input. The rank
//! `(Layer, Scope)` orders the inputs: a higher rank wins.

use std::path::PathBuf;

use serde::Serialize;

/// The input layer of a value, from the weakest to the strongest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layer {
    /// A built-in default, or a value that follows from another decision.
    Default,
    /// A key of the config file.
    ConfigFile,
    /// An environment variable.
    Environment,
    /// A value that the caller gives: a flag, a tool parameter, an editor
    /// setting, or an API option.
    Given,
}

/// The scope of an input. In one layer, an input of one analysis wins over an
/// input of every analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// The input applies to every analysis, for example `--production`.
    Global,
    /// The input applies to one analysis, for example `--production-health`.
    Analysis,
}

/// The surface input that gave a [`Layer::Given`] value. The name is the
/// spelling of the input on that surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", content = "name", rename_all = "kebab-case")]
pub enum Origin {
    /// A CLI flag, for example `--production`.
    Flag(&'static str),
    /// An MCP tool parameter, for example `production_dead_code`.
    ToolParam(&'static str),
    /// An editor setting, for example `fallow.production`.
    EditorSetting(&'static str),
    /// An option of the Rust API, for example `analysis.production_override`.
    ApiOption(&'static str),
    /// An option of the Node bindings, for example `production`.
    NodeOption(&'static str),
}

/// The input that gave a value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Source {
    /// A built-in default. `rule` names the default.
    Default {
        /// The name of the default.
        rule: &'static str,
    },
    /// A value that follows from another decision or input. `from` names it.
    Derived {
        /// The name of the decision or input.
        from: &'static str,
    },
    /// A key of the config file. `file` is the top file of the `extends`
    /// chain, because the merge of the chain keeps no file per key.
    ConfigFile {
        /// The top config file.
        #[serde(serialize_with = "crate::serde_path::serialize")]
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
        file: PathBuf,
        /// The config key, for example `production.deadCode`.
        key: &'static str,
    },
    /// An environment variable.
    Environment {
        /// The name of the variable, for example `FALLOW_PRODUCTION`.
        name: &'static str,
    },
    /// A value that the caller gives.
    Given {
        /// The surface input.
        origin: Origin,
    },
}

impl Source {
    /// The input layer of this source. [`Source::Default`] and
    /// [`Source::Derived`] are in [`Layer::Default`].
    #[must_use]
    pub const fn layer(&self) -> Layer {
        match self {
            Self::Default { .. } | Self::Derived { .. } => Layer::Default,
            Self::ConfigFile { .. } => Layer::ConfigFile,
            Self::Environment { .. } => Layer::Environment,
            Self::Given { .. } => Layer::Given,
        }
    }
}

/// A value with the input that gave it and the scope of that input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "Sourced_for_{T}"))]
pub struct Sourced<T> {
    /// The value.
    value: T,
    /// The input that gave the value.
    source: Source,
    /// The scope of that input.
    scope: Scope,
}

impl<T> Sourced<T> {
    /// A value from `source`, an input with `scope`.
    #[must_use]
    pub const fn new(value: T, source: Source, scope: Scope) -> Self {
        Self {
            value,
            source,
            scope,
        }
    }

    /// The value.
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// The value, without its source.
    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }

    /// The input that gave the value.
    #[must_use]
    pub const fn source(&self) -> &Source {
        &self.source
    }

    /// The rank of the input. Of two inputs for one decision, the input with
    /// the higher rank wins.
    #[must_use]
    pub const fn rank(&self) -> (Layer, Scope) {
        (self.source.layer(), self.scope)
    }

    /// The value changed by `f`, with the same source and scope.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Sourced<U> {
        Sourced {
            value: f(self.value),
            source: self.source,
            scope: self.scope,
        }
    }
}

/// A value that the caller gives, with the surface input that carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Given<T> {
    /// The value.
    pub value: T,
    /// The surface input.
    pub origin: Origin,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn source_serializes_with_kebab_case_kind() {
        let source = Source::ConfigFile {
            file: PathBuf::from("project/.fallowrc.json"),
            key: "production.deadCode",
        };
        assert_eq!(
            serde_json::to_value(&source).unwrap(),
            serde_json::json!({
                "kind": "config-file",
                "file": "project/.fallowrc.json",
                "key": "production.deadCode",
            })
        );
    }
}
