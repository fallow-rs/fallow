//! `fallow coverage setup --cloud`: a Fallow Cloud plan on the setup recipe
//! engine, and `--wait`, which polls the activation endpoint until the cloud
//! reports every setup step as done.
//!
//! Fallow Cloud joins runtime coverage to the static inventory only when the
//! beacon, the inventory, the source maps and the deployment registration
//! carry the same commit SHA. The plan therefore emits one SHA source per
//! step: the platform env in the beacon snippet, `$GITHUB_SHA` in CI. It names
//! environment variables for keys, never a key value. The plan writes nothing.

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use fallow_output::{
    CoverageSetupCi, CoverageSetupCiProvider, CoverageSetupCloud, CoverageSetupSourceMaps,
    CoverageSetupWait, CoverageSetupWaitOutcome,
};

use super::cloud_client::{CloudError, map_http_failure};
use super::cloud_transport::{self, CloudAuth, CloudBody, CloudOutcome};
use super::upload_common::{resolve_api_key, resolve_git_sha, url_encode_path_segment};
use super::{
    CoverageSetupContext, FrameworkKind, SetupSnippet, SetupTarget, build_setup_envelope,
    detect_setup_members, display_member_path, member_path_prefix, prefixed_member_path,
};
use crate::api::{DEFAULT_API_URL, api_url};

/// Environment variable that holds the full API key in CI and on the server.
const API_KEY_ENV: &str = "FALLOW_API_KEY";
/// Build-time variable that holds the ingest-only browser key.
const BROWSER_KEY_ENV: &str = "VITE_FALLOW_API_KEY";
/// Build-time variable that holds the commit SHA of a browser bundle.
const BROWSER_COMMIT_SHA_ENV: &str = "VITE_FALLOW_COMMIT_SHA";
/// Runtime variables that hold the deployed commit SHA, in the order the
/// server snippets read them: an explicit value first, then the platforms that
/// set one on their own.
const COMMIT_SHA_ENV: [&str; 4] = [
    "FALLOW_COMMIT_SHA",
    "VERCEL_GIT_COMMIT_SHA",
    "RENDER_GIT_COMMIT",
    "RAILWAY_GIT_COMMIT_SHA",
];
/// Placeholder when the project id cannot be detected. Plain characters, so a
/// pasted command fails on the project id and not on shell syntax.
const PROJECT_ID_PLACEHOLDER: &str = "OWNER/REPO";
const POLL_INTERVAL: Duration = Duration::from_secs(10);
const ACTIVATION_OPERATION: &str = "activation";
/// Exit code when `--wait` times out before every step is done.
const WAIT_TIMEOUT_EXIT_CODE: u8 = 1;

/// Arguments for `fallow coverage setup --cloud`.
#[derive(Debug, Clone, Default)]
pub struct CloudSetupArgs {
    /// Emit the plan as JSON.
    pub json: bool,
    /// Include field definitions in JSON output.
    pub explain: bool,
    /// Poll the activation endpoint until every step is done.
    pub wait: bool,
    /// Longest wait in seconds.
    pub wait_timeout_secs: u64,
    /// Explicit project id. Overrides the CI env and the git remote.
    pub project_id: Option<String>,
    /// Explicit commit SHA. Overrides `git rev-parse HEAD`.
    pub git_sha: Option<String>,
    /// Explicit API key for `--wait`. Overrides `$FALLOW_API_KEY`.
    pub api_key: Option<String>,
    /// Explicit API base URL. Overrides `$FALLOW_API_URL`.
    pub api_endpoint: Option<String>,
}

/// Run `fallow coverage setup --cloud`.
pub(super) fn run(
    args: &CloudSetupArgs,
    root: &Path,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let inputs = match resolve_inputs(args, root) {
        Ok(inputs) => inputs,
        Err(message) => {
            eprintln!("fallow coverage setup --cloud: {message}");
            return ExitCode::from(2);
        }
    };
    let mut envelope = build_cloud_envelope(root, args.explain, &inputs);
    if !args.wait {
        return print_and_exit(&envelope, args.json, json_style, ExitCode::SUCCESS);
    }
    // A person reads the plan while the wait runs; JSON prints once at the end.
    if !args.json {
        print_human_plan(&envelope);
    }
    let Some(cloud) = envelope.cloud.as_mut() else {
        return ExitCode::from(2);
    };
    let wait = wait_for_activation(args, cloud, !args.json);
    let exit = wait_exit_code(&wait);
    if !args.json {
        print_wait_result(&wait);
        return exit;
    }
    cloud.wait = Some(wait);
    print_and_exit(&envelope, true, json_style, exit)
}

