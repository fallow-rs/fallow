/// Check if an import specifier is a virtual module that does not correspond to a real file.
///
/// The `virtual:` prefix is a convention established by Vite and widely adopted across
/// the JS/TS bundler ecosystem. Plugins create virtual modules with this prefix
/// (e.g., `virtual:pwa-register`, `virtual:uno.css`, `virtual:generated-pages`).
/// These should never be flagged as unlisted dependencies or unresolved imports.
pub fn is_virtual_module(name: &str) -> bool {
    name.starts_with("virtual:")
}

/// Check if a package name is a platform built-in module (Node.js, Bun, Deno, Cloudflare Workers).
pub fn is_builtin_module(name: &str) -> bool {
    // Bun built-in modules (e.g., `bun`, `bun:sqlite`, `bun:test`, `bun:ffi`)
    if name == "bun" || name.starts_with("bun:") {
        return true;
    }
    // Cloudflare Workers built-in modules (e.g., `cloudflare:workers`, `cloudflare:sockets`)
    if name.starts_with("cloudflare:") {
        return true;
    }
    // Sass/SCSS built-in modules (e.g., `sass:math`, `sass:string`, `sass:color`).
    // Imported via `@use 'sass:string'` and provided by the Sass compiler itself,
    // never installed via npm. See issue #104.
    if name.starts_with("sass:") {
        return true;
    }
    // Deno standard library, imported as bare `std` or subpaths like `std/path`.
    // The `jsr:` / `npm:` Deno import schemes (e.g. `jsr:@std/path`) are handled
    // earlier, in the resolver: `jsr:` and URL specifiers resolve to an external
    // file and `npm:<pkg>` normalizes to its package name, so neither reaches the
    // unresolved-import path that consults this predicate. See issue #624.
    if name == "std" || name.starts_with("std/") {
        return true;
    }
    // k6 runtime modules (e.g., `k6`, `k6/http`, `k6/execution`) are provided
    // by the k6 load-test runtime, not npm resolution.
    if name == "k6" || name.starts_with("k6/") {
        return true;
    }
    // Node built-in modules importable with OR without the `node:` prefix. The
    // mandatory-`node:`-prefix family (`sqlite`, `sea`, `test`, `test/reporters`)
    // is intentionally absent here; see `NODE_PREFIX_ONLY_BUILTINS` below.
    let builtins = [
        "assert",
        "assert/strict",
        "async_hooks",
        "buffer",
        "child_process",
        "cluster",
        "console",
        "constants",
        "crypto",
        "dgram",
        "diagnostics_channel",
        "dns",
        "dns/promises",
        "domain",
        "events",
        "fs",
        "fs/promises",
        "http",
        "http2",
        "https",
        "inspector",
        "inspector/promises",
        "module",
        "net",
        "os",
        "path",
        "path/posix",
        "path/win32",
        "perf_hooks",
        "process",
        "punycode",
        "querystring",
        "readline",
        "readline/promises",
        "repl",
        "stream",
        "stream/consumers",
        "stream/promises",
        "stream/web",
        "string_decoder",
        "sys",
        "timers",
        "timers/promises",
        "tls",
        "trace_events",
        "tty",
        "url",
        "util",
        "util/types",
        "v8",
        "vm",
        "wasi",
        "worker_threads",
        "zlib",
    ];
    // Node built-in modules that ONLY exist with a mandatory `node:` prefix. Node
    // refuses to resolve these as bare specifiers, and real npm packages share the
    // bare names (`sqlite`, `sea`, `test`), so they are classified as builtins only
    // when the `node:` prefix is present.
    // https://nodejs.org/api/modules.html#built-in-modules-with-mandatory-node-prefix
    const NODE_PREFIX_ONLY_BUILTINS: &[&str] = &["sea", "sqlite", "test", "test/reporters"];

    if let Some(stripped) = name.strip_prefix("node:") {
        // With the `node:` prefix, both the always-available list and the
        // mandatory-prefix list are valid builtin specifiers.
        return builtins.contains(&stripped) || NODE_PREFIX_ONLY_BUILTINS.contains(&stripped);
    }
    // Bare specifier (no `node:` prefix): only the always-available list matches.
    // Mandatory-prefix modules written without the prefix are real npm packages,
    // not builtins. All known builtins and their subpaths (fs/promises, path/posix,
    // stream/consumers, etc.) are listed explicitly in the array above. No fallback
    // root-segment matching: it would false-positive on npm packages like
    // test-utils, url-parse, path-browserify, stream-browserify, events-emitter.
    builtins.contains(&name)
}

