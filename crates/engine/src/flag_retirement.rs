//! Per-flag retirement report for `fallow flags --retirement`.
//!
//! The per-site flag findings group into one row per flag identity. Each
//! detector adds a reason and its evidence to a row. The report is advisory:
//! every action is `auto_fixable: false`, and nothing here removes code.

use std::path::{Path, PathBuf};

use fallow_config::WorkspaceInfo;
use fallow_types::extract::FlagSiteFacts;
use fallow_types::flag_retirement::{
    FlagAgeMode, FlagRetirementReport, FlagSiteRole, RetirementAction, RetirementActionType,
    RetirementEvidence, RetirementFlag, RetirementFlagKind, RetirementReason, RetirementSite,
    RetirementSummary,
};
use fallow_types::results::{FeatureFlag, FlagKind};
use rustc_hash::FxHashMap;

/// File-name markers of story files. A story renders a component in
/// isolation, so a flag that only a story reads is not live in production.
const STORY_FILE_MARKERS: &[&str] = &[".stories.", ".story."];

/// Description of the one action on a retirement candidate.
const REVIEW_DESCRIPTION: &str = "Review this flag for retirement. The evidence lists the reasons.";

/// One site of a flag, before the sites group into rows.
#[derive(Debug, Clone)]
pub struct RetirementSiteInput {
    /// Absolute path of the file.
    pub path: PathBuf,
    /// Flag identifier.
    pub flag_name: String,
    /// How the flag was detected.
    pub kind: RetirementFlagKind,
    /// SDK provider label, if known.
    pub sdk_name: Option<String>,
    /// 1-based line.
    pub line: u32,
    /// 0-based byte column.
    pub col: u32,
    /// What the site does with the flag.
    pub role: FlagSiteRole,
    /// Unused exports inside the block that the site guards.
    pub guarded_dead_exports: Vec<String>,
    /// Facts about the guard of the site.
    pub facts: FlagSiteFacts,
    /// The literal value of a `const` flag, on its definition site.
    pub literal: Option<String>,
    /// Why no code reads this definition, when that is known.
    pub unread: Option<String>,
}

/// Flag facts that only the retirement report reads. The per-site
/// `feature_flags[]` array does not carry them.
#[derive(Debug, Default)]
pub struct RetirementFacts {
    /// Guard facts of each flag read, keyed by file, line and column.
    pub site_facts: FxHashMap<(PathBuf, u32, u32), FlagSiteFacts>,
    /// Sites that are not per-site flag findings: literal `const` flags
    /// (a definition and the guard reads) and unused registry members.
    pub constant_sites: Vec<RetirementSiteInput>,
    /// Definition sites that no code reads, keyed by file, line and column,
    /// with the reason.
    pub unread_definitions: FxHashMap<(PathBuf, u32, u32), String>,
}

impl RetirementFacts {
    /// Retirement sites for per-site flag findings, with their guard facts,
    /// followed by the sites of literal `const` flags.
    #[must_use]
    pub fn sites_for(&self, flags: &[FeatureFlag]) -> Vec<RetirementSiteInput> {
        flags
            .iter()
            .map(|flag| {
                let mut site = RetirementSiteInput::from_feature_flag(flag);
                let key = (flag.path.clone(), flag.line, flag.col);
                if let Some(facts) = self.site_facts.get(&key) {
                    site.facts = *facts;
                    if facts.definition() {
                        site.role = FlagSiteRole::Definition;
                    }
                }
                site.unread = self.unread_definitions.get(&key).cloned();
                site
            })
            .chain(self.constant_sites.iter().cloned())
            .collect()
    }
}

impl RetirementSiteInput {
    /// The retirement site of a per-site flag finding, without guard facts.
    #[must_use]
    pub fn from_feature_flag(flag: &FeatureFlag) -> Self {
        Self {
            path: flag.path.clone(),
            flag_name: flag.flag_name.clone(),
            kind: retirement_kind(flag.kind),
            sdk_name: flag.sdk_name.clone(),
            line: flag.line,
            col: flag.col,
            role: FlagSiteRole::Read,
            guarded_dead_exports: flag.guarded_dead_exports.clone(),
            facts: FlagSiteFacts::default(),
            literal: None,
            unread: None,
        }
    }
}

