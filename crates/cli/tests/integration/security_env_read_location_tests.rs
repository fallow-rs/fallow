//! `client-server-leak` points at the secret read or the server-only import,
//! not at line 1, so `fallow security --gate new` fails when a change adds a
//! read or an import below line 1 of a client module that already existed
//! (issue #4881). A secret read in a React Router `loader` gives no candidate
//! in each read form (issue #4882).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap/expect to keep fixture setup concise"
)]

use crate::common::{commit_all, fallow_bin, git};
use std::path::Path;
use std::process::Command;

const USE_CLIENT_PACKAGE: &str = r#"{ "name": "uc", "private": true, "main": "src/index.ts" }"#;

const REACT_ROUTER_PACKAGE: &str = r#"{ "name": "rr", "private": true, "type": "module",
  "dependencies": { "react": "^19.0.0", "react-router": "^7.0.0" },
  "devDependencies": { "@react-router/dev": "^7.0.0" } }"#;

fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

/// A git repository with a `base` commit and a `change` commit on top.
fn repo(base: &[(&str, &str)], change: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    write(dir.path(), base);
    commit_all(dir.path(), "base");
    write(dir.path(), change);
    commit_all(dir.path(), "change");
    dir
}

fn react_router_base(home: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("package.json", REACT_ROUTER_PACKAGE),
        (
            "app/routes.ts",
            "import { index, type RouteConfig } from '@react-router/dev/routes'\n\n\
             export default [index('routes/home.tsx')] satisfies RouteConfig\n",
        ),
        (
            "app/root.tsx",
            "import { Outlet } from 'react-router'\n\n\
             export default function App() {\n\treturn <Outlet />\n}\n",
        ),
        ("app/routes/home.tsx", home),
    ]
}

/// Run `fallow security` with `args`. Returns the exit code and the JSON.
fn security(root: &Path, args: &[&str]) -> (i32, serde_json::Value) {
    let out = Command::new(fallow_bin())
        .arg("security")
        .args(args)
        .args(["--format", "json", "--quiet", "--no-cache"])
        .arg("--root")
        .arg(root)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("invalid JSON ({err}): {stdout}"));
    (crate::common::exit_code(&out), json)
}

fn gate_new(root: &Path) -> (i32, serde_json::Value) {
    security(root, &["--gate", "new", "--changed-since", "HEAD~1"])
}

/// The only finding of the output. Fails when there is not exactly one.
fn only_finding(json: &serde_json::Value) -> &serde_json::Value {
    let findings = json["security_findings"]
        .as_array()
        .expect("security_findings array");
    assert_eq!(findings.len(), 1, "{json:#}");
    &findings[0]
}

#[test]
fn gate_fails_on_a_secret_read_added_to_an_existing_use_client_file() {
    let dir = repo(
        &[
            ("package.json", USE_CLIENT_PACKAGE),
            ("src/index.ts", "export { Widget } from './widget'\n"),
            (
                "src/widget.tsx",
                "'use client'\n\nexport function Widget() { return null }\n",
            ),
        ],
        &[(
            "src/widget.tsx",
            "'use client'\n\nexport function Widget() {\n\
             \tconst key = process.env.API_SECRET_KEY\n\treturn key\n}\n",
        )],
    );

    let (_, json) = security(dir.path(), &[]);
    let finding = only_finding(&json);
    assert_eq!(finding["line"], 4, "{finding:#}");
    assert_eq!(finding["col"], 25, "{finding:#}");
    assert_eq!(finding["candidate"]["sink"]["line"], 4);
    let hop = &finding["trace"][0];
    assert_eq!(hop["role"], "secret-source");
    assert_eq!(hop["line"], 4);
    assert_eq!(
        hop["secret_reads"],
        serde_json::json!([{ "name": "process.env.API_SECRET_KEY", "line": 4, "col": 25 }])
    );

    let (code, json) = gate_new(dir.path());
    assert_eq!(code, 8, "{json:#}");
    assert_eq!(json["gate"]["new_count"], 1);
}

#[test]
fn gate_fails_on_a_secret_read_added_to_an_existing_route_module() {
    let dir = repo(
        &react_router_base("export default function Home() {\n\treturn <main>Home</main>\n}\n"),
        &[(
            "app/routes/home.tsx",
            "export default function Home() {\n\
             \tconst key = process.env.API_SECRET_KEY\n\
             \treturn <main data-key={key}>Home</main>\n}\n",
        )],
    );

    let (code, json) = gate_new(dir.path());
    assert_eq!(code, 8, "{json:#}");
    let finding = only_finding(&json);
    assert_eq!(finding["path"], "app/routes/home.tsx");
    assert_eq!(finding["line"], 2);
}

