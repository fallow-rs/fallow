//! Firebase plugin.
//!
//! Detects Firebase projects and marks the default Firebase Messaging service
//! worker file as always used.

use super::Plugin;

const ENABLERS: &[&str] = &["firebase"];

const ALWAYS_USED: &[&str] = &[
    "public/firebase-messaging-sw.js",
    "**/public/firebase-messaging-sw.js",
];

define_plugin! {
    struct FirebasePlugin => "firebase",
    enablers: ENABLERS,
    always_used: ALWAYS_USED,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn protects_root_and_nested_messaging_workers() {
        let plugin = FirebasePlugin;

        assert_eq!(
            plugin.always_used(),
            &[
                "public/firebase-messaging-sw.js",
                "**/public/firebase-messaging-sw.js"
            ]
        );
    }

    #[test]
    fn enables_on_exact_firebase_dependency() {
        let plugin = FirebasePlugin;
        let deps = vec!["firebase".to_string()];

        assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn does_not_enable_on_firebase_subpackages() {
        let plugin = FirebasePlugin;
        let deps = vec!["@firebase/messaging".to_string()];

        assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }
}