const fn retirement_kind(kind: FlagKind) -> RetirementFlagKind {
    match kind {
        FlagKind::EnvironmentVariable => RetirementFlagKind::EnvironmentVariable,
        FlagKind::SdkCall => RetirementFlagKind::SdkCall,
        FlagKind::ConfigObject => RetirementFlagKind::ConfigObject,
    }
}

/// How the report orders its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RetirementSort {
    /// Oldest flag first. Flags without an age come last.
    #[default]
    Age,
    /// Fewest read sites first.
    Sites,
    /// Flag name, ascending.
    Name,
}

/// Options that narrow and order the rows of the report.
#[derive(Debug, Clone, Default)]
pub struct RetirementOptions {
    /// Row order.
    pub sort: RetirementSort,
    /// Keep only flags at least this many days old. A flag without an age
    /// does not pass.
    pub min_age_days: Option<u64>,
    /// Keep only flags with at least one of these reasons. Empty keeps all.
    pub reasons: Vec<RetirementReason>,
    /// Keep only the first N rows after the sort.
    pub top: Option<usize>,
}

/// Identity of a flag: detection kind, SDK provider, name and workspace.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FlagKey {
    kind: RetirementFlagKind,
    sdk_name: Option<String>,
    flag_name: String,
    workspace: Option<String>,
}

/// Group flag sites into one row per flag and run the reason detectors.
///
/// `root` makes paths relative. `workspaces` adds the workspace root to the
/// flag identity, so two packages that use the same flag name get two rows.
/// Rows come back sorted by name; [`finish_report`] applies the final order.
#[must_use]
pub fn aggregate_flags(
    sites: Vec<RetirementSiteInput>,
    root: &Path,
    workspaces: &[WorkspaceInfo],
) -> Vec<RetirementFlag> {
    let mut groups: FxHashMap<FlagKey, Vec<RetirementSiteInput>> = FxHashMap::default();
    for site in sites {
        let key = FlagKey {
            kind: site.kind,
            sdk_name: site.sdk_name.clone(),
            flag_name: site.flag_name.clone(),
            workspace: workspace_of(&site.path, root, workspaces),
        };
        groups.entry(key).or_default().push(site);
    }
    let mut rows: Vec<RetirementFlag> = groups
        .into_iter()
        .map(|(key, sites)| build_row(key, sites, root))
        .collect();
    rows.sort_by(compare_identity);
    rows
}

fn build_row(key: FlagKey, mut inputs: Vec<RetirementSiteInput>, root: &Path) -> RetirementFlag {
    inputs.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
            .then(a.role.cmp(&b.role))
    });
    inputs.dedup_by(|a, b| {
        a.path == b.path && a.line == b.line && a.col == b.col && a.role == b.role
    });
    let sites: Vec<RetirementSite> = inputs
        .iter()
        .map(|input| {
            let path = relative(&input.path, root);
            RetirementSite {
                in_test: is_test_or_story(&path),
                path,
                line: input.line,
                col: input.col,
                role: input.role,
            }
        })
        .collect();
    let reads: Vec<&RetirementSite> = sites
        .iter()
        .filter(|site| site.role == FlagSiteRole::Read)
        .collect();
    let read_sites = reads.len();
    let test_only = read_sites > 0 && reads.iter().all(|site| site.in_test);

    let mut row = RetirementFlag {
        flag_name: key.flag_name,
        kind: key.kind,
        sdk_name: key.sdk_name,
        workspace: key.workspace,
        sites,
        read_sites,
        test_only,
        first_seen: None,
        oldest_surviving_site: None,
        last_touched: None,
        age_days: None,
        reasons: Vec::new(),
        evidence: Vec::new(),
        actions: Vec::new(),
    };
    detect_single_read_site(&mut row);
    detect_test_only(&mut row);
    detect_literal_constant(&mut row, &inputs, root);
    detect_guard_facts(&mut row, &inputs, root);
    detect_guards_dead_code(&mut row, &inputs, root);
    detect_defined_never_read(&mut row, &inputs, root);
    row
}