#[test]
fn gate_fails_on_a_secret_read_added_to_an_imported_module() {
    let mut base = react_router_base(
        "import { label } from '../components/shared'\n\n\
         export default function Home() {\n\treturn <main>{label}</main>\n}\n",
    );
    base.push(("app/components/shared.ts", "export const label = 'x'\n"));
    let dir = repo(
        &base,
        &[(
            "app/components/shared.ts",
            "export const label = 'x'\nexport const key = process.env.API_SECRET_KEY\n",
        )],
    );

    let (code, json) = gate_new(dir.path());
    assert_eq!(code, 8, "{json:#}");
    let finding = only_finding(&json);
    assert_eq!(finding["path"], "app/routes/home.tsx");
    let hop = &finding["trace"][1];
    assert_eq!(hop["role"], "secret-source");
    assert_eq!(hop["path"], "app/components/shared.ts");
    assert_eq!(hop["line"], 2);
}

#[test]
fn gate_fails_on_a_server_only_import_added_to_an_existing_use_client_file() {
    let dir = repo(
        &[
            ("package.json", USE_CLIENT_PACKAGE),
            ("src/index.ts", "export { Widget } from './widget'\n"),
            (
                "src/widget.tsx",
                "'use client'\n\nexport function Widget() { return null }\n",
            ),
        ],
        &[(
            "src/widget.tsx",
            "'use client'\nimport { readFileSync } from 'node:fs'\n\n\
             export function Widget() { return readFileSync('x', 'utf8') }\n",
        )],
    );

    let (code, json) = gate_new(dir.path());
    assert_eq!(code, 8, "{json:#}");
    let finding = only_finding(&json);
    assert_eq!(finding["category"], "server-only-import");
    assert_eq!(finding["line"], 2);
}

#[test]
fn gate_passes_when_a_change_leaves_an_old_secret_read_alone() {
    let read = "'use client'\n\nexport function Widget() {\n\
                \tconst key = process.env.API_SECRET_KEY\n\treturn key\n}\n";
    let dir = repo(
        &[
            ("package.json", USE_CLIENT_PACKAGE),
            ("src/index.ts", "export { Widget } from './widget'\n"),
            ("src/widget.tsx", read),
        ],
        &[(
            "src/widget.tsx",
            "'use client'\n\nexport function Widget() {\n\
             \tconst key = process.env.API_SECRET_KEY\n\treturn key\n}\n\
             export const retries = 3\n",
        )],
    );

    let (code, json) = gate_new(dir.path());
    assert_eq!(code, 0, "{json:#}");
}

#[test]
fn a_secret_read_in_a_loader_gives_no_candidate_in_any_form() {
    for read in [
        "const { API_SECRET_KEY } = process.env",
        "const API_SECRET_KEY = process.env['API_SECRET_KEY']",
        "const API_SECRET_KEY = process.env.API_SECRET_KEY",
    ] {
        let home = format!(
            "import {{ useLoaderData }} from 'react-router'\n\n\
             export function loader() {{\n\t{read}\n\
             \treturn {{ configured: Boolean(API_SECRET_KEY) }}\n}}\n\n\
             export default function Home() {{\n\
             \tconst {{ configured }} = useLoaderData<typeof loader>()\n\
             \treturn <main>{{configured ? 'yes' : 'no'}}</main>\n}}\n"
        );
        let dir = tempfile::tempdir().unwrap();
        let mut files = react_router_base("");
        files.retain(|(path, _)| *path != "app/routes/home.tsx");
        write(dir.path(), &files);
        write(dir.path(), &[("app/routes/home.tsx", &home)]);

        let (_, json) = security(dir.path(), &[]);
        let findings = json["security_findings"].as_array().unwrap();
        assert!(findings.is_empty(), "{read}: {json:#}");
    }
}

#[test]
fn a_destructured_secret_read_in_client_code_points_at_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let files = react_router_base(
        "export default function Home() {\n\
         \tconst { API_SECRET_KEY } = process.env\n\
         \treturn <main data-key={API_SECRET_KEY}>Home</main>\n}\n",
    );
    write(dir.path(), &files);

    let (_, json) = security(dir.path(), &[]);
    let finding = only_finding(&json);
    assert_eq!(finding["line"], 2);
    assert_eq!(finding["col"], 9);
}
