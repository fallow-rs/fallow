//! Shell tokenization: splitting on operators, skipping env wrappers and package managers.

use super::ENV_WRAPPERS;
use std::borrow::Cow;

/// A literal shell argument with its original position for script forwarding.
pub(super) struct ShellWord<'a> {
    pub value: Cow<'a, str>,
    pub start: usize,
}

/// Preserve the literal argument prefix without evaluating shell expansions.
pub(super) fn split_words(source: &str) -> Vec<ShellWord<'_>> {
    let mut words = Vec::new();
    let mut rest = source.trim_start();
    while !rest.is_empty() {
        let Some(end) = shell_word_end(rest) else {
            break;
        };
        words.push(ShellWord {
            value: decode_shell_word(&rest[..end]),
            start: source.len() - rest.len(),
        });
        rest = rest[end..].trim_start();
    }
    words
}

fn shell_word_end(source: &str) -> Option<usize> {
    let mut quote = None;
    let windows_path = starts_windows_path(source);
    let mut chars = source.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '\\' if quote != Some('\'') && !windows_path => {
                chars.next()?;
            }
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            ch if Some(ch) == quote => quote = None,
            // Dynamic command substitutions can produce arbitrary argument boundaries.
            '`' if quote != Some('\'') => return None,
            '$' if quote != Some('\'') && chars.peek().is_some_and(|(_, ch)| *ch == '(') => {
                return None;
            }
            ch if quote.is_none() && ch.is_whitespace() => return Some(index),
            _ => {}
        }
    }
    quote.is_none().then_some(source.len())
}

fn decode_shell_word(raw: &str) -> Cow<'_, str> {
    // package.json scripts also run under cmd.exe, where these are path separators.
    if starts_windows_path(raw) {
        return Cow::Borrowed(
            raw.strip_prefix('"')
                .and_then(|path| path.strip_suffix('"'))
                .unwrap_or(raw),
        );
    }
    if !raw.contains(['\'', '"', '\\']) {
        return Cow::Borrowed(raw);
    }
    let mut value = String::with_capacity(raw.len());
    let mut quote = None;
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if quote != Some('\'') => {
                if let Some(next) = chars.next() {
                    if quote == Some('"') && !matches!(next, '"' | '\\' | '$' | '`' | '\n') {
                        value.push('\\');
                    }
                    if next != '\n' {
                        value.push(next);
                    }
                }
            }
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            ch if Some(ch) == quote => quote = None,
            _ => value.push(ch),
        }
    }
    Cow::Owned(value)
}

fn starts_windows_path(source: &str) -> bool {
    let source = source.strip_prefix('"').unwrap_or(source);
    source.starts_with(".\\")
        || source.starts_with("..\\")
        || source.starts_with("\\\\")
        || matches!(source.as_bytes(), [drive, b':', b'\\', ..] if drive.is_ascii_alphabetic())
}

/// Bun runtime boolean flags that may precede an executed file/binary
/// (`bun --bun <bin>`, `bun --watch <file>`, `bun --hot run dev`). Bun documents
/// these as flags that go immediately after `bun`, before the `run`/file/binary
/// target. None consume a value, so they can be skipped to reach the target.
/// Value-taking flags (`--filter <glob>`) are deliberately absent: an unrecognized
/// leading flag makes the parser treat the command as a script delegation rather
/// than guess where the binary starts. Source: Bun runtime docs (oven-sh/bun
/// docs/runtime/index.mdx, watch-mode.mdx).
const BUN_RUNTIME_FLAGS: &[&str] = &["--bun", "--watch", "--hot", "--smol", "--no-clear-screen"];

/// Split a script string on shell operators (`&&`, `||`, `;`, `|`, `&`).
/// Respects single and double quotes.
pub fn split_shell_operators(script: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let bytes = script.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut word_start = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < len {
        let b = bytes[i];

        if b == b'\\' && !in_single_quote && !starts_windows_path(&script[word_start..]) {
            i = (i + 2).min(len);
            continue;
        }

        if b == b'\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            i += 1;
            continue;
        }
        if b == b'"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            i += 1;
            continue;
        }

        if in_single_quote || in_double_quote {
            i += 1;
            continue;
        }

        if let Some(op_len) = shell_operator_len(bytes, i) {
            segments.push(&script[start..i]);
            i += op_len;
            start = i;
            word_start = i;
            continue;
        }

        if b.is_ascii_whitespace() {
            word_start = i + 1;
        }
        i += 1;
    }

    if start < len {
        segments.push(&script[start..]);
    }

    segments
}

