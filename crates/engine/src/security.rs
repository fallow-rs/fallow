//! Security metadata helpers owned by the engine boundary.
//!
//! The severity policy and catalogue-title lookup are shared with the core
//! analyze ranking pass via fallow-security so the two surfaces can never
//! diverge.

pub use fallow_security::{derive_security_severity, security_catalogue_title};