/// The validated project id, commit SHA and endpoint of one run.
struct CloudInputs {
    project_id: Option<String>,
    git_sha: Option<String>,
    endpoint: String,
}

fn resolve_inputs(args: &CloudSetupArgs, root: &Path) -> Result<CloudInputs, String> {
    let project_id = detect_project_id(args.project_id.as_deref(), root)?;
    // An explicit SHA must be valid; HEAD is a best-effort default.
    let git_sha = match args.git_sha.as_deref() {
        Some(explicit) => Some(resolve_git_sha(Some(explicit), root)?),
        None => resolve_git_sha(None, root).ok(),
    };
    let endpoint = args.api_endpoint.as_deref().map_or_else(
        || api_url(""),
        |base| base.trim().trim_end_matches('/').to_owned(),
    );
    validate_endpoint(&endpoint)?;
    Ok(CloudInputs {
        project_id,
        git_sha,
        endpoint,
    })
}

/// The endpoint lands in a shell command, so it must be a plain http or https
/// URL: no quote, whitespace, `$`, backtick or backslash.
fn validate_endpoint(endpoint: &str) -> Result<(), String> {
    let has_scheme = endpoint.starts_with("https://") || endpoint.starts_with("http://");
    let plain = endpoint.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'.' | b':' | b'/' | b'_' | b'-' | b'~' | b'%' | b'[' | b']'
            )
    });
    if has_scheme && plain {
        Ok(())
    } else {
        Err(format!(
            "API endpoint '{endpoint}' must be an http or https URL of [A-Za-z0-9.:/_~%-] characters"
        ))
    }
}

/// A build path that is safe in a shell word and a YAML scalar: segments of
/// `[A-Za-z0-9@._-]` joined by `/`, no `..`, and no leading `-`.
fn is_safe_build_path(path: &str) -> bool {
    is_valid_project_id(path)
}

fn print_and_exit(
    envelope: &fallow_output::CoverageSetupOutput,
    json: bool,
    json_style: crate::json_style::JsonStyle,
    exit: ExitCode,
) -> ExitCode {
    if !json {
        print_human_plan(envelope);
        return exit;
    }
    let payload = match fallow_output::serialize_coverage_setup_json_output(
        envelope.clone(),
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    ) {
        Ok(payload) => payload,
        Err(err) => {
            eprintln!("fallow coverage setup --cloud: failed to build JSON output: {err}");
            return ExitCode::from(2);
        }
    };
    match json_style.serialize(&payload) {
        Ok(text) => {
            println!("{text}");
            exit
        }
        Err(err) => {
            eprintln!("fallow coverage setup --cloud: failed to write JSON output: {err}");
            ExitCode::from(2)
        }
    }
}

/// The project id from the flag, the CI env or the git remote. An explicit id
/// must pass the server repo rule; a detected id that does not is dropped, so
/// no snippet or command carries a value the cloud would refuse.
fn detect_project_id(explicit: Option<&str>, root: &Path) -> Result<Option<String>, String> {
    if let Some(explicit) = explicit {
        let trimmed = explicit.trim();
        return if is_valid_project_id(trimmed) {
            Ok(Some(trimmed.to_owned()))
        } else {
            Err(format!(
                "project id '{trimmed}' must be segments of [A-Za-z0-9@._-] separated by '/', without '..'"
            ))
        };
    }
    Ok(super::upload_common::resolve_project_id(None, root)
        .ok()
        .filter(|id| is_valid_project_id(id)))
}

/// The repo rule of the Fallow Cloud API: segments of `[A-Za-z0-9@._-]`
/// joined by `/`, no `..`, and no leading `-` (a value that starts with `-`
/// would read as a flag in the emitted commands).
pub(super) fn is_valid_project_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 255
        && !id.contains("..")
        && !id.starts_with('-')
        && id.split('/').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'@' | b'.' | b'_' | b'-'))
        })
}

