use std::path::PathBuf;
use std::process::ExitCode;

use fallow_config::{ConfigWriteError, OutputFormat};
use fallow_types::suppress::is_valid_policy_identifier;
use serde_json::json;

use super::{InitArgs, RulePackContext};
use crate::error::emit_error;

pub fn run(args: &InitArgs, ctx: &RulePackContext<'_>) -> ExitCode {
    let selected = match select_template(args, ctx.output) {
        Ok(selected) => selected,
        Err(code) => return code,
    };

    let rel_path = match available_pack_path(args, ctx, &selected.pack_name) {
        Ok(path) => path,
        Err(code) => return code,
    };

    if let Err(code) = write_rule_pack_file(ctx, &rel_path, &selected) {
        return code;
    }

    let rel_string = path_to_config_string(&rel_path);
    let rules = match validate_generated_pack(ctx, &rel_string) {
        Ok(rules) => rules,
        Err(code) => return code,
    };

    let config_result = if args.no_config {
        ConfigUpdateResult::Skipped
    } else {
        update_config(ctx, &rel_string)
    };

    emit_result(
        ctx,
        &InitResult {
            pack_path: rel_string,
            template: args.template.clone(),
            rules,
            config: config_result,
        },
    )
}

struct SelectedTemplate {
    template: &'static super::templates::Template,
    pack_name: String,
}

fn select_template(args: &InitArgs, output: OutputFormat) -> Result<SelectedTemplate, ExitCode> {
    let Some(template) = super::templates::by_name(&args.template) else {
        return Err(emit_error(
            &format!(
                "unknown rule-pack template '{}'; available templates: {}",
                args.template,
                available_template_names()
            ),
            2,
            output,
        ));
    };

    let pack_name = default_pack_name(args, template).to_string();
    if !is_valid_policy_identifier(&pack_name) {
        return Err(emit_error(
            &format!(
                "invalid rule-pack name '{pack_name}'; use only ASCII letters, digits, '.', '_', and '-'"
            ),
            2,
            output,
        ));
    }

    Ok(SelectedTemplate {
        template,
        pack_name,
    })
}

fn default_pack_name<'a>(
    args: &'a InitArgs,
    template: &'static super::templates::Template,
) -> &'a str {
    args.name.as_deref().unwrap_or_else(|| {
        if args.template == "starter" {
            "team-policy"
        } else {
            template.name
        }
    })
}

fn available_pack_path(
    args: &InitArgs,
    ctx: &RulePackContext<'_>,
    pack_name: &str,
) -> Result<PathBuf, ExitCode> {
    let rel_path = pack_relative_path(&args.dir, pack_name)
        .map_err(|message| emit_error(&message, 2, ctx.output))?;
    let abs_path = ctx.root.join(&rel_path);
    if abs_path.exists() {
        return Err(emit_error(
            &format!("rule-pack file already exists: {}", rel_path.display()),
            2,
            ctx.output,
        ));
    }
    Ok(rel_path)
}

fn write_rule_pack_file(
    ctx: &RulePackContext<'_>,
    rel_path: &std::path::Path,
    selected: &SelectedTemplate,
) -> Result<(), ExitCode> {
    let abs_path = ctx.root.join(rel_path);
    if let Some(parent) = abs_path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        return Err(emit_error(
            &format!(
                "failed to create rule-pack directory '{}': {err}",
                parent.display()
            ),
            2,
            ctx.output,
        ));
    }

    let rendered = super::templates::render(selected.template, &selected.pack_name);
    std::fs::write(&abs_path, rendered).map_err(|err| {
        emit_error(
            &format!(
                "failed to write rule-pack file '{}': {err}",
                rel_path.display()
            ),
            2,
            ctx.output,
        )
    })
}

fn validate_generated_pack(ctx: &RulePackContext<'_>, rel_path: &str) -> Result<usize, ExitCode> {
    fallow_config::load_rule_packs(ctx.root, std::slice::from_ref(&rel_path.to_string()))
        .map(|packs| packs.first().map_or(0, |pack| pack.rules.len()))
        .map_err(|errors| {
            let message = errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n  - ");
            emit_error(
                &format!("generated rule pack failed validation:\n  - {message}"),
                2,
                ctx.output,
            )
        })
}

struct InitResult {
    pack_path: String,
    template: String,
    rules: usize,
    config: ConfigUpdateResult,
}

