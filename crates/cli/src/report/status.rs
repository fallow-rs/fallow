use fallow_types::envelope::TypeAwareMeta;
use fallow_types::semantic::SemanticCompleteness;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanStatus {
    Ok,
    Warning,
    Inactive,
}

impl HumanStatus {
    const fn prefix(self) -> &'static str {
        match self {
            Self::Ok => "[OK]",
            Self::Warning => "[W]",
            Self::Inactive => "[-]",
        }
    }
}

pub fn line(status: HumanStatus, message: impl std::fmt::Display) -> String {
    format!("{} {message}", status.prefix())
}

pub fn type_aware_meta_status(meta: &TypeAwareMeta) -> HumanStatus {
    let identity_status = meta.identity.as_ref().map(|identity| identity.completeness);
    if identity_status == Some(SemanticCompleteness::Unavailable)
        || meta
            .queries
            .iter()
            .any(|query| query.status == SemanticCompleteness::Unavailable)
    {
        return HumanStatus::Inactive;
    }
    if identity_status == Some(SemanticCompleteness::Partial)
        || meta
            .queries
            .iter()
            .any(|query| query.status == SemanticCompleteness::Partial)
        || meta.abstained_count > 0
    {
        return HumanStatus::Warning;
    }
    HumanStatus::Ok
}

pub const fn semantic_status(status: SemanticCompleteness) -> HumanStatus {
    match status {
        SemanticCompleteness::Complete => HumanStatus::Ok,
        SemanticCompleteness::Partial => HumanStatus::Warning,
        SemanticCompleteness::Unavailable => HumanStatus::Inactive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_prefixes_are_stable_and_scannable() {
        assert_eq!(line(HumanStatus::Ok, "ready"), "[OK] ready");
        assert_eq!(line(HumanStatus::Warning, "partial"), "[W] partial");
        assert_eq!(
            line(HumanStatus::Inactive, "unavailable"),
            "[-] unavailable"
        );
    }
}