fn build_cloud_envelope(
    root: &Path,
    explain: bool,
    inputs: &CloudInputs,
) -> fallow_output::CoverageSetupOutput {
    let project_id = inputs.project_id.as_deref();
    let target = SetupTarget::Cloud {
        project_id: project_id.unwrap_or(PROJECT_ID_PLACEHOLDER),
        surface: None,
    };
    let mut envelope = build_setup_envelope(root, explain, target);
    envelope.commands.truncate(1);
    envelope.next_steps = cloud_next_steps();
    if project_id.is_none() {
        envelope.warnings.push(format!(
            "Project id was not detected; the plan uses {PROJECT_ID_PLACEHOLDER}. Pass --project-id <owner/repo>."
        ));
    }
    let (cloud, unsafe_dirs) = cloud_plan(root, inputs);
    for dir in unsafe_dirs {
        envelope.warnings.push(format!(
            "The build directory '{dir}' has characters that are not safe in a shell command; add its source map upload by hand."
        ));
    }
    envelope.cloud = Some(cloud);
    envelope
}

fn cloud_next_steps() -> Vec<String> {
    vec![
        "Install @fallow-cli/beacon and add the snippets to your application.".to_owned(),
        format!(
            "Create a full API key in the Fallow Cloud dashboard and store it as the CI secret and server env var {API_KEY_ENV}."
        ),
        "Add the cloud.ci.steps to the CI job that builds and deploys. Every step uses the same commit SHA.".to_owned(),
        "Deploy, send production traffic, then run the cloud.wait_command until it reports every step as done.".to_owned(),
    ]
}

/// The cloud plan, plus the build directories left out because they are not
/// safe in a shell command.
fn cloud_plan(root: &Path, inputs: &CloudInputs) -> (CoverageSetupCloud, Vec<String>) {
    let project_id = inputs.project_id.as_deref();
    let endpoint = inputs.endpoint.as_str();
    let members = detect_setup_members(root);
    // Only a Vite browser app gets a browser snippet. A meta-framework gets the
    // server snippet only, so it needs no browser key or build variable.
    let has_browser = members
        .iter()
        .any(|member| member.context.framework == FrameworkKind::ViteBrowser);
    let (source_maps, unsafe_dirs) = source_map_plan(root, &members);
    let project = project_id.unwrap_or(PROJECT_ID_PLACEHOLDER);
    let mut wait_command = "fallow coverage setup --cloud --wait".to_owned();
    if project_id.is_some() {
        let _ = write!(wait_command, " --project-id={project}");
    }
    if endpoint != DEFAULT_API_URL {
        let _ = write!(wait_command, " --api-endpoint={endpoint}");
    }
    wait_command.push_str(" --git-sha=\"$DEPLOYED_SHA\"");
    let cloud = CoverageSetupCloud {
        project_id: project_id.map(str::to_owned),
        git_sha: inputs.git_sha.clone(),
        endpoint: endpoint.to_owned(),
        api_key_env: API_KEY_ENV.to_owned(),
        browser_key_env: has_browser.then(|| BROWSER_KEY_ENV.to_owned()),
        commit_sha_env: COMMIT_SHA_ENV
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        ci: CoverageSetupCi {
            provider: CoverageSetupCiProvider::GithubActions,
            workflows_detected: detect_deploy_workflows(root),
            steps: github_actions_steps(project, endpoint, &source_maps, has_browser),
            instructions: ci_instructions(has_browser),
        },
        source_maps,
        activation_url: project_id.map(|id| activation_url(endpoint, id, None)),
        wait_command,
        wait: None,
    };
    (cloud, unsafe_dirs)
}

/// The build output directory of a framework, relative to its member root.
const fn build_output_dir(framework: FrameworkKind) -> Option<&'static str> {
    match framework {
        FrameworkKind::ViteBrowser | FrameworkKind::Astro | FrameworkKind::NestJs => Some("dist"),
        FrameworkKind::NextJs => Some(".next"),
        FrameworkKind::Nuxt => Some(".output"),
        FrameworkKind::SvelteKit => Some(".svelte-kit/output"),
        FrameworkKind::Remix => Some("build"),
        FrameworkKind::PlainNode | FrameworkKind::Other => None,
    }
}

