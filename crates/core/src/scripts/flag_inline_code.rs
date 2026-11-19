//! The packages that the inline code of a script runtime loads
//! (issue #4711).
//!
//! `node -e "require('shelljs').ls()"` and
//! `node --input-type=module -e "import x from 'shelljs'"` load `shelljs`
//! from the code in the flag value. No source file imports the package, so
//! without a credit it is reported as an unused dependency.

use std::path::Path;

use crate::analyze::is_builtin_module;
use crate::discover::FileId;
use crate::extract::parse_from_content;
use crate::resolve::{extract_package_name, is_bare_specifier, is_valid_package_name};

/// The virtual file name of the inline code. The TypeScript parser also
/// reads the plain JavaScript of `node -e` and the TypeScript of `tsx -e`.
const INLINE_CODE_PATH: &str = "[eval].ts";

/// Return the packages that the inline code of `binary` loads. `args` are
/// the tokens after the binary.
pub(super) fn inline_code_packages(binary: &str, args: &[&str]) -> Vec<String> {
    let mut packages = Vec::new();
    for code in crate::scripts::argument_entries::inline_code_values(binary, args) {
        code_packages(code, &mut packages);
    }
    packages
}

/// Add the packages that the `import`, `export from`, `import()` and
/// `require()` specifiers of `code` name.
fn code_packages(code: &str, packages: &mut Vec<String>) {
    let module = parse_from_content(FileId(0), Path::new(INLINE_CODE_PATH), code);
    let specifiers = module
        .imports
        .iter()
        .map(|import| import.source.as_str())
        .chain(module.re_exports.iter().map(|re| re.source.as_str()))
        .chain(
            module
                .dynamic_imports
                .iter()
                .map(|dynamic| dynamic.source.as_str()),
        )
        .chain(module.require_calls.iter().map(|call| call.source.as_str()));
    for specifier in specifiers {
        if !is_bare_specifier(specifier) || is_builtin_module(specifier) {
            continue;
        }
        let name = extract_package_name(specifier);
        if is_valid_package_name(&name) && !is_builtin_module(&name) && !packages.contains(&name) {
            packages.push(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::scripts::parse_script;

    /// The packages that the commands of `script` credit through flags.
    fn flag_packages(script: &str) -> Vec<String> {
        parse_script(script)
            .into_iter()
            .flat_map(|command| command.flag_packages)
            .collect()
    }

    #[test]
    fn inline_code_credits_the_packages_that_it_loads() {
        for script in [
            "node -e \"require('shelljs').find('dist/**/*.d.ts')\"",
            "node --input-type=module -e \"import shelljs from 'shelljs'; shelljs.ls('.')\"",
            "node --eval \"require('shelljs/package.json')\"",
            "node --eval=\"require('shelljs')\"",
            "node -p \"require('shelljs').ls('.').length\"",
            "node --print \"import('shelljs')\"",
            "npx tsx -e \"import { ls } from 'shelljs'; ls('.')\"",
        ] {
            assert_eq!(flag_packages(script), ["shelljs"], "{script}");
        }
    }

    #[test]
    fn inline_code_credits_no_file_or_builtin_module() {
        let script = "node -e \"require('./build.js'); require('node:fs'); require('path'); \
                      require('fs'); require('@scope/pkg/sub')\"";
        assert_eq!(flag_packages(script), ["@scope/pkg"]);
    }

    #[test]
    fn inline_code_flag_after_the_script_is_a_script_argument() {
        assert!(flag_packages("node scripts/run.js -e \"require('shelljs')\"").is_empty());
    }
}
