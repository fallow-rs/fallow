/**
 * VS Code settings types. These shape `settings.json` entries under the
 * `fallow.*` namespace.
 */

export type { IssueTypeConfig } from "./generated/issue-types.js";

export type DuplicationMode = "strict" | "mild" | "weak" | "semantic";

export type DiagnosticSeveritySetting = "warning" | "information" | "hint";

export type TraceLevel = "off" | "messages" | "verbose";
