//! Railway plugin.
//!
//! Railway infrastructure as code lives in `.railway/railway.ts`. Each
//! `service(name, { ... })` call from `railway/iac` declares the build, start,
//! and pre-deploy commands of one service. The plugin credits the packages
//! and local files that those commands use, relative to the service root
//! directory. A service that deploys an image, a template, or a GitHub
//! repository of another project runs no code from this project, so the
//! plugin skips it.

use std::path::{Path, PathBuf};

use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Expression, ImportDeclarationSpecifier,
    ObjectExpression, Program, Statement,
};
use oxc_ast_visit::{Visit, walk};
use rustc_hash::{FxHashMap, FxHashSet};

use super::config_parser::{self, lexical_normalize};
use super::{Plugin, PluginResult};
use crate::scripts;

const ENABLERS: &[&str] = &["railway"];
const CONFIG_PATTERNS: &[&str] = &[".railway/railway.ts"];
const ALWAYS_USED: &[&str] = &[".railway/railway.ts"];
const HIDDEN_DIRS: &[&str] = &[".railway"];
const TOOLING_DEPENDENCIES: &[&str] = &["railway"];

const IAC_MODULE: &str = "railway/iac";
const SOURCE_TYPES: &[&str] = &["github", "image", "template"];

define_plugin! {
    struct RailwayPlugin => "railway",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    discovery_hidden_dirs: HIDDEN_DIRS,
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();
        super::add_import_referenced_dependencies(&mut result, source, config_path);

        // The config lives in `.railway/`, so the project directory is the
        // parent of that directory.
        let project_dir = config_path
            .parent()
            .and_then(Path::parent)
            .unwrap_or(root);
        let local_repositories = local_repositories(project_dir, root);
        let services = config_parser::extract_from_source(source, config_path, |program| {
            Some(collect_services(program, &local_repositories))
        })
        .unwrap_or_default();

        let mut packages = FxHashSet::default();
        for service in services {
            let cwd = service
                .root_directory
                .map_or_else(|| project_dir.to_path_buf(), |dir| {
                    lexical_normalize(&project_dir.join(dir))
                });
            for command in &service.commands {
                credit_command(command, &cwd, &mut packages, &mut result.setup_files);
            }
        }
        let mut packages: Vec<String> = packages.into_iter().collect();
        packages.sort_unstable();
        result.referenced_dependencies.extend(packages);
        result
    }
}

/// The commands of one service and the directory that they run in.
#[derive(Debug, Default, PartialEq, Eq)]
struct ServiceCommands {
    root_directory: Option<String>,
    commands: Vec<String>,
}

/// Credit the packages and local files of one service command.
fn credit_command(
    command: &str,
    cwd: &Path,
    packages: &mut FxHashSet<String>,
    files: &mut Vec<PathBuf>,
) {
    let analysis = scripts::analyze_command(command, cwd, &FxHashMap::default());
    packages.extend(analysis.used_packages);
    for file in analysis.config_files.iter().chain(&analysis.entry_files) {
        files.push(lexical_normalize(&cwd.join(file)));
    }
    if let Some(program) = command.split_whitespace().next()
        && (program.starts_with("./") || program.starts_with("../"))
    {
        files.push(lexical_normalize(&cwd.join(program)));
    }
}

fn collect_services(program: &Program<'_>, local_repositories: &[String]) -> Vec<ServiceCommands> {
    let mut collector = ServiceCollector {
        service_names: FxHashSet::default(),
        source_types: FxHashMap::default(),
        bindings: FxHashMap::default(),
        local_repositories,
        services: Vec::new(),
    };
    collector.read_top_level(program);
    if collector.service_names.is_empty() {
        return Vec::new();
    }
    collector.visit_program(program);
    collector.services
}

struct ServiceCollector<'a, 'r> {
    service_names: FxHashSet<String>,
    source_types: FxHashMap<String, &'static str>,
    bindings: FxHashMap<String, &'a Expression<'a>>,
    local_repositories: &'r [String],
    services: Vec<ServiceCommands>,
}