fn source_map_plan(
    root: &Path,
    members: &[super::CoverageSetupMember],
) -> (CoverageSetupSourceMaps, Vec<String>) {
    let (dirs, unsafe_dirs): (Vec<String>, Vec<String>) = members
        .iter()
        .filter_map(|member| {
            let dir = build_output_dir(member.context.framework)?;
            let prefix = member_path_prefix(&display_member_path(root, &member.root));
            Some(prefixed_member_path(&prefix, dir))
        })
        .partition(|dir| is_safe_build_path(dir));
    let bundled = !(dirs.is_empty() && unsafe_dirs.is_empty());
    (source_maps_for(dirs, bundled), unsafe_dirs)
}

fn source_maps_for(dirs: Vec<String>, bundled: bool) -> CoverageSetupSourceMaps {
    if !bundled {
        return CoverageSetupSourceMaps {
            needed: false,
            dirs,
            reason: "No bundler was detected. A runtime that runs the source files needs no source maps; add the upload when the build transpiles the code.".to_owned(),
        };
    }
    CoverageSetupSourceMaps {
        needed: true,
        dirs,
        reason: "The production build bundles or transpiles the code. Make the build emit source maps, so runtime positions map back to the source files.".to_owned(),
    }
}

/// The runtime surface tag of one member. A single-member project keeps the
/// default tag of its runtime (`server` or `browser`); in a workspace with
/// more members each member gets the name of its directory.
pub(super) fn member_surface(
    root: &Path,
    member: &super::CoverageSetupMember,
    member_count: usize,
) -> Option<String> {
    if member_count <= 1 || member.root == root {
        return None;
    }
    let name = member
        .root
        .file_name()?
        .to_string_lossy()
        .to_ascii_lowercase();
    let valid = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    valid.then_some(name)
}

/// Commands that deploy an app from a workflow step.
const DEPLOY_COMMANDS: [&str; 12] = [
    "flyctl deploy",
    "fly deploy",
    "wrangler deploy",
    "wrangler pages deploy",
    "vercel deploy",
    "vercel --prod",
    "netlify deploy",
    "actions/deploy-pages",
    "railway up",
    "gcloud run deploy",
    "amazon-ecs-deploy",
    "helm upgrade",
];

/// Workflow files that look like a deploy workflow, relative to the root: a
/// file named for a deploy, else a file that runs a known deploy command.
fn detect_deploy_workflows(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join(".github").join("workflows")) else {
        return Vec::new();
    };
    let mut named = Vec::new();
    let mut by_command = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_yaml = Path::new(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml"));
        if !is_yaml {
            continue;
        }
        let path = format!(".github/workflows/{name}");
        if name.to_ascii_lowercase().contains("deploy") {
            named.push(path);
        } else if std::fs::read_to_string(entry.path()).is_ok_and(|content| {
            let content = content.to_ascii_lowercase();
            DEPLOY_COMMANDS
                .iter()
                .any(|command| content.contains(command))
        }) {
            by_command.push(path);
        }
    }
    let mut workflows = if named.is_empty() { by_command } else { named };
    workflows.sort();
    workflows
}

fn ci_instructions(has_browser: bool) -> String {
    let mut text = format!(
        "Add the steps to the GitHub Actions job that builds and deploys, around its build and deploy steps. \
         Store a full API key as the repository secret {API_KEY_ENV}. \
         The checkout needs fetch-depth: 0, because the inventory reads git history. \
         Every step uses $GITHUB_SHA, so the inventory, the source maps, the deployment and the beacon commitSha name one commit."
    );
    if has_browser {
        let _ = write!(
            text,
            " Store an ingest-only key (fallow_pub_k1_) as the secret {BROWSER_KEY_ENV} and pass it and {BROWSER_COMMIT_SHA_ENV} to the build step."
        );
    }
    let _ = write!(
        text,
        " A server runtime reads the SHA from {}: set it on the deploy target, for example with a Docker build arg.",
        COMMIT_SHA_ENV.join(", ")
    );
    text
}

