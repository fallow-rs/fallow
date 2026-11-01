//! Source positions of config list entries.
//!
//! The loaded config keeps no source positions. An editor that marks one
//! entry of a list setting (for example an `ignoreDependencies` glob that
//! matched nothing) finds the entry again with [`FallowConfig::locate_list_entry`].

use std::path::{Path, PathBuf};

use super::FallowConfig;
use super::override_effect::key_sets_rule;
use super::parsing::{ConfigFormat, parse_config_to_value};

/// The place where a config file declares one entry of a list setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEntrySpan {
    /// The canonical path of the config file that declares the entry. For an
    /// `extends` chain, this is the file whose list is in effect.
    pub path: PathBuf,
    /// The byte offset where the entry starts, at its opening quote.
    pub start: usize,
    /// The byte offset where the entry ends, after its closing quote.
    pub end: usize,
}

impl FallowConfig {
    /// Find where the config at `config_path` declares `entry` in the
    /// top-level list setting `setting` (for example `ignoreFindings`).
    ///
    /// A list in a file replaces the list of the configs it extends, so the
    /// entry belongs to the last source in merge order that sets `setting`.
    /// Returns `None` when the extends chain does not resolve, when a remote
    /// `https://` config can set the list, or when the declaring file does not
    /// hold `entry` as a string in the list.
    #[must_use]
    pub fn locate_list_entry(
        config_path: &Path,
        setting: &str,
        entry: &str,
    ) -> Option<ConfigEntrySpan> {
        Self::locate_in_setting(config_path, setting, |path, content| {
            entry_range(path, content, setting, entry)
        })
    }

    /// Find the key that sets `rule` in the `overrides` entry with exactly
    /// these `files` globs. `rule` is the canonical name, and the key can be
    /// an alias. The rules of [`Self::locate_list_entry`] apply, because a
    /// config file replaces the whole `overrides` list of what it extends.
    #[must_use]
    pub fn locate_override_rule(
        config_path: &Path,
        rule: &str,
        files: &[String],
    ) -> Option<ConfigEntrySpan> {
        Self::locate_in_setting(config_path, "overrides", |path, content| {
            override_rule_range(path, content, rule, files)
        })
    }

    /// Run `range_in` on the last source in merge order that sets `setting`.
    fn locate_in_setting(
        config_path: &Path,
        setting: &str,
        range_in: impl Fn(&Path, &str) -> Option<(usize, usize)>,
    ) -> Option<ConfigEntrySpan> {
        let merge_order = Self::merge_order(config_path)?;
        for source in merge_order.iter().rev() {
            let path = source.as_deref()?;
            let sets_list =
                parse_config_to_value(path).is_ok_and(|value| value.get(setting).is_some());
            if !sets_list {
                continue;
            }
            let content = std::fs::read_to_string(path).ok()?;
            let (start, end) = range_in(path, &content)?;
            return Some(ConfigEntrySpan {
                path: path.to_path_buf(),
                start,
                end,
            });
        }
        None
    }
}

/// The byte range of the key that sets `rule` in one config file. The loader
/// skips a leading byte order mark, so the range counts it back in.
fn override_rule_range(
    path: &Path,
    content: &str,
    rule: &str,
    files: &[String],
) -> Option<(usize, usize)> {
    let text = content.trim_start_matches('\u{FEFF}');
    let offset = content.len() - text.len();
    let (start, end) = match ConfigFormat::from_path(path) {
        ConfigFormat::Json => json_override_rule_range(text, rule, files),
        ConfigFormat::Toml => toml_override_rule_range(text, rule, files),
    }?;
    Some((start + offset, end + offset))
}

fn json_override_rule_range(content: &str, rule: &str, files: &[String]) -> Option<(usize, usize)> {
    let parsed = jsonc_parser::parse_to_ast(
        content,
        &jsonc_parser::CollectOptions::default(),
        &crate::jsonc::parse_options(),
    )
    .ok()?;
    let jsonc_parser::ast::Value::Object(root) = parsed.value? else {
        return None;
    };
    root.get_array("overrides")?
        .elements
        .iter()
        .find_map(|element| {
            let jsonc_parser::ast::Value::Object(entry) = element else {
                return None;
            };
            let globs = entry.get_array("files")?;
            let same_files = globs.elements.len() == files.len()
                && globs.elements.iter().zip(files).all(|(glob, file)| {
                    matches!(glob, jsonc_parser::ast::Value::StringLit(literal)
                        if literal.value == file.as_str())
                });
            if !same_files {
                return None;
            }
            entry
                .get_object("rules")?
                .properties
                .iter()
                .find_map(|property| {
                    let (name, range) = match &property.name {
                        jsonc_parser::ast::ObjectPropName::String(literal) => {
                            (literal.value.as_ref(), literal.range)
                        }
                        jsonc_parser::ast::ObjectPropName::Word(word) => (word.value, word.range),
                    };
                    key_sets_rule(name, rule).then_some((range.start, range.end))
                })
        })
}

