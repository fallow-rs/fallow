#![allow(
    clippy::unwrap_used,
    reason = "tests use unwrap to keep fixture setup concise"
)]

use fallow_engine::session::AnalysisSession;

#[test]
fn shared_state_is_opt_in_and_survives_cache_modes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"type":"module","main":"writer.ts"}"#,
    )
    .unwrap();
    std::fs::write(root.join("owner.ts"), "export const state = {};").unwrap();
    std::fs::write(
        root.join("writer.ts"),
        "import {state} from './owner'; state.value = 1; window.custom = 2;",
    )
    .unwrap();
    std::fs::write(
        root.join("reader.ts"),
        "export const read = () => window.custom;",
    )
    .unwrap();
    let mut baseline = None;
    for (requested, no_cache) in [
        (false, false),
        (true, false),
        (true, false),
        (false, false),
        (true, true),
    ] {
        let session = AnalysisSession::load_default_with_config(root, |config| {
            config.no_cache = no_cache;
            if requested {
                config.request_shared_state();
            }
        });
        let result = session.analyze_dead_code().unwrap();
        let json = serde_json::to_value(&result.results).unwrap();
        let mut ordinary = json.clone();
        ordinary.as_object_mut().unwrap().remove("shared_state");
        if let Some(base) = &baseline {
            assert_eq!(&ordinary, base);
        } else {
            baseline = Some(ordinary);
        }
        if requested {
            assert_eq!(
                json["shared_state"]["imported_writes"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(json["shared_state"]["globals"][0]["root"], "window");
            assert_eq!(json["shared_state"]["globals"][0]["slot"], "custom");
            assert_eq!(json["shared_state"]["complete"], true);
        } else {
            assert!(json.get("shared_state").is_none());
        }
    }
}

#[test]
fn shared_state_resolves_unique_value_origins_and_suppresses_before_grouping() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    for (path, source) in [
        ("package.json", r#"{"type":"module","main":"writer.ts"}"#),
        ("owner.ts", "export const state = {};"),
        ("other.ts", "export const state = {};"),
        ("unique.ts", "export {state as config} from './owner';"),
        (
            "ambiguous.ts",
            "export * from './owner'; export * from './other';",
        ),
        (
            "writer.ts",
            r#"import {config} from './unique';
import * as ns from './owner';
import {state as duplicate} from './owner';
import {state as ambiguous} from './ambiguous';
import type {state as typeOnly} from './owner';
config["a.b"] = 1; ns.state.x++; ns.state = {}; duplicate.z ||= 1;
ambiguous.x = 2; typeOnly.x = 2; const alias = config; alias.x = 2;
config[key] = 2; config.push(1);
export function initialize() { config.inside = true; }
window.flag = 1; globalThis.flag = 2; window.only = 1; globalThis.only = 2;
// fallow-ignore-next-line shared-state -- intentional local initialization
window.suppressed = 1;
window.location = '/';
"#,
        ),
        (
            "reader.ts",
            "export const read = () => [window.flag, globalThis.flag, window.only, globalThis.only, window.suppressed, window.location];",
        ),
        (
            "types.d.ts",
            "declare global { interface Window { flag: number; } } export {};",
        ),
        ("writer.test.ts", "window.testOnly = 1;"),
    ] {
        std::fs::write(root.join(path), source).unwrap();
    }
    let session = AnalysisSession::load_default_with_config(root, |config| {
        config.request_shared_state();
        config
            .shared_state_allowlist
            .push(["window".to_owned(), "only".to_owned()]);
    });
    let result = session.analyze_dead_code().unwrap();
    let inventory = result.results.shared_state.unwrap();
    let properties: Vec<_> = inventory
        .imported_writes
        .iter()
        .map(|write| write.property.clone())
        .collect();
    assert_eq!(
        properties,
        vec![vec!["a.b"], vec!["x"], vec!["z"], vec!["inside"]]
    );
    for write in &inventory.imported_writes {
        assert!(write.declaration.path.ends_with("owner.ts"));
        assert!(write.actions.iter().all(|action| !action.auto_fixable));
    }
    let slots: Vec<_> = inventory
        .globals
        .iter()
        .map(|group| (group.root.as_str(), group.slot.as_str()))
        .collect();
    assert_eq!(
        slots,
        vec![
            ("globalThis", "flag"),
            ("globalThis", "only"),
            ("window", "flag")
        ]
    );
    let ordinary = AnalysisSession::load_default(root)
        .analyze_dead_code()
        .unwrap();
    assert!(ordinary.results.stale_suppressions.is_empty());
}