fn github_actions_steps(
    project: &str,
    endpoint: &str,
    source_maps: &CoverageSetupSourceMaps,
    has_browser: bool,
) -> String {
    let fallow = format!("npx --yes fallow@{}", env!("CARGO_PKG_VERSION"));
    // The uploads and the registration must reach the same server.
    let (inventory_endpoint, maps_endpoint) = if endpoint == DEFAULT_API_URL {
        (String::new(), String::new())
    } else {
        (
            format!(" --api-endpoint={endpoint}"),
            format!(" --endpoint={endpoint}"),
        )
    };
    let mut steps = String::from(
        "- uses: actions/checkout@v4\n  with:\n    fetch-depth: 0 # the inventory reads git history\n",
    );
    if has_browser {
        let _ = write!(
            steps,
            "# On the build step, add:\n#   env:\n#     {BROWSER_COMMIT_SHA_ENV}: ${{{{ github.sha }}}}\n#     {BROWSER_KEY_ENV}: ${{{{ secrets.{BROWSER_KEY_ENV} }}}}\n"
        );
    }
    let _ = write!(
        steps,
        "# After the build step:\n- name: Upload the Fallow Cloud inventory{maps}\n  env:\n    {API_KEY_ENV}: ${{{{ secrets.{API_KEY_ENV} }}}}\n  run: |\n    {fallow} coverage upload-inventory --project-id={project} --git-sha=\"$GITHUB_SHA\"{inventory_endpoint}\n",
        maps = if source_maps.needed {
            " and source maps"
        } else {
            ""
        },
    );
    for dir in &source_maps.dirs {
        let _ = writeln!(
            steps,
            "    {fallow} coverage upload-source-maps --dir={dir} --repo={project} --git-sha=\"$GITHUB_SHA\"{maps_endpoint}"
        );
    }
    let _ = write!(
        steps,
        "# After the deploy step:\n- name: Register the deployment with Fallow Cloud\n  env:\n    {API_KEY_ENV}: ${{{{ secrets.{API_KEY_ENV} }}}}\n  run: |\n    curl -fsS -X POST \"{endpoint}/v1/deployments\" \\\n      -H \"Authorization: Bearer ${API_KEY_ENV}\" \\\n      -H \"Content-Type: application/json\" \\\n      -d \"{{\\\"repo\\\":\\\"{project}\\\",\\\"gitSha\\\":\\\"$GITHUB_SHA\\\"}}\"\n"
    );
    steps
}

fn activation_url(endpoint: &str, project_id: &str, sha: Option<&str>) -> String {
    let base = format!(
        "{endpoint}/v1/repos/{}/activation",
        url_encode_path_segment(project_id)
    );
    match sha {
        Some(sha) => format!("{base}?sha={}", url_encode_path_segment(sha)),
        None => base,
    }
}

fn node_commit_sha_expr() -> String {
    COMMIT_SHA_ENV
        .iter()
        .map(|name| format!("process.env.{name}"))
        .collect::<Vec<_>>()
        .join(" ??\n    ")
}

/// The `createNodeBeacon` options for Fallow Cloud, indented by `indent`.
fn node_beacon_options(project_id: &str, surface: &str, indent: &str) -> String {
    let lines = [
        format!("apiKey: process.env.{API_KEY_ENV},"),
        format!("projectId: \"{project_id}\","),
        "endpoint: process.env.FALLOW_API_URL ?? \"https://api.fallow.cloud\",".to_owned(),
        format!(
            "commitSha:\n{indent}  {},",
            node_commit_sha_expr().replace('\n', &format!("\n{indent}"))
        ),
        "coverageOrigin: \"production\",".to_owned(),
        format!("runtimeSurface: \"{surface}\","),
    ];
    lines.iter().fold(String::new(), |mut out, line| {
        let _ = writeln!(out, "{indent}{line}");
        out
    })
}

fn node_beacon_module(project_id: &str, surface: &str) -> String {
    format!(
        "import {{ createNodeBeacon }} from \"@fallow-cli/beacon\";\n\nconst fallowBeacon = createNodeBeacon({{\n{}}});\n\nfallowBeacon.start();\n",
        node_beacon_options(project_id, surface, "  ")
    )
}

