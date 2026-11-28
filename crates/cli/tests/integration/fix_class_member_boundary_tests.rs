#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! A type-aware class member removal keeps the end of the member before it.
//! When the next kept member could continue that member, the fixer writes a
//! `;` in front of the kept member, and the post-edit check accepts the edit.

use std::path::Path;

use oxc_span::SourceType;

use crate::common::{parse_json, run_fallow_raw_with_type_aware_sidecar};

const CLASSES: &str = r#"export const k = Symbol("k");

export class Field {
  value = 1
  rmA(): void {}
  [k](): void {}
}

export class AfterMethod {
  keep(): void {}
  rmB(): void {}
  [k](): void {}
}

export class Typed {
  value!: number
  rmC(): void {}
  *[Symbol.iterator](): Generator<number> {
    yield 1;
  }
}

export class Between {
  rmD(): void {}
  keep(): void {}
  rmE(): void {}
  [k](): void {}
}

export class Word {
  value = 1
  rmF(): void {}
  as(): void {}
}

export class Indexed {
  [key: string]: unknown
  rmG(): void {}
  [k](): void {}
}

export class IndexedGenerator {
  [key: string]: unknown
  rmH(): void {}
  *[Symbol.iterator](): Generator<number> {
    yield 1;
  }
}
"#;

const FIXED: &str = r#"export const k = Symbol("k");

export class Field {
  value = 1
  ;[k](): void {}
}

export class AfterMethod {
  keep(): void {}
  ;[k](): void {}
}

export class Typed {
  value!: number
  ;*[Symbol.iterator](): Generator<number> {
    yield 1;
  }
}

export class Between {
  keep(): void {}
  ;[k](): void {}
}

export class Word {
  value = 1
  as(): void {}
}

export class Indexed {
  [key: string]: unknown
  ;[k](): void {}
}

export class IndexedGenerator {
  [key: string]: unknown
  ;*[Symbol.iterator](): Generator<number> {
    yield 1;
  }
}
"#;

const INDEX: &str = r#"import { k, Field, AfterMethod, Typed, Between, Word, Indexed, IndexedGenerator } from "./c.js";

console.log(k, new Field().value);
new AfterMethod().keep();
console.log(new Typed().value);
new Between().keep();
const word = new Word();
word.as();
console.log(word.value, new Indexed(), new IndexedGenerator());
"#;

fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"class-member-boundary","private":true,"type":"module","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"module":"nodenext","moduleResolution":"nodenext","strict":true,"target":"es2022"},"include":["src/**/*.ts"]}"#,
    )
    .unwrap();
    std::fs::write(root.join("src/c.ts"), CLASSES).unwrap();
    std::fs::write(root.join("src/index.ts"), INDEX).unwrap();
}

fn parses_as_typescript(text: &str) -> bool {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, text, SourceType::ts()).parse();
    !parsed.fatal_error && !parsed.diagnostics.has_errors()
}

#[test]
fn type_aware_fix_removes_members_before_a_member_that_could_join_the_line_above() {
    assert!(parses_as_typescript(CLASSES));
    assert!(parses_as_typescript(FIXED));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(root);
    let root_arg = root.to_string_lossy();

    let output = run_fallow_raw_with_type_aware_sidecar(&[
        "fix",
        "--root",
        &root_arg,
        "--type-aware",
        "--yes",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    let mut removed: Vec<&str> = json["fixes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|fix| fix["type"] == "remove_class_member" && fix["applied"] == true)
        .map(|fix| fix["name"].as_str().unwrap())
        .collect();
    removed.sort_unstable();
    assert_eq!(
        removed,
        ["rmA", "rmB", "rmC", "rmD", "rmE", "rmF", "rmG", "rmH"],
        "{json:#}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/c.ts")).unwrap(),
        FIXED
    );
}