impl<'a> ServiceCollector<'a, '_> {
    fn read_top_level(&mut self, program: &'a Program<'a>) {
        for statement in &program.body {
            match statement {
                Statement::ImportDeclaration(import) if import.source.value == IAC_MODULE => {
                    for specifier in import.specifiers.iter().flatten() {
                        let ImportDeclarationSpecifier::ImportSpecifier(named) = specifier else {
                            continue;
                        };
                        let imported = named.imported.name();
                        let local = named.local.name.to_string();
                        if imported == "service" {
                            self.service_names.insert(local);
                        } else if let Some(kind) =
                            SOURCE_TYPES.iter().find(|kind| imported == **kind)
                        {
                            self.source_types.insert(local, kind);
                        }
                    }
                }
                Statement::VariableDeclaration(declaration) => {
                    for declarator in &declaration.declarations {
                        if let BindingPattern::BindingIdentifier(id) = &declarator.id
                            && let Some(init) = &declarator.init
                        {
                            self.bindings.insert(id.name.to_string(), init);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Follow identifiers through top-level bindings to their value.
    fn resolve_binding<'b>(&self, mut expr: &'b Expression<'a>) -> &'b Expression<'a> {
        let mut seen = FxHashSet::default();
        while let Expression::Identifier(id) = expr
            && seen.insert(id.name.as_str())
            && let Some(init) = self.bindings.get(id.name.as_str())
        {
            expr = init;
        }
        expr
    }

    fn read_service(&mut self, call: &CallExpression<'a>) {
        let Some(Expression::ObjectExpression(service)) =
            call.arguments.get(1).and_then(Argument::as_expression)
        else {
            return;
        };

        let source = config_parser::find_property(service, "source")
            .map(|prop| self.resolve_binding(&prop.value));
        let source_object = source.and_then(as_object);
        let source_call = match source {
            Some(Expression::CallExpression(call)) => Some(&**call),
            _ => None,
        };
        let repository = source_object.and_then(|obj| string_property(obj, "repo"));
        let source_type = source_call
            .and_then(|call| match &call.callee {
                Expression::Identifier(id) => self.source_types.get(id.name.as_str()).copied(),
                _ => None,
            })
            .map(str::to_string)
            .or_else(|| source_object.and_then(|obj| string_property(obj, "type")))
            .or_else(|| {
                let has = |key| source_object.and_then(|obj| string_property(obj, key));
                if repository.is_some() {
                    Some("github".to_string())
                } else if has("image").is_some() {
                    Some("image".to_string())
                } else {
                    has("template").map(|_| "template".to_string())
                }
            });
        let source_repository = source_call
            .and_then(|call| call.arguments.first())
            .and_then(Argument::as_expression)
            .and_then(config_parser::expression_to_string)
            .or(repository)
            .and_then(|value| github_repository(&value));
        let is_other_project = match source_type.as_deref() {
            Some("image" | "template") => true,
            Some("github") => source_repository.is_some_and(|repo| {
                !self.local_repositories.is_empty() && !self.local_repositories.contains(&repo)
            }),
            _ => false,
        };
        if is_other_project {
            return;
        }

        let source_config = match source_call {
            Some(call) => call
                .arguments
                .get(1)
                .and_then(Argument::as_expression)
                .and_then(as_object),
            None => source_object,
        };
        let root_directory = source_config
            .and_then(|obj| string_property(obj, "rootDirectory"))
            .or_else(|| string_property(service, "root"))
            .or_else(|| string_property(service, "rootDirectory"));

        let build = property_value(service, "build");
        let mut commands = Vec::new();
        for value in [
            build,
            build
                .and_then(as_object)
                .and_then(|obj| property_value(obj, "buildCommand")),
            property_value(service, "start"),
            property_value(service, "preDeploy"),
            property_value(service, "deploy")
                .and_then(as_object)
                .and_then(|obj| property_value(obj, "preDeployCommand")),
        ]
        .into_iter()
        .flatten()
        {
            commands.extend(string_or_strings(value));
        }

        self.services.push(ServiceCommands {
            root_directory,
            commands,
        });
    }
}

impl<'a> Visit<'a> for ServiceCollector<'a, '_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Expression::Identifier(callee) = &call.callee
            && self.service_names.contains(callee.name.as_str())
        {
            self.read_service(call);
        }
        walk::walk_call_expression(self, call);
    }
}

fn as_object<'a>(expr: &'a Expression<'a>) -> Option<&'a ObjectExpression<'a>> {
    match expr {
        Expression::ObjectExpression(obj) => Some(obj),
        _ => None,
    }
}

fn property_value<'a>(obj: &'a ObjectExpression<'a>, key: &str) -> Option<&'a Expression<'a>> {
    config_parser::find_property(obj, key).map(|prop| &prop.value)
}

