/// Engine-private attribution, retained until finding suppression is applied.
#[derive(Clone, Debug)]
pub struct TailwindOccurrence {
    pub(crate) value: String,
    pub(crate) path: String,
    pub(crate) line: u32,
}

pub(super) struct TailwindScan {
    pub(super) analytics: Vec<fallow_output::TailwindArbitraryValue>,
    pub(super) occurrences: Vec<TailwindOccurrence>,
}

/// Pick one deterministic surviving site per normalized token.
pub fn surviving_representatives(
    occurrences: &[TailwindOccurrence],
    include: impl Fn(&TailwindOccurrence) -> bool,
) -> Vec<&TailwindOccurrence> {
    let mut survivors: Vec<_> = occurrences.iter().filter(|site| include(site)).collect();
    survivors.sort_by(|a, b| (&a.value, &a.path, a.line).cmp(&(&b.value, &b.path, b.line)));
    survivors.dedup_by(|a, b| a.value == b.value);
    survivors
}
