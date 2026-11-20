//! Reads of non-public env vars with their offsets, for the security
//! `client-server-leak` rule (issues #4881 and #4882).

use fallow_types::extract::{EnvRead, EnvReadForm};

use crate::tests::{parse_ts, parse_tsx};

/// Each read as `(name, text at the offset, form)`.
fn reads(source: &str, env_reads: &[EnvRead]) -> Vec<(String, String, EnvReadForm)> {
    env_reads
        .iter()
        .map(|read| {
            let rest = &source[read.span_start as usize..];
            let word: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '\'')
                .collect();
            (read.name.clone(), word, read.form)
        })
        .collect()
}

#[test]
fn records_the_dotted_the_computed_and_the_destructured_form() {
    let source = "export function read() {\n\
                  \tconst a = process.env.DOTTED_KEY\n\
                  \tconst b = process.env['COMPUTED_KEY']\n\
                  \tconst { DESTRUCTURED_KEY, RENAMED_KEY: renamed, DEFAULT_KEY = 'x' } = process.env\n\
                  \tconst c = import.meta.env.VITE_SECRET_KEY_NOT\n\
                  \tconst d = import.meta.env.API_TOKEN\n\
                  \treturn [a, b, c, d, DESTRUCTURED_KEY, renamed, DEFAULT_KEY]\n\
                  }\n";
    let info = parse_ts(source);
    assert_eq!(
        reads(source, &info.env_reads),
        [
            (
                "process.env.DOTTED_KEY".to_owned(),
                "DOTTED_KEY".to_owned(),
                EnvReadForm::Dotted
            ),
            (
                "process.env.COMPUTED_KEY".to_owned(),
                "'COMPUTED_KEY'".to_owned(),
                EnvReadForm::Computed
            ),
            (
                "process.env.DESTRUCTURED_KEY".to_owned(),
                "DESTRUCTURED_KEY".to_owned(),
                EnvReadForm::Destructured
            ),
            (
                "process.env.RENAMED_KEY".to_owned(),
                "RENAMED_KEY".to_owned(),
                EnvReadForm::Destructured
            ),
            (
                "process.env.DEFAULT_KEY".to_owned(),
                "DEFAULT_KEY".to_owned(),
                EnvReadForm::Destructured
            ),
            (
                "import.meta.env.API_TOKEN".to_owned(),
                "API_TOKEN".to_owned(),
                EnvReadForm::Dotted
            ),
        ]
    );
}

#[test]
fn skips_public_vars_dynamic_keys_and_rest_elements() {
    let info = parse_tsx(
        "const key = 'X'\n\
         const a = process.env.NEXT_PUBLIC_URL\n\
         const b = process.env[key]\n\
         const { NODE_ENV, ...rest } = process.env\n\
         const env = process.env\n\
         export default function Page() { return [a, b, NODE_ENV, rest, env] }\n",
    );
    assert!(info.env_reads.is_empty(), "{:?}", info.env_reads);
}