enum ConfigUpdateResult {
    Updated(String),
    AlreadyPresent(String),
    Skipped,
    Missing,
    ManualSnippet { config_path: String, reason: String },
    Error(ExitCode),
}

fn update_config(ctx: &RulePackContext<'_>, rel_path: &str) -> ConfigUpdateResult {
    let config_path = if let Some(config_path) = ctx.config_path {
        if config_path.is_absolute() {
            config_path.clone()
        } else {
            ctx.root.join(config_path)
        }
    } else {
        let Some(config_path) = fallow_config::FallowConfig::find_config_path(ctx.root) else {
            return ConfigUpdateResult::Missing;
        };
        config_path
    };
    let config_display = root_relative(ctx.root, &config_path);
    match fallow_config::add_rule_pack_path(&config_path, rel_path) {
        Ok(true) => ConfigUpdateResult::Updated(config_display),
        Ok(false) => ConfigUpdateResult::AlreadyPresent(config_display),
        Err(ConfigWriteError::Io(err)) => ConfigUpdateResult::Error(emit_error(
            &format!(
                "failed to update config file '{}': {err}",
                config_path.display()
            ),
            2,
            ctx.output,
        )),
        Err(err) => ConfigUpdateResult::ManualSnippet {
            config_path: config_display,
            reason: err.to_string(),
        },
    }
}

fn emit_result(ctx: &RulePackContext<'_>, result: &InitResult) -> ExitCode {
    if let ConfigUpdateResult::Error(code) = result.config {
        return code;
    }

    if matches!(ctx.output, OutputFormat::Json) {
        return emit_json_result(result);
    }

    emit_human_result(result);
    ExitCode::SUCCESS
}

fn emit_json_result(result: &InitResult) -> ExitCode {
    let (config_updated, config_path) = match &result.config {
        ConfigUpdateResult::Updated(path) => (true, Some(path.as_str())),
        ConfigUpdateResult::AlreadyPresent(path)
        | ConfigUpdateResult::ManualSnippet {
            config_path: path, ..
        } => (false, Some(path.as_str())),
        ConfigUpdateResult::Skipped
        | ConfigUpdateResult::Missing
        | ConfigUpdateResult::Error(_) => (false, None),
    };
    crate::report::emit_json(
        &json!({
            "kind": "rule-pack-init",
            "pack_path": result.pack_path,
            "template": result.template,
            "rules": result.rules,
            "config_updated": config_updated,
            "config_path": config_path,
        }),
        "rule-pack-init",
    )
}

fn emit_human_result(result: &InitResult) {
    println!(
        "Created {} (template: {}, {} {})",
        result.pack_path,
        result.template,
        result.rules,
        crate::report::plural(result.rules)
    );
    match &result.config {
        ConfigUpdateResult::Updated(path) => {
            println!("Added \"{}\" to rulePacks in {path}", result.pack_path);
        }
        ConfigUpdateResult::AlreadyPresent(path) => {
            println!(
                "rulePacks in {path} already includes \"{}\"",
                result.pack_path
            );
        }
        ConfigUpdateResult::Skipped => {
            println!("Config update skipped (--no-config)");
            print_snippet(&result.pack_path);
        }
        ConfigUpdateResult::Missing => {
            println!("No fallow config file found.");
            print_snippet(&result.pack_path);
        }
        ConfigUpdateResult::ManualSnippet {
            config_path,
            reason,
        } => {
            println!("Could not update {config_path}: {reason}");
            print_snippet(&result.pack_path);
        }
        ConfigUpdateResult::Error(_) => unreachable!("handled above"),
    }
    println!("Next: fallow rule-pack test {}", result.pack_path);
}

fn print_snippet(rel_path: &str) {
    println!("Add this to your fallow config:");
    println!("  \"rulePacks\": [\"{rel_path}\"]");
}

fn available_template_names() -> String {
    super::templates::TEMPLATES
        .iter()
        .map(|template| template.name)
        .collect::<Vec<_>>()
        .join(", ")
}

fn pack_relative_path(dir: &str, pack_name: &str) -> Result<std::path::PathBuf, String> {
    let dir = std::path::Path::new(dir);
    if dir.is_absolute() {
        return Err("rule-pack directory must be project-relative".to_string());
    }
    if dir
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("rule-pack directory must not contain '..'".to_string());
    }
    Ok(dir.join(format!("{pack_name}.jsonc")))
}

fn path_to_config_string(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn root_relative(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .map_or_else(|_| path_to_config_string(path), path_to_config_string)
}
