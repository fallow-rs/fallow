import type { AuditBrief } from "../model/adapter";

export const REVIEW_BRIEF_SCHEMA_VERSION = 6;

type JsonRecord = Record<string, unknown>;
type Guard<T> = (value: unknown) => value is T;
type OptionalField = readonly [key: string, guard: Guard<unknown>];

export type ReviewContract = AuditBrief & {
  kind: "audit-brief";
  command: "audit-brief";
  schema_version: number;
};

export interface GuideContract {
  kind: "review-walkthrough-guide";
  command: "review-walkthrough-guide";
  schema_version: number;
  graph_snapshot_hash: string;
  digest: JsonRecord & {
    decisions?: { emitted_signal_ids?: string[] };
  };
  direction: { order: string[] };
  change_anchors: Array<{
    change_anchor: string;
    file: string;
    start_line: number;
    line_count: number;
    previous_change_anchor?: string;
  }>;
  agent_schema: { judgment_shape: string };
}

export interface WalkthroughValidationContract {
  kind: "review-walkthrough-validation";
  command: "review-walkthrough-validation";
  schema_version: number;
  graph_snapshot_hash: string;
  stale: boolean;
  accepted: Array<{
    signal_id: string;
    change_anchor: string;
    anchor_kind: string;
    agent_framing: string;
    concern?: string;
    deterministic: boolean;
  }>;
  rejected: Array<{
    signal_id: string;
    change_anchor: string;
    reason: string;
  }>;
}

const isRecord = (value: unknown): value is JsonRecord =>
  typeof value === "object" && value !== null && !Array.isArray(value);
const isString = (value: unknown): value is string => typeof value === "string";
const isNumber = (value: unknown): value is number =>
  typeof value === "number" && Number.isFinite(value);
const isBoolean = (value: unknown): value is boolean => typeof value === "boolean";
const isArrayOf = <T>(value: unknown, guard: Guard<T>): value is T[] =>
  Array.isArray(value) && value.every(guard);
const isOptional = <T>(value: unknown, guard: Guard<T>): boolean =>
  value === undefined || guard(value);
const hasOptionalFields = (value: JsonRecord, fields: readonly OptionalField[]): boolean =>
  fields.every(([key, guard]) => isOptional(value[key], guard));

const hasHeader = (value: unknown, kind: string): value is JsonRecord =>
  isRecord(value) &&
  value["kind"] === kind &&
  value["command"] === kind &&
  value["schema_version"] === REVIEW_BRIEF_SCHEMA_VERSION;

const isScore = (value: unknown): value is JsonRecord =>
  isRecord(value) &&
  isOptional(value["fan_io"], isNumber) &&
  isOptional(value["security_taint"], isNumber) &&
  isOptional(value["risk_zone"], isNumber) &&
  isOptional(value["change_shape"], isNumber) &&
  isOptional(value["total"], isNumber);

const isFocusEntry = (value: unknown): value is JsonRecord =>
  isRecord(value) &&
  isString(value["file"]) &&
  isOptional(value["label"], isString) &&
  isOptional(value["reason"], isString) &&
  isOptional(value["score"], isScore);

const isFocus = (value: unknown): value is JsonRecord =>
  isRecord(value) &&
  isOptional(value["review_here"], (entries): entries is JsonRecord[] =>
    isArrayOf(entries, isFocusEntry),
  ) &&
  isOptional(value["deprioritized"], (entries): entries is JsonRecord[] =>
    isArrayOf(entries, isFocusEntry),
  );

const isUnit = (value: unknown): value is JsonRecord =>
  isRecord(value) &&
  isString(value["module_dir"]) &&
  isOptional(value["files"], (files): files is string[] => isArrayOf(files, isString));

const isPartition = (value: unknown): value is JsonRecord =>
  isRecord(value) &&
  isOptional(value["units"], (units): units is JsonRecord[] => isArrayOf(units, isUnit)) &&
  isOptional(value["order"], (order): order is string[] => isArrayOf(order, isString));

const isTriage = (value: unknown): value is JsonRecord =>
  isRecord(value) &&
  isOptional(value["files"], isNumber) &&
  isOptional(value["risk_class"], isString) &&
  isOptional(value["review_effort"], isString);

const isSummary = (value: unknown): value is JsonRecord =>
  isRecord(value) &&
  isOptional(value["dead_code_issues"], isNumber) &&
  isOptional(value["duplication_clone_groups"], isNumber) &&
  isOptional(value["complexity_findings"], isNumber);