fn string_property(obj: &ObjectExpression<'_>, key: &str) -> Option<String> {
    config_parser::find_property(obj, key)
        .and_then(|prop| config_parser::expression_to_string(&prop.value))
}

/// Return a string value, or the string elements of an array value.
fn string_or_strings(expr: &Expression<'_>) -> Vec<String> {
    if let Some(value) = config_parser::expression_to_string(expr) {
        return vec![value];
    }
    match expr {
        Expression::ArrayExpression(array) => array
            .elements
            .iter()
            .filter_map(|element| element.as_expression())
            .filter_map(config_parser::expression_to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// Return the `owner/name` of a GitHub repository reference in lowercase.
///
/// Accepts `owner/name`, `https://github.com/owner/name.git`,
/// `git+https://...`, `git@github.com:owner/name.git`, and `github:owner/name`.
fn github_repository(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.strip_prefix("git+").unwrap_or(value);
    let value = value.strip_suffix('/').unwrap_or(value);
    let value = value
        .strip_suffix(".git")
        .or_else(|| value.strip_suffix(".git/"))
        .unwrap_or(value);
    let value = value.strip_suffix('/').unwrap_or(value);
    let lower = value.to_ascii_lowercase();

    let is_owner_and_name = |candidate: &str| {
        !candidate.contains(':')
            && candidate.split_once('/').is_some_and(|(owner, name)| {
                !owner.is_empty() && !name.is_empty() && !name.contains('/')
            })
    };
    if is_owner_and_name(&lower) {
        return Some(lower);
    }
    ["github.com/", "github.com:", "github/", "github:"]
        .iter()
        .filter_map(|marker| lower.rfind(marker).map(|at| &lower[at + marker.len()..]))
        .find(|candidate| is_owner_and_name(candidate))
        .map(str::to_string)
}

/// Return the GitHub repositories of this project, from the `repository`
/// field of `package.json` and from the remotes in `.git/config`.
fn local_repositories(project_dir: &Path, root: &Path) -> Vec<String> {
    let mut repositories = Vec::new();
    let mut dirs = vec![project_dir];
    if root != project_dir {
        dirs.push(root);
    }
    for dir in &dirs {
        let Ok(content) = std::fs::read_to_string(dir.join("package.json")) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let repository = manifest.get("repository");
        let url = repository
            .and_then(serde_json::Value::as_str)
            .or_else(|| repository.and_then(|repo| repo.get("url")?.as_str()));
        if let Some(name) = url.and_then(github_repository) {
            repositories.push(name);
        }
    }
    if let Ok(git_config) = std::fs::read_to_string(root.join(".git").join("config")) {
        repositories.extend(git_remote_urls(&git_config).filter_map(github_repository));
    }
    repositories.sort_unstable();
    repositories.dedup();
    repositories
}

fn git_remote_urls(git_config: &str) -> impl Iterator<Item = &str> {
    git_config.lines().filter_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "url").then(|| value.trim())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn services(source: &str, local: &[&str]) -> Vec<ServiceCommands> {
        let local: Vec<String> = local.iter().map(|repo| (*repo).to_string()).collect();
        config_parser::extract_from_source(source, Path::new(".railway/railway.ts"), |program| {
            Some(collect_services(program, &local))
        })
        .unwrap_or_default()
    }

    const CONFIG: &str = r"
import { defineRailway, github, image, project, service } from 'railway/iac';

const storefrontSource = github('fruit-stand/storefront', { rootDirectory: 'services/storefront' });

export default defineRailway(() => {
  const storefront = service('storefront', {
    source: storefrontSource,
    build: 'node scripts/build.ts',
    start: 'node src/serve.ts',
    preDeploy: 'npx storefront-tool prepare',
  });
  const inventory = service('inventory', {
    root: 'services/inventory',
    start: './scripts/deploy.ts',
  });
  const analytics = service('analytics', {
    source: github('other-stand/analytics'),
    start: 'node scripts/build.ts',
  });
  const dashboard = service('dashboard', {
    source: image('nginx:latest'),
    start: 'node dashboard/start.ts',
    preDeploy: 'npx external-tool prepare',
  });
  return project('fruit-stand', { resources: [storefront, inventory, analytics, dashboard] });
});
";

    #[test]
    fn reads_services_of_this_project() {
        let found = services(CONFIG, &["fruit-stand/storefront"]);
        assert_eq!(
            found,
            [
                ServiceCommands {
                    root_directory: Some("services/storefront".to_string()),
                    commands: vec![
                        "node scripts/build.ts".to_string(),
                        "node src/serve.ts".to_string(),
                        "npx storefront-tool prepare".to_string(),
                    ],
                },
                ServiceCommands {
                    root_directory: Some("services/inventory".to_string()),
                    commands: vec!["./scripts/deploy.ts".to_string()],
                },
            ]
        );
    }

    #[test]
    fn keeps_github_services_when_the_project_repository_is_unknown() {
        let found = services(CONFIG, &[]);
        assert_eq!(found.len(), 3, "image service is skipped: {found:?}");
    }

    #[test]
    fn reads_build_objects_and_deploy_commands() {
        let source = r"
import { service as svc } from 'railway/iac';
svc('api', {
  source: { repo: 'acme/api', rootDirectory: 'api' },
  build: { buildCommand: ['pnpm exec tsc', 'node build.ts'] },
  deploy: { preDeployCommand: 'npx migrate-tool up' },
});
svc('cache', { source: { image: 'redis' }, start: 'node x.ts' });
";
        let found = services(source, &["acme/api"]);
        assert_eq!(
            found,
            [ServiceCommands {
                root_directory: Some("api".to_string()),
                commands: vec![
                    "pnpm exec tsc".to_string(),
                    "node build.ts".to_string(),
                    "npx migrate-tool up".to_string(),
                ],
            }]
        );
    }

    #[test]
    fn ignores_service_calls_without_the_iac_import() {
        let source = "const service = () => {}; service('x', { start: 'node a.ts' });";
        assert!(services(source, &[]).is_empty());
    }

    #[test]
    fn normalizes_github_repository_references() {
        for (input, expected) in [
            ("fruit-stand/Storefront", Some("fruit-stand/storefront")),
            (
                "git+https://github.com/fruit-stand/storefront.git",
                Some("fruit-stand/storefront"),
            ),
            (
                "git@github.com:fruit-stand/storefront.git",
                Some("fruit-stand/storefront"),
            ),
            (
                "github:fruit-stand/storefront",
                Some("fruit-stand/storefront"),
            ),
            ("https://gitlab.com/group/sub/project", None),
        ] {
            assert_eq!(github_repository(input).as_deref(), expected, "{input}");
        }
    }

    #[test]
    fn credits_commands_relative_to_the_service_directory() {
        let mut packages = FxHashSet::default();
        let mut files = Vec::new();
        let cwd = Path::new("/project/services/inventory");
        credit_command("./scripts/deploy.ts", cwd, &mut packages, &mut files);
        credit_command(
            "npx storefront-tool prepare",
            cwd,
            &mut packages,
            &mut files,
        );
        credit_command("node src/serve.ts", cwd, &mut packages, &mut files);
        assert!(packages.contains("storefront-tool"), "{packages:?}");
        assert!(files.contains(&cwd.join("scripts/deploy.ts")), "{files:?}");
        assert!(files.contains(&cwd.join("src/serve.ts")), "{files:?}");
    }
}
