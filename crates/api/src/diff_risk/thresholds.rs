//! Coverage thresholds in the base and head of a config file.

use rustc_hash::FxHashMap;

use super::text::LineHit;

const COVERAGE_KEYS: &[&str] = &["branches", "functions", "lines", "statements"];

#[derive(Clone, Eq, Hash, PartialEq)]
enum Scope {
    Key(String),
    Index(usize),
}

// A repeated path can still occur in separate object literals of one program.
type Thresholds = FxHashMap<(Vec<Scope>, String), Option<(f64, usize)>>;

fn record(values: &mut Thresholds, scope: &[Scope], key: &str, value: f64, offset: usize) {
    values
        .entry((scope.to_vec(), key.to_owned()))
        .and_modify(|entry| *entry = None)
        .or_insert(Some((value, offset)));
}

/// Thresholds in a coverage or quality config that the change made less strict.
#[must_use]
pub fn weakened_threshold_hits(base: &str, head: &str) -> Vec<LineHit> {
    hits_for_config("", base, head)
}

pub(super) fn hits_for_config(path: &str, base: &str, head: &str) -> Vec<LineHit> {
    let path = std::path::Path::new(path);
    let yaml = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml"));
    let extensionless = path
        .file_name()
        .is_some_and(|name| name == ".nycrc" || name == ".c8rc");
    let read = |source: &str| {
        let mut rest = source.trim_start_matches('\u{feff}');
        if yaml || (extensionless && !matches!(next_token(&mut rest), Some("{" | "["))) {
            yaml_values(source)
        } else {
            object_values(source)
        }
    };
    let mut hits = compare(&read(base), read(head), head);
    hits.extend(super::text::quality_threshold_hits(base, head));
    hits.sort_by_key(|hit| hit.line);
    hits
}

fn compare(base: &Thresholds, head: Thresholds, source: &str) -> Vec<LineHit> {
    let mut hits: Vec<_> = head
        .into_iter()
        .filter_map(|(path, entry)| {
            let (after, offset) = entry?;
            let (before, _) = (*base.get(&path)?)?;
            let (_, key) = path;
            (after < before).then(|| (offset, format!("{key}: {before} -> {after}")))
        })
        .collect();
    if hits.is_empty() {
        return Vec::new();
    }
    hits.sort_unstable_by_key(|(offset, _)| *offset);
    let lines = fallow_types::extract::compute_line_offsets(source);
    hits.into_iter()
        .map(|(offset, evidence)| LineHit {
            line: fallow_types::extract::byte_offset_to_line_col(
                &lines,
                u32::try_from(offset).unwrap_or(u32::MAX),
            )
            .0,
            evidence,
        })
        .collect()
}

fn yaml_values(source: &str) -> Thresholds {
    let mut values = Thresholds::default();
    let Ok(doc) = fallow_config::yaml::parse_with_locations(source) else {
        return values;
    };
    let mut objects = vec![(doc.root(), Vec::<Scope>::new())];
    while let Some((node, parents)) = objects.pop() {
        if let Some(entries) = node.as_sequence() {
            objects.extend(entries.enumerate().map(|(index, entry)| {
                let mut path = parents.clone();
                path.push(Scope::Index(index));
                (entry, path)
            }));
        }
        if let Some(map) = node.as_mapping() {
            for (key, value) in map.iter() {
                let Some(name) = key.as_str() else { continue };

                if COVERAGE_KEYS.contains(&name)
                    && let Some(number) = value.scalar_text().and_then(|text| text.parse().ok())
                    && let Some(range) = key.byte_range()
                {
                    record(&mut values, &parents, name, number, range.start);
                } else {
                    let mut path = parents.clone();
                    path.push(Scope::Key(name.to_owned()));
                    objects.push((value, path));
                }
            }
        }
    }
    values
}

fn object_values(source: &str) -> Thresholds {
    let mut values = Thresholds::default();
    let mut scope = Vec::new();
    let mut containers = Vec::new();
    let mut rest = source;
    let mut previous = ("", 0);
    let mut before_previous = ("", 0);
    while let Some(token) = next_token(&mut rest) {
        let start = source.len() - rest.len() - token.len();
        let owner = matches!(previous.0, ":" | "=").then(|| unquote(before_previous.0));
        match token {
            "{" | "[" | "(" => {
                containers.push((token, scope.len()));
                if token != "("
                    && let Some(key) = owner
                {
                    scope.push(Scope::Key(key.to_owned()));
                }
                if token == "[" {
                    scope.push(Scope::Index(0));
                }
            }
            "}" | "]" | ")" => {
                if let Some((_, parent_depth)) = containers.pop() {
                    scope.truncate(parent_depth);
                }
            }
            "," if matches!(containers.last(), Some(("[", _))) => {
                if let Some(Scope::Index(index)) = scope.last_mut() {
                    *index += 1;
                }
            }
            _ => {
                if let Some(key) = owner
                    && COVERAGE_KEYS.contains(&key)
                    && let Ok(number) = unquote(token).parse()
                {
                    record(&mut values, &scope, key, number, before_previous.1);
                }
            }
        }
        before_previous = previous;
        previous = (token, start);
    }
    values
}

