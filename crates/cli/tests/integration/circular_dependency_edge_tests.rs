//! The edges of a `circular_dependencies` finding: each edge exists in the
//! source, and it points at the statement that makes the hop of the cycle.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow_in_root};

fn write_project(package_json: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::write(root.join("package.json"), package_json).expect("write package");
    for (path, source) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        std::fs::write(path, source).expect("write source");
    }
    dir
}

/// `(path, line, col)` of each edge of each reported cycle.
fn cycle_edges(json: &serde_json::Value) -> Vec<Vec<(String, u64, u64)>> {
    json["circular_dependencies"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|finding| {
            finding["edges"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|edge| {
                    (
                        edge["path"].as_str().unwrap().to_owned(),
                        edge["line"].as_u64().unwrap(),
                        edge["col"].as_u64().unwrap(),
                    )
                })
                .collect()
        })
        .collect()
}

/// A barrel that re-exports a module which imports the barrel. The edge from
/// the barrel points at its `export ... from` statement, not at line 1 (#4737).
#[test]
fn re_export_edge_points_at_the_re_export_statement() {
    for re_export in [r#"export * from "./a";"#, r#"export { a } from "./a";"#] {
        let index =
            format!("/**\n * Header comment.\n */\n\n{re_export}\nexport * from \"./b\";\n");
        let dir = write_project(
            r#"{ "name": "r", "version": "1.0.0", "main": "src/index.ts" }"#,
            &[
                ("src/index.ts", index.as_str()),
                (
                    "src/a.ts",
                    "import { shared } from \"./index\";\n\nexport const a = shared;\n",
                ),
                (
                    "src/b.ts",
                    "export const b = 1;\nexport const shared = 2;\n",
                ),
            ],
        );
        let json = parse_json(&run_fallow_in_root(
            "dead-code",
            dir.path(),
            &["--format", "json", "--quiet"],
        ));
        let edges = cycle_edges(&json);
        assert_eq!(edges.len(), 1, "{re_export}: expected one cycle: {edges:?}");
        let index_edge = edges[0]
            .iter()
            .find(|(path, _, _)| path == "src/index.ts")
            .expect("edge from the barrel");
        assert_eq!(index_edge.1, 5, "{re_export}: edge line: {edges:?}");
    }
}

/// A line suppression above the re-export statement hides the cycle, the
/// same as one above an import.
#[test]
fn line_suppression_above_a_re_export_hides_the_cycle() {
    let dir = write_project(
        r#"{ "name": "r", "version": "1.0.0", "main": "src/index.ts" }"#,
        &[
            (
                "src/index.ts",
                "// fallow-ignore-next-line circular-dependency\nexport * from \"./a\";\nexport * from \"./b\";\n",
            ),
            (
                "src/a.ts",
                "import { shared } from \"./index\";\n\nexport const a = shared;\n",
            ),
            (
                "src/b.ts",
                "export const b = 1;\nexport const shared = 2;\n",
            ),
        ],
    );
    let json = parse_json(&run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet"],
    ));
    assert!(cycle_edges(&json).is_empty(), "{json}");
    assert_eq!(
        json["stale_suppressions"].as_array().map(Vec::len),
        Some(0),
        "{json}"
    );
}

/// The `tailwindcss` package itself: a stylesheet with `@tailwind` gets a
/// synthetic `tailwindcss` import, which resolves to the package entry. That
/// import must not close a cycle with the entry stylesheet (#4740).
#[test]
fn synthetic_tailwind_import_does_not_point_into_its_own_package() {
    let dir = write_project(
        r#"{"name":"tailwindcss","version":"4.0.0","private":true,"style":"index.css","exports":{".":{"style":"./index.css"}}}"#,
        &[
            ("index.css", "@import './utilities.css' layer(utilities);\n"),
            ("utilities.css", "@tailwind utilities;\n"),
        ],
    );
    let json = parse_json(&run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet"],
    ));
    assert!(cycle_edges(&json).is_empty(), "{json}");
}
