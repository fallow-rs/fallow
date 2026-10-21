//! Tools that read their file arguments but never execute them.

use super::tool_name;

/// A tool that reads its file arguments but never executes them: a formatter,
/// linter, spell checker, or code-quality checker (issue #2954). Its positional
/// arguments are not entry points. The tool itself still counts as a used
/// dependency, and its `--config` argument still counts as a config file.
pub(super) struct FileTargetTool {
    /// Binary names of the tool.
    pub(super) names: &'static [&'static str],
    /// Flags whose value names a module that the tool loads and runs, such as
    /// a custom formatter, plugin, or parser. A path value of such a flag stays
    /// reachable.
    pub(super) loading_flags: &'static [&'static str],
    /// Flags whose value names a directory of modules that the tool loads,
    /// such as a rule directory. The source files directly in that directory
    /// stay reachable.
    pub(super) directory_flags: &'static [&'static str],
}

/// Every [`FileTargetTool`], sorted by first name.
pub(super) const FILE_TARGET_TOOLS: &[FileTargetTool] = &[
    FileTargetTool {
        names: &["alex"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["biome", "rome"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["cspell"],
        loading_flags: &["--reporter"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["depcruise", "dependency-cruiser"],
        loading_flags: &["--webpack-config"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["dprint"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["editorconfig-checker", "eclint"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["ember-template-lint"],
        loading_flags: &["--config-path"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["eslint", "eslint_d"],
        loading_flags: &["-f", "--format", "--parser"],
        directory_flags: &["--rulesdir"],
    },
    FileTargetTool {
        names: &["htmlhint"],
        loading_flags: &[],
        directory_flags: &["--rulesdir"],
    },
    FileTargetTool {
        names: &["jscpd"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["jshint"],
        loading_flags: &["--reporter"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["madge"],
        loading_flags: &["--webpack-config", "--require-config"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["markdownlint"],
        loading_flags: &["-r", "--rules"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["markdownlint-cli2"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["markuplint"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["oxfmt", "oxlint"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["prettier"],
        loading_flags: &["--plugin"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["remark"],
        loading_flags: &["-u", "--use", "-r", "--rc-path"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["secretlint"],
        loading_flags: &["--secretlintrc"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["standard", "semistandard", "ts-standard"],
        loading_flags: &["--parser"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["stylelint"],
        loading_flags: &["--custom-formatter", "--custom-syntax"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["textlint"],
        loading_flags: &["-f", "--format", "--rule", "--plugin", "--preset"],
        directory_flags: &["--rulesdir"],
    },
    FileTargetTool {
        names: &["tslint"],
        loading_flags: &[],
        directory_flags: &["-r", "--rules-dir", "-s", "--formatters-dir"],
    },
    FileTargetTool {
        names: &["xo"],
        loading_flags: &["--reporter"],
        directory_flags: &[],
    },
];

/// Return the [`FileTargetTool`] that `binary` invokes. A path such as
/// `./node_modules/.bin/eslint` resolves to its file name.
pub(super) fn file_target_tool(binary: &str) -> Option<&'static FileTargetTool> {
    let name = tool_name(binary);
    FILE_TARGET_TOOLS
        .iter()
        .find(|tool| tool.names.contains(&name))
}
