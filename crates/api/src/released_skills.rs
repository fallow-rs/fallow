//! The released fallow skills (`npm/fallow/skills/<name>/`), embedded at
//! build time. `fallow agent install` writes them to disk, and the MCP server
//! serves them through the skills extension.

use std::io::{self, Read as _};

include!(concat!(env!("OUT_DIR"), "/embedded_skill.rs"));

impl EmbeddedSkillFile {
    /// The uncompressed bytes of the file.
    ///
    /// # Errors
    ///
    /// Returns an error when the embedded gzip payload is corrupt.
    pub fn bytes(&self) -> io::Result<Vec<u8>> {
        let mut decoder = flate2::read::GzDecoder::new(self.gzip);
        let mut bytes = Vec::with_capacity(self.raw_len);
        decoder.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

/// The frontmatter of a `SKILL.md`: every field, in source order.
pub type Frontmatter = serde_json::Map<String, serde_json::Value>;

/// Parse the YAML frontmatter at the start of a `SKILL.md`.
///
/// Each value keeps its YAML type: a string, a boolean, a number, a null, a
/// sequence or a mapping. A plain scalar that is not a valid JSON number,
/// such as `.inf`, keeps its source text as a string.
///
/// # Errors
///
/// Returns an error when the text has no frontmatter block, the block is not
/// valid YAML, or a mapping key is not a scalar. CRLF line ends are accepted.
pub fn parse_frontmatter(skill_md: &str) -> Result<Frontmatter, String> {
    let skill_md = skill_md.replace("\r\n", "\n");
    let block = skill_md
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n").map(|(block, _)| block))
        .ok_or("SKILL.md has no frontmatter block")?;
    let document = fallow_config::yaml::parse(block).map_err(|e| e.to_string())?;
    match yaml_to_json(document.root())? {
        serde_json::Value::Object(fields) => Ok(fields),
        _ => Err("SKILL.md frontmatter is not a mapping".to_string()),
    }
}

fn yaml_to_json(node: fallow_config::yaml::YamlNode<'_>) -> Result<serde_json::Value, String> {
    use serde_json::Value;
    if node.is_null() {
        return Ok(Value::Null);
    }
    if let Some(flag) = node.as_bool() {
        return Ok(Value::Bool(flag));
    }
    if let Some(text) = node.as_str() {
        return Ok(Value::String(text.to_string()));
    }
    if let Some(mapping) = node.as_mapping() {
        let mut fields = Frontmatter::new();
        for (key, value) in mapping.iter() {
            let key = key.scalar_text().ok_or("frontmatter key is not a scalar")?;
            fields.insert(key.to_string(), yaml_to_json(value)?);
        }
        return Ok(Value::Object(fields));
    }
    if let Some(items) = node.as_sequence() {
        return items
            .map(yaml_to_json)
            .collect::<Result<_, _>>()
            .map(Value::Array);
    }
    let text = node.scalar_text().ok_or("unsupported frontmatter value")?;
    Ok(serde_json::from_str::<serde_json::Number>(text)
        .map_or_else(|_| Value::String(text.to_string()), Value::Number))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_keeps_every_field() {
        let fields = parse_frontmatter(
            "---\nname: demo\ndescription: \"Use: when asked\"\nlicense: MIT\nbeta: true\n---\n# Demo\n",
        )
        .expect("valid frontmatter");
        assert_eq!(
            serde_json::Value::Object(fields),
            serde_json::json!({
                "name": "demo",
                "description": "Use: when asked",
                "license": "MIT",
                "beta": true,
            })
        );
    }

    #[test]
    fn frontmatter_keeps_value_types() {
        let fields = parse_frontmatter(
            "---\nname: demo\nversion: 1.10\ncount: 3\nempty: ~\nquoted: \"42\"\n\
             allowed-tools: [Read, Grep]\nmetadata:\n  owner: fallow\n---\n",
        )
        .expect("valid frontmatter");
        assert_eq!(
            serde_json::Value::Object(fields),
            serde_json::json!({
                "name": "demo",
                "version": 1.10,
                "count": 3,
                "empty": null,
                "quoted": "42",
                "allowed-tools": ["Read", "Grep"],
                "metadata": { "owner": "fallow" },
            })
        );
    }

    #[test]
    fn frontmatter_rejects_a_missing_block() {
        assert!(parse_frontmatter("# No frontmatter\n").is_err());
        assert!(parse_frontmatter("---\n- a\n- b\n---\n").is_err());
    }

    #[test]
    fn every_embedded_skill_has_name_and_description() {
        for skill in EMBEDDED_SKILLS {
            let skill_md = skill
                .files
                .iter()
                .find(|f| f.path == "SKILL.md")
                .expect("every skill ships SKILL.md");
            let text = String::from_utf8(skill_md.bytes().expect("decompress SKILL.md"))
                .expect("SKILL.md is UTF-8");
            let fields = parse_frontmatter(&text).expect("valid frontmatter");
            assert_eq!(fields["name"], skill.name);
            assert!(
                fields["description"]
                    .as_str()
                    .is_some_and(|d| !d.is_empty())
            );
            for file in skill.files {
                let bytes = file.bytes().expect("decompress skill file");
                assert_eq!(bytes.len(), file.raw_len, "{}", file.path);
            }
        }
    }
}
