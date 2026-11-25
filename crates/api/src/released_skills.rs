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
/// A string value stays a string, and a boolean stays a boolean. Other
/// scalars keep their source text as a string. A nested mapping or sequence
/// is an error, because no released skill uses one.
///
/// # Errors
///
/// Returns an error when the text has no frontmatter block, the block is not
/// valid YAML, or a value is not a scalar. CRLF line ends are accepted.
pub fn parse_frontmatter(skill_md: &str) -> Result<Frontmatter, String> {
    let skill_md = skill_md.replace("\r\n", "\n");
    let block = skill_md
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n").map(|(block, _)| block))
        .ok_or("SKILL.md has no frontmatter block")?;
    let document = fallow_config::yaml::parse(block).map_err(|e| e.to_string())?;
    let mapping = document
        .root()
        .as_mapping()
        .ok_or("SKILL.md frontmatter is not a mapping")?;
    let mut fields = Frontmatter::new();
    for (key, value) in mapping.iter() {
        let key = key.scalar_text().ok_or("frontmatter key is not a scalar")?;
        let value = if let Some(flag) = value.as_bool() {
            serde_json::Value::Bool(flag)
        } else {
            let text = value
                .scalar_text()
                .ok_or_else(|| format!("frontmatter field `{key}` is not a scalar"))?;
            serde_json::Value::String(text.to_string())
        };
        fields.insert(key.to_string(), value);
    }
    Ok(fields)
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
    fn frontmatter_rejects_missing_block_and_nested_values() {
        assert!(parse_frontmatter("# No frontmatter\n").is_err());
        assert!(parse_frontmatter("---\nname: demo\nmetadata:\n  a: b\n---\n").is_err());
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
