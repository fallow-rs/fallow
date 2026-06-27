//! Next.js framework plugin.
//!
//! Detects Next.js projects and marks App Router/Pages Router convention files,
//! middleware, instrumentation, and metadata files as entry points.
//! Parses next.config to extract pageExtensions and referenced dependencies.

#[cfg(test)]
use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

const REACT_COMPILER_BABEL_PLUGIN: &str = "babel-plugin-react-compiler";

const PAGE_EXPORTS: &[&str] = &[
    "default",
    "metadata",
    "generateMetadata",
    "viewport",
    "generateViewport",
    "generateStaticParams",
    "dynamic",
    "dynamicParams",
    "revalidate",
    "fetchCache",
    "runtime",
    "preferredRegion",
    "maxDuration",
];
const LAYOUT_EXPORTS: &[&str] = &[
    "default",
    "metadata",
    "generateMetadata",
    "viewport",
    "generateViewport",
    "generateStaticParams",
    "dynamic",
    "dynamicParams",
    "revalidate",
    "fetchCache",
    "runtime",
    "preferredRegion",
    "maxDuration",
];
const ROUTE_EXPORTS: &[&str] = &[
    "GET",
    "POST",
    "PUT",
    "PATCH",
    "DELETE",
    "HEAD",
    "OPTIONS",
    "dynamic",
    "dynamicParams",
    "revalidate",
    "fetchCache",
    "runtime",
    "preferredRegion",
    "maxDuration",
];
const PAGES_ROUTER_EXPORTS: &[&str] = &[
    "default",
    "getStaticProps",
    "getStaticPaths",
    "getServerSideProps",
    "config",
];
const PAGES_APP_EXPORTS: &[&str] = &["default", "reportWebVitals"];
const PAGES_API_EXPORTS: &[&str] = &["default", "config"];
const DEFAULT_ONLY_EXPORTS: &[&str] = &["default"];
const MIDDLEWARE_EXPORTS: &[&str] = &["default", "middleware", "config"];
const PROXY_EXPORTS: &[&str] = &["default", "proxy", "config"];
const INSTRUMENTATION_EXPORTS: &[&str] = &["register", "onRequestError"];
const INSTRUMENTATION_CLIENT_EXPORTS: &[&str] = &["onRouterTransitionStart"];
const MDX_COMPONENT_EXPORTS: &[&str] = &["useMDXComponents"];
const ICON_EXPORTS: &[&str] = &["default", "size", "contentType", "generateImageMetadata"];
const OG_IMAGE_EXPORTS: &[&str] = &[
    "default",
    "size",
    "contentType",
    "generateImageMetadata",
    "alt",
];
const MANIFEST_EXPORTS: &[&str] = &["default"];
const SITEMAP_EXPORTS: &[&str] = &["default", "generateSitemaps"];
const ROBOTS_EXPORTS: &[&str] = &["default"];
const GLOBAL_NOT_FOUND_EXPORTS: &[&str] = &["default", "metadata", "generateMetadata"];