fn detect_defined_never_read(
    row: &mut RetirementFlag,
    inputs: &[RetirementSiteInput],
    root: &Path,
) {
    if row.read_sites > 0 {
        return;
    }
    for input in inputs {
        let Some(detail) = &input.unread else {
            continue;
        };
        add_reason(
            row,
            RetirementEvidence {
                reason: RetirementReason::DefinedNeverRead,
                path: relative(&input.path, root),
                line: input.line,
                detail: detail.clone(),
            },
        );
    }
}

fn detect_literal_constant(row: &mut RetirementFlag, inputs: &[RetirementSiteInput], root: &Path) {
    if row.kind != RetirementFlagKind::Constant {
        return;
    }
    let definition = inputs.iter().find(|input| input.literal.is_some());
    let Some(site) = definition.or_else(|| inputs.first()) else {
        return;
    };
    let detail = site.literal.as_ref().map_or_else(
        || "the flag is a const with a literal value".to_string(),
        |value| format!("const {} = {value}", row.flag_name),
    );
    add_reason(
        row,
        RetirementEvidence {
            reason: RetirementReason::LiteralConstant,
            path: relative(&site.path, root),
            line: site.line,
            detail,
        },
    );
}

fn detect_guard_facts(row: &mut RetirementFlag, inputs: &[RetirementSiteInput], root: &Path) {
    for input in inputs {
        if input.facts.identical_branches() {
            add_reason(
                row,
                RetirementEvidence {
                    reason: RetirementReason::IdenticalBranches,
                    path: relative(&input.path, root),
                    line: input.line,
                    detail: "both branches of the guard are the same code".to_string(),
                },
            );
        }
        if input.facts.empty_branch() {
            add_reason(
                row,
                RetirementEvidence {
                    reason: RetirementReason::EmptyBranch,
                    path: relative(&input.path, root),
                    line: input.line,
                    detail: "one branch of the guard is empty".to_string(),
                },
            );
        }
    }
}

fn detect_single_read_site(row: &mut RetirementFlag) {
    if row.read_sites != 1 {
        return;
    }
    let Some(site) = row
        .sites
        .iter()
        .find(|site| site.role == FlagSiteRole::Read)
    else {
        return;
    };
    let evidence = RetirementEvidence {
        reason: RetirementReason::SingleReadSite,
        path: site.path.clone(),
        line: site.line,
        detail: "the flag has one read site".to_string(),
    };
    add_reason(row, evidence);
}

fn detect_test_only(row: &mut RetirementFlag) {
    if !row.test_only {
        return;
    }
    let Some(site) = row
        .sites
        .iter()
        .find(|site| site.role == FlagSiteRole::Read)
    else {
        return;
    };
    let noun = if row.read_sites == 1 {
        "site is"
    } else {
        "sites are"
    };
    let evidence = RetirementEvidence {
        reason: RetirementReason::TestOnly,
        path: site.path.clone(),
        line: site.line,
        detail: format!(
            "all {} read {noun} in test, story or mock files",
            row.read_sites
        ),
    };
    add_reason(row, evidence);
}

fn detect_guards_dead_code(row: &mut RetirementFlag, inputs: &[RetirementSiteInput], root: &Path) {
    for input in inputs {
        if input.guarded_dead_exports.is_empty() {
            continue;
        }
        let evidence = RetirementEvidence {
            reason: RetirementReason::GuardsDeadCode,
            path: relative(&input.path, root),
            line: input.line,
            detail: format!(
                "the guarded block holds unused exports: {}",
                input.guarded_dead_exports.join(", ")
            ),
        };
        add_reason(row, evidence);
    }
}

/// Record a reason once and keep every piece of evidence for it.
fn add_reason(row: &mut RetirementFlag, evidence: RetirementEvidence) {
    if !row.reasons.contains(&evidence.reason) {
        row.reasons.push(evidence.reason);
    }
    row.evidence.push(evidence);
}

