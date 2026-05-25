//! RedwoodSDK plugin.
//!
//! RedwoodSDK apps built with `rwsdk/vite` use `src/worker.*` as the
//! Cloudflare worker entrypoint by convention. The existing Vite plugin owns
//! `vite.config.*` parsing so this plugin only contributes RedwoodSDK's runtime
//! worker convention.

use super::Plugin;

const ENABLERS: &[&str] = &["rwsdk"];

const ENTRY_PATTERNS: &[&str] = &["src/worker.{ts,tsx,js,jsx,mts,mjs}"];

define_plugin! {
    struct RedwoodSdkPlugin => "redwoodsdk",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_patterns_cover_worker_js_like_extensions() {
        let plugin = RedwoodSdkPlugin;

        assert!(
            plugin
                .entry_patterns()
                .contains(&"src/worker.{ts,tsx,js,jsx,mts,mjs}")
        );
    }

    #[test]
    fn does_not_claim_vite_config_patterns() {
        let plugin = RedwoodSdkPlugin;

        assert!(
            plugin.config_patterns().is_empty(),
            "Vite config parsing belongs to the Vite plugin to avoid active-plugin collisions"
        );
    }
}
