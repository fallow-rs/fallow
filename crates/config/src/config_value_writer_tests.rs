use super::*;
use serde_json::json;

const BASELINE_PATH: [&str; 2] = ["regression", "baseline"];

fn baseline() -> serde_json::Value {
    json!({"totalIssues": 5, "analysisIdentity": {"mode": "type-aware", "capabilities": []}})
}

fn set_json(content: &str) -> ConfigWriteResult<String> {
    set_config_value_in_string(content, ConfigFormat::Json, &BASELINE_PATH, &baseline())
}

fn set_toml(content: &str) -> ConfigWriteResult<String> {
    set_config_value_in_string(content, ConfigFormat::Toml, &BASELINE_PATH, &baseline())
}

fn parse_jsonc(out: &str) -> serde_json::Value {
    CstRootNode::parse(out, &crate::jsonc::parse_options())
        .unwrap()
        .to_serde_value()
        .unwrap()
}

fn parse_toml(out: &str) -> toml::Value {
    toml::from_str::<toml::Value>(out).unwrap()
}

#[test]
fn json_sets_value_in_empty_object() {
    let out = set_json("{}").unwrap();
    assert_eq!(
        parse_jsonc(&out),
        json!({"regression": {"baseline": baseline()}})
    );
}

#[test]
fn json_keeps_unrelated_settings() {
    let out = set_json(r#"{"entry": ["src/main.ts"], "production": true}"#).unwrap();
    let parsed = parse_jsonc(&out);
    assert_eq!(parsed["entry"], json!(["src/main.ts"]));
    assert_eq!(parsed["production"], json!(true));
}

#[test]
fn json_replaces_value_and_keeps_sibling_key() {
    let out = set_json(r#"{"regression": {"baseline": {"totalIssues": 99}, "tolerance": "5%"}}"#)
        .unwrap();
    let parsed = parse_jsonc(&out);
    assert_eq!(parsed["regression"]["baseline"]["totalIssues"], json!(5));
    assert_eq!(parsed["regression"]["tolerance"], json!("5%"));
}

#[test]
fn json_string_value_equal_to_key_is_not_the_key() {
    let out = set_json(r#"{"ignorePatterns": ["regression"], "rules": {"unused-files": "warn"}}"#)
        .unwrap();
    let parsed = parse_jsonc(&out);
    assert_eq!(parsed["rules"], json!({"unused-files": "warn"}));
    assert_eq!(parsed["regression"]["baseline"]["totalIssues"], json!(5));
}

#[test]
fn json_keeps_comments_with_key_text_and_braces() {
    let input = "{\n  // \"regression\": {\n  /* } */\n  \"entry\": [\"a\"]\n}\n";
    let out = set_json(input).unwrap();
    assert!(out.contains("// \"regression\": {"), "{out}");
    assert!(out.contains("/* } */"), "{out}");
    let parsed = parse_jsonc(&out);
    assert_eq!(parsed["entry"], json!(["a"]));
    assert_eq!(parsed["regression"]["baseline"], baseline());
}

#[test]
fn json_accepts_trailing_comma() {
    let out = set_json("{\n  \"entry\": [\"a\"],\n}").unwrap();
    let parsed = parse_jsonc(&out);
    assert_eq!(parsed["entry"], json!(["a"]));
    assert_eq!(parsed["regression"]["baseline"], baseline());
}

#[test]
fn json_keeps_trailing_line_comment() {
    let out = set_json("{\n  \"entry\": [\"a\"] // note\n}\n").unwrap();
    assert!(out.contains("// note"), "{out}");
    assert_eq!(parse_jsonc(&out)["regression"]["baseline"], baseline());
}

#[test]
fn json_rejects_parent_that_is_not_an_object() {
    let err = set_json(r#"{"regression": 5}"#).unwrap_err();
    assert!(matches!(err, ConfigWriteError::InvalidShape(_)), "{err}");
}

#[test]
fn json_rejects_unclosed_document() {
    let err = set_json(r#"{ "regression": { "baseline": {"#).unwrap_err();
    assert!(matches!(err, ConfigWriteError::JsonParse(_)), "{err}");
}

#[test]
fn keeps_crlf_and_bom() {
    let input = format!("{BOM}{{\r\n  \"entry\": [\"a\"]\r\n}}\r\n");
    let out = set_json(&input).unwrap();
    assert!(out.starts_with(BOM));
    assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
    let body = out.strip_prefix(BOM).unwrap();
    assert_eq!(parse_jsonc(body)["regression"]["baseline"], baseline());
}

#[test]
fn rejects_empty_key_path() {
    let err = set_config_value_in_string("{}", ConfigFormat::Json, &[], &baseline()).unwrap_err();
    assert!(matches!(err, ConfigWriteError::InvalidShape(_)), "{err}");
}

#[test]
fn toml_sets_value_in_empty_document() {
    let out = set_toml("").unwrap();
    assert!(out.contains("[regression.baseline]"), "{out}");
    assert!(out.contains("totalIssues = 5"), "{out}");
    assert!(!out.lines().any(|line| line == "[regression]"), "{out}");
}

#[test]
fn toml_keeps_unrelated_tables() {
    let out = set_toml("[rules]\nunused-files = \"warn\"\n").unwrap();
    assert!(out.contains("[rules]"), "{out}");
    let parsed = parse_toml(&out);
    assert_eq!(
        parsed["regression"]["baseline"]["totalIssues"].as_integer(),
        Some(5)
    );
}

#[test]
fn toml_replaces_section_in_place() {
    let out =
        set_toml("[regression.baseline]\ntotalIssues = 99\n\n[rules]\nunused-files = \"warn\"\n")
            .unwrap();
    assert!(!out.contains("99"), "{out}");
    let parsed = parse_toml(&out);
    assert_eq!(parsed["rules"]["unused-files"].as_str(), Some("warn"));
    let baseline_at = out.find("[regression.baseline]").unwrap();
    let rules_at = out.find("[rules]").unwrap();
    assert!(baseline_at < rules_at, "{out}");
}

#[test]
fn toml_writes_nested_object_as_sub_table() {
    let out = set_toml("").unwrap();
    let parsed = parse_toml(&out);
    assert_eq!(
        parsed["regression"]["baseline"]["analysisIdentity"]["mode"].as_str(),
        Some("type-aware")
    );
}

#[test]
fn toml_ignores_header_in_comment() {
    let out = set_toml("# [regression.baseline]\n[rules]\nunused-files = \"warn\"\n").unwrap();
    assert!(
        out.lines().any(|line| line == "# [regression.baseline]"),
        "{out}"
    );
    let parsed = parse_toml(&out);
    assert_eq!(parsed["rules"]["unused-files"].as_str(), Some("warn"));
    assert_eq!(
        parsed["regression"]["baseline"]["totalIssues"].as_integer(),
        Some(5)
    );
}

#[test]
fn toml_replaces_indented_table_and_keeps_similar_name() {
    let out = set_toml("  [regression.baseline]\n  totalIssues = 99\n[regression-notes]\nx = 1\n")
        .unwrap();
    assert!(!out.contains("99"), "{out}");
    let parsed = parse_toml(&out);
    assert_eq!(parsed["regression-notes"]["x"].as_integer(), Some(1));
}

#[test]
fn toml_sets_value_in_inline_table() {
    let out = set_toml("regression = { baseline = { totalIssues = 99 } }\n").unwrap();
    let parsed = parse_toml(&out);
    assert_eq!(
        parsed["regression"]["baseline"]["totalIssues"].as_integer(),
        Some(5)
    );
}

#[test]
fn toml_rejects_parent_that_is_not_a_table() {
    let err = set_toml("regression = 5\n").unwrap_err();
    assert!(matches!(err, ConfigWriteError::InvalidShape(_)), "{err}");
}

#[test]
fn toml_rejects_null() {
    let err =
        set_config_value_in_string("", ConfigFormat::Toml, &BASELINE_PATH, &json!({"a": null}))
            .unwrap_err();
    assert!(matches!(err, ConfigWriteError::InvalidShape(_)), "{err}");
}

#[test]
fn toml_rejects_invalid_document() {
    let err = set_toml("[rules\n").unwrap_err();
    assert!(matches!(err, ConfigWriteError::TomlParse(_)), "{err}");
}

#[test]
fn render_writes_each_header_line_as_comment() {
    let out = render_toml_config(&json!({"entry": ["x"]}), Some("a\nb")).unwrap();
    assert!(out.starts_with("# a\n# b\n\n"), "{out}");
}

#[test]
fn render_puts_root_scalar_before_tables() {
    let config = json!({"rules": {"unused-files": "error"}, "includeEntryExports": true});
    let out = render_toml_config(&config, None).unwrap();
    assert!(
        out.find("includeEntryExports") < out.find("[rules]"),
        "{out}"
    );
    assert_eq!(
        toml::from_str::<serde_json::Value>(&out).unwrap(),
        config,
        "{out}"
    );
}

#[test]
fn json_replaces_null_parent() {
    let out = set_json(r#"{"regression": null}"#).unwrap();
    assert_eq!(parse_jsonc(&out)["regression"]["baseline"], baseline());
}

#[test]
fn toml_replace_keeps_comment_above_header_and_drops_old_sub_table() {
    let input = "[rules]\nunused-files = \"warn\"\n\n# saved counts\n[regression.baseline]\ntotalIssues = 99\n\n[regression.baseline.analysisIdentity]\nmode = \"syntactic\"\nstale = 1\n";
    let out = set_toml(input).unwrap();
    assert!(
        out.contains("# saved counts\n[regression.baseline]"),
        "{out}"
    );
    assert!(!out.contains("stale"), "{out}");
    let parsed = parse_toml(&out);
    assert_eq!(
        parsed["regression"]["baseline"]["analysisIdentity"]["mode"].as_str(),
        Some("type-aware")
    );
}

#[test]
fn toml_sets_value_under_dotted_keys() {
    let out =
        set_toml("regression.tolerance = \"5%\"\nregression.baseline.totalIssues = 99\n").unwrap();
    assert!(!out.contains("99"), "{out}");
    let parsed = parse_toml(&out);
    assert_eq!(parsed["regression"]["tolerance"].as_str(), Some("5%"));
    assert_eq!(
        parsed["regression"]["baseline"]["totalIssues"].as_integer(),
        Some(5)
    );
}

#[test]
fn render_writes_array_of_objects_as_array_of_tables() {
    let config = json!({"overrides": [{"files": ["a.ts"], "rules": {"unused-files": "off"}}]});
    let out = render_toml_config(&config, None).unwrap();
    assert!(out.contains("[[overrides]]"), "{out}");
    assert_eq!(toml::from_str::<serde_json::Value>(&out).unwrap(), config);
}

#[test]
fn render_escapes_strings() {
    let config = json!({"entry": ["a\"b", "c\\d"]});
    let out = render_toml_config(&config, None).unwrap();
    assert_eq!(toml::from_str::<serde_json::Value>(&out).unwrap(), config);
}

#[test]
fn render_rejects_non_object_root() {
    let err = render_toml_config(&json!([1]), None).unwrap_err();
    assert!(matches!(err, ConfigWriteError::InvalidShape(_)), "{err}");
}

#[test]
fn string_array_escapes_quotes_and_backslashes() {
    assert_eq!(toml_string_array(&["packages/*"]), r#"["packages/*"]"#);
    let out = toml_string_array(&["a\"b", "c\\d"]);
    let parsed = toml::from_str::<toml::Value>(&format!("x = {out}")).unwrap();
    let items: Vec<&str> = parsed["x"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    assert_eq!(items, ["a\"b", "c\\d"]);
}