fn toml_override_rule_range(content: &str, rule: &str, files: &[String]) -> Option<(usize, usize)> {
    let document = toml_edit::Document::parse(content).ok()?;
    let overrides = document.as_table().get("overrides")?;
    let entries: Vec<&dyn toml_edit::TableLike> = match overrides.as_array_of_tables() {
        Some(tables) => tables
            .iter()
            .map(|table| table as &dyn toml_edit::TableLike)
            .collect(),
        None => overrides
            .as_array()?
            .iter()
            .filter_map(toml_edit::Value::as_inline_table)
            .map(|table| table as &dyn toml_edit::TableLike)
            .collect(),
    };
    entries.into_iter().find_map(|entry| {
        let globs = entry.get("files")?.as_array()?;
        let same_files = globs.len() == files.len()
            && globs
                .iter()
                .zip(files)
                .all(|(glob, file)| glob.as_str() == Some(file.as_str()));
        if !same_files {
            return None;
        }
        let rules = entry.get("rules")?.as_table_like()?;
        rules.iter().find_map(|(name, _)| {
            if !key_sets_rule(name, rule) {
                return None;
            }
            let span = rules.key(name)?.span()?;
            Some((span.start, span.end))
        })
    })
}

/// The byte range of `entry` in the `setting` list of one config file. The
/// loader skips a leading byte order mark, so the range counts it back in.
fn entry_range(path: &Path, content: &str, setting: &str, entry: &str) -> Option<(usize, usize)> {
    let text = content.trim_start_matches('\u{FEFF}');
    let offset = content.len() - text.len();
    let (start, end) = match ConfigFormat::from_path(path) {
        ConfigFormat::Json => json_entry_range(text, setting, entry),
        ConfigFormat::Toml => toml_entry_range(text, setting, entry),
    }?;
    Some((start + offset, end + offset))
}

fn json_entry_range(content: &str, setting: &str, entry: &str) -> Option<(usize, usize)> {
    let parsed = jsonc_parser::parse_to_ast(
        content,
        &jsonc_parser::CollectOptions::default(),
        &crate::jsonc::parse_options(),
    )
    .ok()?;
    let jsonc_parser::ast::Value::Object(root) = parsed.value? else {
        return None;
    };
    root.get_array(setting)?
        .elements
        .iter()
        .find_map(|element| match element {
            jsonc_parser::ast::Value::StringLit(literal) if literal.value == entry => {
                Some((literal.range.start, literal.range.end))
            }
            _ => None,
        })
}

