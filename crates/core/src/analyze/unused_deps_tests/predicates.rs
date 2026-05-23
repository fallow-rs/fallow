// ---- is_builtin_module tests (via predicates, used in find_unlisted_dependencies) ----

#[test]
fn builtin_module_subpaths() {
    assert!(super::super::super::predicates::is_builtin_module(
        "fs/promises"
    ));
    assert!(super::super::super::predicates::is_builtin_module(
        "stream/consumers"
    ));
    assert!(super::super::super::predicates::is_builtin_module(
        "node:fs/promises"
    ));
    assert!(super::super::super::predicates::is_builtin_module(
        "readline/promises"
    ));
}

#[test]
fn builtin_module_cloudflare_workers() {
    assert!(super::super::super::predicates::is_builtin_module(
        "cloudflare:workers"
    ));
    assert!(super::super::super::predicates::is_builtin_module(
        "cloudflare:sockets"
    ));
}

#[test]
fn builtin_module_bun() {
    assert!(super::super::super::predicates::is_builtin_module("bun"));
    assert!(super::super::super::predicates::is_builtin_module(
        "bun:sqlite"
    ));
    assert!(!super::super::super::predicates::is_builtin_module(
        "bun-types"
    ));
    assert!(!super::super::super::predicates::is_builtin_module(
        "@types/bun"
    ));
    assert!(!super::super::super::predicates::is_builtin_module(
        "bunyan"
    ));
}

#[test]
fn builtin_module_deno_std() {
    assert!(super::super::super::predicates::is_builtin_module("std"));
    assert!(super::super::super::predicates::is_builtin_module(
        "std/path"
    ));
}

// ---- is_implicit_dependency tests (used in find_unused_dependencies) ----

#[test]
fn implicit_dep_react_dom() {
    assert!(super::super::super::predicates::is_implicit_dependency(
        "react-dom"
    ));
    assert!(super::super::super::predicates::is_implicit_dependency(
        "react-dom/client"
    ));
}

#[test]
fn implicit_dep_next_packages() {
    assert!(super::super::super::predicates::is_implicit_dependency(
        "@next/font"
    ));
    assert!(super::super::super::predicates::is_implicit_dependency(
        "@next/mdx"
    ));
    assert!(super::super::super::predicates::is_implicit_dependency(
        "@next/bundle-analyzer"
    ));
    assert!(super::super::super::predicates::is_implicit_dependency(
        "@next/env"
    ));
}

#[test]
fn implicit_dep_websocket_addons() {
    assert!(super::super::super::predicates::is_implicit_dependency(
        "utf-8-validate"
    ));
    assert!(super::super::super::predicates::is_implicit_dependency(
        "bufferutil"
    ));
}

// ---- is_path_alias tests (used in find_unlisted_dependencies) ----

#[test]
fn path_alias_not_reported_as_unlisted() {
    // These should be detected as path aliases and skipped
    assert!(super::super::super::predicates::is_path_alias(
        "@/components/Foo"
    ));
    assert!(super::super::super::predicates::is_path_alias(
        "~/utils/helper"
    ));
    assert!(super::super::super::predicates::is_path_alias(
        "#internal/auth"
    ));
    assert!(super::super::super::predicates::is_path_alias(
        "@Components/Button"
    ));
}

#[test]
fn scoped_npm_packages_not_path_aliases() {
    assert!(!super::super::super::predicates::is_path_alias(
        "@angular/core"
    ));
    assert!(!super::super::super::predicates::is_path_alias(
        "@emotion/react"
    ));
    assert!(!super::super::super::predicates::is_path_alias(
        "@nestjs/common"
    ));
}