/// Return the byte length of a shell operator at position `i`, or `None`.
///
/// Checks two-char operators (`&&`, `||`) before single-char ones (`&`, `|`, `;`)
/// to avoid splitting `&&` as two `&` operators.
fn shell_operator_len(bytes: &[u8], i: usize) -> Option<usize> {
    let b = bytes[i];
    let next = bytes.get(i + 1).copied();

    if matches!((b, next), (b'&', Some(b'&')) | (b'|', Some(b'|'))) {
        return Some(2);
    }

    if b == b';' {
        return Some(1);
    }
    if b == b'|' && next != Some(b'|') {
        return Some(1);
    }
    if b == b'&' && next != Some(b'&') {
        return Some(1);
    }

    None
}

/// Skip env var assignments (`KEY=value`) and env wrapper commands (`cross-env`, `dotenv`, `env`)
/// at the start of a token list. Returns the index of the first real command token, or `None`
/// if all tokens were consumed.
pub fn skip_initial_wrappers(tokens: &[&str], mut idx: usize) -> Option<usize> {
    while idx < tokens.len() && super::is_env_assignment(tokens[idx]) {
        idx += 1;
    }
    if idx >= tokens.len() {
        return None;
    }

    while idx < tokens.len() && ENV_WRAPPERS.contains(&tokens[idx]) {
        idx += 1;
        while idx < tokens.len() && super::is_env_assignment(tokens[idx]) {
            idx += 1;
        }
        if idx < tokens.len() && tokens[idx] == "--" {
            idx += 1;
        }
    }
    if idx >= tokens.len() {
        return None;
    }

    Some(idx)
}

