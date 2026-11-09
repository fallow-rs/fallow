//! Explain the optional inventory separately from finding severity metadata.
pub fn explain() -> fallow_output::ExplainOutput {
    fallow_output::ExplainOutput {
        id: "fallow/shared-state".to_owned(),
        name: "Shared-state inventory".to_owned(),
        summary: "Direct imported property writes and custom global access sites".to_owned(),
        rationale: "An imported-property write identifies one runtime ESM binding declaration. A global group retains reads and writes in different non-test files under the same exact root and direct slot. Neither observation proves a runtime dependency, shared runtime or unsafe execution order. Observations do not affect finding totals, severity, health, gates or automatic fixes.".to_owned(),
        example: "import {state} from './owner'; state.value = 1; // imported property write\nwindow.custom = 1; // grouped with window.custom reads in another source file".to_owned(),
        how_to_fix: "Review ownership and initialization manually. Use fallow-ignore-next-line shared-state for intentional sites, or sharedStateAllowlist entries such as [\"window\", \"custom\"]. Aliases, dynamic keys, type-only imports, ambiguous origins and mutator calls are outside this inventory.".to_owned(),
        docs: "https://fallow.tools/docs/cli/architecture".to_owned(),
    }
}
