//! Shared helpers for analyzing whether an exported enum declaration can be
//! safely removed in its entirety. Used by both `exports.rs` (when the export
//! itself is unused) and `enum_members.rs` (when every member of the enum is
//! unused, even if the enum has importers).

#[derive(Clone, Copy)]
pub(super) struct EnumDeclarationRange {
    pub(super) start_line: usize,
    pub(super) end_line: usize,
}

fn strip_enum_modifier<'a>(s: &'a str, modifier: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(modifier)?;
    rest.chars()
        .next()
        .is_some_and(char::is_whitespace)
        .then(|| rest.trim_start())
}

pub(super) fn declares_exported_enum(line: &str, enum_name: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(mut rest) = trimmed.strip_prefix("export").map(str::trim_start) else {
        return false;
    };

    for _ in 0..2 {
        if let Some(next) = strip_enum_modifier(rest, "declare") {
            rest = next;
        } else if let Some(next) = strip_enum_modifier(rest, "const") {
            rest = next;
        }
    }

    let Some(after_enum) = rest.strip_prefix("enum").map(str::trim_start) else {
        return false;
    };
    let Some(after_name) = after_enum.strip_prefix(enum_name) else {
        return false;
    };
    after_name
        .chars()
        .next()
        .is_some_and(|ch| ch.is_whitespace() || ch == '{')
}

fn find_enum_declaration_range(lines: &[&str], line_idx: usize) -> Option<EnumDeclarationRange> {
    let start_line = lines.get(line_idx)?;
    let export_col = start_line.find("export")?;
    if !start_line[..export_col].trim().is_empty() {
        return None;
    }

    let mut brace_depth = 0i32;
    let mut saw_open_brace = false;

    for (idx, line) in lines.iter().enumerate().skip(line_idx) {
        let chars: Vec<char> = line.chars().collect();
        for (char_idx, ch) in chars.iter().enumerate() {
            match ch {
                '{' => {
                    saw_open_brace = true;
                    brace_depth += 1;
                }
                '}' if saw_open_brace => {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        let suffix: String = chars[char_idx + 1..].iter().collect();
                        if suffix.trim().trim_end_matches(';').trim().is_empty() {
                            return Some(EnumDeclarationRange {
                                start_line: line_idx,
                                end_line: idx,
                            });
                        }
                        return None;
                    }
                }
                _ => {}
            }
        }
    }

    None
}

fn is_ident_char(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_ascii_alphanumeric()
}

fn contains_identifier(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(idx, _)| {
        let before = text[..idx].chars().next_back();
        let after = text[idx + name.len()..].chars().next();
        !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char)
    })
}

fn has_identifier_outside_range(lines: &[&str], name: &str, range: EnumDeclarationRange) -> bool {
    lines
        .iter()
        .enumerate()
        .filter(|(idx, _)| *idx < range.start_line || *idx > range.end_line)
        .any(|(_, line)| contains_identifier(line, name))
}

/// Returns the byte range of an exported enum declaration if it is safe to
/// remove the whole block: the line declares `export enum NAME`, the brace
/// structure is well-formed, and the enum name does not appear anywhere
/// outside the declaration body in the same file. Importers in OTHER files
/// are not consulted; callers must guarantee the enum is dead at the
/// project level before invoking this.
pub(super) fn removable_exported_enum_range(
    lines: &[&str],
    line_idx: usize,
    enum_name: &str,
) -> Option<EnumDeclarationRange> {
    let line = *lines.get(line_idx)?;
    if !declares_exported_enum(line, enum_name) {
        return None;
    }
    let range = find_enum_declaration_range(lines, line_idx)?;
    (!has_identifier_outside_range(lines, enum_name, range)).then_some(range)
}