/// Advance past package manager prefixes (`npx`, `pnpx`, `bunx`, `yarn exec`, `pnpm dlx`, etc.).
/// Returns the index of the actual binary token, or `None` if the command delegates to a named
/// script (e.g., `npm run build`, `yarn build`).
pub fn advance_past_package_manager(tokens: &[&str], mut idx: usize) -> Option<usize> {
    let token = tokens[idx];
    if matches!(token, "npx" | "pnpx" | "bunx") {
        idx += 1;
        while idx < tokens.len() && tokens[idx].starts_with('-') {
            let flag = tokens[idx];
            idx += 1;
            if matches!(flag, "--package" | "-p") && idx < tokens.len() {
                idx += 1;
            }
        }
    } else if token == "bun" {
        idx += 1;
        let mut saw_runtime_flag = false;
        while idx < tokens.len() && BUN_RUNTIME_FLAGS.contains(&tokens[idx]) {
            idx += 1;
            saw_runtime_flag = true;
        }
        if idx >= tokens.len() {
            return None;
        }
        let subcmd = tokens[idx];
        if subcmd == "exec" || subcmd == "x" {
            idx += 1;
        } else if matches!(subcmd, "run" | "run-script") || !saw_runtime_flag {
            return None;
        }
    } else if matches!(token, "yarn" | "pnpm" | "npm") {
        if idx + 1 < tokens.len() {
            let subcmd = tokens[idx + 1];
            if subcmd == "exec" || subcmd == "dlx" {
                idx += 2;
            } else {
                return None;
            }
        } else {
            return None;
        }
    }
    if idx >= tokens.len() {
        return None;
    }

    Some(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_words_preserve_quoted_and_escaped_boundaries() {
        for (source, expected) in [
            (
                r#"varlock run -p "./env -- is-ci ignored" -- publint"#,
                vec![
                    "varlock",
                    "run",
                    "-p",
                    "./env -- is-ci ignored",
                    "--",
                    "publint",
                ],
            ),
            (
                r"varlock run -p ./env\ --\ is-ci\ ignored -- publint",
                vec![
                    "varlock",
                    "run",
                    "-p",
                    "./env -- is-ci ignored",
                    "--",
                    "publint",
                ],
            ),
            (
                r#"va"r"lock run -- 'is-ci'"#,
                vec!["varlock", "run", "--", "is-ci"],
            ),
            (r#"echo "one\q" """#, vec!["echo", r"one\q", ""]),
            (
                r#""C:\Program Files\tool.exe" arg"#,
                vec![r"C:\Program Files\tool.exe", "arg"],
            ),
            (
                r".\tools\runner.exe arg",
                vec![r".\tools\runner.exe", "arg"],
            ),
            (
                r"\\server\share\runner.exe arg",
                vec![r"\\server\share\runner.exe", "arg"],
            ),
        ] {
            let words = split_words(source);
            assert_eq!(
                words
                    .iter()
                    .map(|word| word.value.as_ref())
                    .collect::<Vec<_>>(),
                expected,
                "{source}"
            );
        }
    }

    #[test]
    fn literal_words_keep_raw_offsets_for_forwarding() {
        let source = r#" npm --prefix "./path with spaces" run task -- -p "./env -- ignored""#;
        let words = split_words(source);
        assert_eq!(&source[words[6].start..], r#"-p "./env -- ignored""#);
    }

    #[test]
    fn literal_words_stop_at_opaque_argument_after_known_prefix() {
        for source in [
            "vite $(pwd) -- is-ci",
            "vite `pwd` -- is-ci",
            "vite 'unclosed",
            "vite \\",
        ] {
            let words = split_words(source);
            assert_eq!(
                words
                    .iter()
                    .map(|word| word.value.as_ref())
                    .collect::<Vec<_>>(),
                vec!["vite"],
                "{source}"
            );
        }
    }

    #[test]
    fn escaped_operators_remain_inside_the_argument() {
        assert_eq!(
            split_shell_operators(r"echo value\;tail && publint"),
            vec![r"echo value\;tail ", " publint"]
        );
    }

    #[test]
    fn quoted_windows_path_retains_following_shell_command() {
        let source = r#""C:\Program Files\" && publint"#;
        assert_eq!(
            split_shell_operators(source),
            vec![r#""C:\Program Files\" "#, " publint"]
        );
        let commands = super::super::parse_script(source);
        assert_eq!(
            commands
                .iter()
                .map(|command| command.binary.as_str())
                .collect::<Vec<_>>(),
            vec!["C:\\Program Files\\", "publint"]
        );
    }

    #[test]
    fn operator_len_double_ampersand() {
        assert_eq!(shell_operator_len(b"&&", 0), Some(2));
    }

    #[test]
    fn operator_len_double_pipe() {
        assert_eq!(shell_operator_len(b"||", 0), Some(2));
    }

    #[test]
    fn operator_len_semicolon() {
        assert_eq!(shell_operator_len(b";", 0), Some(1));
    }

    #[test]
    fn operator_len_single_pipe() {
        assert_eq!(shell_operator_len(b"|x", 0), Some(1));
    }

    #[test]
    fn operator_len_single_ampersand() {
        assert_eq!(shell_operator_len(b"&x", 0), Some(1));
    }

    #[test]
    fn operator_len_non_operator() {
        assert_eq!(shell_operator_len(b"abc", 0), None);
        assert_eq!(shell_operator_len(b"xyz", 1), None);
    }

    #[test]
    fn operator_len_ampersand_at_end_of_slice() {
        assert_eq!(shell_operator_len(b"&", 0), Some(1));
    }

    #[test]
    fn operator_len_pipe_at_end_of_slice() {
        assert_eq!(shell_operator_len(b"|", 0), Some(1));
    }

    #[test]
    fn split_empty_input() {
        let segments = split_shell_operators("");
        assert!(segments.is_empty());
    }

    #[test]
    fn split_only_operators() {
        let segments = split_shell_operators("&&||;");
        assert!(segments.iter().all(|s| s.is_empty()));
    }

    #[test]
    fn split_single_quoted_operators_preserved() {
        let segments = split_shell_operators("echo 'a && b || c'");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], "echo 'a && b || c'");
    }

    #[test]
    fn split_double_quoted_operators_preserved() {
        let segments = split_shell_operators("echo \"a | b ; c\"");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], "echo \"a | b ; c\"");
    }

    #[test]
    fn split_nested_single_in_double_quotes() {
        let segments = split_shell_operators("echo \"it's fine\" && jest");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].trim(), "jest");
    }

    #[test]
    fn split_nested_double_in_single_quotes() {
        let segments = split_shell_operators("echo 'say \"hello\"' && jest");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].trim(), "jest");
    }

    #[test]
    fn split_no_operators() {
        let segments = split_shell_operators("webpack --mode production");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], "webpack --mode production");
    }

    #[test]
    fn split_trailing_operator() {
        let segments = split_shell_operators("server &");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], "server ");
    }

    #[test]
    fn split_mixed_operators() {
        let segments = split_shell_operators("a && b || c ; d | e & f");
        assert_eq!(segments.len(), 6);
        assert_eq!(segments[0].trim(), "a");
        assert_eq!(segments[1].trim(), "b");
        assert_eq!(segments[2].trim(), "c");
        assert_eq!(segments[3].trim(), "d");
        assert_eq!(segments[4].trim(), "e");
        assert_eq!(segments[5].trim(), "f");
    }

    #[test]
    fn skip_wrappers_no_wrappers() {
        let tokens = vec!["webpack", "--mode", "production"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), Some(0));
    }

    #[test]
    fn skip_wrappers_env_prefix() {
        let tokens = vec!["env", "NODE_ENV=production", "webpack"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), Some(2));
    }

    #[test]
    fn skip_wrappers_cross_env_prefix() {
        let tokens = vec!["cross-env", "NODE_ENV=production", "webpack"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), Some(2));
    }

    #[test]
    fn skip_wrappers_dotenv_with_separator() {
        let tokens = vec!["dotenv", "--", "webpack"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), Some(2));
    }

    #[test]
    fn skip_wrappers_env_var_only() {
        let tokens = vec!["NODE_ENV=production", "CI=true"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), None);
    }

    #[test]
    fn skip_wrappers_cross_env_only() {
        let tokens = vec!["cross-env", "NODE_ENV=production"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), None);
    }

    #[test]
    fn skip_wrappers_multiple_env_vars_then_binary() {
        let tokens = vec!["NODE_ENV=test", "CI=true", "DEBUG=1", "jest"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), Some(3));
    }

    #[test]
    fn skip_wrappers_starting_at_nonzero_index() {
        let tokens = vec!["ignored", "cross-env", "NODE_ENV=prod", "webpack"];
        assert_eq!(skip_initial_wrappers(&tokens, 1), Some(3));
    }

    #[test]
    fn advance_npm_run_returns_none() {
        let tokens = vec!["npm", "run", "build"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_npm_run_script_returns_none() {
        let tokens = vec!["npm", "run-script", "test"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_yarn_bare_returns_none() {
        let tokens = vec!["yarn", "build"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_yarn_exec() {
        let tokens = vec!["yarn", "exec", "jest", "--coverage"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(2));
    }

    #[test]
    fn advance_pnpm_exec() {
        let tokens = vec!["pnpm", "exec", "vitest", "run"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(2));
    }

    #[test]
    fn advance_pnpm_dlx() {
        let tokens = vec!["pnpm", "dlx", "create-react-app"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(2));
    }

    #[test]
    fn advance_npx_simple() {
        let tokens = vec!["npx", "eslint", "src"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(1));
    }

    #[test]
    fn advance_npx_with_flags() {
        let tokens = vec!["npx", "--yes", "--package", "@scope/tool", "eslint"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(4));
    }

    #[test]
    fn advance_pnpx_simple() {
        let tokens = vec!["pnpx", "vitest"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(1));
    }

    #[test]
    fn advance_bunx_simple() {
        let tokens = vec!["bunx", "esbuild", "src/index.ts"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(1));
    }

    #[test]
    fn advance_no_package_manager() {
        let tokens = vec!["webpack", "--mode", "production"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(0));
    }

    #[test]
    fn advance_bare_npm_returns_none() {
        let tokens = vec!["npm"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bare_yarn_returns_none() {
        let tokens = vec!["yarn"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_npx_with_only_flags() {
        let tokens = vec!["npx", "--yes"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bun_exec() {
        let tokens = vec!["bun", "exec", "jest"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(2));
    }

    #[test]
    fn advance_bun_run_returns_none() {
        let tokens = vec!["bun", "run", "dev"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bun_runtime_flag_then_binary() {
        let tokens = vec!["bun", "--bun", "prek", "install"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(2));
    }

    #[test]
    fn advance_bun_multiple_runtime_flags_then_binary() {
        let tokens = vec!["bun", "--bun", "--watch", "prek"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(3));
    }

    #[test]
    fn advance_bun_runtime_flag_then_run_is_script() {
        let tokens = vec!["bun", "--watch", "run", "dev"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bun_x_executes_binary() {
        let tokens = vec!["bun", "x", "cowsay"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(2));
    }

    #[test]
    fn advance_bun_unknown_leading_flag_returns_none() {
        let tokens = vec!["bun", "--filter", "foo", "run", "build"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bun_bare_name_returns_none() {
        let tokens = vec!["bun", "scripts/build.ts"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bun_runtime_flag_only_returns_none() {
        let tokens = vec!["bun", "--watch"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }
}
