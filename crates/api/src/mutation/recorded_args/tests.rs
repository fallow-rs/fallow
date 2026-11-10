use serde_json::json;

use super::*;

fn function(key: &str, inputs: Vec<Vec<serde_json::Value>>) -> RecordedFunction {
    RecordedFunction {
        key: key.to_owned(),
        inputs,
        not_replayable: Vec::new(),
    }
}

const NOW: u64 = 1_800_000_000;

#[test]
fn values_mode_keeps_the_inputs_and_shapes_mode_keeps_only_their_types() {
    let dir = tempfile::tempdir().expect("dir");
    let functions = [function("src/a.ts#add", vec![vec![json!(2), json!("x")]])];
    store(dir.path(), RecordArgsMode::Values, &functions, NOW).expect("store");
    let inputs = load(dir.path(), "src/a.ts#add", NOW).expect("entry");
    assert_eq!(inputs.mode, RecordArgsMode::Values);
    assert_eq!(inputs.inputs[0].args, Some(vec![json!(2), json!("x")]));
    assert_eq!(inputs.inputs[0].shape, json!(["number", "string"]));
    assert_eq!(inputs.inputs[0].input_id.len(), 12);

    store(dir.path(), RecordArgsMode::Shapes, &functions, NOW).expect("store");
    let text = std::fs::read_to_string(store_path(dir.path())).expect("file");
    assert!(!text.contains("\"x\""), "a shape holds no value: {text}");
    let inputs = load(dir.path(), "src/a.ts#add", NOW).expect("entry");
    assert_eq!(inputs.inputs[0].args, None);
}

#[test]
fn an_input_with_a_secret_key_or_a_high_entropy_string_is_not_stored() {
    let dir = tempfile::tempdir().expect("dir");
    let functions = [function(
        "src/a.ts#login",
        vec![
            vec![json!({ "user": "ann", "password": "hunter2" })],
            vec![json!({ "headers": { "Authorization": "Bearer x" } })],
            vec![json!("q9fQ2xLmZ7rT4bW8nKc3VhYp6Zt")],
            vec![json!({ "user": "bob", "id": "123e4567-e89b-12d3-a456-426614174000" })],
        ],
    )];
    store(dir.path(), RecordArgsMode::Values, &functions, NOW).expect("store");
    let text = std::fs::read_to_string(store_path(dir.path())).expect("file");
    for secret in ["hunter2", "Bearer x", "q9fQ2xLmZ7rT4bW8nKc3VhYp6Zt"] {
        assert!(!text.contains(secret), "{secret} leaked: {text}");
    }
    let inputs = load(dir.path(), "src/a.ts#login", NOW).expect("entry");
    assert_eq!(inputs.inputs.len(), 1, "the plain input stays");
    assert_eq!(
        inputs.inputs[0].args.as_ref().expect("values")[0]["user"],
        "bob"
    );
    let reasons: Vec<&str> = inputs
        .not_replayable
        .iter()
        .map(|refusal| refusal.reason.as_str())
        .collect();
    assert!(
        reasons.contains(&"a value under a secret key name"),
        "{reasons:?}"
    );
    assert!(
        reasons.contains(&"a string that looks like a secret"),
        "{reasons:?}"
    );
}

#[test]
fn an_entry_past_its_time_to_live_is_gone_and_a_new_run_replaces_its_functions() {
    let dir = tempfile::tempdir().expect("dir");
    store(
        dir.path(),
        RecordArgsMode::Values,
        &[function("src/a.ts#old", vec![vec![json!(1)]])],
        NOW,
    )
    .expect("store");
    assert!(load(dir.path(), "src/a.ts#old", NOW + TIME_TO_LIVE_SECS - 1).is_some());
    assert!(load(dir.path(), "src/a.ts#old", NOW + TIME_TO_LIVE_SECS + 1).is_none());

    store(
        dir.path(),
        RecordArgsMode::Values,
        &[function("src/a.ts#new", vec![vec![json!(2)]])],
        NOW + TIME_TO_LIVE_SECS + 1,
    )
    .expect("store");
    let text = std::fs::read_to_string(store_path(dir.path())).expect("file");
    assert!(
        !text.contains("src/a.ts#old"),
        "an expired entry is removed: {text}"
    );
    assert!(load(dir.path(), "src/a.ts#new", NOW + TIME_TO_LIVE_SECS + 1).is_some());
}

