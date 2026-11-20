//! The subcommands of `fallow coverage`.

use std::path::PathBuf;

#[derive(clap::Subcommand)]
pub enum CoverageCli {
    /// Resumable first-run setup: license, runtime coverage add-on, recipe, analysis.
    Setup {
        #[command(flatten)]
        args: crate::coverage::CoverageSetupCli,
    },
    /// Analyze runtime coverage from a local artifact or explicit cloud source.
    ///
    /// Cloud mode is opt-in only. `FALLOW_API_KEY` by itself never selects
    /// cloud mode; pass `--cloud` / `--runtime-coverage-cloud`, or set
    /// `FALLOW_RUNTIME_COVERAGE_SOURCE=cloud`.
    Analyze {
        /// File or directory containing local runtime coverage input. A
        /// single local capture is free. Continuous or multi-capture
        /// monitoring needs a license (see `fallow license`).
        #[arg(
            long,
            value_name = "PATH",
            conflicts_with = "cloud",
            help_heading = "Input"
        )]
        runtime_coverage: Option<PathBuf>,

        /// Fetch latest runtime facts from Fallow Cloud for the selected repo.
        #[arg(long, visible_alias = "runtime-coverage-cloud", help_heading = "Input")]
        cloud: bool,

        /// Fallow Cloud API key. Order: flag > $FALLOW_API_KEY > `fallow cloud login` key.
        #[arg(long, value_name = "KEY", help_heading = "Fallow Cloud")]
        api_key: Option<String>,

        /// Override the Fallow Cloud base URL.
        #[arg(long, value_name = "URL", help_heading = "Fallow Cloud")]
        api_endpoint: Option<String>,

        /// Repository identifier, for example `owner/repo`.
        ///
        /// Defaults to $FALLOW_REPO, then the parsed origin URL from
        /// `git remote get-url origin`. Slashes are percent-encoded as one
        /// URL segment when calling the cloud runtime-context endpoint.
        #[arg(long, value_name = "OWNER/REPO", help_heading = "Fallow Cloud")]
        repo: Option<String>,

        /// Optional monorepo/project disambiguator.
        #[arg(long, value_name = "ID", help_heading = "Fallow Cloud")]
        project_id: Option<String>,

        /// Runtime observation window to request from cloud (1..=90 days).
        #[arg(
            long,
            value_name = "DAYS",
            default_value_t = 30,
            help_heading = "Fallow Cloud"
        )]
        coverage_period: u16,

        /// Optional runtime environment filter.
        #[arg(long, value_name = "ENV", help_heading = "Fallow Cloud")]
        environment: Option<String>,

        /// Optional commit SHA filter for cloud runtime facts.
        #[arg(long, value_name = "SHA", help_heading = "Fallow Cloud")]
        commit_sha: Option<String>,

        /// Analyze production code only.
        #[arg(long, help_heading = "Detection")]
        production: bool,

        /// Threshold for hot-path classification.
        #[arg(long, default_value_t = 100, help_heading = "Detection")]
        min_invocations_hot: u64,

        /// Minimum total trace volume before high-confidence verdicts.
        #[arg(long, value_name = "N", help_heading = "Detection")]
        min_observation_volume: Option<u32>,

        /// Fraction of total trace count below which an invoked function is low traffic.
        #[arg(long, value_name = "RATIO", help_heading = "Detection")]
        low_traffic_threshold: Option<f64>,

        /// Show only the top N runtime findings and hot paths.
        #[arg(long, help_heading = "Output")]
        top: Option<usize>,

        /// Show the first-class blast-radius section in human output.
        #[arg(long, help_heading = "Output")]
        blast_radius: bool,

        /// Show the first-class importance section in human output.
        #[arg(long, help_heading = "Output")]
        importance: bool,

        /// List cloud runtime functions with no local counterpart on stderr.
        #[arg(long, help_heading = "Output")]
        debug_unmatched: bool,
    },
    /// Upload a static function inventory to Fallow Cloud. Needs a fallow
    /// cloud API key. Unlocks the `untracked` filter on the dashboard by
    /// pairing runtime coverage data with the AST view of "every function
    /// that exists". See `https://fallow.tools/docs/analysis/runtime-coverage/`.
    ///
    /// This command makes network calls to Fallow Cloud. `fallow dead-code`
    /// stays offline.
    ///
    /// Exit codes: 0 ok · 7 network · 10 validation · 11 payload too large
    /// · 12 auth rejected · 13 server error.
    UploadInventory {
        /// Fallow Cloud API key (bearer token).
        ///
        /// Precedence: this flag > $FALLOW_API_KEY. Generate at
        /// `https://fallow.cloud/settings#api-keys`.
        ///
        /// Security: prefer $FALLOW_API_KEY on shared CI runners. Passing a
        /// secret on the command line may be visible to other processes via
        /// `ps` and can leak into shell history or process audit logs.
        #[arg(long, value_name = "KEY", help_heading = "Fallow Cloud")]
        api_key: Option<String>,

        /// Override the Fallow Cloud base URL.
        ///
        /// Useful for staging and on-premise deployments. Also respects
        /// $FALLOW_API_URL when this flag is not set.
        #[arg(long, value_name = "URL", help_heading = "Fallow Cloud")]
        api_endpoint: Option<String>,

        /// Project identifier, for example `fallow-cloud-api` or `owner/repo`.
        ///
        /// Defaults to $GITHUB_REPOSITORY, then $CI_PROJECT_PATH, then the
        /// parsed origin URL from `git remote get-url origin`.
        #[arg(long, value_name = "PROJECT_ID", help_heading = "Fallow Cloud")]
        project_id: Option<String>,

        /// Explicit git SHA for this inventory.
        ///
        /// Default: `git rev-parse HEAD`. The inventory is keyed on this
        /// value; the cloud back-fills hourly buckets with a matching SHA.
        #[arg(long, value_name = "SHA", help_heading = "Fallow Cloud")]
        git_sha: Option<String>,

        /// Bind the upload to a Vercel deployment. Defaults to $VERCEL_DEPLOYMENT_ID
        /// only when $VERCEL=1. With Vercel OIDC authentication, the cloud verifies
        /// the deployment against the supplied git SHA.
        #[arg(long, value_name = "DEPLOYMENT_ID", help_heading = "Fallow Cloud")]
        vercel_deployment_id: Option<String>,

        /// Additional glob patterns to exclude from the walk.
        ///
        /// Applied after the existing fallow ignore rules. Repeatable.
        #[arg(long, value_name = "GLOB", num_args = 0.., help_heading = "Input")]
        exclude_paths: Vec<String>,

        /// Prefix for inventory paths to match the runtime beacon, for example
        /// `/app` for Docker or `/var/task` for Lambda. Defaults to repo-relative
        /// paths. Must start with `/` and use POSIX separators.
        #[arg(long, value_name = "PREFIX", help_heading = "Input")]
        path_prefix: Option<String>,

        /// Print what would be uploaded and exit. No network call.
        #[arg(long, help_heading = "Run")]
        dry_run: bool,

        /// Also upload importer edges (which files import each function) and the
        /// framework entry routes that can statically reach each function. Opt-in:
        /// this builds the import graph by running the full static analysis, whereas
        /// the default upload is a fast per-file walk. The graph is cached, so a CI
        /// step that already ran analysis pays little extra.
        #[arg(long, help_heading = "Input")]
        with_callers: bool,

        /// Proceed even when the working tree has uncommitted changes.
        ///
        /// Warning: the inventory is generated from the working copy, so it
        /// may not match the uploaded git SHA. Commit or stash first if you
        /// want a SHA-exact upload.
        #[arg(long, help_heading = "Run")]
        allow_dirty: bool,

        /// Treat transient upload failures as warnings instead of errors
        /// (exit 0). Validation and auth errors still fail hard; this only
        /// downgrades transport and server errors.
        #[arg(long, help_heading = "Run")]
        ignore_upload_errors: bool,
    },
    /// Upload JavaScript source maps to Fallow Cloud for bundled runtime coverage.
    ///
    /// Uploads build `.map` files under the selected repo and git SHA so the
    /// cloud can resolve bundled runtime paths to original sources. Run from
    /// the repo root to preserve each map's repo-relative path and source base.
    UploadSourceMaps {
        /// Directory to scan recursively for source maps.
        #[arg(
            long,
            value_name = "PATH",
            default_value = "dist",
            help_heading = "Input"
        )]
        dir: PathBuf,

        /// Glob pattern, relative to --dir, selecting maps to upload.
        #[arg(
            long,
            value_name = "GLOB",
            default_value = "**/*.map",
            help_heading = "Input"
        )]
        include: String,

        /// Glob pattern, relative to --dir, selecting files to skip.
        ///
        /// Repeatable. Defaults to `**/node_modules/**`.
        #[arg(
            long,
            value_name = "GLOB",
            default_value = "**/node_modules/**",
            help_heading = "Input"
        )]
        exclude: Vec<String>,

        /// Repo name used in the API path.
        ///
        /// Defaults to package.json repository.url, then `git remote get-url origin`.
        #[arg(long, value_name = "NAME", help_heading = "Fallow Cloud")]
        repo: Option<String>,

        /// Commit SHA to key uploads under.
        ///
        /// Defaults to $GITHUB_SHA, $CI_COMMIT_SHA, $COMMIT_SHA, then
        /// `git rev-parse HEAD`.
        #[arg(long, value_name = "SHA", help_heading = "Fallow Cloud")]
        git_sha: Option<String>,

        /// Bind the upload to a Vercel deployment. Defaults to $VERCEL_DEPLOYMENT_ID
        /// only when $VERCEL=1. With Vercel OIDC authentication, the cloud verifies
        /// the deployment against the supplied git SHA.
        #[arg(long, value_name = "DEPLOYMENT_ID", help_heading = "Fallow Cloud")]
        vercel_deployment_id: Option<String>,

        /// Override the Fallow Cloud base URL.
        #[arg(long, value_name = "URL", help_heading = "Fallow Cloud")]
        endpoint: Option<String>,

        /// Send only the basename as fileName by default.
        ///
        /// Use `--strip-path=false` when your runtime coverage reports bundle
        /// paths relative to the build directory, such as `assets/app.js`.
        /// When two maps share a basename, each of them is sent under its path
        /// in the build directory, so that no map replaces another.
        #[arg(long, value_name = "BOOL", default_value_t = true, action = clap::ArgAction::Set, help_heading = "Input")]
        strip_path: bool,

        /// Print what would be uploaded and exit. No network call.
        #[arg(long, help_heading = "Run")]
        dry_run: bool,

        /// Parallel upload fanout.
        #[arg(long, value_name = "N", default_value_t = 4, help_heading = "Run")]
        concurrency: usize,

        /// Stop on first upload error.
        #[arg(long, help_heading = "Run")]
        fail_fast: bool,
    },
    /// Upload static dead-code findings to Fallow Cloud for the source-evidence viewer.
    ///
    /// Runs fallow's static analysis and uploads the `unused_export` and
    /// `dead_file` verdicts under the selected repo + git SHA. The cloud
    /// overlays them on the source view alongside the runtime coverage overlay.
    /// Findings are replace-by-SHA: each run sends the complete set for the SHA.
    UploadStaticFindings {
        /// Fallow Cloud API key (bearer token).
        ///
        /// Precedence: this flag > $FALLOW_API_KEY. Generate at
        /// `https://fallow.cloud/settings#api-keys`. This must be a live API
        /// key, not a publishable ingest key.
        ///
        /// Security: prefer $FALLOW_API_KEY on shared CI runners. Passing a
        /// secret on the command line may be visible to other processes via
        /// `ps` and can leak into shell history or process audit logs.
        #[arg(long, value_name = "KEY", help_heading = "Fallow Cloud")]
        api_key: Option<String>,

        /// Override the Fallow Cloud base URL.
        ///
        /// Useful for staging and on-premise deployments. Also respects
        /// $FALLOW_API_URL when this flag is not set.
        #[arg(long, value_name = "URL", help_heading = "Fallow Cloud")]
        api_endpoint: Option<String>,

        /// Project identifier, for example `fallow-cloud-api` or `owner/repo`.
        ///
        /// Defaults to $GITHUB_REPOSITORY, then $CI_PROJECT_PATH, then the
        /// parsed origin URL from `git remote get-url origin`.
        #[arg(long, value_name = "PROJECT_ID", help_heading = "Fallow Cloud")]
        project_id: Option<String>,

        /// Explicit git SHA for these findings.
        ///
        /// Default: `git rev-parse HEAD`. Findings are keyed on this value and
        /// fully replace any prior set uploaded for the same SHA.
        #[arg(long, value_name = "SHA", help_heading = "Fallow Cloud")]
        git_sha: Option<String>,

        /// Proceed even when the working tree has uncommitted changes.
        ///
        /// Warning: findings are generated from the working copy, so they may
        /// not match the uploaded git SHA. Commit or stash first if you want a
        /// SHA-exact upload.
        #[arg(long, help_heading = "Run")]
        allow_dirty: bool,

        /// Print what would be uploaded and exit. No network call.
        #[arg(long, help_heading = "Run")]
        dry_run: bool,

        /// Treat transient upload failures as warnings instead of errors
        /// (exit 0). Validation and auth errors still fail hard; this only
        /// downgrades transport and server errors.
        #[arg(long, help_heading = "Run")]
        ignore_upload_errors: bool,
    },
    /// Read the production facts of changed code from Fallow Cloud.
    ///
    /// Sends the changed files or functions to the cloud review-packet
    /// endpoint and prints the answer as JSON: per function the production
    /// calls, the tracking state and the evidence window. It runs no local
    /// analysis and pulls no full runtime context, so the answer stays small.
    /// Without --file and --function, the files changed against the base are
    /// sent. The base resolves like `fallow audit`: `--base` (alias of
    /// `--changed-since`), then $FALLOW_AUDIT_BASE, then the merge-base with
    /// the upstream or the remote default branch.
    ReviewPacket {
        /// Fallow Cloud API key. Order: flag > $FALLOW_API_KEY > `fallow cloud login` key.
        #[arg(long, value_name = "KEY", help_heading = "Fallow Cloud")]
        api_key: Option<String>,

        /// Override the Fallow Cloud base URL.
        #[arg(long, value_name = "URL", help_heading = "Fallow Cloud")]
        api_endpoint: Option<String>,

        /// Repository identifier, for example `owner/repo`.
        ///
        /// Defaults to $FALLOW_REPO, then the parsed origin URL from
        /// `git remote get-url origin`.
        #[arg(long, value_name = "OWNER/REPO", help_heading = "Fallow Cloud")]
        repo: Option<String>,

        /// Optional monorepo/project disambiguator.
        #[arg(long, value_name = "ID", help_heading = "Fallow Cloud")]
        project_id: Option<String>,

        /// Runtime observation window (1..=90 days). The cloud default is 30.
        #[arg(long, value_name = "DAYS", help_heading = "Fallow Cloud")]
        coverage_period: Option<u16>,

        /// Optional commit SHA of the deployment to read.
        #[arg(long, value_name = "SHA", help_heading = "Fallow Cloud")]
        commit_sha: Option<String>,

        /// Repo-relative file to include. Repeatable.
        #[arg(long = "file", value_name = "PATH", help_heading = "Scope")]
        files: Vec<String>,

        /// Function to include, as FILE:NAME or FILE:NAME:LINE. Repeatable.
        #[arg(
            long = "function",
            value_name = "FILE:NAME[:LINE]",
            help_heading = "Scope"
        )]
        functions: Vec<String>,
    },
    /// Read how production behavior changed between two deployments.
    ///
    /// Prints the cloud deployment change report as JSON: per function the
    /// change kind (new_called, new_not_called, heated_up, cooled_down,
    /// stopped, unchanged) between the deployment and its base. Pass the
    /// base deployment commit with `--base <sha>`; without it, the cloud
    /// picks the previous deployment with production runtime.
    DeploymentChanges {
        /// Fallow Cloud API key. Order: flag > $FALLOW_API_KEY > `fallow cloud login` key.
        #[arg(long, value_name = "KEY", help_heading = "Fallow Cloud")]
        api_key: Option<String>,

        /// Override the Fallow Cloud base URL.
        #[arg(long, value_name = "URL", help_heading = "Fallow Cloud")]
        api_endpoint: Option<String>,

        /// Repository identifier, for example `owner/repo`.
        ///
        /// Defaults to $FALLOW_REPO, then the parsed origin URL from
        /// `git remote get-url origin`.
        #[arg(long, value_name = "OWNER/REPO", help_heading = "Fallow Cloud")]
        repo: Option<String>,

        /// Commit SHA of the deployment. Default: `git rev-parse HEAD`.
        #[arg(long, value_name = "SHA", help_heading = "Fallow Cloud")]
        sha: Option<String>,

        /// Show only one change kind.
        #[arg(long, value_name = "KIND", help_heading = "Output", value_parser = [
            "stopped", "new_not_called", "heated_up", "cooled_down", "new_called", "unchanged",
        ])]
        change: Option<String>,

        /// Page size (1..=200).
        #[arg(long, value_name = "N", help_heading = "Output")]
        limit: Option<u16>,

        /// Cursor from `meta.cursor` of the previous page.
        #[arg(long, value_name = "CURSOR", help_heading = "Output")]
        cursor: Option<String>,
    },
}
