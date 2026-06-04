import type { SecurityFinding, SecurityOutput, TraceHopRole } from "./types.js";

/**
 * Options for building the `fallow security` argument vector. These mirror the
 * subset of CLI globals that `fallow security` accepts; `--production` and the
 * `--dupes-*` flags are deliberately NOT included because the CLI rejects them
 * with exit 2 under the `security` subcommand (see `unsupported_security_global`
 * in `crates/cli/src/main.rs`). This divergence from `buildAnalysisArgs` is the
 * reason this builder exists separately.
 */
export interface SecurityArgsOptions {
  readonly configPath: string;
  readonly changedSince: string;
}

/**
 * Build the argument vector for the standalone `fallow security --format json`
 * run that backs the Security Candidates view. Kept pure (no VS Code / config
 * access) so the flag-forwarding rules can be unit-tested. Never emits
 * `--production` or any `--dupes-*` flag, which `fallow security` rejects.
 */
export const buildSecurityArgs = (options: SecurityArgsOptions): string[] => {
  const args = ["security", "--format", "json", "--quiet"];

  if (options.changedSince) {
    args.push("--changed-since", options.changedSince);
  }

  if (options.configPath) {
    args.push("--config", options.configPath);
  }

  return args;
};

/** Count the security candidates in a result; `null` (nothing to show) is 0. */
export const countSecurityFindings = (result: SecurityOutput | null): number =>
  result ? result.security_findings.length : 0;

/**
 * Human-facing label for a finding. Mirrors `security_finding_label` in
 * `crates/cli/src/security.rs`: `client-server-leak` keeps its bespoke kind;
 * `tainted-sink` renders `"<category> (CWE-<n>)"` when both are present, else
 * the category, else `"tainted-sink"`. The extension cannot call the Rust
 * catalogue-title lookup, so it renders the raw `category` id (honest, no
 * fabrication).
 */
export const securityFindingLabel = (finding: SecurityFinding): string => {
  if (finding.kind === "client-server-leak") {
    return "client-server-leak";
  }

  const category = finding.category ?? null;
  const title = category ?? "tainted-sink";

  if (finding.cwe !== undefined && finding.cwe !== null) {
    return `${title} (CWE-${finding.cwe})`;
  }
  return title;
};

/**
 * Human-facing label for a trace hop role. Mirrors `hop_role_label` in
 * `crates/cli/src/security.rs`. Exhaustive over every `TraceHopRole` value so a
 * new wire role fails the type-check here (parity with the Rust
 * `hop_role_labels_cover_every_role` test).
 */
export const hopRoleLabel = (role: TraceHopRole): string => {
  switch (role) {
    case "client-boundary":
      return "client boundary";
    case "intermediate":
      return "intermediate";
    case "secret-source":
      return "secret source";
    case "sink":
      return "sink site";
  }
};

/**
 * Detect a clap "unrecognized subcommand" error for `security`, raised when the
 * resolved CLI predates the `fallow security` command. Lets the caller degrade
 * to a one-line "update fallow" warning instead of surfacing a raw stderr blob.
 * Handles modern clap (`unrecognized subcommand 'security'`) and the legacy
 * phrasing (`The subcommand 'security' wasn't recognized`). Unrelated errors
 * return false so genuine failures stay loud.
 */
export const parseUnknownSubcommand = (message: string): boolean => {
  if (/unrecognized subcommand '?security'?/i.test(message)) {
    return true;
  }
  if (/subcommand '?security'? (?:wasn't|was not) recognized/i.test(message)) {
    return true;
  }
  return false;
};
