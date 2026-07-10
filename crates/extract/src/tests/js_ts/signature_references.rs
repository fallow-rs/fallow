use crate::tests::parse_ts;

fn signature_pairs(source: &str) -> Vec<(String, String)> {
    parse_ts(source)
        .public_signature_type_references
        .into_iter()
        .map(|reference| (reference.export_name, reference.type_name))
        .collect()
}

#[test]
fn signature_references_preserve_export_and_per_owner_order_with_duplicates() {
    let source = r"
type Input = { value: string };
type Output = { value: number };
function local(first: Input, second: Input): Output {
    return { value: first.value.length + second.value.length };
}
export { local as beta };
export { local as alpha };
";

    assert_eq!(
        signature_pairs(source),
        vec![
            ("beta".to_string(), "Input".to_string()),
            ("beta".to_string(), "Input".to_string()),
            ("beta".to_string(), "Output".to_string()),
            ("alpha".to_string(), "Input".to_string()),
            ("alpha".to_string(), "Input".to_string()),
            ("alpha".to_string(), "Output".to_string()),
        ]
    );
}

#[test]
fn signature_references_preserve_named_and_anonymous_default_exports() {
    let named = r"
type Input = { value: string };
type Output = { value: number };
export default function convert(value: Input): Output {
    return { value: value.value.length };
}
";
    let anonymous = r"
type Input = { value: string };
type Output = { value: number };
export default function (value: Input): Output {
    return { value: value.value.length };
}
";
    let expected = vec![
        ("default".to_string(), "Input".to_string()),
        ("default".to_string(), "Output".to_string()),
    ];

    assert_eq!(signature_pairs(named), expected);
    assert_eq!(signature_pairs(anonymous), expected);
}