#[test]
fn a_store_that_does_not_parse_reads_as_empty() {
    let dir = tempfile::tempdir().expect("dir");
    std::fs::create_dir_all(dir.path().join("mutation")).expect("dir");
    std::fs::write(store_path(dir.path()), "{ not json").expect("write");
    assert!(load(dir.path(), "src/a.ts#add", NOW).is_none());
    store(
        dir.path(),
        RecordArgsMode::Values,
        &[function("src/a.ts#add", vec![vec![json!(1)]])],
        NOW,
    )
    .expect("a broken store is replaced");
    assert!(load(dir.path(), "src/a.ts#add", NOW).is_some());
}

#[test]
fn the_input_id_does_not_depend_on_the_key_order_of_an_object() {
    let left = input_id(&[json!({ "a": 1, "b": 2 })]);
    let right = input_id(&[json!({ "b": 2, "a": 1 })]);
    assert_eq!(left, right);
    assert_ne!(left, input_id(&[json!({ "a": 1, "b": 3 })]));
}

#[test]
fn the_keys_of_a_map_and_an_escaped_object_count_as_key_names() {
    let dir = tempfile::tempdir().expect("dir");
    let functions = [function(
        "src/a.ts#login",
        vec![
            vec![json!({ "$replay": "map", "entries": [["password", "hunter2"]] })],
            vec![json!({ "$replay": "object", "entries": [["$replay", 1], ["apiToken", "abc"]] })],
        ],
    )];
    store(dir.path(), RecordArgsMode::Values, &functions, NOW).expect("store");
    let text = std::fs::read_to_string(store_path(dir.path())).expect("file");
    assert!(!text.contains("hunter2"), "{text}");
    assert!(!text.contains("\"abc\""), "{text}");
}

#[test]
fn credential_shapes_and_a_secret_word_in_a_sentence_are_not_stored() {
    let dir = tempfile::tempdir().expect("dir");
    let secrets = [
        "Bearer q9fQ2xLmZ7rT4bW8nKc3VhYp6Zt",
        "Basic dXNlcjpwYXNz",
        "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.abcdefghijklmnop",
        "-----BEGIN EXAMPLE BLOCK-----\nAAAA",
        "https://user:hunter2@example.com/db",
        "use the key 9fQ2xLmZ7rT4bW8nKc3VhYp6aB1 now",
    ];
    let functions = [function(
        "src/a.ts#connect",
        secrets.iter().map(|secret| vec![json!(secret)]).collect(),
    )];
    store(dir.path(), RecordArgsMode::Values, &functions, NOW).expect("store");
    let text = std::fs::read_to_string(store_path(dir.path())).expect("file");
    for secret in [
        "q9fQ2xLm",
        "dXNlcjpwYXNz",
        "eyJhbGci",
        "BEGIN EXAMPLE",
        "hunter2",
        "9fQ2xLmZ",
    ] {
        assert!(!text.contains(secret), "{secret} leaked: {text}");
    }
    for plain in ["hello world", "a plain sentence with words"] {
        assert!(secret_reason(&json!(plain)).is_none(), "{plain}");
    }
}

#[test]
fn an_entry_from_the_future_is_not_fresh_and_the_file_is_private() {
    let dir = tempfile::tempdir().expect("dir");
    store(
        dir.path(),
        RecordArgsMode::Values,
        &[function("src/a.ts#add", vec![vec![json!(1)]])],
        NOW + 1000,
    )
    .expect("store");
    assert!(load(dir.path(), "src/a.ts#add", NOW).is_none());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(store_path(dir.path()))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("mutation"))
        .expect("dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn an_unknown_reason_becomes_a_fixed_reason_and_reasons_are_sorted() {
    let dir = tempfile::tempdir().expect("dir");
    let recorded = RecordedFunction {
        key: "src/a.ts#f".to_owned(),
        inputs: vec![vec![json!({ "token": "x" })]],
        not_replayable: vec![
            RecordRefusal {
                reason: "the value cannot be read: Invalid token abc123".to_owned(),
                count: 1,
            },
            RecordRefusal {
                reason: "a function".to_owned(),
                count: 2,
            },
        ],
    };
    let pass = RecordPass {
        functions: vec![recorded.clone(), recorded],
        error: None,
    };
    let output = save_recording(dir.path(), RecordArgsMode::Values, Some(&pass)).expect("output");
    assert_eq!(output.functions.len(), 1, "a key is one function");
    let reasons: Vec<&str> = output.functions[0]
        .not_replayable
        .iter()
        .map(|refusal| refusal.reason.as_str())
        .collect();
    assert_eq!(
        reasons,
        [
            "a function",
            "a value that cannot be read",
            "a value under a secret key name"
        ]
    );
    let text = serde_json::to_string(&output).expect("json");
    assert!(!text.contains("abc123"), "{text}");
}