fn next_instrumentation(project_id: &str, surface: &str) -> String {
    format!(
        "export async function register() {{\n  if (process.env.NEXT_RUNTIME === \"nodejs\") {{\n    const {{ createNodeBeacon }} = await import(\"@fallow-cli/beacon\");\n    const beacon = createNodeBeacon({{\n{}    }});\n    beacon.start();\n  }}\n}}\n",
        node_beacon_options(project_id, surface, "      ")
    )
}

fn nuxt_server_plugin(project_id: &str, surface: &str) -> String {
    format!(
        "export default defineNitroPlugin(async () => {{\n  const {{ createNodeBeacon }} = await import(\"@fallow-cli/beacon\");\n  const beacon = createNodeBeacon({{\n{}  }});\n  beacon.start();\n}});\n",
        node_beacon_options(project_id, surface, "    ")
    )
}

fn browser_beacon_module(project_id: &str, surface: &str) -> String {
    format!(
        "import {{ createBrowserBeacon }} from \"@fallow-cli/beacon/browser\";\n\nconst fallowBeacon = createBrowserBeacon({{\n  apiKey: import.meta.env.{BROWSER_KEY_ENV},\n  projectId: \"{project_id}\",\n  endpoint: import.meta.env.VITE_FALLOW_API_URL ?? \"https://api.fallow.cloud\",\n  commitSha: import.meta.env.{BROWSER_COMMIT_SHA_ENV},\n  coverageOrigin: \"production\",\n  runtimeSurface: \"{surface}\",\n}});\n\nfallowBeacon.start();\n"
    )
}

fn snippet(label: &'static str, path: &str, reason: &'static str, content: String) -> SetupSnippet {
    SetupSnippet {
        label,
        path: path.to_owned(),
        reason,
        content,
    }
}

/// Beacon snippets for Fallow Cloud: the HTTP transport, the deployed commit
/// SHA and the production origin. The paths match the local recipe.
pub(super) fn cloud_snippets(
    context: &CoverageSetupContext,
    project_id: &str,
    surface: Option<&str>,
) -> Vec<SetupSnippet> {
    let server = surface.unwrap_or("server");
    let node = || node_beacon_module(project_id, server);
    match context.framework {
        FrameworkKind::NextJs => vec![snippet(
            "Next.js instrumentation",
            "instrumentation.ts",
            "Start the Node runtime beacon during Next.js startup, with the deployed commit SHA.",
            next_instrumentation(project_id, server),
        )],
        FrameworkKind::NestJs => vec![snippet(
            "NestJS bootstrap",
            "src/main.ts",
            "Start the Node runtime beacon before creating the Nest app.",
            node(),
        )],
        FrameworkKind::Nuxt => vec![snippet(
            "Nuxt server plugin",
            "server/plugins/fallow.ts",
            "Start the Node runtime beacon when the Nuxt server boots.",
            nuxt_server_plugin(project_id, server),
        )],
        FrameworkKind::SvelteKit => vec![snippet(
            "SvelteKit server hook",
            "src/hooks.server.ts",
            "Start the Node runtime beacon before handling server requests.",
            node(),
        )],
        FrameworkKind::Astro => vec![snippet(
            "Astro middleware",
            "src/middleware.ts",
            "Start the Node runtime beacon from the server middleware module.",
            node(),
        )],
        FrameworkKind::Remix => vec![snippet(
            "Remix server entry",
            "app/entry.server.tsx",
            "Start the Node runtime beacon from the server entry module.",
            node(),
        )],
        FrameworkKind::ViteBrowser => vec![snippet(
            "Vite browser entry",
            "src/main.ts",
            "Start the browser runtime beacon from the client entry module, with the commit SHA of the build.",
            browser_beacon_module(project_id, surface.unwrap_or("browser")),
        )],
        FrameworkKind::PlainNode | FrameworkKind::Other => vec![snippet(
            "Node entrypoint",
            &context.node_entry_path,
            "Start the Node runtime beacon before application code handles traffic.",
            node(),
        )],
    }
}

