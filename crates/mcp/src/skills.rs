//! The skills extension (`io.modelcontextprotocol/skills`, SEP-2640).
//!
//! The server serves the released fallow skills that `fallow-api` embeds at
//! build time, so each client gets the skill of the same version as the
//! tools. Each skill lives at `skill://<name>/SKILL.md`. Its manifest lists
//! every file with the SHA-256 digest and the byte size of the served bytes.
//! The manifest is complete, so the server does not offer directory reads.

use std::sync::OnceLock;

use fallow_api::released_skills::{EMBEDDED_SKILLS, EmbeddedSkill, parse_frontmatter};
use fallow_types::mcp_manifest::MCP_SKILLS_ENV;
use rmcp::ErrorData as McpError;
use rmcp::model::{
    CacheScope, GetSkillResult, ListSkillsResult, ReadResourceResult, ResourceContents, SKILL_MD,
    Skill, SkillFrontmatter, SkillResource, SkillResources,
};

const SKILL_URI_PREFIX: &str = "skill://";

/// How long a host may cache a skill result. The skills are compiled into
/// the binary, so they change only with an upgrade of the server. The limit
/// bounds how long a host can serve the skill of a replaced binary.
const CACHE_TTL_MS: u64 = 60 * 60 * 1000;

/// One file of a served skill.
struct ServedFile {
    uri: String,
    text: String,
}

/// One served skill: its entry and the files of its manifest.
struct ServedSkill {
    entry: Skill,
    files: Vec<ServedFile>,
}

fn catalog() -> &'static [ServedSkill] {
    static CATALOG: OnceLock<Vec<ServedSkill>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        EMBEDDED_SKILLS
            .iter()
            .filter_map(|skill| match serve(skill) {
                Ok(served) => Some(served),
                Err(error) => {
                    tracing::warn!(skill = skill.name, %error, "embedded skill not served");
                    None
                }
            })
            .collect()
    })
}

fn serve(skill: &EmbeddedSkill) -> Result<ServedSkill, String> {
    let root = format!("{SKILL_URI_PREFIX}{}", skill.name);
    let mut files = Vec::with_capacity(skill.files.len());
    let mut manifest = Vec::with_capacity(skill.files.len());
    let mut frontmatter = None;
    for file in skill.files {
        let bytes = file.bytes().map_err(|e| format!("{}: {e}", file.path))?;
        let uri = format!("{root}/{}", file.path);
        manifest.push(SkillResource::from_bytes(uri.clone(), &bytes));
        let text = String::from_utf8(bytes).map_err(|e| format!("{}: {e}", file.path))?;
        if file.path == SKILL_MD {
            frontmatter = Some(parse_frontmatter(&text)?);
        }
        files.push(ServedFile { uri, text });
    }
    let mut fields = frontmatter.ok_or("the skill has no SKILL.md")?;
    let name = take_string(&mut fields, "name")?;
    let description = take_string(&mut fields, "description")?;
    let mut entry_frontmatter = SkillFrontmatter::new(name, description);
    entry_frontmatter.extra = fields;
    let entry = Skill::new(
        format!("{root}/{SKILL_MD}"),
        entry_frontmatter,
        SkillResources::Files(manifest),
    );
    entry.validate().map_err(|e| e.to_string())?;
    Ok(ServedSkill { entry, files })
}

fn take_string(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<String, String> {
    match fields.remove(key) {
        Some(serde_json::Value::String(value)) => Ok(value),
        _ => Err(format!("SKILL.md frontmatter has no string `{key}`")),
    }
}

/// Whether the server serves skills: the build embeds at least one, and
/// [`MCP_SKILLS_ENV`] does not turn them off. The server declares the
/// extension only when it does.
pub fn available() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| enabled_from(|key| std::env::var_os(key))) && !catalog().is_empty()
}

fn enabled_from(lookup: impl Fn(&str) -> Option<std::ffi::OsString>) -> bool {
    let Some(value) = lookup(MCP_SKILLS_ENV) else {
        return true;
    };
    let value = value.to_string_lossy().trim().to_ascii_lowercase();
    !matches!(value.as_str(), "0" | "false" | "off" | "no")
}

