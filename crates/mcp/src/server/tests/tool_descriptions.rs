use std::collections::BTreeMap;

use rmcp::ServerHandler;

use super::super::FallowMcp;

const DESCRIPTION_FIXTURE: &str = include_str!("fixtures/tool-descriptions.json");
const SERVER_SOURCE: &str = include_str!("../mod.rs");

/// Live `tools/list` descriptions, per tool. This is one of the two channels
/// `tools/list` carries; the input schemas are the other, and
/// [`live_tool_schema_bytes`] budgets them.
fn live_tool_descriptions() -> BTreeMap<String, String> {
    let server = FallowMcp::new();
    server
        .tool_router
        .list_all()
        .iter()
        .map(|tool| {
            (
                tool.name.to_string(),
                tool.description.as_deref().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn descriptions_match(fixture: &str, live: &BTreeMap<String, String>) -> bool {
    serde_json::from_str::<BTreeMap<String, String>>(fixture)
        .is_ok_and(|expected| expected.eq(live))
}

#[test]
fn live_tool_descriptions_match_the_checked_fixture() {
    let live = live_tool_descriptions();
    assert!(
        descriptions_match(DESCRIPTION_FIXTURE, &live),
        "live MCP tool descriptions changed; update the checked fixture only for an intentional wire-contract change"
    );
}

#[test]
fn analyze_description_covers_supported_dependency_override_sources() {
    let live = live_tool_descriptions();
    let analyze = live.get("analyze").expect("analyze description");

    assert!(analyze.contains("top-level `package.json#overrides`"));
    assert!(analyze.contains("Bun `package.json#resolutions`"));
    assert!(!analyze.contains("unused pnpm dependency overrides"));
    assert!(!analyze.contains("misconfigured pnpm dependency overrides"));
}

#[test]
fn description_contract_detects_punctuation_and_whitespace_drift() {
    let live = BTreeMap::from([("example".to_owned(), "alpha beta".to_owned())]);

    assert!(!descriptions_match(r#"{"example":"alpha beta."}"#, &live));
    assert!(!descriptions_match(r#"{"example":"alpha  beta"}"#, &live));
}

#[test]
fn tool_attributes_take_descriptions_from_method_docs() {
    assert!(
        !SERVER_SOURCE.contains("description ="),
        "tool descriptions must live in method docs, not #[tool] arguments"
    );
}

/// Per-tool wire-description ceiling. Every `tools/list` byte is resident in
/// every agent session that connects, whether or not the tool is ever called,
/// so a description that grows without a budget is a permanent tax. Per-flag
/// payload shapes, unit vocabularies, and suppression placements belong in the
/// `fallow://tools/{name}` guide resource, which an agent reads once and only
/// for the tool it is about to call.
const MAX_TOOL_DESCRIPTION_BYTES: usize = 2_000;

/// Total wire-description bytes across every registered tool, as printed by
/// this gate the last time it was re-pinned. The ratchet's high-water mark,
/// not the assertion.
///
/// RE-PIN IT BY RUNNING THE GATE, never by adding up a change's deltas by
/// hand. `cargo test -p fallow-mcp total_tool_description_bytes_stay_within_budget`
/// names the live total in its failure message, and that number is the only
/// correct value for this constant. A hand-summed mark drifts off the live
/// total silently, and every check below that compares the two then measures
/// nothing: a mark BELOW live makes the re-pin check unfirable, and a mark
/// above live hands the next description free budget.
///
/// The target is 35_000, reached by moving one tool's per-flag prose into its
/// `fallow://tools/{name}` guide at a time.
const RECORDED_TOTAL_DESCRIPTION_BYTES: usize = 56_164;

/// Deliberate headroom over [`RECORDED_TOTAL_DESCRIPTION_BYTES`].
///
/// Pinned to the exact live total, the gate failed on a one-word wording fix,
/// which reads as a break rather than as a budget and teaches the next
/// maintainer to raise the number reflexively. A kilobyte absorbs ordinary
/// rewording (a clarified sentence, a corrected flag name) while still
/// catching what the budget exists for: prose that grows by a paragraph.
///
/// It is spendable, and nothing reclaims it on its own. The re-pin check below
/// fires only when the live total drops [`DESCRIPTION_REPIN_BYTES`] below the
/// recorded mark, so growth that stays inside this kilobyte is permanent until
/// the mark is re-pinned by hand. Re-pin it in the same change that spends part
/// of it, or the next author inherits headroom that is already gone.
const TOTAL_DESCRIPTION_SLACK_BYTES: usize = 1_024;

/// The smallest headroom that still lets a maintainer fix a word without the
/// total budget going red. One sentence rewritten is worth a couple of hundred
/// bytes; anything under that and the gate is a tripwire, not a budget.
///
/// This bounds the SLACK CONSTANT, which is the only thing it can bound: with
/// a single total ceiling, headroom after a correct re-pin is
/// [`TOTAL_DESCRIPTION_SLACK_BYTES`] by construction, so asserting on the
/// live headroom at run time would only restate the ceiling that already
/// fired. The compile-time check below is the assertion that can actually
/// fail, and it fails on the change that would cause the harm: shrinking the
/// slack to a tripwire.
const MIN_USABLE_TOTAL_HEADROOM_BYTES: usize = 256;

const _: () = assert!(
    TOTAL_DESCRIPTION_SLACK_BYTES >= MIN_USABLE_TOTAL_HEADROOM_BYTES,
    "the total description slack must stay large enough to reword a sentence"
);

/// Total wire-description ceiling across every registered tool, and the only
/// ceiling on that total: nothing else asserts against it, so this is the
/// check a growing description trips. A NEW capability is the one thing that
/// may raise the recorded mark by a whole description, and only by its own
/// routing summary: the new tool's description carries no per-flag detail
/// (that goes straight into its guide). Re-pinning after a change that spent
/// slack is the other, smaller reason the mark moves up.
const MAX_TOTAL_DESCRIPTION_BYTES: usize =
    RECORDED_TOTAL_DESCRIPTION_BYTES + TOTAL_DESCRIPTION_SLACK_BYTES;

/// How much unused headroom an exception may carry before the test asks for
/// the allowance to be lowered. Without this the list would keep stale numbers
/// and stop being a ratchet.
const MAX_EXCEPTION_SLACK_BYTES: usize = 128;

/// How far the live description total may sit below its recorded mark before
/// the gate asks for a re-pin.
///
/// This is what keeps a budget a ratchet instead of a number that drifts: a
/// real reduction (one tool's prose moved into its guide) has to be banked, or
/// the bytes it freed become silent budget for the next description.
///
/// It is a literal, not a multiple of [`MAX_EXCEPTION_SLACK_BYTES`]. The
/// per-tool ratchet governs one row of the exception table, whose rows hold
/// under 200 bytes of reclaimable slack in total, so "four tools' worth of
/// per-tool reclaim" was never a quantity this total could be harvested by.
/// The quantity that matters here is a wording pass: a reworded sentence is
/// worth a couple of hundred bytes at most and should not demand a re-pin,
/// while moving a tool's per-flag prose into its guide frees thousands and
/// must be banked in the change that freed them.
const DESCRIPTION_REPIN_BYTES: usize = 256;

/// The same ratchet for the schema total, sized independently of
/// [`DESCRIPTION_REPIN_BYTES`] for the same reason
/// [`TOTAL_SCHEMA_SLACK_BYTES`] is sized independently of the description
/// slack: one shared parameter's doc comment renders into nearly every tool's
/// schema, so a single reworded sentence moves this total by tens of tools'
/// worth of bytes. A threshold tuned for one description would fire on every
/// shared-parameter edit and teach the next maintainer to re-pin reflexively.
const SCHEMA_REPIN_BYTES: usize = 1_024;

/// Tools allowed past [`MAX_TOOL_DESCRIPTION_BYTES`], each with the allowance
/// it may spend and the reason it earns one. Two kinds of entry live here.
///
/// Permanent: `code_execute` and `fix_apply`. For those two the prose IS the
/// safety boundary an agent reads before it acts, so holding them to the
/// uniform [`MAX_TOOL_DESCRIPTION_BYTES`] ceiling is not defensible.
/// `code_execute` states the sandbox
/// contract, the host-call allowlist, and the output and timeout bounds
/// before an agent runs JavaScript in this process; `fix_apply` states the
/// dry-run-first mutation contract, and it is the only tool that writes to
/// the project. `fix_apply`'s allowance last moved for a write fallow now
/// declines: a finding whose verdict rests on a source file the run did not
/// fully analyze. That is the one kind of growth this row exists to allow,
/// and it is not per-flag prose;
/// an agent that does not know a removal was withheld reads the run as a
/// clean no-op.
///
/// Temporary: every other row. Those descriptions still carry per-flag detail
/// that belongs in a `fallow://tools/{name}` guide, and a row disappears when
/// its description fits the uniform ceiling without one. `check_health` shows
/// what that costs rather than what it finished: it has had one such split
/// already and is STILL the longest description on the wire holding the
/// largest allowance in this table, so a split is a step to be repeated, not
/// a fix a row has already received.
const DESCRIPTION_BUDGET_EXCEPTIONS: &[(&str, usize)] = &[
    ("code_execute", 4_600),
    ("fix_apply", 4_200),
    ("check_health", 5_500),
    ("audit", 5_150),
    ("fix_preview", 2_750),
    ("analyze", 2_550),
    ("check_runtime_coverage", 2_350),
    ("impact", 2_250),
    ("security_candidates", 2_250),
    ("impact_all", 2_100),
];

fn description_allowance(tool: &str) -> usize {
    DESCRIPTION_BUDGET_EXCEPTIONS
        .iter()
        .find(|(name, _)| *name == tool)
        .map_or(MAX_TOOL_DESCRIPTION_BYTES, |(_, allowance)| *allowance)
}

#[test]
fn tool_descriptions_stay_within_their_byte_budget() {
    for (tool, description) in live_tool_descriptions() {
        let allowance = description_allowance(&tool);
        assert!(
            description.len() <= allowance,
            "{tool} wire description is {} bytes, over its {allowance}-byte budget; \
             move the per-flag detail into its fallow://tools/{tool} guide rather than \
             raising the number",
            description.len()
        );
    }
}

fn total_description_bytes() -> usize {
    live_tool_descriptions()
        .values()
        .map(std::string::String::len)
        .sum()
}

#[test]
fn total_tool_description_bytes_stay_within_budget() {
    let total = total_description_bytes();
    assert!(
        total <= MAX_TOTAL_DESCRIPTION_BYTES,
        "tools/list carries {total} description bytes, over the \
         {MAX_TOTAL_DESCRIPTION_BYTES}-byte budget every agent session pays on connect \
         ({RECORDED_TOTAL_DESCRIPTION_BYTES} recorded plus {TOTAL_DESCRIPTION_SLACK_BYTES} \
         slack); move per-flag detail into the tool's fallow://tools/{{name}} guide"
    );
}

#[test]
fn total_tool_description_budget_keeps_no_stale_headroom() {
    let total = total_description_bytes();
    assert!(
        RECORDED_TOTAL_DESCRIPTION_BYTES.saturating_sub(total) <= DESCRIPTION_REPIN_BYTES,
        "tools/list is down to {total} description bytes but the ratchet still records \
         {RECORDED_TOTAL_DESCRIPTION_BYTES}; bank the win by setting \
         RECORDED_TOTAL_DESCRIPTION_BYTES to {total}, so the freed bytes are not \
         spendable by the next description"
    );
}

#[test]
fn budget_exceptions_keep_no_stale_headroom() {
    let live = live_tool_descriptions();
    for (tool, allowance) in DESCRIPTION_BUDGET_EXCEPTIONS {
        let description = live
            .get(*tool)
            .unwrap_or_else(|| panic!("budget exception {tool} is not a registered tool"));
        assert!(
            allowance.saturating_sub(description.len()) <= MAX_EXCEPTION_SLACK_BYTES,
            "{tool} is {} bytes but its exception allows {allowance}; lower the allowance, \
             or drop the row when the description fits the {MAX_TOOL_DESCRIPTION_BYTES}-byte ceiling",
            description.len()
        );
    }
}

/// Serialized `tools/list` input-schema bytes, per tool.
///
/// The description budget above covers `tool.description` and nothing else,
/// which left the larger half of the payload ungoverned: a parameter with a
/// 500-byte doc comment cost 500 wire bytes and zero budget bytes, because
/// schemars renders a doc comment into the schema's `description`. Every
/// `tools/list` byte is resident in every agent session that connects, whether
/// or not the tool is ever called, so both channels are budgeted the same way.
fn live_tool_schema_bytes() -> BTreeMap<String, usize> {
    let server = FallowMcp::new();
    server
        .tool_router
        .list_all()
        .iter()
        .map(|tool| {
            (
                tool.name.to_string(),
                serde_json::to_string(&tool.input_schema)
                    .expect("input schema serializes")
                    .len(),
            )
        })
        .collect()
}

fn total_schema_bytes() -> usize {
    live_tool_schema_bytes().values().sum()
}

/// Total input-schema bytes across every registered tool, as printed by this
/// gate the last time it was re-pinned. The ratchet's high-water mark, not the
/// assertion.
///
/// Re-pin it the same way as [`RECORDED_TOTAL_DESCRIPTION_BYTES`]: run
/// `cargo test -p fallow-mcp total_tool_schema_bytes_stay_within_budget` and
/// copy the live total out of the failure message. Never sum a change's
/// deltas by hand; one edited parameter doc comment renders into every tool
/// that takes that parameter, so the arithmetic is wrong long before it looks
/// wrong.
const RECORDED_TOTAL_SCHEMA_BYTES: usize = 83_660;

/// Deliberate headroom over [`RECORDED_TOTAL_SCHEMA_BYTES`], for the same
/// reason [`TOTAL_DESCRIPTION_SLACK_BYTES`] exists: pinned to the exact live
/// total, a clarified parameter sentence reads as a break rather than as a
/// budget. Schemas are shared across tools (one `workspace` sentence lands on
/// nearly every one of them), so a reworded shared parameter moves this total
/// by far more than one reworded description moves that one; the slack is
/// sized for a shared-parameter edit, not a single-tool one.
const TOTAL_SCHEMA_SLACK_BYTES: usize = 2_048;

/// Total input-schema ceiling across every registered tool.
const MAX_TOTAL_SCHEMA_BYTES: usize = RECORDED_TOTAL_SCHEMA_BYTES + TOTAL_SCHEMA_SLACK_BYTES;

#[test]
fn total_tool_schema_bytes_stay_within_budget() {
    let total = total_schema_bytes();
    assert!(
        total <= MAX_TOTAL_SCHEMA_BYTES,
        "tools/list carries {total} input-schema bytes, over the \
         {MAX_TOTAL_SCHEMA_BYTES}-byte budget every agent session pays on connect \
         ({RECORDED_TOTAL_SCHEMA_BYTES} recorded plus {TOTAL_SCHEMA_SLACK_BYTES} slack); \
         a parameter doc comment is wire text, so shorten it or drop the parameter"
    );
}

#[test]
fn total_tool_schema_budget_keeps_no_stale_headroom() {
    let total = total_schema_bytes();
    assert!(
        RECORDED_TOTAL_SCHEMA_BYTES.saturating_sub(total) <= SCHEMA_REPIN_BYTES,
        "tools/list is down to {total} input-schema bytes but the ratchet still records \
         {RECORDED_TOTAL_SCHEMA_BYTES}; bank the win by setting RECORDED_TOTAL_SCHEMA_BYTES \
         to {total}, so the freed bytes are not spendable by the next parameter"
    );
}

/// Server `instructions` bytes, as printed by this gate the last time it was
/// re-pinned.
///
/// Budgeted for the same reason the two totals above are: `instructions` is
/// resident in every session that connects, whether or not any tool is called,
/// and it is the one surface with no per-item ceiling to hold it back. It
/// earns its size by reaching every tool at one session-level cost, which is
/// exactly why an unbudgeted one grows: each sentence is cheaper here than in
/// a description, and the bill still arrives once per session. Re-pin it by
/// running `cargo test -p fallow-mcp server_instructions_stay_within_budget`
/// and copying the live total out of the failure message.
const RECORDED_INSTRUCTION_BYTES: usize = 1_165;

/// Deliberate headroom over [`RECORDED_INSTRUCTION_BYTES`], sized like
/// [`TOTAL_DESCRIPTION_SLACK_BYTES`]: enough to reword a routing sentence,
/// not enough to add a paragraph.
const INSTRUCTION_SLACK_BYTES: usize = 512;

const MAX_INSTRUCTION_BYTES: usize = RECORDED_INSTRUCTION_BYTES + INSTRUCTION_SLACK_BYTES;

fn instruction_bytes() -> usize {
    let server = FallowMcp::new();
    ServerHandler::get_info(&server)
        .instructions
        .as_deref()
        .map_or(0, str::len)
}

#[test]
fn server_instructions_stay_within_budget() {
    let total = instruction_bytes();
    assert!(
        total <= MAX_INSTRUCTION_BYTES,
        "server instructions carry {total} bytes, over the {MAX_INSTRUCTION_BYTES}-byte \
         budget every agent session pays on connect ({RECORDED_INSTRUCTION_BYTES} recorded \
         plus {INSTRUCTION_SLACK_BYTES} slack); route the detail to a tool guide or a \
         fallow:// resource instead"
    );
}

#[test]
fn server_instruction_budget_keeps_no_stale_headroom() {
    let total = instruction_bytes();
    assert!(
        RECORDED_INSTRUCTION_BYTES.saturating_sub(total) <= DESCRIPTION_REPIN_BYTES,
        "server instructions are down to {total} bytes but the ratchet still records \
         {RECORDED_INSTRUCTION_BYTES}; bank the win by setting RECORDED_INSTRUCTION_BYTES \
         to {total}, so the freed bytes are not spendable by the next sentence"
    );
}

/// The catalogue resource is the terse channel and the wire description is the
/// long one. `crates/mcp/src/tool_guides.rs` says a drift test holds that
/// ordering; this is that test.
#[test]
fn catalogue_lines_stay_shorter_than_the_wire_description() {
    let live = live_tool_descriptions();
    for tool in fallow_types::mcp_manifest::MCP_TOOLS {
        let wire = live
            .get(tool.name)
            .unwrap_or_else(|| panic!("{} is in the manifest but not registered", tool.name));
        assert!(
            tool.description.len() < wire.len(),
            "{}: the fallow://tools catalogue line is {} bytes and the tools/list description \
             is {}; the catalogue is the terse channel, so long prose belongs in the wire \
             description or in the tool's fallow://tools/{{name}} guide",
            tool.name,
            tool.description.len(),
            wire.len()
        );
    }
}

/// How many registered tools carry the subprocess byte cap, counted off the
/// live `tools/list` schemas rather than off the parameter structs in
/// `crates/mcp/src/params.rs`. Those two numbers differ: one struct is shared
/// by several tools, so counting struct definitions undercounts the wire. The
/// compatibility entry documents the wire, so this is the number it must
/// spell.
fn tools_carrying_the_subprocess_output_cap() -> Vec<String> {
    let server = FallowMcp::new();
    server
        .tool_router
        .list_all()
        .iter()
        .filter(|tool| {
            serde_json::to_value(&tool.input_schema)
                .ok()
                .and_then(|schema| {
                    schema
                        .pointer("/properties/max_output_bytes/description")
                        .and_then(|description| description.as_str())
                        .map(|description| description.starts_with("Byte cap for this call"))
                })
                .unwrap_or(false)
        })
        .map(|tool| tool.name.to_string())
        .collect()
}

/// The compatibility entry for `max_output_bytes` states how many tools take
/// it. It first shipped saying fifteen, the number of parameter structs in
/// `params.rs`, while the wire carried nineteen tools: several tools flatten
/// one struct. A reader sizing a migration against that entry counted the
/// wrong surface, so the doc's number is pinned to the live schemas here.
#[test]
fn the_compatibility_entry_counts_the_tools_that_carry_the_output_cap() {
    let carriers = tools_carrying_the_subprocess_output_cap();
    assert!(
        !carriers.is_empty(),
        "no registered tool carries the subprocess max_output_bytes description"
    );

    let doc = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/backwards-compatibility.md"),
    )
    .expect("docs/backwards-compatibility.md is readable");

    let spelled = [
        ("thirteen", 13),
        ("fourteen", 14),
        ("fifteen", 15),
        ("sixteen", 16),
        ("seventeen", 17),
        ("eighteen", 18),
        ("nineteen", 19),
        ("twenty", 20),
        ("twenty-one", 21),
        ("twenty-two", 22),
    ]
    .into_iter()
    .find_map(|(word, count)| {
        doc.contains(&format!(
            "on the {word} subprocess-backed tools that take it"
        ))
        .then_some((word, count))
    });

    let (word, documented) = spelled.expect(
        "docs/backwards-compatibility.md must say \"on the <count> subprocess-backed tools \
         that take it\" in the max_output_bytes entry",
    );
    assert_eq!(
        documented,
        carriers.len(),
        "the compatibility entry says {word} tools take max_output_bytes but {} do: {}",
        carriers.len(),
        carriers.join(", ")
    );
}