fn unquote(token: &str) -> &str {
    if let Some(quote @ ('\'' | '"')) = token.chars().next() {
        return token[1..].strip_suffix(quote).unwrap_or(token);
    }
    token
}

fn next_token<'a>(rest: &mut &'a str) -> Option<&'a str> {
    loop {
        let text = rest.trim_start();
        if text.starts_with("//") {
            *rest = text.split_once('\n').map_or("", |(_, tail)| tail);
            continue;
        }
        if let Some(comment) = text.strip_prefix("/*") {
            *rest = comment.split_once("*/").map_or("", |(_, tail)| tail);
            continue;
        }
        let mut chars = text.char_indices();
        let (_, first) = chars.next()?;
        let is_word = |ch: char| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '$');
        let end = if matches!(first, '\'' | '"' | '`') {
            loop {
                match chars.next() {
                    Some((_, '\\')) => {
                        chars.next();
                    }
                    Some((i, ch)) if ch == first => break i + ch.len_utf8(),
                    None => break text.len(),
                    _ => {}
                }
            }
        } else if is_word(first) {
            text.find(|ch| !is_word(ch)).unwrap_or(text.len())
        } else {
            first.len_utf8()
        };
        let (token, after) = text.split_at(end);
        *rest = after;
        return Some(token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_thresholds_follow_their_scopes_across_reordering() {
        let base = r"export default defineConfig({ test: { coverage: { thresholds: {
            lines: 80,
            'src/lib/**': { lines: 60 },
            'src/ui/**': { lines: 90 },
        } } } })";
        let head = r"export default defineConfig({ test: { coverage: { thresholds: {
            'src/new/**': { lines: 50 },
            'src/ui/**': { lines: 85 },
            lines: 80,
        } } } })";
        assert_eq!(
            weakened_threshold_hits(base, head),
            vec![LineHit {
                line: 3,
                evidence: "lines: 90 -> 85".to_owned(),
            }]
        );
    }

    #[test]
    fn quality_thresholds_keep_dotted_toml_keys_and_json_overrides() {
        let cases = [
            (
                "fallow.toml",
                "health.maxCrap = 30",
                "[ \"health\" ]\nmaxCrap = '45'",
                2,
                "maxCrap: 30 -> 45",
            ),
            (
                ".fallowrc.json",
                r#"{"health":{"thresholdOverrides":[{"files":["a.ts"],"maxCognitive":20},{"files":["b.ts"],"maxCognitive":30}]}}"#,
                r#"{"health":{"thresholdOverrides":[{"files":["a.ts"],"maxCognitive":20},{"files":["b.ts"],"maxCognitive":40}]}}"#,
                1,
                "maxCognitive: 30 -> 40",
            ),
        ];
        for (file, base, head, line, evidence) in cases {
            assert_eq!(
                hits_for_config(file, base, head),
                vec![LineHit {
                    line,
                    evidence: evidence.into()
                }]
            );
        }
    }

    #[test]
    fn yaml_thresholds_keep_their_mapping_and_line() {
        let base = "thresholds:\n  global:\n    lines: 80\n  library:\n    lines: 65\n";
        let head = "thresholds:\n  library:\n    lines: '60'\n  global:\n    lines: 80\n";
        assert_eq!(
            hits_for_config(".nycrc.yml", base, head),
            vec![LineHit {
                line: 3,
                evidence: "lines: 65 -> 60".into()
            }]
        );
    }

    #[test]
    fn coverage_arrays_compare_thresholds_at_the_same_index() {
        let base = r#"{"projects":[{"lines":65},{"lines":80}]}"#;
        let head = r#"{"projects":[{"lines":70},
            {"lines":75}]}"#;
        for path in ["jest.config.json", "vitest.workspace.ts"] {
            assert_eq!(
                hits_for_config(path, base, head),
                vec![LineHit {
                    line: 2,
                    evidence: "lines: 80 -> 75".into()
                }],
            );
        }
    }

    #[test]
    fn yaml_sequences_compare_thresholds_at_the_same_index() {
        let base = "projects:\n  - branches: 65\n  - branches: 80\n";
        let head = "projects:\n  - branches: 70\n  - branches: 75\n";
        assert_eq!(
            hits_for_config(".nycrc.yml", base, head),
            vec![LineHit {
                line: 3,
                evidence: "branches: 80 -> 75".into()
            }],
        );
    }

    #[test]
    fn extensionless_coverage_configs_read_each_sides_format() {
        let json = "/* tool config */\n// global and per-directory goals\n{\"global\":{\"lines\":80},\"lib\":{\"lines\":65}}";
        let yaml = "# coverage goals\nlib:\n  lines: 60\nglobal:\n  lines: 85\n";
        for path in ["packages/app/.nycrc", ".c8rc"] {
            assert_eq!(
                hits_for_config(path, json, yaml),
                vec![LineHit {
                    line: 3,
                    evidence: "lines: 65 -> 60".into()
                }],
            );
            assert_eq!(
                hits_for_config(path, yaml, json),
                vec![LineHit {
                    line: 3,
                    evidence: "lines: 85 -> 80".into()
                }],
            );
        }
    }
}
