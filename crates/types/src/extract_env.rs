//! Env var-name predicates that decide whether an env read is public by
//! convention. `crate::extract` re-exports every public item.

/// Env var-name prefixes that frameworks inline into the client bundle by
/// convention. A read of one of these is normal and safe, so it does NOT count
/// as a secret source (issue #890). Shared by the extract layer (so public env
/// vars never become source signals) and the bespoke `client-server-leak` rule.
pub const PUBLIC_ENV_PREFIXES: &[&str] = &[
    "NEXT_PUBLIC_",
    "VITE_",
    "NUXT_PUBLIC_",
    "REACT_APP_",
    "PUBLIC_",
    "GATSBY_",
    "EXPO_PUBLIC_",
    "STORYBOOK_",
];

/// Exact env var names that are public by convention (no prefix).
pub const PUBLIC_ENV_EXACT: &[&str] = &["NODE_ENV"];

/// Env var-name tokens that usually describe public build or deployment
/// metadata rather than secrets. Secret-shaped names win over these tokens.
pub const PUBLIC_ENV_METADATA_TOKENS: &[&str] =
    &["BRANCH", "ENVIRONMENT", "MODE", "REF", "SHA", "TAG"];

/// Env var-name tokens that should keep a variable source-backed even when the
/// name also contains public metadata tokens such as `REF` or `SHA`.
pub const SECRET_ENV_TOKENS: &[&str] = &[
    "AUTH",
    "CREDENTIAL",
    "CREDENTIALS",
    "KEY",
    "PASS",
    "PASSWORD",
    "PRIVATE",
    "SECRET",
    "TOKEN",
];

fn env_name_has_token(name: &str, tokens: &[&str]) -> bool {
    name.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .any(|part| tokens.contains(&part))
}

/// Whether an env var name is public-by-convention (build-inlined into the
/// client bundle), and therefore not a secret.
#[must_use]
pub fn is_public_env_var(name: &str) -> bool {
    if PUBLIC_ENV_EXACT.contains(&name) || PUBLIC_ENV_PREFIXES.iter().any(|p| name.starts_with(p)) {
        return true;
    }
    env_name_has_token(name, PUBLIC_ENV_METADATA_TOKENS)
        && !env_name_has_token(name, SECRET_ENV_TOKENS)
}

/// Whether a flattened member path is a PUBLIC env-secret read
/// (`process.env.NEXT_PUBLIC_X`, `import.meta.env.VITE_Y`), which must not be
/// recorded as a secret source. Non-env paths (`req.query.id`) are never public.
#[must_use]
pub fn is_public_env_path(path: &str) -> bool {
    for object in ["process.env.", "import.meta.env."] {
        if let Some(var) = path.strip_prefix(object) {
            return is_public_env_var(var);
        }
    }
    false
}
