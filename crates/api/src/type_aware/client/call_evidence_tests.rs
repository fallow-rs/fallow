use super::*;

fn param(index: u32, optional: bool, has_default: bool) -> MethodParam {
    MethodParam {
        index,
        name: Some(format!("p{index}")),
        pattern: false,
        rest: false,
        optional,
        has_default,
        written: false,
        decorated: false,
        condition_reads: 0,
        line: 3,
        col: 2,
    }
}

fn call(arg_count: u32, args: Vec<(bool, Option<u32>)>) -> MethodCall {
    MethodCall {
        path: "src/use.ts".to_string(),
        line: 1,
        col: 0,
        arg_count,
        spread_from: None,
        args: args
            .into_iter()
            .map(|(maybe_undefined, pass_through)| CallArgument {
                maybe_undefined,
                pass_through,
            })
            .collect(),
    }
}

#[test]
fn a_default_is_unused_only_when_no_argument_can_be_undefined() {
    let defined = [call(1, vec![(false, None)]), call(1, vec![(false, None)])];
    let refs: Vec<&MethodCall> = defined.iter().collect();
    assert_eq!(
        method_verdict(&param(0, false, true), &refs, false),
        Some(ConstantParameterReason::UnusedDefault)
    );
    let mixed = [call(1, vec![(false, None)]), call(1, vec![(true, None)])];
    let refs: Vec<&MethodCall> = mixed.iter().collect();
    assert_eq!(method_verdict(&param(0, false, true), &refs, false), None);
    let required = [call(1, vec![(false, None)]), call(1, vec![(false, None)])];
    let refs: Vec<&MethodCall> = required.iter().collect();
    assert_eq!(method_verdict(&param(0, false, false), &refs, false), None);
}

#[test]
fn a_parameter_that_no_call_passes_is_never_passed_unless_a_self_call_passes_it() {
    let omitted = [call(0, vec![]), call(0, vec![])];
    let refs: Vec<&MethodCall> = omitted.iter().collect();
    assert_eq!(
        method_verdict(&param(0, true, false), &refs, false),
        Some(ConstantParameterReason::NeverPassed)
    );
    assert_eq!(method_verdict(&param(0, true, false), &refs, true), None);
    assert_eq!(method_verdict(&param(0, false, false), &refs, false), None);
}

#[test]
fn a_spread_hides_the_argument() {
    let mut spread = call(1, vec![]);
    spread.spread_from = Some(0);
    let calls = [spread, call(1, vec![(false, None)])];
    let refs: Vec<&MethodCall> = calls.iter().collect();
    assert_eq!(method_verdict(&param(0, false, true), &refs, false), None);
}

#[test]
fn a_suppressed_method_line_gives_no_finding() {
    let class = MethodClassCandidate {
        path: PathBuf::from("/p/src/svc.ts"),
        class_name: "Service".to_string(),
        methods: vec!["save".to_string()],
        suppressed_lines: vec![3],
    };
    let result = MethodResult {
        class_id: 0,
        method: "save".to_string(),
        outcome: MethodOutcome::Complete,
        reason: None,
        line: 2,
        params: vec![param(0, false, true)],
        calls: vec![call(1, vec![(false, None)]), call(1, vec![(false, None)])],
    };
    assert!(method_findings(Path::new("/p"), &class, &result).is_empty());
    let open = MethodClassCandidate {
        suppressed_lines: Vec::new(),
        ..class
    };
    let findings = method_findings(Path::new("/p"), &open, &result);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].function_name, "Service.save");
    assert_eq!(
        findings[0].evidence,
        Some(ConstantParameterEvidence::TypeAware)
    );
    assert_eq!(findings[0].reason, ConstantParameterReason::UnusedDefault);
}