/// Dockerfile lines for a Node runtime: V8 coverage and the deployed SHA.
pub(super) fn cloud_dockerfile_snippet(context: &CoverageSetupContext) -> Option<String> {
    context
        .framework
        .runtime_targets()
        .contains(&"node")
        .then(|| {
            "# Build with: docker build --build-arg FALLOW_COMMIT_SHA=$GITHUB_SHA .\nARG FALLOW_COMMIT_SHA\nENV FALLOW_COMMIT_SHA=$FALLOW_COMMIT_SHA\nENV NODE_V8_COVERAGE=/tmp/fallow-v8-coverage".to_owned()
        })
}

/// Cloud caveats for one member.
pub(super) fn cloud_warnings(context: &CoverageSetupContext) -> Vec<String> {
    let targets = context.framework.runtime_targets();
    let mut warnings = Vec::new();
    if targets.contains(&"node") {
        warnings.push(
            "The Node beacon reads V8 coverage: start the server process with NODE_V8_COVERAGE set to a writable directory, or instrument the build with Istanbul.".to_owned(),
        );
    }
    if context.framework == FrameworkKind::ViteBrowser {
        warnings.push(
            "The browser beacon reads Istanbul counters: instrument the production build, for example with oxc-coverage-instrument.".to_owned(),
        );
    }
    warnings
}

fn wait_exit_code(wait: &CoverageSetupWait) -> ExitCode {
    match wait.outcome {
        CoverageSetupWaitOutcome::Complete => ExitCode::SUCCESS,
        CoverageSetupWaitOutcome::Timeout => ExitCode::from(WAIT_TIMEOUT_EXIT_CODE),
        CoverageSetupWaitOutcome::Error => ExitCode::from(2),
    }
}

fn wait_error(message: impl Into<String>) -> CoverageSetupWait {
    CoverageSetupWait {
        outcome: CoverageSetupWaitOutcome::Error,
        activation: None,
        message: Some(message.into()),
    }
}

/// Poll the activation endpoint until it reports `complete`, the timeout
/// passes, or a read fails for a reason a retry cannot fix.
fn wait_for_activation(
    args: &CloudSetupArgs,
    cloud: &CoverageSetupCloud,
    progress: bool,
) -> CoverageSetupWait {
    let Some(project_id) = cloud.project_id.as_deref() else {
        return wait_error("--wait needs a project id. Pass --project-id <owner/repo>.");
    };
    let Some(sha) = cloud.git_sha.as_deref() else {
        return wait_error("--wait needs a commit SHA. Pass --git-sha <sha>.");
    };
    let api_key = match resolve_api_key(args.api_key.as_deref()) {
        Ok(key) => key,
        Err(message) => return wait_error(message),
    };
    let auth = CloudAuth {
        api_key,
        agent_source: None,
    };
    let url = activation_url(&cloud.endpoint, project_id, Some(sha));
    let deadline = Instant::now() + Duration::from_secs(args.wait_timeout_secs);
    let mut last: Option<serde_json::Value> = None;
    let mut last_report = String::new();
    loop {
        match read_activation(&auth, &url, project_id) {
            Ok(data) => {
                if progress {
                    let report = progress_line(&data);
                    if report != last_report {
                        eprintln!("{report}");
                        last_report = report;
                    }
                }
                let complete = data.get("complete").and_then(serde_json::Value::as_bool);
                last = Some(data);
                if complete == Some(true) {
                    return CoverageSetupWait {
                        outcome: CoverageSetupWaitOutcome::Complete,
                        activation: last,
                        message: None,
                    };
                }
            }
            Err(ReadError::Retry(message)) => {
                if progress {
                    eprintln!("fallow coverage setup --cloud: {message}; retrying");
                }
            }
            Err(ReadError::Stop(err)) => {
                let mut wait = wait_error(err.message());
                wait.activation = last;
                return wait;
            }
        }
        let now = Instant::now();
        if now >= deadline {
            return CoverageSetupWait {
                outcome: CoverageSetupWaitOutcome::Timeout,
                activation: last,
                message: None,
            };
        }
        std::thread::sleep(POLL_INTERVAL.min(deadline - now));
    }
}

/// Why one activation read failed: a retry can help (network, rate limit,
/// server error), or it cannot (auth, scope, input, a malformed body).
enum ReadError {
    Retry(String),
    Stop(CloudError),
}

