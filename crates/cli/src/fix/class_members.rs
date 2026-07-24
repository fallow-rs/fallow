use std::ops::Range;
use std::path::{Path, PathBuf};

use fallow_config::OutputFormat;
use fallow_types::output_dead_code::UnusedClassMemberFinding;
use fallow_types::semantic::{
    SemanticCandidateDecision, SemanticCandidateDecisionKind, SemanticEditGuard,
};
use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};

use super::io::bytes_with_optional_bom;
use super::plan::{CapturedHashes, FixPlan, SkipReason, read_source_with_hash_check};

pub(super) struct ClassMemberFixInput<'a> {
    pub(super) root: &'a Path,
    pub(super) findings: &'a [UnusedClassMemberFinding],
    pub(super) hashes: &'a CapturedHashes,
    pub(super) plan: &'a mut FixPlan,
    pub(super) output: OutputFormat,
    pub(super) dry_run: bool,
    pub(super) fixes: &'a mut Vec<serde_json::Value>,
}

struct PlannedMember<'a> {
    finding: &'a UnusedClassMemberFinding,
    decision: &'a SemanticCandidateDecision,
    range: Range<usize>,
}

/// Apply only API-approved, exact-span class-member fixes.
pub(super) fn apply_class_member_fixes(input: ClassMemberFixInput<'_>) {
    let ClassMemberFixInput {
        root,
        findings,
        hashes,
        plan,
        output,
        dry_run,
        fixes,
    } = input;
    let mut by_file: FxHashMap<PathBuf, Vec<&UnusedClassMemberFinding>> = FxHashMap::default();
    for finding in findings {
        if finding.member.kind == fallow_types::extract::MemberKind::ClassMethod
            && finding
                .semantic
                .as_ref()
                .is_some_and(|decision| decision.closed_world_eligible)
        {
            by_file
                .entry(finding.member.path.clone())
                .or_default()
                .push(finding);
        }
    }

    for (path, findings) in by_file {
        apply_file(ClassMemberFileInput {
            root,
            path: &path,
            findings: &findings,
            hashes,
            plan,
            output,
            dry_run,
            fixes,
        });
    }
}

struct ClassMemberFileInput<'a> {
    root: &'a Path,
    path: &'a Path,
    findings: &'a [&'a UnusedClassMemberFinding],
    hashes: &'a CapturedHashes,
    plan: &'a mut FixPlan,
    output: OutputFormat,
    dry_run: bool,
    fixes: &'a mut Vec<serde_json::Value>,
}

fn apply_file(input: ClassMemberFileInput<'_>) {
    let ClassMemberFileInput {
        root,
        path,
        findings,
        hashes,
        plan,
        output,
        dry_run,
        fixes,
    } = input;
    let Some((content, metadata)) = read_source_with_hash_check(root, path, hashes, plan) else {
        return;
    };
    let bom_offset = usize::from(metadata.had_bom) * '\u{FEFF}'.len_utf8();
    let Some(mut planned) = plan_members(&content, findings, bom_offset) else {
        plan.skip(path.to_path_buf(), SkipReason::ContentChanged);
        return;
    };
    planned.sort_by_key(|member| std::cmp::Reverse(member.range.start));

    let relative = path.strip_prefix(root).unwrap_or(path);
    for member in &planned {
        record_fix(
            output,
            dry_run,
            relative,
            member.finding,
            member.decision,
            fixes,
        );
    }
    if dry_run {
        return;
    }

    let original = bytes_with_optional_bom(content.clone(), &metadata);
    let mut rewritten = content;
    for member in planned {
        rewritten.replace_range(member.range, "");
    }
    let rewritten = bytes_with_optional_bom(rewritten, &metadata);
    plan.stage_existing(path.to_path_buf(), &original, rewritten);
}

fn plan_members<'a>(
    content: &str,
    findings: &'a [&'a UnusedClassMemberFinding],
    bom_offset: usize,
) -> Option<Vec<PlannedMember<'a>>> {
    let mut planned: Vec<PlannedMember<'a>> = Vec::new();
    for finding in findings {
        let decision = finding.semantic.as_ref()?;
        if decision.decision != SemanticCandidateDecisionKind::ConfirmedNoStaticReferences
            || !decision.closed_world_eligible
            || decision.subject.line != finding.member.line
            || decision.subject.col != finding.member.col
            || decision.subject.local_name != finding.member.member_name
            || decision.subject.owner.as_deref() != Some(finding.member.parent_name.as_str())
        {
            return None;
        }
        let guard = decision.edit_guard.as_ref()?;
        let exact = adjusted_guard_range(guard, bom_offset, content.len())?;
        if !content.is_char_boundary(exact.start)
            || !content.is_char_boundary(exact.end)
            || sha256_hex(&content.as_bytes()[exact.clone()]) != guard.declaration_sha256
        {
            return None;
        }
        let range = expand_declaration_range(content, exact);
        if planned
            .iter()
            .any(|existing| ranges_overlap(&existing.range, &range))
        {
            return None;
        }
        planned.push(PlannedMember {
            finding,
            decision,
            range,
        });
    }
    Some(planned)
}

