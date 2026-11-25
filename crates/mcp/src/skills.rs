//! The skills extension (`io.modelcontextprotocol/skills`, SEP-2640).
//!
//! The server serves the released fallow skills that `fallow-api` embeds at
//! build time, so each client gets the skill of the same version as the
//! tools. Each skill lives at `skill://<name>/SKILL.md`. Its manifest lists
//! every file with the SHA-256 digest and the byte size of the served bytes.
//! The manifest is complete, so the server does not offer directory reads.

use std::sync::OnceLock;

use fallow_api::released_skills::{EMBEDDED_SKILLS, EmbeddedSkill, parse_frontmatter};
use rmcp::ErrorData as McpError;
use rmcp::model::{
    GetSkillResult, ListSkillsResult, ReadResourceResult, ResourceContents, SKILL_MD, Skill,
    SkillFrontmatter, SkillResource, SkillResources,
};

const SKILL_URI_PREFIX: &str = "skill://";

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

/// Whether this build serves any skill. The server declares the extension
/// only when it does.
pub fn available() -> bool {
    !catalog().is_empty()
}

/// The `skills/list` result: every served skill on one page.
pub fn list_skills() -> ListSkillsResult {
    ListSkillsResult::with_all_items(catalog().iter().map(|s| s.entry.clone()).collect())
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
        .map(|skill| GetSkillResult::new(skill.entry.clone()))
        .ok_or_else(|| McpError::invalid_params(format!("no skill at {uri}"), None))
}

/// The `resources/read` result for one file of a served skill, or `None`
/// when the URI names no such file.
pub fn read_skill_file(uri: &str) -> Option<ReadResourceResult> {
    if !uri.starts_with(SKILL_URI_PREFIX) {
        return None;
    }
    let file = catalog()
        .iter()
        .flat_map(|skill| &skill.files)
        .find(|file| file.uri == uri)?;
    let contents = ResourceContents::text(file.text.clone(), file.uri.clone())
        .with_mime_type(mime_type(&file.uri));
    Some(ReadResourceResult::new(vec![contents]))
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