const isDecisions = (value: unknown): value is JsonRecord =>
  isRecord(value) &&
  isOptional(value["decisions"], (decisions): decisions is JsonRecord[] =>
    isArrayOf(decisions, isRecord),
  ) &&
  isOptional(value["emitted_signal_ids"], (ids): ids is string[] => isArrayOf(ids, isString));

const isImpactClosure = (value: unknown): value is JsonRecord =>
  isRecord(value) &&
  isOptional(value["coordination_gap"], (gaps): gaps is JsonRecord[] => isArrayOf(gaps, isRecord));

const reviewFields: readonly OptionalField[] = [
  ["verdict", isString],
  ["changed_files_count", isNumber],
  ["base_ref", isString],
  ["base_description", isString],
  ["triage", isTriage],
  ["summary", isSummary],
  ["decisions", isDecisions],
  ["partition", isPartition],
  ["focus", isFocus],
  ["impact_closure", isImpactClosure],
  ["weakening", (entries): entries is JsonRecord[] => isArrayOf(entries, isRecord)],
  ["graph_snapshot_hash", isString],
];

const isReviewContract = (value: unknown): value is ReviewContract =>
  hasHeader(value, "audit-brief") && hasOptionalFields(value, reviewFields);

const isGuideDigest = (value: unknown): value is GuideContract["digest"] =>
  isRecord(value) && isOptional(value["decisions"], isDecisions);

const isDirection = (value: unknown): value is GuideContract["direction"] =>
  isRecord(value) && isArrayOf(value["order"], isString);

const isChangeAnchor = (value: unknown): value is GuideContract["change_anchors"][number] =>
  isRecord(value) &&
  isString(value["change_anchor"]) &&
  isString(value["file"]) &&
  isNumber(value["start_line"]) &&
  isNumber(value["line_count"]) &&
  isOptional(value["previous_change_anchor"], isString);

const isAgentSchema = (value: unknown): value is GuideContract["agent_schema"] =>
  isRecord(value) && isString(value["judgment_shape"]);

const isGuideContract = (value: unknown): value is GuideContract =>
  hasHeader(value, "review-walkthrough-guide") &&
  isString(value["graph_snapshot_hash"]) &&
  isGuideDigest(value["digest"]) &&
  isDirection(value["direction"]) &&
  isArrayOf(value["change_anchors"], isChangeAnchor) &&
  isAgentSchema(value["agent_schema"]);

const isAcceptedJudgment = (
  value: unknown,
): value is WalkthroughValidationContract["accepted"][number] =>
  isRecord(value) &&
  isString(value["signal_id"]) &&
  isString(value["change_anchor"]) &&
  isString(value["anchor_kind"]) &&
  isString(value["agent_framing"]) &&
  isOptional(value["concern"], isString) &&
  isBoolean(value["deterministic"]);

const isRejectedJudgment = (
  value: unknown,
): value is WalkthroughValidationContract["rejected"][number] =>
  isRecord(value) &&
  isString(value["signal_id"]) &&
  isString(value["change_anchor"]) &&
  isString(value["reason"]);

const isWalkthroughValidationContract = (value: unknown): value is WalkthroughValidationContract =>
  hasHeader(value, "review-walkthrough-validation") &&
  isString(value["graph_snapshot_hash"]) &&
  isBoolean(value["stale"]) &&
  isArrayOf(value["accepted"], isAcceptedJudgment) &&
  isArrayOf(value["rejected"], isRejectedJudgment);

const parseJson = (stdout: string): unknown => {
  try {
    return JSON.parse(stdout);
  } catch {
    throw new Error("fallow returned output that couldn't be read as JSON.");
  }
};

const parseContract = <T>(stdout: string, command: string, kind: string, guard: Guard<T>): T => {
  const value = parseJson(stdout);
  if (guard(value)) return value;
  throw new Error(
    `${command} returned incompatible JSON; expected ${kind} schema version ${REVIEW_BRIEF_SCHEMA_VERSION}.`,
  );
};

/** Parses and validates the `fallow review` audit-brief boundary. */
export const parseReviewContract = (stdout: string): ReviewContract =>
  parseContract(stdout, "fallow review", "audit-brief", isReviewContract);

/** Parses and validates the walkthrough-guide boundary. */
export const parseGuideContract = (stdout: string): GuideContract =>
  parseContract(
    stdout,
    "fallow review --walkthrough-guide",
    "review-walkthrough-guide",
    isGuideContract,
  );

/** Parses and validates the walkthrough-file response boundary. */
export const parseWalkthroughValidationContract = (stdout: string): WalkthroughValidationContract =>
  parseContract(
    stdout,
    "fallow review --walkthrough-file",
    "review-walkthrough-validation",
    isWalkthroughValidationContract,
  );
