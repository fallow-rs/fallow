//! Entry probes and the outline of a source file (see `reach.rs`).

mod common;

use common::{assert_valid, run_node};
use fallow_mutation::{
    MutationOptions, Operator, Outline, SourceKind, instrument_with_probes, outline, probe_key,
};

const PATH: &str = "src/fixture.ts";

fn probed(source: &str, kind: SourceKind, options: &MutationOptions) -> (String, Outline) {
    let (result, outline) = match instrument_with_probes(source, PATH, kind, options) {
        Ok(value) => value,
        Err(error) => panic!("instrument failed: {error}"),
    };
    assert_valid(&result.code, kind);
    (result.code, outline)
}

fn parses(code: &str, kind: SourceKind) -> bool {
    std::panic::catch_unwind(|| assert_valid(code, kind)).is_ok()
}

fn no_mutants() -> MutationOptions {
    MutationOptions {
        operators: Vec::new(),
        ..MutationOptions::default()
    }
}

fn probe(ordinal: &str) -> String {
    format!("r{}.{ordinal}", probe_key(PATH))
}

/// Runs the probed code in Node with a runtime that prints each probe id on
/// its first hit, then the output of the program. `None` without `node`.
fn entered(code: &str) -> Option<Vec<String>> {
    let script = format!(
        "const seen = new Set(); globalThis.__fallowMutation = {{ a: () => false, c: (id) => {{ if (id.startsWith('r') && !seen.has(id)) {{ seen.add(id); console.log('probe ' + id); }} }} }};\n{code}"
    );
    let (out, status) = run_node(&script)?;
    assert_eq!(status, Some(0), "the probed code fails: {out}\n---\n{code}");
    Some(out.lines().map(str::to_owned).collect())
}

#[test]
fn probe_key_is_stable_and_hex() {
    let key = probe_key("src/a.ts");
    assert_eq!(key.len(), 16);
    assert!(key.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(key, probe_key("src/a.ts"));
    assert_ne!(key, probe_key("src/b.ts"));
}

#[test]
fn outline_lists_outer_function_bodies_only() {
    let source = "import { x } from './x';\nexport function f(a = 1) { return () => a; }\nconst g = (b) => b + x;\nclass C { field = 2; static { init(); } m() { return 1; } get v() { return 2; } }\nexport const o = { k() { return 3; } };\n";
    let outline = outline(source, SourceKind::Ts).expect("outline");
    let bodies: Vec<&str> = outline
        .bodies
        .iter()
        .map(|body| &source[body.span.0 as usize..body.span.1 as usize])
        .collect();
    assert_eq!(
        bodies,
        [
            "{ return () => a; }",
            "b + x",
            "{ return 1; }",
            "{ return 2; }",
            "{ return 3; }",
        ]
    );
}

#[test]
fn a_body_change_keeps_the_skeleton() {
    let before = "export const f = (a) => a + 1;\nexport function g() { return 1; }\n";
    let after = "export const f = (a) => a - 1;\nexport function g() { return 1; }\n";
    let old = outline(before, SourceKind::Ts).expect("outline");
    let new = outline(after, SourceKind::Ts).expect("outline");
    assert_eq!(old.skeleton, new.skeleton);
    assert_ne!(old.bodies[0].hash, new.bodies[0].hash);
    assert_eq!(old.bodies[1].hash, new.bodies[1].hash);
}

#[test]
fn code_outside_bodies_changes_the_skeleton() {
    let base = "export function f(a) { return a; }\nclass C { x = 1; }\n";
    for changed in [
        // a parameter: `f.length` and default values
        "export function f(a, b) { return a; }\nclass C { x = 1; }\n",
        // a class field initializer
        "export function f(a) { return a; }\nclass C { x = 2; }\n",
        // a new import
        "import './side';\nexport function f(a) { return a; }\nclass C { x = 1; }\n",
        // a decorator
        "export function f(a) { return a; }\n@sealed class C { x = 1; }\n",
        // a new function
        "export function f(a) { return a; }\nexport function g() {}\nclass C { x = 1; }\n",
    ] {
        let old = outline(base, SourceKind::Ts).expect("outline");
        let new = outline(changed, SourceKind::Ts).expect("outline");
        assert_ne!(old.skeleton, new.skeleton, "{changed}");
    }
}

#[test]
fn each_outer_function_gets_one_probe_and_the_module_one() {
    let source = "export const f = (a) => a + 1;\nexport function g(b) { \"use strict\"\n return b; }\nconst o = { get v() { return 1; } };\n";
    let (code, outline) = probed(source, SourceKind::Ts, &no_mutants());
    assert_eq!(outline.bodies.len(), 3);
    for ordinal in ["m", "0", "1", "2"] {
        let call = format!("__fallow_m_r(\"{}\")", probe(ordinal));
        assert_eq!(code.matches(&call).count(), 1, "{ordinal}\n{code}");
    }
    // The directive stays the first statement of the body.
    assert!(
        code.contains("{ \"use strict\";typeof __fallow_m_r"),
        "{code}"
    );
}

#[test]
fn probes_report_the_entered_functions() {
    let source = "function used(a) { return a + 1; }\nfunction unused(a) { return a - 1; }\nconst arrow = (a) => used(a);\nconsole.log(arrow(1));\n";
    let (code, _) = probed(source, SourceKind::Js, &no_mutants());
    let Some(lines) = entered(&code) else {
        return;
    };
    assert_eq!(
        lines,
        [
            format!("probe {}", probe("m")),
            format!("probe {}", probe("2")),
            format!("probe {}", probe("0")),
            "2".to_owned(),
        ]
    );
}

#[test]
fn probes_and_mutants_share_one_text() {
    let sources = [
        "export const f = (a) => a > 0 ? a + 1 : a - 1;\n",
        "export const f = (a) => a + 1;\nexport const g = (b) => (b && f(b));\n",
        "export const pick = (xs) => xs.filter((x) => x > 1).map((x) => x * 2);\n",
        "export const h = cond ? (a) => a + 1 : (b) => b - 1;\n",
        "export function k(a) {if (a < 1) { return a + 1; } return a - 1;}\n",
        "export const obj = { m(a) { return a === 1; }, n: (b) => ({ v: b + 1 }) };\n",
        "class C extends B { constructor(a) { super(a); this.a = a + 1; } }\n",
        "export async function* gen(a) { yield a + 1; }\n",
        "export const t = (a) => `${a + 1}`;\n",
        "export const s = a => a+1\n(0)\n",
    ];
    for source in sources {
        let (code, _) = probed(source, SourceKind::Js, &MutationOptions::default());
        let (plain, _) = probed(source, SourceKind::Js, &no_mutants());
        assert!(code.contains("__fallow_m_a("), "{source}");
        for text in [&code, &plain] {
            let calls = text.matches("__fallow_m_r(\"r").count();
            assert!(calls >= 2, "{text}");
        }
    }
}

#[test]
fn probed_code_runs_like_the_original_with_each_mutant_off() {
    let source = "const f = (a) => a > 0 ? a + 1 : a - 1;\nfunction g(xs) { return xs.map((x) => x * 2).filter((x) => x > 2); }\nconst o = { get v() { return f(2); } };\nconsole.log(f(1), f(-1), g([1, 2, 3]).join(','), o.v);\n";
    let Some((expected, _)) = run_node(source) else {
        return;
    };
    let (code, _) = probed(source, SourceKind::Js, &MutationOptions::default());
    let Some(lines) = entered(&code) else {
        return;
    };
    let output: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|line| !line.starts_with("probe "))
        .collect();
    assert_eq!(output.join("\n"), expected.trim_end());
}

