//! `unused-component-input` / `unused-component-output`: an Angular `@Input()` /
//! signal `input()` / `model()` read nowhere, or an `@Output()` / signal
//! `output()` emitted nowhere, inside its own component. Covers the full abstain
//! ladder from the design review: inline-template credit, `this.foo` script
//! credit, external `templateUrl` cross-file credit, `inputs:` sentinel credit,
//! whole-component `extends`-abstain, `this.bar.emit()` output credit, the three
//! signal shapes, and the `model()` input-only rule.

use super::common::{create_config, fixture_path};

#[test]
fn flags_dead_inputs_outputs_and_holds_every_abstain() {
    let root = fixture_path("angular-unused-component-io");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let inputs: Vec<(&str, &str)> = results
        .unused_component_inputs
        .iter()
        .map(|f| (f.input.component_name.as_str(), f.input.input_name.as_str()))
        .collect();
    let outputs: Vec<(&str, &str)> = results
        .unused_component_outputs
        .iter()
        .map(|f| {
            (
                f.output.component_name.as_str(),
                f.output.output_name.as_str(),
            )
        })
        .collect();

    // --- Positive cases ---
    // A dead @Input AND a dead @Output on the same component are both flagged.
    assert!(
        inputs.contains(&("dead-io.component", "deadInput")),
        "a @Input read nowhere should be flagged: {inputs:?}"
    );
    assert!(
        outputs.contains(&("dead-io.component", "deadOutput")),
        "an @Output emitted nowhere should be flagged: {outputs:?}"
    );
    // Signal input() / output() / model() unread are flagged; model is an input.
    assert!(
        inputs.contains(&("signal-io.component", "size")),
        "an unread signal input() should be flagged: {inputs:?}"
    );
    assert!(
        outputs.contains(&("signal-io.component", "toggled")),
        "an unemitted signal output() should be flagged: {outputs:?}"
    );
    assert!(
        inputs.contains(&("signal-io.component", "value")),
        "an unread model() should be flagged as an input: {inputs:?}"
    );
    // model() must NEVER surface as an output (its update: emit is framework-driven).
    assert!(
        !outputs.iter().any(|(_, name)| *name == "value"),
        "a model() must never be flagged as an output: {outputs:?}"
    );

    // --- Abstain cases (none of these may appear in either list) ---
    let abstaining_components = [
        "template-used.component",        // read only in the inline template
        "script-used.component",          // read only via this.count in a method
        "external-template.component",    // read only in the external .html
        "inputs-array.component",         // listed in decorator inputs: [...]
        "extends.component",              // extends a base class (whole-component abstain)
        "emitted-output.component",       // output emitted via this.saved.emit(...)
        "template-emit-output.component", // output emitted via template (click)="picked.emit()"
        "spread-this.component",          // { ...this } forwards every member opaquely
        "observable-output.component", // @Output() is an Observable stream, not new EventEmitter()
    ];
    for component in abstaining_components {
        assert!(
            !inputs.iter().any(|(c, _)| *c == component),
            "{component} must not produce any unused-component-input: {inputs:?}"
        );
        assert!(
            !outputs.iter().any(|(c, _)| *c == component),
            "{component} must not produce any unused-component-output: {outputs:?}"
        );
    }

    // --- No duplicate findings (one harvest per class span) ---
    assert_eq!(
        inputs.len(),
        3,
        "exactly three inputs flagged, no duplicates: {inputs:?}"
    );
    assert_eq!(
        outputs.len(),
        2,
        "exactly two outputs flagged, no duplicates: {outputs:?}"
    );
}
