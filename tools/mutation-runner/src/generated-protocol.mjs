// Generated from crates/api/mutation-runner-protocol.json. Do not edit.
const deepFreeze = (value) => {
  if (value !== null && typeof value === "object") {
    for (const entry of Object.values(value)) deepFreeze(entry);
    Object.freeze(value);
  }
  return value;
};

export const MUTATION_RUNNER_PROTOCOL = deepFreeze({
  schema_version: 1,
  wire_protocol_version: 1,
  runtime_semantics_version: 1,
  session_token_env: "FALLOW_MUTATION_SESSION_TOKEN",
  request_types: ["init", "dry_run", "run", "reload", "shutdown"],
  response_types: ["ready", "dry_run_result", "run_result", "reloaded", "error"],
  test_statuses: ["pass", "fail", "skip", "todo"],
  run_statuses: ["killed", "survived", "timeout", "runtime_error"],
  failure_kinds: ["assertion", "error", "timeout"],
  reload_methods: ["invalidate", "noop", "restart_required"],
  error_codes: [
    "protocol-version",
    "bad-request",
    "runner-not-found",
    "runner-version",
    "runner-load-failed",
    "config-load-failed",
    "test-file-missing",
    "internal",
  ],
  capabilities: [
    "per_test_attribution",
    "test_filter",
    "hot_switch",
    "bail",
    "fresh_modules_per_run",
    "batch_mutants",
  ],
  isolation_modes: ["process", "none"],
  in_band_markers: ["fallow-filtered", "fallow-bailed"],
  ready_notes: ["typecheck-not-run"],
  runtime: {
    global_name: "__fallowMutation",
    active_function: "__fallow_m_a",
    hit_function: "__fallow_m_c",
  },
  runners: [
    {
      name: "vitest",
      package: "vitest",
      min_version: "4.1.0",
      max_version_exclusive: "6.0.0",
    },
    {
      name: "node-test",
      package: "node",
      min_version: "22.18.0",
      max_version_exclusive: "27.0.0",
    },
    {
      name: "bun-test",
      package: "bun",
      min_version: "1.4.0",
      max_version_exclusive: "2.0.0",
    },
  ],
});
export const WIRE_PROTOCOL_VERSION = MUTATION_RUNNER_PROTOCOL.wire_protocol_version;
export const SESSION_TOKEN_ENV = MUTATION_RUNNER_PROTOCOL.session_token_env;
export const REQUEST_TYPES = MUTATION_RUNNER_PROTOCOL.request_types;
export const ERROR_CODES = MUTATION_RUNNER_PROTOCOL.error_codes;
export const GLOBAL_NAME = MUTATION_RUNNER_PROTOCOL.runtime.global_name;
export const RUNNERS = MUTATION_RUNNER_PROTOCOL.runners;
