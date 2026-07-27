// Generated from crates/api/type-aware-protocol.json. Do not edit.
export const TYPE_AWARE_PROTOCOL = Object.freeze({
  schema_version: 1,
  wire_protocol_version: 6,
  semantic_schema_version: 2,
  analysis_operation: "semantic-queries",
  status_operation: "status",
  query_operations: ["symbol-use", "symbol-trace", "api-surface", "symbol-impact", "type-coupling"],
  session_envelope_types: ["analyze", "shutdown"],
  backend: {
    family: "typescript-go",
    version: "7.0.2",
  },
  sidecar: {
    package: "fallow-type-aware",
    version_source: "workspace-package",
  },
});
export const WIRE_PROTOCOL_VERSION = TYPE_AWARE_PROTOCOL.wire_protocol_version;
export const SEMANTIC_SCHEMA_VERSION = TYPE_AWARE_PROTOCOL.semantic_schema_version;
export const ANALYSIS_OPERATION = TYPE_AWARE_PROTOCOL.analysis_operation;
export const STATUS_OPERATION = TYPE_AWARE_PROTOCOL.status_operation;
export const QUERY_OPERATIONS = Object.freeze(TYPE_AWARE_PROTOCOL.query_operations);
export const SESSION_ENVELOPE_TYPES = Object.freeze(TYPE_AWARE_PROTOCOL.session_envelope_types);
export const BACKEND_FAMILY = TYPE_AWARE_PROTOCOL.backend.family;
export const BACKEND_VERSION = TYPE_AWARE_PROTOCOL.backend.version;
export const SIDECAR_PACKAGE = TYPE_AWARE_PROTOCOL.sidecar.package;