fn read_activation(
    auth: &CloudAuth,
    url: &str,
    project_id: &str,
) -> Result<serde_json::Value, ReadError> {
    let outcome = match cloud_transport::send(auth, url, &CloudBody::None, ACTIVATION_OPERATION) {
        Ok(outcome) => outcome,
        Err(CloudError::Network(message) | CloudError::Server(message)) => {
            return Err(ReadError::Retry(message));
        }
        Err(err) => return Err(ReadError::Stop(err)),
    };
    let failure = match outcome {
        CloudOutcome::Success(response) => return activation_data(&response.body),
        CloudOutcome::Http(failure) => failure,
    };
    if failure.status == 429 || failure.status >= 500 {
        return Err(ReadError::Retry(format!(
            "HTTP {}: {}",
            failure.status, failure.message
        )));
    }
    Err(ReadError::Stop(map_http_failure(
        &failure,
        ACTIVATION_OPERATION,
        project_id,
    )))
}

/// The `data` object of an activation response body.
fn activation_data(body: &str) -> Result<serde_json::Value, ReadError> {
    let malformed = |reason: String| ReadError::Stop(CloudError::Server(reason));
    let mut body: serde_json::Value = serde_json::from_str(body)
        .map_err(|err| malformed(format!("malformed activation response: {err}")))?;
    body.get_mut("data")
        .map(serde_json::Value::take)
        .filter(serde_json::Value::is_object)
        .ok_or_else(|| malformed("activation response has no data object".to_owned()))
}

/// One line per poll: each step and its state, then the next step.
fn progress_line(data: &serde_json::Value) -> String {
    let steps = data
        .get("steps")
        .and_then(serde_json::Value::as_array)
        .map(|steps| {
            steps
                .iter()
                .filter_map(|step| {
                    let id = step.get("id")?.as_str()?;
                    let state = step.get("state")?.as_str()?;
                    Some(format!("{id}={state}"))
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let next = data
        .get("next")
        .and_then(|next| next.get("message"))
        .and_then(serde_json::Value::as_str)
        .map(|message| format!("\n  next: {message}"))
        .unwrap_or_default();
    format!("activation: {steps}{next}")
}

fn print_human_plan(envelope: &fallow_output::CoverageSetupOutput) {
    println!("fallow coverage setup --cloud");
    println!();
    println!("1. Install the beacon:");
    for command in &envelope.commands {
        println!("   {command}");
    }
    println!();
    println!("2. Start the beacon with the deployed commit SHA:");
    for snippet in &envelope.snippets {
        println!("   {} ({}):", snippet.path, snippet.label);
        for line in snippet.content.lines() {
            println!("     {line}");
        }
    }
    if let Some(dockerfile) = &envelope.dockerfile_snippet {
        println!("   Dockerfile:");
        for line in dockerfile.lines() {
            println!("     {line}");
        }
    }
    let Some(cloud) = &envelope.cloud else {
        return;
    };
    println!();
    println!("3. Add these steps to the CI job that builds and deploys:");
    if !cloud.ci.workflows_detected.is_empty() {
        println!(
            "   Deploy workflows found: {}",
            cloud.ci.workflows_detected.join(", ")
        );
    }
    for line in cloud.ci.steps.lines() {
        println!("     {line}");
    }
    println!("   {}", cloud.ci.instructions);
    println!();
    println!("4. Source maps: {}", cloud.source_maps.reason);
    println!();
    println!("5. Deploy, send traffic, then check the setup:");
    println!("   {}", cloud.wait_command);
    for warning in &envelope.warnings {
        println!("warning: {warning}");
    }
}

fn print_wait_result(wait: &CoverageSetupWait) {
    println!();
    match wait.outcome {
        CoverageSetupWaitOutcome::Complete => println!("Setup is complete."),
        CoverageSetupWaitOutcome::Timeout => {
            println!("Setup is not complete yet. Run the wait command again later.");
        }
        CoverageSetupWaitOutcome::Error => println!(
            "Could not read the setup state: {}",
            wait.message.as_deref().unwrap_or("unknown error")
        ),
    }
}
