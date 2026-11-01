//! The message clauses of the `plugin-config-unreadable` kind.

/// The clause naming why a plugin could not read a config key in full, for one
/// `plugin-config-unreadable` reason token.
///
/// The token set is open, so an unrecognised token renders the general claim
/// rather than nothing: a diagnostic from a plugin added later still reads as a
/// sentence.
pub(super) fn unreadable_situation(reason: &str) -> &'static str {
    match reason {
        "array-form" => "uses the array form, which is not read yet",
        "spread" => "spreads a value that is not statically readable",
        "unreadable-entries" => "has entries that hold no statically readable value",
        "not-object-literal" => "is not a static object literal",
        "unrecognized-call" => "is passed through a call that is not a known config wrapper",
        "import-target-unreadable" => "comes from an imported file that is not statically readable",
        "dynamic-argument" => "receives an argument that is not a static literal",
        _ => "could not be read statically",
    }
}

/// What an unread config key costs, and the configuration option that covers
/// the gap.
///
/// Keyed on the config KEY rather than on the plugin name, because one key is
/// read by several plugins: Module Federation `exposes` and `remotes` reach a
/// build from a standalone config file and inline from the webpack, rspack,
/// rsbuild and vite configs, and both the consequence and the remedy are the
/// same in all five. A key this build does not know falls back to the general
/// claim rather than borrowing another key's remedy, so a plugin added later
/// still renders a sentence that is true.
///
/// Two reasons change the remedy. An unrecognized call was read as a lower
/// bound, so only what the call adds is missing. An unreadable import target
/// holds config that is shared across files, so the remedy names the option
/// and does not ask for an object literal.
pub(super) fn unreadable_key_consequence(key: &str, reason: &str) -> (&'static str, &'static str) {
    match (key, reason) {
        ("exposes", "unrecognized-call") => (
            "only the targets in the object literal it receives are registered as entry points",
            "Name any other exposed files in `dynamicallyLoaded`.",
        ),
        ("remotes", "unrecognized-call") => (
            "only the aliases in the object literal it receives are treated as provided by a \
             remote container",
            "Name any other aliases in `ignoreDependencies`.",
        ),
        ("exposes", "import-target-unreadable") => (
            "the targets that file declares are not registered as entry points",
            "Name the exposed files in `dynamicallyLoaded`.",
        ),
        ("remotes", "import-target-unreadable") => (
            "the aliases that file declares are not treated as provided by a remote container",
            "Name the aliases in `ignoreDependencies`.",
        ),
        ("registerRemotes", _) => (
            "the remotes it registers are not treated as provided by a remote container",
            "Name the remote aliases in `ignoreDependencies`, or pass the remote names as \
             string literals.",
        ),
        ("init" | "createInstance", _) => (
            "the remotes its options declare are not treated as provided by a remote container",
            "Name the remote aliases in `ignoreDependencies`, or pass `remotes` as an array of \
             objects with literal names.",
        ),
        ("loadRemote", _) => (
            "the remote it loads is not treated as provided by a remote container",
            "Name the remote alias in `ignoreDependencies`, or pass the request as a string \
             literal.",
        ),
        ("exposes", _) => (
            "the targets are not registered as entry points",
            "Name the exposed files in `dynamicallyLoaded`.",
        ),
        ("remotes", _) => (
            "the aliases are not treated as provided by a remote container",
            "Name the aliases in `ignoreDependencies`, or declare them as the keys of an object \
             literal, whose values may be computed.",
        ),
        _ => (
            "what it declares is not fully registered",
            "Declare the value as a static object literal.",
        ),
    }
}