fn toml_entry_range(content: &str, setting: &str, entry: &str) -> Option<(usize, usize)> {
    let document = toml_edit::Document::parse(content).ok()?;
    document
        .as_table()
        .get(setting)?
        .as_array()?
        .iter()
        .find(|value| value.as_str() == Some(entry))?
        .span()
        .map(|span| (span.start, span.end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn located_text(span: &ConfigEntrySpan) -> String {
        let content = std::fs::read_to_string(&span.path).expect("read config");
        content[span.start..span.end].to_string()
    }

    fn canonical(path: &Path) -> PathBuf {
        dunce::canonicalize(path).expect("canonical path")
    }

    #[test]
    fn finds_an_override_rule_by_its_alias_in_jsonc() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".fallowrc.jsonc");
        std::fs::write(
            &path,
            r#"{
  "overrides": [
    { "files": ["src/**"], "rules": { "unused-files": "off" } },
    { "files": ["tools/**"], "rules": { "unused-files": "off", "dev-dependency-in-production": "off" } },
  ],
}"#,
        )
        .expect("write config");
        let files = ["tools/**".to_owned()];

        let span =
            FallowConfig::locate_override_rule(&path, "dev-dependencies-in-production", &files)
                .expect("rule is found");
        assert_eq!(located_text(&span), "\"dev-dependency-in-production\"");

        let other = ["other/**".to_owned()];
        assert!(FallowConfig::locate_override_rule(&path, "unused-files", &other).is_none());
    }

    #[test]
    fn finds_an_override_rule_in_toml_tables_and_inline_tables() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("fallow.toml");
        std::fs::write(
            &path,
            "[[overrides]]\nfiles = [\"tools/**\"]\n\n[overrides.rules]\nduplicate-exports = \"off\"\n",
        )
        .expect("write config");
        let files = ["tools/**".to_owned()];
        let span = FallowConfig::locate_override_rule(&path, "duplicate-exports", &files)
            .expect("rule is found");
        assert_eq!(located_text(&span), "duplicate-exports");

        std::fs::write(
            &path,
            "overrides = [{ files = [\"tools/**\"], rules = { re-export-cycle = \"off\" } }]\n",
        )
        .expect("write config");
        let span = FallowConfig::locate_override_rule(&path, "re-export-cycle", &files)
            .expect("rule is found");
        assert_eq!(located_text(&span), "re-export-cycle");
    }

    #[test]
    fn finds_an_override_rule_in_the_extended_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().join("base.json");
        std::fs::write(
            &base,
            r#"{"overrides": [{"files": ["src/**"], "rules": {"duplicate-exports": "off"}}]}"#,
        )
        .expect("write base");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(&path, r#"{"extends": ["./base.json"]}"#).expect("write config");
        let files = ["src/**".to_owned()];

        let span = FallowConfig::locate_override_rule(&path, "duplicate-exports", &files)
            .expect("rule is found");

        assert_eq!(span.path, canonical(&base));
        assert_eq!(located_text(&span), "\"duplicate-exports\"");
    }

    #[test]
    fn finds_an_entry_in_a_jsonc_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".fallowrc.jsonc");
        std::fs::write(
            &path,
            "{\n  // a comment\n  \"ignoreDependencies\": [\"react\", \"@acme/*\",],\n}\n",
        )
        .expect("write config");

        let span = FallowConfig::locate_list_entry(&path, "ignoreDependencies", "@acme/*")
            .expect("entry is found");

        assert_eq!(span.path, canonical(&path));
        assert_eq!(located_text(&span), "\"@acme/*\"");
    }

    #[test]
    fn finds_an_entry_in_a_toml_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("fallow.toml");
        std::fs::write(
            &path,
            "ignoreFindings = [\n  \"src/generated/**\",\n  'legacy/**',\n]\n",
        )
        .expect("write config");

        let span = FallowConfig::locate_list_entry(&path, "ignoreFindings", "legacy/**")
            .expect("entry is found");

        assert_eq!(located_text(&span), "'legacy/**'");
    }

    #[test]
    fn finds_an_entry_in_the_extended_file_that_declares_the_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().join("base.json");
        std::fs::write(&base, r#"{"ignoreFindings": ["src/old/**"]}"#).expect("write base");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &path,
            r#"{"extends": ["./base.json"], "entry": ["src/main.ts"]}"#,
        )
        .expect("write config");

        let span = FallowConfig::locate_list_entry(&path, "ignoreFindings", "src/old/**")
            .expect("entry is found");

        assert_eq!(span.path, canonical(&base));
        assert_eq!(located_text(&span), "\"src/old/**\"");
    }

    #[test]
    fn a_list_in_the_extending_file_replaces_the_base_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("base.json"),
            r#"{"ignoreFindings": ["src/old/**"]}"#,
        )
        .expect("write base");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &path,
            r#"{"extends": ["./base.json"], "ignoreFindings": ["src/old/**"]}"#,
        )
        .expect("write config");

        let span = FallowConfig::locate_list_entry(&path, "ignoreFindings", "src/old/**")
            .expect("entry is found");

        assert_eq!(span.path, canonical(&path));
    }

    #[test]
    fn a_later_extends_target_overrides_an_earlier_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.json"), r#"{"ignoreFindings": ["x/**"]}"#)
            .expect("write a");
        let second = dir.path().join("b.json");
        std::fs::write(&second, r#"{"ignoreFindings": ["x/**"]}"#).expect("write b");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(&path, r#"{"extends": ["./a.json", "./b.json"]}"#).expect("write config");

        let span = FallowConfig::locate_list_entry(&path, "ignoreFindings", "x/**").expect("entry");

        assert_eq!(span.path, canonical(&second));
    }

    #[test]
    fn a_reused_extends_target_brings_its_own_extends_chain_again() {
        let dir = tempfile::tempdir().expect("tempdir");
        let deepest = dir.path().join("e.json");
        std::fs::write(&deepest, r#"{"ignoreFindings": ["x/**"]}"#).expect("write e");
        std::fs::write(dir.path().join("d.json"), r#"{"extends": ["./e.json"]}"#).expect("write d");
        std::fs::write(dir.path().join("f.json"), r#"{"ignoreFindings": ["x/**"]}"#)
            .expect("write f");
        std::fs::write(dir.path().join("g.json"), r#"{"extends": ["./d.json"]}"#).expect("write g");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &path,
            r#"{"extends": ["./d.json", "./f.json", "./g.json"]}"#,
        )
        .expect("write config");

        let span = FallowConfig::locate_list_entry(&path, "ignoreFindings", "x/**").expect("entry");

        assert_eq!(span.path, canonical(&deepest));
    }

    #[test]
    fn a_remote_source_after_the_local_ones_gives_no_location() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(&path, r#"{"extends": ["https://example.com/fallow.json"]}"#)
            .expect("write config");

        assert_eq!(
            FallowConfig::locate_list_entry(&path, "ignoreFindings", "x/**"),
            None
        );
    }

    #[test]
    fn a_byte_order_mark_does_not_shift_the_range() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(&path, "\u{FEFF}{\"ignoreFindings\": [\"a/**\"]}").expect("write config");

        let span = FallowConfig::locate_list_entry(&path, "ignoreFindings", "a/**").expect("entry");

        assert_eq!(located_text(&span), "\"a/**\"");
    }

    #[test]
    fn an_entry_that_is_not_in_the_list_gives_no_location() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(&path, r#"{"ignoreFindings": ["a/**"]}"#).expect("write config");

        assert_eq!(
            FallowConfig::locate_list_entry(&path, "ignoreFindings", "b/**"),
            None
        );
        assert_eq!(
            FallowConfig::locate_list_entry(&path, "ignoreDependencies", "a/**"),
            None
        );
    }
}
