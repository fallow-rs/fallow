//! `fallow-api` integration tests, built as one test binary.
//!
//! Each module used to be a test binary of its own. Every binary linked the
//! whole analyzer, so the build linked it once per file. One binary links it
//! once. Add a new integration test as a module here, not as a new file in
//! `tests/`.

mod common;

mod absent_component_props;

mod audit_attribution;
mod audit_config_patterns;
mod capability_parity;
mod dead_code_baseline;
mod editor_type_aware_scope_order;
mod health_rerun_diagnostics;
mod schema_conformance;
mod scope_parity;
mod trace_class_member;
mod trace_federation_sources;
mod warm_parse_sequence;