fn adjusted_guard_range(
    guard: &SemanticEditGuard,
    bom_offset: usize,
    content_len: usize,
) -> Option<Range<usize>> {
    let start = guard.start.checked_sub(bom_offset)?;
    let end = guard.end.checked_sub(bom_offset)?;
    (start < end && end <= content_len).then_some(start..end)
}

fn expand_declaration_range(content: &str, exact: Range<usize>) -> Range<usize> {
    let bytes = content.as_bytes();
    let line_start = bytes[..exact.start]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let prefix_is_indent = bytes[line_start..exact.start]
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'));
    let line_end = bytes[exact.end..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|offset| exact.end + offset);
    let suffix_end = line_end.unwrap_or(bytes.len());
    let suffix_is_whitespace = bytes[exact.end..suffix_end]
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'));
    if prefix_is_indent && suffix_is_whitespace {
        let end = line_end.map_or(suffix_end, |index| index + 1);
        return line_start..consume_one_blank_line(bytes, end);
    }
    exact
}

fn consume_one_blank_line(bytes: &[u8], start: usize) -> usize {
    let Some(next_newline) = bytes[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|offset| start + offset)
    else {
        return start;
    };
    if bytes[start..next_newline]
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
    {
        next_newline + 1
    } else {
        start
    }
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn record_fix(
    output: OutputFormat,
    dry_run: bool,
    relative: &Path,
    finding: &UnusedClassMemberFinding,
    decision: &SemanticCandidateDecision,
    fixes: &mut Vec<serde_json::Value>,
) {
    if !matches!(output, OutputFormat::Json) {
        let action = if dry_run { "Would remove" } else { "Removed" };
        eprintln!(
            "{action} class member from {}:{} `{}.{}`: {}",
            relative.display(),
            finding.member.line,
            finding.member.parent_name,
            finding.member.member_name,
            decision.explanation,
        );
    }
    let mut record = serde_json::json!({
        "type": "remove_class_member",
        "path": relative.to_string_lossy().replace('\\', "/"),
        "line": finding.member.line,
        "parent": finding.member.parent_name,
        "name": finding.member.member_name,
        "semantic_decision": "confirmed-no-static-references",
        "closed_world_eligible": true,
        "explanation": decision.explanation,
    });
    if !dry_run {
        record["applied"] = serde_json::Value::Bool(true);
    }
    fixes.push(record);
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::extract::MemberKind;
    use fallow_types::results::UnusedMember;
    use fallow_types::semantic::{
        SemanticCandidateDecision, SemanticCompleteness, SemanticNamespace, SemanticSymbol,
    };

    fn eligible_finding(path: PathBuf, source: &str) -> UnusedClassMemberFinding {
        let start = source.find("execute").unwrap();
        let end = source[start..].find('}').unwrap() + start + 1;
        let mut finding = UnusedClassMemberFinding::with_actions(UnusedMember {
            path,
            parent_name: "Worker".to_string(),
            member_name: "execute".to_string(),
            kind: MemberKind::ClassMethod,
            line: 2,
            col: 2,
        });
        finding.set_semantic_decision(SemanticCandidateDecision {
            query_id: 0,
            subject: SemanticSymbol {
                path: PathBuf::from("src/worker.ts"),
                namespace: SemanticNamespace::Value,
                declaration_kind: "class_method".to_string(),
                exported_name: "execute".to_string(),
                local_name: "execute".to_string(),
                owner: Some("Worker".to_string()),
                line: 2,
                col: 2,
            },
            decision: SemanticCandidateDecisionKind::ConfirmedNoStaticReferences,
            status: SemanticCompleteness::Complete,
            owning_projects: vec!["tsconfig.json".to_string()],
            evidence: Vec::new(),
            contract: None,
            closed_world_eligible: true,
            edit_guard: Some(SemanticEditGuard {
                start,
                end,
                declaration_sha256: sha256_hex(&source.as_bytes()[start..end]),
            }),
            reason_code: None,
            explanation: "Guarded fix is available.".to_string(),
            actions: Vec::new(),
            total_evidence_count: 0,
            truncated: false,
            omissions: Vec::new(),
        });
        finding
    }

    #[test]
    fn expands_a_standalone_member_to_its_whole_source_line() {
        let source = "class Worker {\n  execute(): void {}\n  keep(): void {}\n}\n";
        let start = source.find("execute").unwrap();
        let end = source[start..].find('}').unwrap() + start + 1;
        let range = expand_declaration_range(source, start..end);

        assert_eq!(&source[range], "  execute(): void {}\n");
    }

    #[test]
    fn keeps_an_inline_member_removal_bounded_to_the_declaration() {
        let source = "class Worker { execute(): void {} keep(): void {} }\n";
        let start = source.find("execute").unwrap();
        let end = source[start..].find('}').unwrap() + start + 1;
        let range = expand_declaration_range(source, start..end);

        assert_eq!(&source[range], "execute(): void {}");
    }

    #[test]
    fn applies_an_eligible_guarded_member_and_preserves_the_used_sibling() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("src/worker.ts");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let source = "class Worker {\n  execute(): void {}\n\n  keep(): void {}\n}\n";
        std::fs::write(&path, source).unwrap();
        let finding = eligible_finding(path.clone(), source);
        let mut hashes = CapturedHashes::default();
        hashes.insert(path.clone(), xxhash_rust::xxh3::xxh3_64(source.as_bytes()));
        let mut plan = FixPlan::new();
        let mut fixes = Vec::new();

        apply_class_member_fixes(ClassMemberFixInput {
            root: directory.path(),
            findings: &[finding],
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Json,
            dry_run: false,
            fixes: &mut fixes,
        });
        let outcome = plan.commit();

        assert!(outcome.failed.is_empty());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "class Worker {\n  keep(): void {}\n}\n"
        );
        assert_eq!(
            fixes[0]["semantic_decision"],
            "confirmed-no-static-references"
        );
        assert_eq!(fixes[0]["applied"], true);
    }

    #[test]
    fn refuses_an_eligible_guarded_class_property() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("src/worker.ts");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let source = "class Worker {\n  execute = 1;\n  keep = 2;\n}\n";
        std::fs::write(&path, source).unwrap();
        let start = source.find("execute").unwrap();
        let end = source[start..].find(';').unwrap() + start + 1;
        let mut finding = eligible_finding(path.clone(), source);
        finding.member.kind = MemberKind::ClassProperty;
        let decision = finding.semantic.as_mut().unwrap();
        decision.subject.declaration_kind = "class_property".to_string();
        decision.edit_guard = Some(SemanticEditGuard {
            start,
            end,
            declaration_sha256: sha256_hex(&source.as_bytes()[start..end]),
        });
        let mut hashes = CapturedHashes::default();
        hashes.insert(path.clone(), xxhash_rust::xxh3::xxh3_64(source.as_bytes()));
        let mut plan = FixPlan::new();
        let mut fixes = Vec::new();

        apply_class_member_fixes(ClassMemberFixInput {
            root: directory.path(),
            findings: &[finding],
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Json,
            dry_run: false,
            fixes: &mut fixes,
        });
        let outcome = plan.commit();

        assert!(outcome.failed.is_empty());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "class Worker {\n  execute = 1;\n  keep = 2;\n}\n"
        );
        assert!(fixes.is_empty());
    }

    #[test]
    fn refuses_a_stale_semantic_declaration_guard() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("src/worker.ts");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let analyzed = "class Worker {\n  execute(): void {}\n}\n";
        let changed = "class Worker {\n  execute(value: string): void {}\n}\n";
        std::fs::write(&path, changed).unwrap();
        let finding = eligible_finding(path.clone(), analyzed);
        let mut hashes = CapturedHashes::default();
        hashes.insert(path.clone(), xxhash_rust::xxh3::xxh3_64(changed.as_bytes()));
        let mut plan = FixPlan::new();
        let mut fixes = Vec::new();

        apply_class_member_fixes(ClassMemberFixInput {
            root: directory.path(),
            findings: &[finding],
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Json,
            dry_run: false,
            fixes: &mut fixes,
        });

        assert_eq!(plan.skipped().len(), 1);
        assert_eq!(plan.skipped()[0].reason, SkipReason::ContentChanged);
        assert!(fixes.is_empty());
        assert_eq!(std::fs::read_to_string(path).unwrap(), changed);
    }
}