/// Dependencies that are used implicitly (not via imports).
pub(in crate::analyze) fn is_implicit_dependency(name: &str) -> bool {
    if name.starts_with("@types/") {
        return true;
    }

    // Framework runtime dependencies that are used implicitly (e.g., JSX runtime,
    // bundler injection) and never appear as explicit imports in source code.
    let implicit_deps = [
        "react-dom",
        "react-dom/client",
        "react-native",
        "@next/font",
        "@next/mdx",
        "@next/bundle-analyzer",
        "@next/env",
        // WebSocket optional native addons (peer deps of ws)
        "utf-8-validate",
        "bufferutil",
    ];
    implicit_deps.contains(&name)
}

/// Check if a package name looks like a TypeScript path alias rather than an npm package.
///
/// Common patterns: `@/components`, `@app/utils`, `~/lib`, `#internal/module`,
/// `@Components/Button` (`PascalCase` tsconfig paths).
/// These are typically defined in tsconfig.json `paths` or package.json `imports`.
pub(in crate::analyze) fn is_path_alias(name: &str) -> bool {
    // `#` prefix is Node.js imports maps (package.json "imports" field)
    if name.starts_with('#') {
        return true;
    }
    // `~/`, `~~/`, and `@@/` are common alias conventions
    // (e.g., Nuxt, custom tsconfig)
    if name.starts_with("~/") || name.starts_with("~~/") || name.starts_with("@@/") {
        return true;
    }
    // `@/` is a very common path alias (e.g., `@/components/Foo`)
    if name.starts_with("@/") {
        return true;
    }
    // npm scoped packages MUST be lowercase (npm registry requirement).
    // PascalCase `@Scope` or `@Scope/path` patterns are tsconfig path aliases,
    // not npm packages. E.g., `@Components`, `@Hooks/useApi`, `@Services/auth`.
    if name.starts_with('@') {
        let scope = name.split('/').next().unwrap_or(name);
        if scope.len() > 1 && scope.chars().nth(1).is_some_and(|c| c.is_ascii_uppercase()) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // is_builtin_module tests
    #[test]
    fn builtin_module_fs() {
        assert!(is_builtin_module("fs"));
    }

    #[test]
    fn builtin_module_path() {
        assert!(is_builtin_module("path"));
    }

    #[test]
    fn builtin_module_with_node_prefix() {
        assert!(is_builtin_module("node:fs"));
        assert!(is_builtin_module("node:path"));
        assert!(is_builtin_module("node:crypto"));
    }

    #[test]
    fn builtin_module_all_known() {
        let known = [
            "assert",
            "buffer",
            "child_process",
            "cluster",
            "console",
            "constants",
            "crypto",
            "dgram",
            "dns",
            "domain",
            "events",
            "fs",
            "http",
            "http2",
            "https",
            "module",
            "net",
            "os",
            "path",
            "perf_hooks",
            "process",
            "punycode",
            "querystring",
            "readline",
            "repl",
            "stream",
            "string_decoder",
            "sys",
            "timers",
            "tls",
            "tty",
            "url",
            "util",
            "v8",
            "vm",
            "wasi",
            "worker_threads",
            "zlib",
        ];
        for name in &known {
            assert!(is_builtin_module(name), "{name} should be a builtin module");
        }
    }

    #[test]
    fn not_builtin_module() {
        assert!(!is_builtin_module("react"));
        assert!(!is_builtin_module("lodash"));
        assert!(!is_builtin_module("express"));
        assert!(!is_builtin_module("@scope/pkg"));
    }

    #[test]
    fn not_builtin_similar_names() {
        assert!(!is_builtin_module("filesystem"));
        assert!(!is_builtin_module("pathlib"));
        assert!(!is_builtin_module("node:react"));
    }

    /// Regression: npm packages whose name starts with a Node builtin name
    /// (e.g., "test-utils", "url-parse") must not be classified as builtins.
    #[test]
    fn not_builtin_npm_packages_with_builtin_prefix() {
        assert!(!is_builtin_module("test-utils/helpers"));
        assert!(!is_builtin_module("url-parse"));
        assert!(!is_builtin_module("path-browserify"));
        assert!(!is_builtin_module("stream-browserify"));
        assert!(!is_builtin_module("events-emitter"));
        assert!(!is_builtin_module("util-deprecate"));
        assert!(!is_builtin_module("os-tmpdir"));
        assert!(!is_builtin_module("net-ping"));
    }

    #[test]
    fn builtin_k6_runtime_modules() {
        assert!(is_builtin_module("k6"));
        assert!(is_builtin_module("k6/http"));
        assert!(is_builtin_module("k6/execution"));
    }

    #[test]
    fn not_builtin_k6_similar_packages() {
        assert!(!is_builtin_module("k6-tools"));
        assert!(!is_builtin_module("k6ish"));
        assert!(!is_builtin_module("k6x/http"));
        assert!(!is_builtin_module("@types/k6"));
    }

    // is_implicit_dependency tests
    #[test]
    fn implicit_dep_types_packages() {
        assert!(is_implicit_dependency("@types/node"));
        assert!(is_implicit_dependency("@types/react"));
        assert!(is_implicit_dependency("@types/jest"));
    }

    #[test]
    fn not_implicit_dep() {
        assert!(!is_implicit_dependency("react"));
        assert!(!is_implicit_dependency("@scope/types"));
        assert!(!is_implicit_dependency("types"));
        assert!(!is_implicit_dependency("typescript"));
        assert!(!is_implicit_dependency("prettier"));
        assert!(!is_implicit_dependency("eslint"));
    }

    // is_tooling_dependency tests
    #[test]
    fn tooling_dep_prefixes() {
        assert!(crate::plugins::is_known_tooling_dependency("@types/node"));
        assert!(crate::plugins::is_known_tooling_dependency("prettier"));
        assert!(crate::plugins::is_known_tooling_dependency("husky"));
        assert!(crate::plugins::is_known_tooling_dependency("lint-staged"));
        assert!(crate::plugins::is_known_tooling_dependency("commitlint"));
        assert!(crate::plugins::is_known_tooling_dependency(
            "@commitlint/config-conventional"
        ));
        assert!(crate::plugins::is_known_tooling_dependency("stylelint"));
    }

    #[test]
    fn tooling_dep_plugin_handled_not_blanket() {
        // These prefixes removed — handled by plugin config parsing
        assert!(!crate::plugins::is_known_tooling_dependency("eslint"));
        assert!(!crate::plugins::is_known_tooling_dependency(
            "eslint-plugin-react"
        ));
        assert!(!crate::plugins::is_known_tooling_dependency(
            "@typescript-eslint/parser"
        ));
        assert!(!crate::plugins::is_known_tooling_dependency("postcss"));
        assert!(!crate::plugins::is_known_tooling_dependency("autoprefixer"));
        assert!(!crate::plugins::is_known_tooling_dependency("tailwindcss"));
        assert!(!crate::plugins::is_known_tooling_dependency(
            "@tailwindcss/forms"
        ));
    }

    #[test]
    fn tooling_dep_exact_matches() {
        assert!(crate::plugins::is_known_tooling_dependency("typescript"));
        assert!(crate::plugins::is_known_tooling_dependency("prettier"));
        assert!(crate::plugins::is_known_tooling_dependency("turbo"));
        assert!(crate::plugins::is_known_tooling_dependency("concurrently"));
        assert!(crate::plugins::is_known_tooling_dependency("cross-env"));
        assert!(crate::plugins::is_known_tooling_dependency("rimraf"));
        assert!(crate::plugins::is_known_tooling_dependency("npm-run-all"));
        assert!(crate::plugins::is_known_tooling_dependency("nodemon"));
        assert!(crate::plugins::is_known_tooling_dependency("ts-node"));
        assert!(crate::plugins::is_known_tooling_dependency("tsx"));
    }

    #[test]
    fn not_tooling_dep() {
        assert!(!crate::plugins::is_known_tooling_dependency("react"));
        assert!(!crate::plugins::is_known_tooling_dependency("next"));
        assert!(!crate::plugins::is_known_tooling_dependency("lodash"));
        assert!(!crate::plugins::is_known_tooling_dependency("express"));
        assert!(!crate::plugins::is_known_tooling_dependency(
            "@emotion/react"
        ));
    }

    // New tooling dependency tests (Issue 2)
    #[test]
    fn tooling_dep_testing_frameworks() {
        assert!(crate::plugins::is_known_tooling_dependency("jest"));
        assert!(crate::plugins::is_known_tooling_dependency("vitest"));
        assert!(crate::plugins::is_known_tooling_dependency("@jest/globals"));
        assert!(crate::plugins::is_known_tooling_dependency(
            "@vitest/coverage-v8"
        ));
        assert!(crate::plugins::is_known_tooling_dependency(
            "@testing-library/react"
        ));
        assert!(crate::plugins::is_known_tooling_dependency(
            "@testing-library/jest-dom"
        ));
        assert!(crate::plugins::is_known_tooling_dependency(
            "@playwright/test"
        ));
    }

    #[test]
    fn tooling_dep_environments_and_cli() {
        assert!(crate::plugins::is_known_tooling_dependency("happy-dom"));
        assert!(crate::plugins::is_known_tooling_dependency("jsdom"));
        assert!(crate::plugins::is_known_tooling_dependency("knip"));
    }

    // is_path_alias tests
    #[test]
    fn path_alias_at_slash() {
        assert!(is_path_alias("@/components"));
    }

    #[test]
    fn path_alias_tilde() {
        assert!(is_path_alias("~/lib"));
    }

    #[test]
    fn path_alias_hash_imports_map() {
        assert!(is_path_alias("#internal/module"));
    }

    #[test]
    fn path_alias_pascal_case_scope() {
        assert!(is_path_alias("@Components/Button"));
    }

    #[test]
    fn not_path_alias_regular_package() {
        assert!(!is_path_alias("react"));
    }

    #[test]
    fn not_path_alias_scoped_npm_package() {
        assert!(!is_path_alias("@scope/pkg"));
    }

    #[test]
    fn not_path_alias_emotion_react() {
        assert!(!is_path_alias("@emotion/react"));
    }

    #[test]
    fn not_path_alias_lodash() {
        assert!(!is_path_alias("lodash"));
    }

    #[test]
    fn not_path_alias_lowercase_short_scope() {
        assert!(!is_path_alias("@s/lowercase"));
    }

    // is_virtual_module tests
    #[test]
    fn virtual_module_vite_convention() {
        assert!(is_virtual_module("virtual:pwa-register"));
        assert!(is_virtual_module("virtual:pwa-register/react"));
        assert!(is_virtual_module("virtual:uno.css"));
        assert!(is_virtual_module("virtual:unocss"));
        assert!(is_virtual_module("virtual:generated-layouts"));
        assert!(is_virtual_module("virtual:generated-pages"));
        assert!(is_virtual_module("virtual:icons/mdi/home"));
        assert!(is_virtual_module("virtual:windi.css"));
        assert!(is_virtual_module("virtual:windi-devtools"));
        assert!(is_virtual_module("virtual:svg-icons-register"));
        assert!(is_virtual_module("virtual:remix/server-build"));
        assert!(is_virtual_module("virtual:emoji-mart-lang-importer"));
    }

    #[test]
    fn not_virtual_module() {
        assert!(!is_virtual_module("react"));
        assert!(!is_virtual_module("lodash"));
        assert!(!is_virtual_module("@scope/pkg"));
        assert!(!is_virtual_module("node:fs"));
        assert!(!is_virtual_module("cloudflare:workers"));
    }

    // ---------------------------------------------------------------
    // is_path_alias edge cases
    // ---------------------------------------------------------------

    #[test]
    fn path_alias_pascal_case_scopes() {
        assert!(is_path_alias("@Components/Button"));
        assert!(is_path_alias("@Hooks/useApi"));
        assert!(is_path_alias("@Services/auth"));
        assert!(is_path_alias("@Utils/format"));
        assert!(is_path_alias("@Lib/helpers"));
    }

    #[test]
    fn path_alias_hash_imports() {
        // All hash-prefixed imports are treated as path aliases
        // (Node.js package.json "imports" field or custom aliases)
        assert!(is_path_alias("#/utils"));
        assert!(is_path_alias("#subpath"));
        assert!(is_path_alias("#internal/module"));
        assert!(is_path_alias("#lib"));
        assert!(is_path_alias("#components/Button"));
    }

    #[test]
    fn path_alias_tilde_imports() {
        assert!(is_path_alias("~/components"));
        assert!(is_path_alias("~/lib/helpers"));
        assert!(is_path_alias("~/utils/format"));
        assert!(is_path_alias("~/styles/theme"));
        assert!(is_path_alias("~~/shared/theme"));
        assert!(is_path_alias("@@/shared/theme"));
    }

    /// Tilde without slash is NOT a path alias — it's a bare specifier.
    #[test]
    fn not_path_alias_bare_tilde() {
        assert!(!is_path_alias("~some-package"));
        assert!(!is_path_alias("~"));
    }

    #[test]
    fn path_alias_at_slash_subpaths() {
        assert!(is_path_alias("@/components"));
        assert!(is_path_alias("@/utils/helpers"));
        assert!(is_path_alias("@/lib/api/client"));
        assert!(is_path_alias("@/styles"));
    }

    #[test]
    fn not_path_alias_regular_npm_packages() {
        assert!(!is_path_alias("lodash"));
        assert!(!is_path_alias("react"));
        assert!(!is_path_alias("express"));
        assert!(!is_path_alias("next"));
        assert!(!is_path_alias("typescript"));
        assert!(!is_path_alias("zod"));
    }

    #[test]
    fn not_path_alias_scoped_npm_packages() {
        assert!(!is_path_alias("@types/node"));
        assert!(!is_path_alias("@types/react"));
        assert!(!is_path_alias("@babel/core"));
        assert!(!is_path_alias("@babel/preset-env"));
        assert!(!is_path_alias("@emotion/react"));
        assert!(!is_path_alias("@emotion/styled"));
        assert!(!is_path_alias("@tanstack/react-query"));
        assert!(!is_path_alias("@testing-library/react"));
        assert!(!is_path_alias("@nestjs/core"));
        assert!(!is_path_alias("@prisma/client"));
    }

    /// The PascalCase heuristic checks the second character (after `@`).
    /// Single-char scope names like `@s/pkg` are lowercase and thus not aliases.
    #[test]
    fn not_path_alias_edge_case_scopes() {
        assert!(!is_path_alias("@s/lowercase"));
        assert!(!is_path_alias("@a/package"));
        assert!(!is_path_alias("@x/something"));
    }

    /// Bare `@` without a slash is not a valid npm scope — but it's also
    /// not detected as a path alias because `@` alone has no uppercase second char.
    #[test]
    fn not_path_alias_bare_at_sign() {
        assert!(!is_path_alias("@"));
    }

    // ---------------------------------------------------------------
    // Builtin module edge cases
    // ---------------------------------------------------------------

    /// Subpath imports of builtins should be recognized.
    #[test]
    fn builtin_module_subpath_imports() {
        assert!(is_builtin_module("assert/strict"));
        assert!(is_builtin_module("dns/promises"));
        assert!(is_builtin_module("fs/promises"));
        assert!(is_builtin_module("path/posix"));
        assert!(is_builtin_module("path/win32"));
        assert!(is_builtin_module("readline/promises"));
        assert!(is_builtin_module("stream/consumers"));
        assert!(is_builtin_module("stream/promises"));
        assert!(is_builtin_module("stream/web"));
        assert!(is_builtin_module("timers/promises"));
        assert!(is_builtin_module("util/types"));
        assert!(is_builtin_module("inspector/promises"));
        // `test/reporters` is a mandatory-`node:`-prefix module; the bare form is
        // covered as a non-builtin in `not_builtin_module_bare_prefix_only_names`.
    }

    /// Subpath builtins with `node:` prefix.
    #[test]
    fn builtin_module_subpath_with_node_prefix() {
        assert!(is_builtin_module("node:fs/promises"));
        assert!(is_builtin_module("node:path/posix"));
        assert!(is_builtin_module("node:stream/web"));
        assert!(is_builtin_module("node:timers/promises"));
        assert!(is_builtin_module("node:util/types"));
        assert!(is_builtin_module("node:test/reporters"));
    }

    /// Mandatory-`node:`-prefix builtins are recognized only with the prefix.
    /// Node documents these as built-in modules that require the `node:` scheme.
    /// See issue #627.
    #[test]
    fn builtin_module_node_prefix_only() {
        assert!(is_builtin_module("node:sqlite"));
        assert!(is_builtin_module("node:sea"));
        assert!(is_builtin_module("node:test"));
        assert!(is_builtin_module("node:test/reporters"));
    }

    /// The bare forms of mandatory-`node:`-prefix modules are NOT builtins: Node
    /// refuses to resolve them without the prefix, and real npm packages share the
    /// names. npm packages that merely start with one of the names stay deps too.
    /// See issue #627.
    #[test]
    fn not_builtin_module_bare_prefix_only_names() {
        assert!(!is_builtin_module("sqlite"));
        assert!(!is_builtin_module("sea"));
        assert!(!is_builtin_module("test"));
        assert!(!is_builtin_module("test/reporters"));
        // npm packages with a prefix-only builtin as a name prefix.
        assert!(!is_builtin_module("sqlite3"));
        assert!(!is_builtin_module("better-sqlite3"));
        assert!(!is_builtin_module("node-sqlite3"));
        assert!(!is_builtin_module("seamless"));
        assert!(!is_builtin_module("test-utils"));
        // Unknown names under the `node:` scheme are still not builtins.
        assert!(!is_builtin_module("node:sqlite3"));
        assert!(!is_builtin_module("node:not-a-builtin"));
    }

    /// Bun built-in modules.
    #[test]
    fn builtin_module_bun() {
        assert!(is_builtin_module("bun"));
        assert!(is_builtin_module("bun:sqlite"));
        assert!(is_builtin_module("bun:test"));
        assert!(is_builtin_module("bun:ffi"));
        assert!(is_builtin_module("bun:jsc"));
    }

    /// npm packages whose names merely start with `bun` are not Bun builtins.
    #[test]
    fn not_builtin_module_bun_like_packages() {
        assert!(!is_builtin_module("bun-types"));
        assert!(!is_builtin_module("bunyan"));
        assert!(!is_builtin_module("@types/bun"));
        assert!(!is_builtin_module("bun-types/path"));
    }

    /// Cloudflare Workers built-in modules.
    #[test]
    fn builtin_module_cloudflare_workers() {
        assert!(is_builtin_module("cloudflare:workers"));
        assert!(is_builtin_module("cloudflare:sockets"));
        assert!(is_builtin_module("cloudflare:email"));
    }

    /// Deno standard library.
    #[test]
    fn builtin_module_deno_std() {
        assert!(is_builtin_module("std"));
        assert!(is_builtin_module("std/path"));
        assert!(is_builtin_module("std/fs"));
    }

    /// Sass/SCSS built-in modules. Imported via `@use 'sass:math'` etc. and
    /// provided by the Sass compiler. Never installed via npm. See issue #104.
    #[test]
    fn builtin_module_sass() {
        assert!(is_builtin_module("sass:math"));
        assert!(is_builtin_module("sass:string"));
        assert!(is_builtin_module("sass:color"));
        assert!(is_builtin_module("sass:list"));
        assert!(is_builtin_module("sass:map"));
        assert!(is_builtin_module("sass:meta"));
        assert!(is_builtin_module("sass:selector"));
    }

    /// npm packages that merely start with `sass` (not the built-in prefix) must
    /// still be treated as npm dependencies. The guard is the `sass:` prefix, not
    /// the substring `sass`.
    #[test]
    fn not_builtin_module_sass_like_packages() {
        assert!(!is_builtin_module("sass"));
        assert!(!is_builtin_module("sass-loader"));
        assert!(!is_builtin_module("@types/sass"));
    }

    /// Non-existent subpath builtins should not match.
    #[test]
    fn not_builtin_module_fake_subpaths() {
        assert!(!is_builtin_module("fs/extra"));
        assert!(!is_builtin_module("path/utils"));
        assert!(!is_builtin_module("stream/transform"));
    }

    // ---------------------------------------------------------------
    // is_virtual_module edge cases
    // ---------------------------------------------------------------

    /// Empty string and prefix-only edge cases.
    #[test]
    fn virtual_module_edge_cases() {
        assert!(is_virtual_module("virtual:"));
        assert!(!is_virtual_module(""));
        assert!(!is_virtual_module("Virtual:something"));
        assert!(!is_virtual_module("VIRTUAL:something"));
    }

    // ---------------------------------------------------------------
    // is_implicit_dependency edge cases
    // ---------------------------------------------------------------

    #[test]
    fn implicit_dep_react_dom_and_native() {
        assert!(is_implicit_dependency("react-dom"));
        assert!(is_implicit_dependency("react-dom/client"));
        assert!(is_implicit_dependency("react-native"));
    }

    #[test]
    fn implicit_dep_next_packages() {
        assert!(is_implicit_dependency("@next/font"));
        assert!(is_implicit_dependency("@next/mdx"));
        assert!(is_implicit_dependency("@next/bundle-analyzer"));
        assert!(is_implicit_dependency("@next/env"));
    }

    #[test]
    fn implicit_dep_websocket_native_addons() {
        assert!(is_implicit_dependency("utf-8-validate"));
        assert!(is_implicit_dependency("bufferutil"));
    }

    /// Packages that look similar to implicit deps but are NOT.
    #[test]
    fn not_implicit_dep_similar_names() {
        assert!(!is_implicit_dependency("react"));
        assert!(!is_implicit_dependency("react-dom-extra"));
        assert!(!is_implicit_dependency("@next/swc"));
        assert!(!is_implicit_dependency("react-native-web"));
        assert!(!is_implicit_dependency("@types"));
    }

    // ---------------------------------------------------------------
    // is_path_alias additional coverage
    // ---------------------------------------------------------------

    #[test]
    fn path_alias_hash_prefix() {
        assert!(is_path_alias("#internal/module"));
        assert!(is_path_alias("#app/utils"));
    }

    #[test]
    fn path_alias_tilde_prefix() {
        assert!(is_path_alias("~/store/auth"));
    }

    #[test]
    fn path_alias_at_slash_prefix() {
        assert!(is_path_alias("@/hooks/useAuth"));
    }

    #[test]
    fn path_alias_pascal_case_scope_additional() {
        assert!(is_path_alias("@Hooks/useAuth"));
        assert!(is_path_alias("@Components/Button"));
        assert!(is_path_alias("@Services/api"));
    }

    #[test]
    fn not_path_alias_lowercase_scoped_packages() {
        assert!(!is_path_alias("@angular/core"));
        assert!(!is_path_alias("@emotion/styled"));
        assert!(!is_path_alias("@tanstack/react-query"));
    }

    #[test]
    fn not_path_alias_bare_packages() {
        assert!(!is_path_alias("react"));
        assert!(!is_path_alias("lodash"));
        assert!(!is_path_alias("express"));
    }

    // ---------------------------------------------------------------
    // is_virtual_module
    // ---------------------------------------------------------------

    #[test]
    fn virtual_module_prefix() {
        assert!(is_virtual_module("virtual:pwa-register"));
        assert!(is_virtual_module("virtual:uno.css"));
        assert!(is_virtual_module("virtual:generated-pages"));
    }

    #[test]
    fn not_virtual_module_non_virtual_imports() {
        assert!(!is_virtual_module("react"));
        assert!(!is_virtual_module("@virtual/package"));
        assert!(!is_virtual_module("./virtual-file"));
    }
}
