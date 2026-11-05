//! Parameters of the mutation MCP tools.

use super::*;

/// Parameters for the static mutation plan. The plan runs no project code.
#[derive(Default, Deserialize, Serialize, JsonSchema)]
pub struct MutationPlanParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Base ref for the default changed-code scope. Omitted: the base is
    /// detected as for `audit`.
    pub changed_since: Option<String>,

    /// Plan every source file instead of the changed code.
    pub all: Option<bool>,

    /// Plan only these files or directories, relative to the root.
    pub paths: Option<Vec<String>>,

    /// Maximum number of mutants to plan; the rest are `pending`. A run fills
    /// the limit with covered mutants, so it can also run a `pending` one.
    pub max_mutants: Option<usize>,

    /// Kebab-case operator names, for example `relational-operator`.
    /// Omitted: the `mutation.operators` config key, else every operator.
    pub operators: Option<Vec<String>>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// The Cargo package of a Rust workspace with more than one package.
    /// Valid only on a Rust workspace, where the plan covers the whole package
    /// unless `paths` or `changed_since` is set.
    pub package: Option<String>,
}

/// Parameters for one mutant of a saved mutation report.
#[derive(Deserialize, Serialize, JsonSchema)]
pub struct MutationInspectParams {
    /// The `mutant_id` from a mutation plan or report.
    #[schemars(length(min = 1))]
    pub mutant_id: String,

    /// Path to a saved `fallow mutation` report or `mutation_plan` output.
    /// A relative path resolves against `root`. Without `root`, it resolves
    /// against the working directory of the MCP server. Omitted: the report
    /// that the last `fallow mutation` run saved in the Fallow cache of the
    /// project.
    #[schemars(length(min = 1))]
    pub report: Option<String>,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file. Used only without `report`, to find the
    /// cache directory that holds the report of the last run.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Used only
    /// without `report`, as for `config`. Defaults to false and never grants
    /// process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// The Cargo package of a Rust workspace with more than one package.
    /// Valid only on a Rust workspace.
    pub package: Option<String>,
}

/// Parameters for the open tasks of a saved mutation report.
#[derive(Default, Deserialize, Serialize, JsonSchema)]
pub struct MutationTasksParams {
    /// Path to a saved `fallow mutation --format json` report. A relative
    /// path resolves against `root`. Without `root`, it resolves against the
    /// working directory of the MCP server. Omitted: the report that the last
    /// `fallow mutation` run saved in the Fallow cache of the project.
    #[schemars(length(min = 1))]
    pub report: Option<String>,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file. Used only without `report`, to find the
    /// cache directory that holds the report of the last run.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Used only
    /// without `report`, as for `config`. Defaults to false and never grants
    /// process-global trust.
    pub allow_remote_extends: Option<bool>,
}