/// The `skills/list` result: every served skill on one page.
pub fn list_skills() -> ListSkillsResult {
    ListSkillsResult::with_all_items(catalog().iter().map(|s| s.entry.clone()).collect())
        .with_ttl_ms(CACHE_TTL_MS)
        .with_cache_scope(CacheScope::Public)
}

/// The `SKILL.md` URI of every served skill, in listing order.
pub fn skill_uris() -> impl Iterator<Item = &'static str> {
    catalog().iter().map(|skill| skill.entry.uri.as_str())
}

/// The `skills/get` result for one `SKILL.md` URI.
///
/// # Errors
///
/// Returns `-32602` (invalid params) when no served skill has that URI.
pub fn get_skill(uri: &str) -> Result<GetSkillResult, McpError> {
    catalog()
        .iter()
        .find(|skill| skill.entry.uri == uri)
        .map(|skill| {
            GetSkillResult::new(skill.entry.clone())
                .with_ttl_ms(CACHE_TTL_MS)
                .with_cache_scope(CacheScope::Public)
        })
        .ok_or_else(|| {
            McpError::invalid_params(
                format!("no skill at {uri}"),
                Some(serde_json::json!({ "known_skills": skill_uris().collect::<Vec<_>>() })),
            )
        })
}

/// The `resources/read` result for a `skill://` URI, or `None` when the URI
/// has another scheme.
///
/// # Errors
///
/// Returns `-32602` (invalid params) when no served skill has that file. The
/// error data lists the files of the skill that the URI names, or every
/// served skill when the URI names none.
pub fn read_skill_file(uri: &str) -> Option<Result<ReadResourceResult, McpError>> {
    let path = uri.strip_prefix(SKILL_URI_PREFIX)?;
    let found = catalog()
        .iter()
        .flat_map(|skill| &skill.files)
        .find(|file| file.uri == uri);
    let Some(file) = found else {
        return Some(Err(unknown_skill_file_error(uri, path)));
    };
    let contents = ResourceContents::text(file.text.clone(), file.uri.clone())
        .with_mime_type(mime_type(&file.uri));
    Some(Ok(ReadResourceResult::new(vec![contents])
        .with_ttl_ms(CACHE_TTL_MS)
        .with_cache_scope(CacheScope::Public)))
}

fn unknown_skill_file_error(uri: &str, path: &str) -> McpError {
    let name = path.split('/').next().unwrap_or_default();
    let skill = catalog().iter().find(|skill| skill.entry.name() == name);
    let data = match skill {
        Some(skill) => serde_json::json!({
            "skill": skill.entry.uri,
            "known_uris": skill.files.iter().map(|file| file.uri.as_str()).collect::<Vec<_>>(),
        }),
        None => serde_json::json!({ "known_skills": skill_uris().collect::<Vec<_>>() }),
    };
    McpError::invalid_params(format!("no skill file at {uri}"), Some(data))
}

fn mime_type(uri: &str) -> &'static str {
    let extension = std::path::Path::new(uri)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("md") => "text/markdown",
        Some("yaml" | "yml") => "application/yaml",
        _ => "text/plain",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_skill_is_served() {
        let served: Vec<&str> = catalog().iter().map(|skill| skill.entry.name()).collect();
        let embedded: Vec<&str> = EMBEDDED_SKILLS.iter().map(|skill| skill.name).collect();
        assert_eq!(
            served, embedded,
            "a skill that fails to load must fail this test"
        );
    }

    #[test]
    fn the_env_switch_turns_skills_off() {
        let with = |value: &'static str| {
            move |key: &str| (key == MCP_SKILLS_ENV).then(|| std::ffi::OsString::from(value))
        };
        assert!(enabled_from(|_| None));
        assert!(enabled_from(with("1")));
        for off in ["0", "false", "off", "no", " OFF "] {
            assert!(!enabled_from(with(off)), "{off}");
        }
    }
}