/// Count, filter, order and limit the rows, and add the review action to
/// each candidate.
#[must_use]
pub fn finish_report(
    mut rows: Vec<RetirementFlag>,
    age_mode: FlagAgeMode,
    generated_at_clock: Option<String>,
    options: &RetirementOptions,
) -> FlagRetirementReport {
    for row in &mut rows {
        row.reasons
            .sort_by_key(|reason| RetirementReason::ALL.iter().position(|r| r == reason));
        row.evidence.sort_by(|a, b| {
            reason_rank(a.reason)
                .cmp(&reason_rank(b.reason))
                .then(a.path.cmp(&b.path))
                .then(a.line.cmp(&b.line))
        });
        if !row.reasons.is_empty() {
            row.actions = vec![RetirementAction {
                kind: RetirementActionType::ReviewRetirement,
                auto_fixable: false,
                description: REVIEW_DESCRIPTION.to_string(),
            }];
        }
    }
    let summary = summarize(&rows);
    rows.retain(|row| passes_filters(row, options));
    rows.sort_by(|a, b| compare_for_sort(a, b, options.sort));
    if let Some(top) = options.top {
        rows.truncate(top);
    }
    FlagRetirementReport {
        generated_at_clock,
        age_mode,
        summary,
        flags: rows,
    }
}

fn reason_rank(reason: RetirementReason) -> usize {
    RetirementReason::ALL
        .iter()
        .position(|r| *r == reason)
        .unwrap_or(usize::MAX)
}

fn summarize(rows: &[RetirementFlag]) -> RetirementSummary {
    let mut summary = RetirementSummary {
        distinct_flags: rows.len(),
        ..RetirementSummary::default()
    };
    for row in rows {
        if !row.reasons.is_empty() {
            summary.candidates += 1;
        }
        for reason in &row.reasons {
            *summary.by_reason.entry(*reason).or_default() += 1;
        }
    }
    summary
}

fn passes_filters(row: &RetirementFlag, options: &RetirementOptions) -> bool {
    if let Some(min_age) = options.min_age_days
        && row.age_days.is_none_or(|age| age < min_age)
    {
        return false;
    }
    options.reasons.is_empty() || row.reasons.iter().any(|r| options.reasons.contains(r))
}

fn compare_for_sort(
    a: &RetirementFlag,
    b: &RetirementFlag,
    sort: RetirementSort,
) -> std::cmp::Ordering {
    let primary = match sort {
        // Oldest first; a flag without an age sorts after every aged flag.
        RetirementSort::Age => match (a.age_days, b.age_days) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        },
        RetirementSort::Sites => a.read_sites.cmp(&b.read_sites),
        RetirementSort::Name => std::cmp::Ordering::Equal,
    };
    primary.then_with(|| compare_identity(a, b))
}

fn compare_identity(a: &RetirementFlag, b: &RetirementFlag) -> std::cmp::Ordering {
    a.flag_name
        .cmp(&b.flag_name)
        .then(a.kind.cmp(&b.kind))
        .then(a.sdk_name.cmp(&b.sdk_name))
        .then(a.workspace.cmp(&b.workspace))
}

