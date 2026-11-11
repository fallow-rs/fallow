//! Conversions from CLI argument enums to their config and engine types.

use super::{BaselineModeArg, TypeAwareRequireArg};

impl From<BaselineModeArg> for fallow_engine::baseline::HealthBaselineMode {
    fn from(value: BaselineModeArg) -> Self {
        match value {
            BaselineModeArg::Count => Self::Count,
            BaselineModeArg::Identity => Self::Identity,
        }
    }
}

impl From<TypeAwareRequireArg> for fallow_config::TypeAwareRequire {
    fn from(value: TypeAwareRequireArg) -> Self {
        match value {
            TypeAwareRequireArg::BestEffort => Self::BestEffort,
            TypeAwareRequireArg::Complete => Self::Complete,
        }
    }
}