define_plugin!(
    struct NextJsPlugin => "nextjs",
    enablers: &["next"],
    entry_patterns: &[
        "app/**/page.{ts,tsx,js,jsx}",
        "app/**/layout.{ts,tsx,js,jsx}",
        "app/**/loading.{ts,tsx,js,jsx}",
        "app/**/error.{ts,tsx,js,jsx}",
        "app/**/not-found.{ts,tsx,js,jsx}",
        "app/**/template.{ts,tsx,js,jsx}",
        "app/**/default.{ts,tsx,js,jsx}",
        "app/**/route.{ts,tsx,js,jsx}",
        "app/**/global-error.{ts,tsx,js,jsx}",
        "app/**/forbidden.{ts,tsx,js,jsx}",
        "app/**/unauthorized.{ts,tsx,js,jsx}",
        "app/global-not-found.{ts,tsx,js,jsx}",
        "app/**/opengraph-image.{ts,tsx,js,jsx}",
        "app/**/twitter-image.{ts,tsx,js,jsx}",
        "app/**/icon.{ts,tsx,js,jsx}",
        "app/**/apple-icon.{ts,tsx,js,jsx}",
        "app/**/manifest.{ts,tsx,js,jsx}",
        "app/**/sitemap.{ts,tsx,js,jsx}",
        "app/**/robots.{ts,tsx,js,jsx}",
        "pages/**/*.{ts,tsx,js,jsx}",
        "src/app/**/page.{ts,tsx,js,jsx}",
        "src/app/**/layout.{ts,tsx,js,jsx}",
        "src/app/**/loading.{ts,tsx,js,jsx}",
        "src/app/**/error.{ts,tsx,js,jsx}",
        "src/app/**/not-found.{ts,tsx,js,jsx}",
        "src/app/**/template.{ts,tsx,js,jsx}",
        "src/app/**/default.{ts,tsx,js,jsx}",
        "src/app/**/route.{ts,tsx,js,jsx}",
        "src/app/**/global-error.{ts,tsx,js,jsx}",
        "src/app/**/forbidden.{ts,tsx,js,jsx}",
        "src/app/**/unauthorized.{ts,tsx,js,jsx}",
        "src/app/global-not-found.{ts,tsx,js,jsx}",
        "src/app/**/opengraph-image.{ts,tsx,js,jsx}",
        "src/app/**/twitter-image.{ts,tsx,js,jsx}",
        "src/app/**/icon.{ts,tsx,js,jsx}",
        "src/app/**/apple-icon.{ts,tsx,js,jsx}",
        "src/app/**/manifest.{ts,tsx,js,jsx}",
        "src/app/**/sitemap.{ts,tsx,js,jsx}",
        "src/app/**/robots.{ts,tsx,js,jsx}",
        "src/pages/**/*.{ts,tsx,js,jsx}",
        "middleware.{ts,js}",
        "src/middleware.{ts,js}",
        "proxy.{ts,js}",
        "src/proxy.{ts,js}",
        "instrumentation.{ts,js}",
        "instrumentation-client.{ts,js}",
        "src/instrumentation.{ts,js}",
        "src/instrumentation-client.{ts,js}",
    ],
    config_patterns: &["next.config.{ts,js,mjs,cjs}"],
    always_used: &[
        "next.config.{ts,js,mjs,cjs}",
        "next-env.d.ts",
        "favicon.ico",
        "mdx-components.{ts,tsx,js,jsx}",
        "src/mdx-components.{ts,tsx,js,jsx}",
        "src/i18n/request.{ts,js}",
        "src/i18n/routing.{ts,js}",
        "i18n/request.{ts,js}",
        "i18n/routing.{ts,js}",
    ],
    tooling_dependencies: &[
        "next",
        "@next/font",
        "@next/mdx",
        "@next/bundle-analyzer",
        "@next/env",
        "server-only",
        "client-only",
    ],
    used_exports: [
        ("app/**/page.{ts,tsx,js,jsx}", PAGE_EXPORTS),
        ("app/**/layout.{ts,tsx,js,jsx}", LAYOUT_EXPORTS),
        ("app/**/loading.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("app/**/error.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("app/**/not-found.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("app/**/template.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("app/**/default.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("app/**/route.{ts,tsx,js,jsx}", ROUTE_EXPORTS),
        ("app/**/global-error.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("app/**/forbidden.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("app/**/unauthorized.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("app/global-not-found.{ts,tsx,js,jsx}", GLOBAL_NOT_FOUND_EXPORTS),
        ("pages/**/*.{ts,tsx,js,jsx}", PAGES_ROUTER_EXPORTS),
        ("pages/_app.{ts,tsx,js,jsx}", PAGES_APP_EXPORTS),
        ("pages/api/**/*.{ts,tsx,js,jsx}", PAGES_API_EXPORTS),
        ("src/app/**/page.{ts,tsx,js,jsx}", PAGE_EXPORTS),
        ("src/app/**/layout.{ts,tsx,js,jsx}", LAYOUT_EXPORTS),
        ("src/app/**/loading.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("src/app/**/error.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("src/app/**/not-found.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("src/app/**/template.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("src/app/**/default.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("src/app/**/route.{ts,tsx,js,jsx}", ROUTE_EXPORTS),
        ("src/app/**/global-error.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("src/app/**/forbidden.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("src/app/**/unauthorized.{ts,tsx,js,jsx}", DEFAULT_ONLY_EXPORTS),
        ("src/app/global-not-found.{ts,tsx,js,jsx}", GLOBAL_NOT_FOUND_EXPORTS),
        ("src/pages/**/*.{ts,tsx,js,jsx}", PAGES_ROUTER_EXPORTS),
        ("src/pages/_app.{ts,tsx,js,jsx}", PAGES_APP_EXPORTS),
        ("src/pages/api/**/*.{ts,tsx,js,jsx}", PAGES_API_EXPORTS),
        ("middleware.{ts,js}", MIDDLEWARE_EXPORTS),
        ("src/middleware.{ts,js}", MIDDLEWARE_EXPORTS),
        ("proxy.{ts,js}", PROXY_EXPORTS),
        ("src/proxy.{ts,js}", PROXY_EXPORTS),
        ("instrumentation.{ts,js}", INSTRUMENTATION_EXPORTS),
        ("src/instrumentation.{ts,js}", INSTRUMENTATION_EXPORTS),
        ("instrumentation-client.{ts,js}", INSTRUMENTATION_CLIENT_EXPORTS),
        ("src/instrumentation-client.{ts,js}", INSTRUMENTATION_CLIENT_EXPORTS),
        ("mdx-components.{ts,tsx,js,jsx}", MDX_COMPONENT_EXPORTS),
        ("src/mdx-components.{ts,tsx,js,jsx}", MDX_COMPONENT_EXPORTS),
        ("app/**/icon.{ts,tsx,js,jsx}", ICON_EXPORTS),
        ("app/**/apple-icon.{ts,tsx,js,jsx}", ICON_EXPORTS),
        ("app/**/opengraph-image.{ts,tsx,js,jsx}", OG_IMAGE_EXPORTS),
        ("app/**/twitter-image.{ts,tsx,js,jsx}", OG_IMAGE_EXPORTS),
        ("app/**/manifest.{ts,tsx,js,jsx}", MANIFEST_EXPORTS),
        ("app/**/sitemap.{ts,tsx,js,jsx}", SITEMAP_EXPORTS),
        ("app/**/robots.{ts,tsx,js,jsx}", ROBOTS_EXPORTS),
        ("src/app/**/icon.{ts,tsx,js,jsx}", ICON_EXPORTS),
        ("src/app/**/apple-icon.{ts,tsx,js,jsx}", ICON_EXPORTS),
        ("src/app/**/opengraph-image.{ts,tsx,js,jsx}", OG_IMAGE_EXPORTS),
        ("src/app/**/twitter-image.{ts,tsx,js,jsx}", OG_IMAGE_EXPORTS),
        ("src/app/**/manifest.{ts,tsx,js,jsx}", MANIFEST_EXPORTS),
        ("src/app/**/sitemap.{ts,tsx,js,jsx}", SITEMAP_EXPORTS),
        ("src/app/**/robots.{ts,tsx,js,jsx}", ROBOTS_EXPORTS),
    ],
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        let page_extensions =
            config_parser::extract_config_string_array(source, config_path, &["pageExtensions"]);
        if !page_extensions.is_empty() {
            let ext_str = page_extensions.join(",");
            let base_patterns = [
                "app/**/page",
                "app/**/layout",
                "app/**/loading",
                "app/**/error",
                "app/**/not-found",
                "app/**/template",
                "app/**/default",
                "app/**/route",
                "app/**/global-error",
                "app/**/forbidden",
                "app/**/unauthorized",
                "app/global-not-found",
                "pages/**/*",
                "pages/_app",
                "pages/_document",
                "pages/api/**/*",
                "middleware",
                "proxy",
                "instrumentation",
                "instrumentation-client",
                "src/app/**/page",
                "src/app/**/layout",
                "src/app/**/loading",
                "src/app/**/error",
                "src/app/**/not-found",
                "src/app/**/template",
                "src/app/**/default",
                "src/app/**/route",
                "src/app/**/global-error",
                "src/app/**/forbidden",
                "src/app/**/unauthorized",
                "src/app/global-not-found",
                "src/pages/**/*",
                "src/pages/_app",
                "src/pages/_document",
                "src/pages/api/**/*",
                "src/middleware",
                "src/proxy",
                "src/instrumentation",
                "src/instrumentation-client",
            ];
            for base in &base_patterns {
                result.push_entry_pattern(format!("{base}.{{{ext_str}}}"));
            }
        }

        let transpile_packages =
            config_parser::extract_config_string_array(source, config_path, &["transpilePackages"]);
        for package in &transpile_packages {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(package));
        }

        if config_parser::extract_config_truthy_bool_or_object(
            source,
            config_path,
            &["reactCompiler"],
        ) {
            result
                .referenced_dependencies
                .push(REACT_COMPILER_BABEL_PLUGIN.to_string());
        }

        result
    },
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabler_is_next() {
        let plugin = NextJsPlugin;
        assert_eq!(plugin.enablers(), &["next"]);
    }

    #[test]
    fn is_enabled_with_next_dep() {
        let plugin = NextJsPlugin;
        let deps = vec!["next".to_string(), "react".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn is_not_enabled_without_next() {
        let plugin = NextJsPlugin;
        let deps = vec!["react".to_string(), "react-dom".to_string()];
        assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn entry_patterns_include_app_router_and_pages() {
        let plugin = NextJsPlugin;
        let patterns = plugin.entry_patterns();
        assert!(patterns.iter().any(|p| p.contains("app/**/page")));
        assert!(patterns.iter().any(|p| p.contains("pages/**/*")));
        assert!(patterns.iter().any(|p| p.contains("middleware")));
        assert!(patterns.iter().any(|p| p.contains("global-not-found")));
    }

    #[test]
    fn entry_patterns_include_src_variants() {
        let plugin = NextJsPlugin;
        let patterns = plugin.entry_patterns();
        assert!(patterns.iter().any(|p| p.starts_with("src/app")));
        assert!(patterns.iter().any(|p| p.starts_with("src/pages")));
        assert!(patterns.contains(&"src/middleware.{ts,js}"));
    }

    #[test]
    fn config_patterns_match_next_config() {
        let plugin = NextJsPlugin;
        let patterns = plugin.config_patterns();
        assert_eq!(patterns, &["next.config.{ts,js,mjs,cjs}"]);
    }

    #[test]
    fn always_used_includes_mdx_component_provider() {
        let plugin = NextJsPlugin;
        let patterns = plugin.always_used();
        assert!(patterns.contains(&"mdx-components.{ts,tsx,js,jsx}"));
        assert!(patterns.contains(&"src/mdx-components.{ts,tsx,js,jsx}"));
    }

    #[test]
    fn used_exports_includes_route_http_methods() {
        let plugin = NextJsPlugin;
        let exports = plugin.used_exports();
        let route_entry = exports
            .iter()
            .find(|(pat, _)| *pat == "app/**/route.{ts,tsx,js,jsx}");
        assert!(route_entry.is_some(), "should have route file used exports");
        let (_, methods) = route_entry.unwrap();
        assert!(methods.contains(&"GET"));
        assert!(methods.contains(&"POST"));
        assert!(methods.contains(&"DELETE"));
        assert!(methods.contains(&"runtime"));
        assert!(methods.contains(&"revalidate"));
    }

    #[test]
    fn used_exports_include_segment_config_and_special_files() {
        let plugin = NextJsPlugin;
        let exports = plugin.used_exports();

        let page_entry = exports
            .iter()
            .find(|(pat, _)| *pat == "app/**/page.{ts,tsx,js,jsx}")
            .expect("should have app page used exports");
        assert!(page_entry.1.contains(&"revalidate"));
        assert!(page_entry.1.contains(&"viewport"));
        assert!(page_entry.1.contains(&"generateMetadata"));

        let proxy_entry = exports
            .iter()
            .find(|(pat, _)| *pat == "src/proxy.{ts,js}")
            .expect("should have proxy used exports");
        assert!(proxy_entry.1.contains(&"proxy"));
        assert!(proxy_entry.1.contains(&"config"));

        let instrumentation_entry = exports
            .iter()
            .find(|(pat, _)| *pat == "instrumentation.{ts,js}")
            .expect("should have instrumentation used exports");
        assert!(instrumentation_entry.1.contains(&"register"));
        assert!(instrumentation_entry.1.contains(&"onRequestError"));

        let loading_entry = exports
            .iter()
            .find(|(pat, _)| *pat == "app/**/loading.{ts,tsx,js,jsx}")
            .expect("should have loading used exports");
        assert!(loading_entry.1.contains(&"default"));

        let mdx_entry = exports
            .iter()
            .find(|(pat, _)| *pat == "mdx-components.{ts,tsx,js,jsx}")
            .expect("should have mdx-components used exports");
        assert!(mdx_entry.1.contains(&"useMDXComponents"));
    }

    #[test]
    fn resolve_config_page_extensions() {
        let source = r#"
            export default {
                pageExtensions: ["tsx", "mdx"]
            };
        "#;
        let plugin = NextJsPlugin;
        let result =
            plugin.resolve_config(Path::new("next.config.ts"), source, Path::new("/project"));
        assert!(
            !result.entry_patterns.is_empty(),
            "pageExtensions should generate entry patterns"
        );
        assert!(
            result.entry_patterns.iter().any(|p| p.contains("tsx,mdx")),
            "entry patterns should use the custom extensions: {:?}",
            result.entry_patterns
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|p| p.starts_with("app/**/page")),
            "should include app router page pattern"
        );
        assert!(
            result.entry_patterns.iter().any(|p| p.starts_with("proxy")),
            "should include proxy when pageExtensions is customized"
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|p| p.starts_with("app/global-not-found")),
            "should include global-not-found when pageExtensions is customized"
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|p| p.starts_with("pages/api/**/*")),
            "should include pages/api when pageExtensions is customized"
        );
    }

    #[test]
    fn resolve_config_page_extensions_wrapped_named_const() {
        // The official @next/mdx idiom: a separate `const nextConfig` passed to a
        // wrapper call. The page entry patterns must still pick up the custom
        // mdx extension. Regression for #1642.
        let source = r#"
            import createMDX from "@next/mdx";
            const nextConfig = { pageExtensions: ["ts", "tsx", "md", "mdx"] };
            const withMDX = createMDX({});
            export default withMDX(nextConfig);
        "#;
        let plugin = NextJsPlugin;
        let result =
            plugin.resolve_config(Path::new("next.config.mjs"), source, Path::new("/project"));
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|p| p.starts_with("app/**/page") && p.contains("mdx")),
            "wrapped named-const config should still yield mdx page entry patterns: {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn resolve_config_page_extensions_includes_src_variants() {
        let source = r#"
            export default {
                pageExtensions: ["tsx"]
            };
        "#;
        let plugin = NextJsPlugin;
        let result =
            plugin.resolve_config(Path::new("next.config.ts"), source, Path::new("/project"));
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|p| p.starts_with("src/app")),
            "should include src/ variants"
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|p| p.starts_with("src/pages")),
            "should include src/pages variants"
        );
    }

    #[test]
    fn resolve_config_extracts_import_deps() {
        let source = r#"
            import withMDX from "@next/mdx";
            export default withMDX({});
        "#;
        let plugin = NextJsPlugin;
        let result =
            plugin.resolve_config(Path::new("next.config.mjs"), source, Path::new("/project"));
        assert!(
            result
                .referenced_dependencies
                .contains(&"@next/mdx".to_string()),
            "should extract @next/mdx as a referenced dependency"
        );
    }

    #[test]
    fn resolve_config_empty_source() {
        let source = "";
        let plugin = NextJsPlugin;
        let result =
            plugin.resolve_config(Path::new("next.config.ts"), source, Path::new("/project"));
        assert!(result.entry_patterns.is_empty());
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_no_page_extensions() {
        let source = r"
            export default {
                reactStrictMode: true
            };
        ";
        let plugin = NextJsPlugin;
        let result =
            plugin.resolve_config(Path::new("next.config.ts"), source, Path::new("/project"));
        assert!(
            result.entry_patterns.is_empty(),
            "no pageExtensions means no extra entry patterns"
        );
    }

    #[test]
    fn resolve_config_transpile_packages_are_referenced_dependencies() {
        let source = r#"
            export default {
                transpilePackages: ["@acme/ui", "lodash-es"]
            };
        "#;
        let plugin = NextJsPlugin;
        let result =
            plugin.resolve_config(Path::new("next.config.ts"), source, Path::new("/project"));
        assert!(
            result
                .referenced_dependencies
                .contains(&"@acme/ui".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"lodash-es".to_string())
        );
    }

    #[test]
    fn resolve_config_react_compiler_true_references_babel_plugin() {
        let source = r#"
            import { defineConfig } from "next/config";

            export default defineConfig({
                reactCompiler: true,
            });
        "#;
        let plugin = NextJsPlugin;
        let result =
            plugin.resolve_config(Path::new("next.config.ts"), source, Path::new("/project"));

        assert!(
            result
                .referenced_dependencies
                .contains(&REACT_COMPILER_BABEL_PLUGIN.to_string())
        );
    }

    #[test]
    fn resolve_config_react_compiler_object_references_babel_plugin() {
        let source = r#"
            module.exports = {
                reactCompiler: { compilationMode: "annotation" },
            };
        "#;
        let plugin = NextJsPlugin;
        let result =
            plugin.resolve_config(Path::new("next.config.js"), source, Path::new("/project"));

        assert!(
            result
                .referenced_dependencies
                .contains(&REACT_COMPILER_BABEL_PLUGIN.to_string())
        );
    }

    #[test]
    fn resolve_config_react_compiler_false_does_not_reference_babel_plugin() {
        let source = r"
            export default {
                reactCompiler: false,
            };
        ";
        let plugin = NextJsPlugin;
        let result =
            plugin.resolve_config(Path::new("next.config.ts"), source, Path::new("/project"));

        assert!(
            !result
                .referenced_dependencies
                .contains(&REACT_COMPILER_BABEL_PLUGIN.to_string())
        );
    }

    #[test]
    fn resolve_config_react_compiler_null_does_not_reference_babel_plugin() {
        let source = r"
            export default {
                reactCompiler: null,
            };
        ";
        let plugin = NextJsPlugin;
        let result =
            plugin.resolve_config(Path::new("next.config.ts"), source, Path::new("/project"));

        assert!(
            !result
                .referenced_dependencies
                .contains(&REACT_COMPILER_BABEL_PLUGIN.to_string())
        );
    }

    #[test]
    fn resolve_config_without_react_compiler_does_not_reference_babel_plugin() {
        let source = r"
            export default {
                reactStrictMode: true,
            };
        ";
        let plugin = NextJsPlugin;
        let result =
            plugin.resolve_config(Path::new("next.config.ts"), source, Path::new("/project"));

        assert!(
            !result
                .referenced_dependencies
                .contains(&REACT_COMPILER_BABEL_PLUGIN.to_string())
        );
    }

    #[test]
    fn tooling_dependencies_include_server_client_only() {
        let plugin = NextJsPlugin;
        let tooling = plugin.tooling_dependencies();
        assert!(tooling.contains(&"server-only"));
        assert!(tooling.contains(&"client-only"));
    }
}
