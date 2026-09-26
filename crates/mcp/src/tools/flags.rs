use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::params::FeatureFlagsParams;

use fallow_api::{
    AnalysisOptions, FeatureFlagsOptions, FeatureFlagsRetirementOptions,
    run_feature_flags as run_api_feature_flags, serialize_feature_flags_programmatic_json,
};
use fallow_types::flag_retirement::FlagAgeMode;
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use super::api_runtime::{
    env_changed_since, env_diff_file, json_success, non_empty_path, programmatic_error_body,
    run_api_blocking, workspace_patterns_from_param,
};
use super::push_remote_extends;

/// Run `feature_flags` through the typed API.
pub async fn run_feature_flags(
    _binary: &str,
    params: FeatureFlagsParams,
) -> Result<CallToolResult, McpError> {
    let options = match feature_flags_options_from_params(&params) {
        Ok(options) => options,
        Err(message) => return Ok(CallToolResult::error(vec![ContentBlock::text(message)])),
    };
    let result = run_api_blocking("feature_flags", move || {
        run_api_feature_flags(&options).and_then(serialize_feature_flags_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![ContentBlock::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

pub fn run_feature_flags_api_value(
    params: &FeatureFlagsParams,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<Option<serde_json::Value>, String> {
    let mut options = feature_flags_options_from_params(params)?;
    options.analysis.cancellation = cancellation;
    let value = run_api_feature_flags(&options)
        .and_then(serialize_feature_flags_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))?;

    Ok(Some(value))
}

/// Build CLI arguments for the `feature_flags` tool.
pub fn build_feature_flags_args(params: &FeatureFlagsParams) -> Vec<String> {
    let mut args = vec![
        "flags".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
        "--explain".to_string(),
    ];

    if let Some(ref root) = params.root {
        args.extend(["--root".to_string(), root.clone()]);
    }
    if let Some(ref config) = params.config {
        args.extend(["--config".to_string(), config.clone()]);
    }
    push_remote_extends(&mut args, params.allow_remote_extends);
    if params.production == Some(true) {
        args.push("--production".to_string());
    }
    if let Some(ref workspace) = params.workspace {
        args.extend(["--workspace".to_string(), workspace.clone()]);
    }
    if params.no_cache == Some(true) {
        args.push("--no-cache".to_string());
    }
    if let Some(threads) = params.threads {
        args.extend(["--threads".to_string(), threads.to_string()]);
    }
    if let Some(top) = params.top {
        args.extend(["--top".to_string(), top.to_string()]);
    }
    if params.retirement == Some(true) {
        args.push("--retirement".to_string());
        if let Some(ref flag_state) = params.flag_state {
            args.extend(["--flag-state".to_string(), flag_state.clone()]);
        }
        if let Some(ref flag_age) = params.flag_age {
            args.extend(["--flag-age".to_string(), flag_age.clone()]);
        }
    }

    args
}

/// Map the retirement parameters. `flag_state` and `flag_age` need
/// `retirement`, like `--flag-state` and `--flag-age` need `--retirement`.
fn retirement_options_from_params(
    params: &FeatureFlagsParams,
) -> Result<Option<FeatureFlagsRetirementOptions>, String> {
    if params.retirement != Some(true) {
        if params.flag_state.is_some() || params.flag_age.is_some() {
            return Err("flag_state and flag_age need retirement: true".to_string());
        }
        return Ok(None);
    }
    let flag_age = match params.flag_age.as_deref() {
        None | Some("blame") => FlagAgeMode::Blame,
        Some("pickaxe") => FlagAgeMode::Pickaxe,
        Some("off") => FlagAgeMode::Off,
        Some(other) => {
            return Err(format!(
                "invalid flag_age '{other}': use blame, pickaxe or off"
            ));
        }
    };
    Ok(Some(FeatureFlagsRetirementOptions {
        flag_age,
        flag_state: non_empty_path(params.flag_state.as_deref()).map(|path| {
            match non_empty_path(params.root.as_deref()) {
                Some(root) if path.is_relative() => root.join(path),
                _ => path,
            }
        }),
        ..FeatureFlagsRetirementOptions::default()
    }))
}

fn feature_flags_options_from_params(
    params: &FeatureFlagsParams,
) -> Result<FeatureFlagsOptions, String> {
    Ok(FeatureFlagsOptions {
        analysis: AnalysisOptions {
            root: non_empty_path(params.root.as_deref()),
            config_path: non_empty_path(params.config.as_deref()),
            allow_remote_extends: params.allow_remote_extends.unwrap_or(false),
            no_cache: params.no_cache == Some(true),
            threads: params.threads,
            ambient_diff_file: env_diff_file(),
            production: params.production == Some(true),
            production_override: params.production,
            ambient_changed_since: env_changed_since(),
            workspace: workspace_patterns_from_param(params.workspace.as_deref()),
            changed_workspaces: None,
            explain: true,
            type_aware: fallow_api::TypeAwareOptions::default(),
            ..AnalysisOptions::default()
        },
        top: params.top,
        retirement: retirement_options_from_params(params)?,
    })
}

#[cfg(test)]
mod tests {
    use rmcp::model::ContentBlock;

    use super::*;

    #[tokio::test]
    async fn run_feature_flags_api_path_returns_json_without_cli_binary() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"flags-api","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("src/index.ts"),
            "if (process.env.FEATURE_ALPHA) {\n  console.log('on');\n}\n",
        )
        .expect("write source");

        let result = run_feature_flags(
            "unused-binary-on-api-path",
            FeatureFlagsParams {
                root: Some(project.path().display().to_string()),
                no_cache: Some(true),
                ..FeatureFlagsParams::default()
            },
        )
        .await
        .expect("mcp result");

        assert!(!result.is_error.unwrap_or(false));
        let [content] = result.content.as_slice() else {
            panic!("expected one content item");
        };
        let ContentBlock::Text(text) = content else {
            panic!("expected text content");
        };
        let json: serde_json::Value = serde_json::from_str(&text.text).expect("json");
        assert_eq!(json["kind"], "feature-flags");
        assert_eq!(
            json["feature_flags"][0]["flag_name"].as_str(),
            Some("FEATURE_ALPHA")
        );
    }

    #[tokio::test]
    async fn top_limit_uses_api_path_without_cli_binary() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"flags-api-top","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("src/index.ts"),
            "if (process.env.FEATURE_ALPHA) {}\nif (process.env.FEATURE_BETA) {}\n",
        )
        .expect("write source");

        let result = run_feature_flags(
            "unused-binary-on-api-path",
            FeatureFlagsParams {
                root: Some(project.path().display().to_string()),
                no_cache: Some(true),
                top: Some(1),
                ..FeatureFlagsParams::default()
            },
        )
        .await;

        let result = result.expect("mcp result");
        assert!(!result.is_error.unwrap_or(false));
        let [content] = result.content.as_slice() else {
            panic!("expected one content item");
        };
        let ContentBlock::Text(text) = content else {
            panic!("expected text content");
        };
        let json: serde_json::Value = serde_json::from_str(&text.text).expect("json");
        assert_eq!(json["feature_flags"].as_array().expect("flags").len(), 1);
    }

    #[test]
    fn retirement_params_map_to_the_api_options() {
        let options = feature_flags_options_from_params(&FeatureFlagsParams {
            root: Some("/repo".to_string()),
            retirement: Some(true),
            flag_state: Some("flag-state.json".to_string()),
            flag_age: Some("off".to_string()),
            ..FeatureFlagsParams::default()
        })
        .expect("options");
        let retirement = options.retirement.expect("retirement options");
        assert_eq!(retirement.flag_age, FlagAgeMode::Off);
        assert_eq!(
            retirement.flag_state,
            Some(std::path::PathBuf::from("/repo/flag-state.json"))
        );
        assert!(
            feature_flags_options_from_params(&FeatureFlagsParams::default())
                .expect("options")
                .retirement
                .is_none()
        );
    }

    #[test]
    fn retirement_params_are_checked() {
        let bad_age = feature_flags_options_from_params(&FeatureFlagsParams {
            retirement: Some(true),
            flag_age: Some("git".to_string()),
            ..FeatureFlagsParams::default()
        })
        .expect_err("invalid flag_age");
        assert!(bad_age.contains("invalid flag_age 'git'"), "{bad_age}");
        let orphan = feature_flags_options_from_params(&FeatureFlagsParams {
            flag_state: Some("state.json".to_string()),
            ..FeatureFlagsParams::default()
        })
        .expect_err("flag_state without retirement");
        assert!(orphan.contains("need retirement"), "{orphan}");
    }

    #[test]
    fn retirement_params_reach_the_cli_args() {
        let args = build_feature_flags_args(&FeatureFlagsParams {
            retirement: Some(true),
            flag_state: Some("state.json".to_string()),
            flag_age: Some("pickaxe".to_string()),
            ..FeatureFlagsParams::default()
        });
        let tail: Vec<&str> = args.iter().map(String::as_str).skip(5).collect();
        assert_eq!(
            tail,
            vec![
                "--retirement",
                "--flag-state",
                "state.json",
                "--flag-age",
                "pickaxe"
            ]
        );
    }
}
