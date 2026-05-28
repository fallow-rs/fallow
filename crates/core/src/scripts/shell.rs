//! Shell tokenization: splitting on operators, skipping env wrappers and package managers.

use super::ENV_WRAPPERS;

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
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < len {
        let b = bytes[i];

        // Toggle quote state
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

        // Inside quotes — skip everything
        if in_single_quote || in_double_quote {
            i += 1;
            continue;
        }

        // Try to match a shell operator and split on it
        if let Some(op_len) = shell_operator_len(bytes, i) {
            segments.push(&script[start..i]);
            i += op_len;
            start = i;
            continue;
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

    // Two-character operators: && ||
    if matches!((b, next), (b'&', Some(b'&')) | (b'|', Some(b'|'))) {
        return Some(2);
    }

    // Single-character operators: ; | &
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
    // Skip env var assignments (KEY=value pairs)
    while idx < tokens.len() && super::is_env_assignment(tokens[idx]) {
        idx += 1;
    }
    if idx >= tokens.len() {
        return None;
    }

    // Skip env wrapper commands (cross-env, dotenv, env)
    while idx < tokens.len() && ENV_WRAPPERS.contains(&tokens[idx]) {
        idx += 1;
        // Skip env var assignments after the wrapper
        while idx < tokens.len() && super::is_env_assignment(tokens[idx]) {
            idx += 1;
        }
        // dotenv uses -- as separator
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
        // Skip npx flags (--yes, --no-install, -p, --package)
        while idx < tokens.len() && tokens[idx].starts_with('-') {
            let flag = tokens[idx];
            idx += 1;
            // --package <name> consumes the next argument
            if matches!(flag, "--package" | "-p") && idx < tokens.len() {
                idx += 1;
            }
        }
    } else if token == "bun" {
        // `bun` is both a script runner and a direct executor:
        //   bun <script> / bun run <script>   -> named script (skip)
        //   bun exec <bin> / bun x <pkg>      -> executes a binary
        //   bun --bun <bin> ...               -> runtime flags, then a binary to run
        // Skip known boolean runtime flags, then classify the target. An unknown
        // leading flag is treated as a script delegation (skip) rather than guessed
        // at, since we cannot tell whether it consumes the following token as a value.
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
        } else if matches!(subcmd, "run" | "run-script") {
            // Delegates to a named script, not a binary invocation
            return None;
        } else if !saw_runtime_flag {
            // Bare `bun <name>` (or `bun --unknown-flag ...`) runs a script; skip.
            return None;
        }
        // else: `bun --bun <bin>`, the post-flag target is a binary invocation.
    } else if matches!(token, "yarn" | "pnpm" | "npm") {
        if idx + 1 < tokens.len() {
            let subcmd = tokens[idx + 1];
            if subcmd == "exec" || subcmd == "dlx" {
                idx += 2;
            } else if matches!(subcmd, "run" | "run-script") {
                // Delegates to a named script, not a binary invocation
                return None;
            } else {
                // Bare `yarn <name>` runs a script — skip
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

    // --- shell_operator_len ---

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
    fn operator_len_semicolon_at_end() {
        assert_eq!(shell_operator_len(b";", 0), Some(1));
    }

    // --- split_shell_operators ---

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

    // --- skip_initial_wrappers ---

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

    // --- advance_past_package_manager ---

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
        // `bun --bun prek install`: --bun forces the bun runtime for the
        // executed binary; `prek` is the binary, not a script.
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
        // Bun documents `bun --watch run dev` (flag before `run`); the target
        // is still a named script, so nothing is credited.
        let tokens = vec!["bun", "--watch", "run", "dev"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bun_x_executes_binary() {
        // `bun x <pkg>` is the bun-native alias of `bunx <pkg>`.
        let tokens = vec!["bun", "x", "cowsay"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(2));
    }

    #[test]
    fn advance_bun_unknown_leading_flag_returns_none() {
        // `--filter` consumes a value; an unrecognized leading flag is treated
        // as a script delegation rather than guessed at (conservative: avoid
        // crediting the flag value as a package).
        let tokens = vec!["bun", "--filter", "foo", "run", "build"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bun_bare_name_returns_none() {
        // Bare `bun <name>` runs a script, like `yarn <name>`.
        let tokens = vec!["bun", "scripts/build.ts"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bun_runtime_flag_only_returns_none() {
        let tokens = vec!["bun", "--watch"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }
}