#[test]
fn a_mutant_keeps_its_id_with_probes() {
    let source = "export const f = (a) => a + 1;\n";
    let plain =
        fallow_mutation::instrument(source, PATH, SourceKind::Ts, &MutationOptions::default())
            .expect("instrument");
    let (with_probes, _) =
        instrument_with_probes(source, PATH, SourceKind::Ts, &MutationOptions::default())
            .expect("instrument");
    let ids = |mutants: &[fallow_mutation::Mutant]| -> Vec<String> {
        mutants.iter().map(|mutant| mutant.id.0.clone()).collect()
    };
    assert_eq!(ids(&plain.mutants), ids(&with_probes.mutants));
    assert!(
        with_probes
            .mutants
            .iter()
            .any(|mutant| mutant.operator == Operator::ArithmeticOperator)
    );
}

#[test]
fn a_source_with_the_probe_name_is_refused() {
    let error = instrument_with_probes(
        "const __fallow_m_r = 1;\n",
        PATH,
        SourceKind::Js,
        &no_mutants(),
    )
    .expect_err("reserved");
    assert!(error.to_string().contains("__fallow_m_r"), "{error}");
}

/// Instruments each source file below `FALLOW_PROBE_CORPUS` with probes and
/// the default operators, and checks that each output parses and places
/// each probe. Run with `cargo test -p fallow-mutation --test reach --
/// --ignored` and the variable set to a project folder.
#[test]
#[ignore = "needs a corpus folder in FALLOW_PROBE_CORPUS"]
fn each_corpus_file_gets_valid_probes() {
    let Ok(root) = std::env::var("FALLOW_PROBE_CORPUS") else {
        return;
    };
    let mut stack = vec![std::path::PathBuf::from(root)];
    let (mut files, mut skipped) = (0, 0);
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !name.starts_with('.') && name != "node_modules" {
                    stack.push(path);
                }
                continue;
            }
            let Some(kind) = SourceKind::from_path(&name) else {
                continue;
            };
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            match instrument_with_probes(&source, PATH, kind, &MutationOptions::default()) {
                Ok((result, outline)) => {
                    files += 1;
                    if !parses(&result.code, kind) {
                        // Compare with the output without probes: a text
                        // that fails there is not a probe defect.
                        let plain = fallow_mutation::instrument(
                            &source,
                            PATH,
                            kind,
                            &MutationOptions::default(),
                        )
                        .map(|plain| parses(&plain.code, kind));
                        assert_eq!(plain, Ok(false), "{}", path.display());
                        skipped += 1;
                        continue;
                    }
                    assert_eq!(
                        result.code.matches("__fallow_m_r(\"r").count(),
                        outline.bodies.len() + 1,
                        "{}",
                        path.display()
                    );
                }
                Err(fallow_mutation::MutationError::ProbePlacement) => {
                    panic!("no probe place in {}", path.display())
                }
                Err(_) => skipped += 1,
            }
        }
    }
    eprintln!("probed {files} files, {skipped} skipped");
    assert!(files > 0);
}