#[test]
fn a_failed_pass_and_a_run_without_a_pass_say_why() {
    let dir = tempfile::tempdir().expect("dir");
    let failed = RecordPass {
        functions: Vec::new(),
        error: Some("the runner records no arguments".to_owned()),
    };
    let output = save_recording(dir.path(), RecordArgsMode::Values, Some(&failed)).expect("output");
    assert!(!output.saved);
    assert_eq!(
        output.error.as_deref(),
        Some("the runner records no arguments")
    );
    let none = save_recording(dir.path(), RecordArgsMode::Shapes, None).expect("output");
    assert!(!none.saved);
    assert_eq!(
        none.error.as_deref(),
        Some("no test ran, so no argument was recorded")
    );
    assert!(save_recording(dir.path(), RecordArgsMode::Off, None).is_none());
}

#[test]
fn the_cache_directory_gets_its_gitignore() {
    let dir = tempfile::tempdir().expect("dir");
    store(
        dir.path(),
        RecordArgsMode::Values,
        &[function("src/a.ts#add", vec![vec![json!(1)]])],
        NOW,
    )
    .expect("store");
    assert!(dir.path().join(".gitignore").is_file());
}

#[test]
fn a_file_path_or_a_file_url_is_no_secret_but_a_key_in_a_url_is() {
    for plain in [
        "/home/runner/work/pkg-types/pkg-types/test/fixture/package.json",
        "/Users/someone/Sites/acme/.cache/mutation-replay/real/pkg-types/test/fixture/package.json5",
        "file:///Users/someone/projects/pkg-types/test/fixtures/nested/package.json",
        "C:\\Users\\someone\\projects\\pkg-types\\test\\fixture\\tsconfig.json",
    ] {
        assert!(secret_reason(&json!(plain)).is_none(), "{plain}");
    }
    let keyed = "https://api.example.com/v1/items?api_key=9fQ2xLmZ7rT4bW8nKc3VhYp6aB1";
    assert!(secret_reason(&json!(keyed)).is_some());
}

#[test]
fn a_long_hex_key_counts_as_a_secret_but_a_short_hash_does_not() {
    assert!(secret_reason(&json!("9e107d9d372bb6826bd81d3542a419d6")).is_some());
    assert!(
        secret_reason(&json!({ "commit": "da39a3ee5e6b4b0d3255bfef95601890afd80709" })).is_some()
    );
    assert!(secret_reason(&json!("9e107d9d")).is_none());
}

#[test]
fn a_base64_key_with_a_slash_and_key_sized_random_bytes_count_as_secrets() {
    let aws_doc_example = format!("wJalrXUtnFEMI/K7MDENG/bPxRfiCY{}", "EXAMPLEKEY");
    assert!(secret_reason(&json!(aws_doc_example)).is_some());
    let key_bytes: Vec<serde_json::Value> = (0_u8..32)
        .map(|byte| json!(byte.wrapping_mul(97).wrapping_add(13)))
        .collect();
    let tagged = json!({ "$replay": "bytes", "type": "Uint8Array", "values": key_bytes });
    assert!(secret_reason(&tagged).is_some());
    let small = json!({ "$replay": "bytes", "type": "Uint8Array", "values": [1, 2, 3] });
    assert!(secret_reason(&small).is_none());
    assert!(secret_reason(&json!("/usr/local/lib/node_modules/some-package")).is_none());
}
