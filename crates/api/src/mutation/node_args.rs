//! The Node flags of the `node --test` command in a `package.json` script.
//! The `node:test` adapter gives these flags to each test process, so a
//! test file loads as it does in the project test script.

use std::fs;
use std::path::Path;

/// The flags with no value that change how Node loads a module.
const SWITCH_FLAGS: &[&str] = &[
    "--experimental-transform-types",
    "--experimental-strip-types",
    "--no-experimental-strip-types",
];

/// The flags with a value that change how Node loads a module, with the
/// long name of each flag. A short name gives its long name.
const VALUE_FLAGS: &[(&str, &str)] = &[
    ("--import", "--import"),
    ("--require", "--require"),
    ("-r", "--require"),
    ("--experimental-loader", "--experimental-loader"),
    ("--loader", "--experimental-loader"),
    ("--conditions", "--conditions"),
    ("-C", "--conditions"),
];

/// The shell operators that separate two commands of a script.
const COMMAND_SEPARATORS: &[&str] = &["&&", "||", ";", "|", "&"];

/// The script that the detection reads first.
const TEST_SCRIPT: &str = "test";

/// Reads the Node flags of the first `node --test` command in the
/// `package.json` scripts at `root`. The `test` script comes first, then
/// the other scripts in name order. The result has only the flags that
/// change module loading, each flag with a value as two entries. An empty
/// list means that no script has such a command, or that the command has
/// no such flag.
pub fn node_test_args(root: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(root.join("package.json")) else {
        return Vec::new();
    };
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(scripts) = manifest.get("scripts").and_then(|value| value.as_object()) else {
        return Vec::new();
    };
    let mut names: Vec<&String> = scripts.keys().collect();
    names.sort_by_key(|name| (name.as_str() != TEST_SCRIPT, name.as_str()));
    names
        .into_iter()
        .filter_map(|name| scripts.get(name).and_then(serde_json::Value::as_str))
        .find_map(script_node_test_args)
        .unwrap_or_default()
}

/// The module loading flags of the first `node --test` command in a
/// script, or `None` when the script has no such command.
fn script_node_test_args(script: &str) -> Option<Vec<String>> {
    split_commands(&split_words(script))
        .into_iter()
        .find_map(node_test_command_args)
}

/// The module loading flags of one command, when the command is
/// `node --test`. Leading `NAME=value` assignments are skipped.
fn node_test_command_args(words: &[String]) -> Option<Vec<String>> {
    let start = words.iter().position(|word| !is_assignment(word))?;
    let (program, args) = words[start..].split_first()?;
    if !is_node(program) || !args.iter().any(|arg| arg == "--test") {
        return None;
    }
    Some(module_flags(args))
}

/// The module loading flags in the arguments, in their order. A value flag
/// takes the next argument or the text after `=`.
fn module_flags(args: &[String]) -> Vec<String> {
    let mut flags = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        if SWITCH_FLAGS.contains(&arg) {
            flags.push(arg.to_owned());
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value)),
            _ => (arg, None),
        };
        let Some((_, long)) = VALUE_FLAGS.iter().find(|(flag, _)| *flag == name) else {
            continue;
        };
        let value = match inline {
            Some(value) => value.to_owned(),
            None => {
                let Some(value) = args.get(index) else {
                    break;
                };
                index += 1;
                value.clone()
            }
        };
        flags.push((*long).to_owned());
        flags.push(value);
    }
    flags
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && !name.starts_with('-')
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
}

/// True for `node`, a path that ends in `node`, or `node.exe`.
fn is_node(program: &str) -> bool {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    name == "node" || name.eq_ignore_ascii_case("node.exe")
}

/// Splits the words at the shell operators into commands.
fn split_commands(words: &[String]) -> Vec<&[String]> {
    words
        .split(|word| COMMAND_SEPARATORS.contains(&word.as_str()))
        .filter(|command| !command.is_empty())
        .collect()
}

/// Splits a script into words at whitespace. A quote keeps its text in one
/// word, and the quote characters are removed. A shell operator is its own
/// word, also with no space around it.
fn split_words(script: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = script.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            } else {
                word.push(ch);
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            ';' | '&' | '|' => {
                push_word(&mut words, &mut word);
                let mut operator = ch.to_string();
                if chars.peek() == Some(&ch) && ch != ';' {
                    chars.next();
                    operator.push(ch);
                }
                words.push(operator);
            }
            ch if ch.is_whitespace() => push_word(&mut words, &mut word),
            ch => word.push(ch),
        }
    }
    push_word(&mut words, &mut word);
    words
}

fn push_word(words: &mut Vec<String>, word: &mut String) {
    if !word.is_empty() {
        words.push(std::mem::take(word));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(script: &str) -> Option<Vec<String>> {
        script_node_test_args(script)
    }

    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn the_flags_of_a_node_test_command_after_other_commands_are_read() {
        assert_eq!(
            args("xo && tsc --noEmit && node --experimental-transform-types --test test/test.ts"),
            Some(owned(&["--experimental-transform-types"]))
        );
    }

    #[test]
    fn value_flags_take_the_next_word_or_the_text_after_the_equals_sign() {
        assert_eq!(
            args(
                "node --import tsx --require=./setup.cjs -r dotenv/config --loader ./l.mjs \
                 --conditions=development -C custom --test"
            ),
            Some(owned(&[
                "--import",
                "tsx",
                "--require",
                "./setup.cjs",
                "--require",
                "dotenv/config",
                "--experimental-loader",
                "./l.mjs",
                "--conditions",
                "development",
                "--conditions",
                "custom",
            ]))
        );
        assert_eq!(
            args("node --experimental-strip-types --no-experimental-strip-types --test"),
            Some(owned(&[
                "--experimental-strip-types",
                "--no-experimental-strip-types"
            ]))
        );
    }

    #[test]
    fn other_flags_and_the_test_files_are_ignored() {
        assert_eq!(
            args("node --test --test-reporter spec --experimental-test-coverage test/a.test.ts"),
            Some(Vec::new())
        );
        assert_eq!(
            args("NODE_ENV=test node --import \"./setup file.mjs\" --test"),
            Some(owned(&["--import", "./setup file.mjs"]))
        );
    }

    #[test]
    fn a_script_without_a_node_test_command_gives_none() {
        assert_eq!(args("node --import tsx src/main.ts"), None);
        assert_eq!(args("vitest run --test"), None);
        assert_eq!(args("echo node --test"), None);
        assert_eq!(
            args("tsc&&node --experimental-transform-types --test"),
            Some(owned(&["--experimental-transform-types"]))
        );
    }

    #[test]
    fn the_test_script_comes_first_and_a_missing_manifest_gives_no_flags() {
        let dir = tempfile::tempdir().expect("dir");
        assert!(node_test_args(dir.path()).is_empty());
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"a":"node --import ./a.mjs --test","test":"node --experimental-transform-types --test test/test.ts"}}"#,
        )
        .expect("write");
        assert_eq!(
            node_test_args(dir.path()),
            owned(&["--experimental-transform-types"])
        );
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"b":"node --import ./b.mjs --test","a":"node --import ./a.mjs --test","test":"xo"}}"#,
        )
        .expect("write");
        assert_eq!(node_test_args(dir.path()), owned(&["--import", "./a.mjs"]));
    }
}