/// Root-relative workspace path of the deepest workspace that holds `path`,
/// or `None` for a file outside every workspace or a project without them.
fn workspace_of(path: &Path, root: &Path, workspaces: &[WorkspaceInfo]) -> Option<String> {
    workspaces
        .iter()
        .filter(|ws| path.starts_with(&ws.root) && ws.root != root)
        .max_by_key(|ws| ws.root.components().count())
        .map(|ws| relative(&ws.root, root))
}

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_test_or_story(relative_path: &str) -> bool {
    if crate::test_paths::is_test_path_str(relative_path) {
        return true;
    }
    let file_name = relative_path.rsplit('/').next().unwrap_or(relative_path);
    let lower = file_name.to_ascii_lowercase();
    STORY_FILE_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/repo";

    fn site(name: &str, path: &str, line: u32) -> RetirementSiteInput {
        RetirementSiteInput {
            path: PathBuf::from(ROOT).join(path),
            flag_name: name.to_string(),
            kind: RetirementFlagKind::EnvironmentVariable,
            sdk_name: None,
            line,
            col: 4,
            role: FlagSiteRole::Read,
            guarded_dead_exports: Vec::new(),
            facts: FlagSiteFacts::default(),
            literal: None,
            unread: None,
        }
    }

    fn sdk_site(name: &str, sdk: &str, path: &str, line: u32) -> RetirementSiteInput {
        RetirementSiteInput {
            kind: RetirementFlagKind::SdkCall,
            sdk_name: Some(sdk.to_string()),
            ..site(name, path, line)
        }
    }

    fn rows(sites: Vec<RetirementSiteInput>) -> Vec<RetirementFlag> {
        aggregate_flags(sites, Path::new(ROOT), &[])
    }

    fn row<'r>(rows: &'r [RetirementFlag], name: &str) -> &'r RetirementFlag {
        rows.iter()
            .find(|row| row.flag_name == name)
            .unwrap_or_else(|| panic!("no row for {name}"))
    }

    #[test]
    fn one_row_per_kind_sdk_and_name() {
        let rows = rows(vec![
            site("FEATURE_A", "src/a.ts", 1),
            site("FEATURE_A", "src/b.ts", 2),
            sdk_site("checkout", "LaunchDarkly", "src/a.ts", 3),
            sdk_site("checkout", "Statsig", "src/a.ts", 4),
            sdk_site("checkout", "LaunchDarkly", "src/c.ts", 5),
        ]);
        assert_eq!(rows.len(), 3);
        assert_eq!(row(&rows, "FEATURE_A").read_sites, 2);
        let launchdarkly = rows
            .iter()
            .find(|row| row.sdk_name.as_deref() == Some("LaunchDarkly"))
            .expect("LaunchDarkly row");
        assert_eq!(launchdarkly.read_sites, 2);
        assert_eq!(launchdarkly.sites[0].path, "src/a.ts");
        assert_eq!(launchdarkly.sites[1].path, "src/c.ts");
    }

    #[test]
    fn workspace_root_is_part_of_the_identity() {
        let workspaces = vec![
            WorkspaceInfo {
                root: PathBuf::from("/repo/packages/web"),
                name: "web".to_string(),
                is_internal_dependency: false,
            },
            WorkspaceInfo {
                root: PathBuf::from("/repo/packages/api"),
                name: "api".to_string(),
                is_internal_dependency: false,
            },
        ];
        let rows = aggregate_flags(
            vec![
                site("FEATURE_A", "packages/web/src/a.ts", 1),
                site("FEATURE_A", "packages/api/src/a.ts", 1),
                site("FEATURE_A", "scripts/a.ts", 1),
            ],
            Path::new(ROOT),
            &workspaces,
        );
        let workspaces: Vec<Option<&str>> = rows.iter().map(|r| r.workspace.as_deref()).collect();
        assert_eq!(
            workspaces,
            vec![None, Some("packages/api"), Some("packages/web")]
        );
    }

    #[test]
    fn single_read_site_needs_exactly_one_read() {
        let rows = rows(vec![
            site("FEATURE_ONE", "src/a.ts", 7),
            site("FEATURE_TWO", "src/a.ts", 1),
            site("FEATURE_TWO", "src/b.ts", 1),
        ]);
        let one = row(&rows, "FEATURE_ONE");
        assert_eq!(one.reasons, vec![RetirementReason::SingleReadSite]);
        assert_eq!(one.evidence[0].path, "src/a.ts");
        assert_eq!(one.evidence[0].line, 7);
        assert!(row(&rows, "FEATURE_TWO").reasons.is_empty());
    }

    #[test]
    fn test_only_needs_every_read_in_test_story_or_mock_files() {
        let rows = rows(vec![
            site("FEATURE_T", "src/a.test.ts", 1),
            site("FEATURE_T", "src/Button.stories.tsx", 1),
            site("FEATURE_T", "src/__mocks__/flags.ts", 1),
            site("FEATURE_MIXED", "src/a.test.ts", 1),
            site("FEATURE_MIXED", "src/a.ts", 1),
        ]);
        let only = row(&rows, "FEATURE_T");
        assert!(only.test_only);
        assert!(only.reasons.contains(&RetirementReason::TestOnly));
        assert!(only.sites.iter().all(|s| s.in_test));
        let mixed = row(&rows, "FEATURE_MIXED");
        assert!(!mixed.test_only);
        assert!(!mixed.reasons.contains(&RetirementReason::TestOnly));
    }

    #[test]
    fn guards_dead_code_lists_the_unused_exports() {
        let mut guarded = site("FEATURE_G", "src/a.ts", 3);
        guarded.guarded_dead_exports = vec!["legacy".to_string(), "old".to_string()];
        let rows = rows(vec![guarded, site("FEATURE_G", "src/b.ts", 9)]);
        let row = row(&rows, "FEATURE_G");
        assert_eq!(row.reasons, vec![RetirementReason::GuardsDeadCode]);
        assert_eq!(
            row.evidence[0].detail,
            "the guarded block holds unused exports: legacy, old"
        );
    }

    #[test]
    fn guard_facts_become_reasons_with_one_evidence_per_site() {
        let mut identical = site("FEATURE_I", "src/a.ts", 3);
        identical.facts = FlagSiteFacts::default().with_identical_branches(true);
        let mut empty = site("FEATURE_I", "src/b.ts", 8);
        empty.facts = FlagSiteFacts::default().with_empty_branch(true);
        let rows = rows(vec![identical, empty]);
        let row = row(&rows, "FEATURE_I");
        assert_eq!(
            row.reasons,
            vec![
                RetirementReason::IdenticalBranches,
                RetirementReason::EmptyBranch
            ]
        );
        assert_eq!(row.evidence[0].path, "src/a.ts");
        assert_eq!(row.evidence[1].path, "src/b.ts");
    }

    #[test]
    fn a_constant_row_is_a_literal_constant_with_the_value_as_evidence() {
        let definition = RetirementSiteInput {
            kind: RetirementFlagKind::Constant,
            role: FlagSiteRole::Definition,
            literal: Some("true".to_string()),
            ..site("FEATURE_C", "src/a.ts", 1)
        };
        let read = RetirementSiteInput {
            kind: RetirementFlagKind::Constant,
            ..site("FEATURE_C", "src/a.ts", 4)
        };
        let rows = rows(vec![definition, read]);
        let row = row(&rows, "FEATURE_C");
        assert_eq!(row.read_sites, 1, "the definition is not a read");
        assert_eq!(
            row.reasons,
            vec![
                RetirementReason::SingleReadSite,
                RetirementReason::LiteralConstant
            ]
        );
        let evidence = row
            .evidence
            .iter()
            .find(|e| e.reason == RetirementReason::LiteralConstant)
            .expect("evidence");
        assert_eq!(evidence.detail, "const FEATURE_C = true");
        assert_eq!(evidence.line, 1);
    }

    #[test]
    fn an_unread_definition_is_defined_never_read_only_without_reads() {
        let definition = RetirementSiteInput {
            kind: RetirementFlagKind::SdkCall,
            role: FlagSiteRole::Definition,
            unread: Some("export `x` is unused".to_string()),
            ..site("show-x", "src/flags.ts", 2)
        };
        let alone = rows(vec![definition.clone()]);
        assert_eq!(alone[0].read_sites, 0);
        assert_eq!(alone[0].reasons, vec![RetirementReason::DefinedNeverRead]);
        assert_eq!(alone[0].evidence[0].detail, "export `x` is unused");

        let read = RetirementSiteInput {
            kind: RetirementFlagKind::SdkCall,
            ..site("show-x", "src/page.ts", 9)
        };
        let with_read = rows(vec![definition, read]);
        assert!(
            !with_read[0]
                .reasons
                .contains(&RetirementReason::DefinedNeverRead)
        );
    }

    #[test]
    fn facts_attach_to_sites_by_file_line_and_column() {
        let flag = FeatureFlag {
            path: PathBuf::from("/repo/src/a.ts"),
            flag_name: "FEATURE_A".to_string(),
            kind: FlagKind::EnvironmentVariable,
            confidence: fallow_types::results::FlagConfidence::High,
            line: 3,
            col: 6,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
            guard_line_start: None,
            guard_line_end: None,
            guarded_dead_exports: Vec::new(),
        };
        let mut facts = RetirementFacts::default();
        facts.site_facts.insert(
            (PathBuf::from("/repo/src/a.ts"), 3, 6),
            FlagSiteFacts::default().with_empty_branch(true),
        );
        let sites = facts.sites_for(std::slice::from_ref(&flag));
        assert!(sites[0].facts.empty_branch());
        let other = FeatureFlag { col: 7, ..flag };
        assert!(!facts.sites_for(&[other])[0].facts.empty_branch());
    }

    fn aged(name: &str, age: Option<u64>, reads: usize) -> RetirementFlag {
        let sites = (0..reads)
            .map(|i| site(name, "src/a.ts", u32::try_from(i).unwrap_or(0) + 1))
            .collect();
        let mut row = rows(sites).remove(0);
        row.age_days = age;
        row
    }

    fn names(report: &FlagRetirementReport) -> Vec<&str> {
        report.flags.iter().map(|r| r.flag_name.as_str()).collect()
    }

    #[test]
    fn sort_by_age_puts_the_oldest_first_and_unknown_ages_last() {
        let report = finish_report(
            vec![
                aged("FEATURE_B", Some(10), 2),
                aged("FEATURE_A", None, 2),
                aged("FEATURE_C", Some(300), 2),
                aged("FEATURE_D", Some(10), 2),
            ],
            FlagAgeMode::Blame,
            None,
            &RetirementOptions::default(),
        );
        assert_eq!(
            names(&report),
            vec!["FEATURE_C", "FEATURE_B", "FEATURE_D", "FEATURE_A"]
        );
    }

    #[test]
    fn sort_by_sites_puts_the_fewest_reads_first() {
        let report = finish_report(
            vec![aged("FEATURE_A", None, 3), aged("FEATURE_B", None, 1)],
            FlagAgeMode::Off,
            None,
            &RetirementOptions {
                sort: RetirementSort::Sites,
                ..RetirementOptions::default()
            },
        );
        assert_eq!(names(&report), vec!["FEATURE_B", "FEATURE_A"]);
    }

    #[test]
    fn filters_apply_after_the_summary() {
        let report = finish_report(
            vec![
                aged("FEATURE_OLD", Some(400), 1),
                aged("FEATURE_NEW", Some(3), 1),
                aged("FEATURE_WIDE", Some(900), 2),
                aged("FEATURE_UNKNOWN", None, 1),
            ],
            FlagAgeMode::Blame,
            None,
            &RetirementOptions {
                min_age_days: Some(30),
                reasons: vec![RetirementReason::SingleReadSite],
                ..RetirementOptions::default()
            },
        );
        assert_eq!(names(&report), vec!["FEATURE_OLD"]);
        assert_eq!(report.summary.distinct_flags, 4);
        assert_eq!(report.summary.candidates, 3);
        assert_eq!(
            report
                .summary
                .by_reason
                .get(&RetirementReason::SingleReadSite),
            Some(&3)
        );
    }

    #[test]
    fn only_candidates_get_the_review_action_and_it_is_never_auto_fixable() {
        let report = finish_report(
            vec![aged("FEATURE_A", None, 1), aged("FEATURE_B", None, 2)],
            FlagAgeMode::Off,
            None,
            &RetirementOptions::default(),
        );
        let candidate = &report.flags[0];
        assert_eq!(candidate.actions.len(), 1);
        assert!(!candidate.actions[0].auto_fixable);
        assert!(report.flags[1].actions.is_empty());
    }

    #[test]
    fn top_limits_rows_after_the_sort() {
        let report = finish_report(
            vec![aged("FEATURE_A", Some(1), 1), aged("FEATURE_B", Some(2), 1)],
            FlagAgeMode::Blame,
            None,
            &RetirementOptions {
                top: Some(1),
                ..RetirementOptions::default()
            },
        );
        assert_eq!(names(&report), vec!["FEATURE_B"]);
        assert_eq!(report.summary.distinct_flags, 2);
    }
}
