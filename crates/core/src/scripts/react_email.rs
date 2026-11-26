//! The `email` binary of React Email (`react-email`).
//!
//! `email dev`, `email build`, `email start` and `email export` read every
//! email template in one directory. `--dir` / `-d` names the directory, and
//! the default is `emails`. React Email walks the subdirectories too and
//! reads `.js`, `.jsx` and `.tsx` files. The templates are entry points.

/// The name of the React Email binary.
const BINARY: &str = "email";

/// The subcommands that read the templates of the email directory.
const TEMPLATE_SUBCOMMANDS: &[&str] = &["dev", "build", "start", "export"];

/// The flags whose value is the email directory.
const DIR_FLAGS: &[&str] = &["-d", "--dir"];

/// The email directory when the command gives no `--dir`.
const DEFAULT_DIR: &str = "emails";

/// The template files in the email directory.
const TEMPLATE_GLOB: &str = "**/*.{js,jsx,tsx}";

/// Return `true` when `name` is the React Email binary.
pub(super) fn is_react_email(name: &str) -> bool {
    name == BINARY
}

/// Return the template glob of an `email` command, or nothing when the
/// subcommand reads no templates or the directory is not static.
pub(super) fn template_globs(args: &[&str]) -> Vec<String> {
    let Some(subcommand) = args.iter().find(|arg| !arg.starts_with('-')) else {
        return Vec::new();
    };
    if !TEMPLATE_SUBCOMMANDS.contains(subcommand) {
        return Vec::new();
    }
    let dir = email_dir(args).unwrap_or(DEFAULT_DIR);
    if dir.is_empty() || dir.contains(['$', '*', '{', '`']) {
        return Vec::new();
    }
    let dir = dir.trim_end_matches('/');
    vec![format!("{dir}/{TEMPLATE_GLOB}")]
}

fn email_dir<'a>(args: &[&'a str]) -> Option<&'a str> {
    let mut iter = args.iter();
    while let Some(&arg) = iter.next() {
        if DIR_FLAGS.contains(&arg) {
            return iter.next().copied();
        }
        if let Some(value) = DIR_FLAGS
            .iter()
            .find_map(|flag| arg.strip_prefix(flag)?.strip_prefix('='))
        {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_flag_names_the_template_folder() {
        assert_eq!(
            template_globs(&["dev", "-p", "3050", "--dir", "src/emails"]),
            vec!["src/emails/**/*.{js,jsx,tsx}"]
        );
        assert_eq!(
            template_globs(&["build", "-d", "./mail/"]),
            vec!["./mail/**/*.{js,jsx,tsx}"]
        );
        assert_eq!(
            template_globs(&["export", "--dir=templates"]),
            vec!["templates/**/*.{js,jsx,tsx}"]
        );
    }

    #[test]
    fn default_folder_is_emails() {
        assert_eq!(template_globs(&["dev"]), vec!["emails/**/*.{js,jsx,tsx}"]);
    }

    #[test]
    fn other_subcommands_and_dynamic_folders_give_nothing() {
        assert!(template_globs(&[]).is_empty());
        assert!(template_globs(&["--help"]).is_empty());
        assert!(template_globs(&["upgrade"]).is_empty());
        assert!(template_globs(&["dev", "--dir", "$EMAIL_DIR"]).is_empty());
    }
}
