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

fn types_for_export(source: &str, export_name: &str) -> Vec<String> {
    signature_pairs(source)
        .into_iter()
        .filter(|(export, _)| export == export_name)
        .map(|(_, type_name)| type_name)
        .collect()
}

#[test]
fn signature_references_include_function_argument_of_wrapper_call() {
    let source = r"
declare function wrap<T>(value: T): T;
interface CardProps { title: string }
export const Card = wrap(function Card({ title }: CardProps) { return title; });
";

    assert_eq!(
        types_for_export(source, "Card"),
        vec!["CardProps".to_string()]
    );
}

#[test]
fn signature_references_include_wrapper_call_type_arguments_and_arrow_argument() {
    let source = r"
declare function withRef<T, P>(render: (props: P, ref: T) => unknown): (props: P) => unknown;
interface FieldProps { label: string }
interface FieldHandle { focus(): void }
export const Field = withRef<FieldHandle, FieldProps>((props: FieldProps) => props.label);
";

    assert_eq!(
        types_for_export(source, "Field"),
        vec![
            "FieldHandle".to_string(),
            "FieldProps".to_string(),
            "FieldProps".to_string(),
        ]
    );
}

#[test]
fn signature_references_include_new_expression_type_arguments() {
    let source = r"
declare class Store<T> { constructor(value: T) }
interface StoreState { count: number }
export const store = new Store<StoreState>({ count: 0 });
";

    assert_eq!(
        types_for_export(source, "store"),
        vec!["StoreState".to_string()]
    );
}

#[test]
fn signature_references_include_type_assertions_on_initializers() {
    let source = r#"
type Level = "low" | "high";
type Shape = { size: number };
export const DEFAULT_LEVEL = "low" as Level;
export const DEFAULT_SHAPE = <Shape>{ size: 1 };
export const WRAPPED = ("high" as Level)!;
"#;

    assert_eq!(
        types_for_export(source, "DEFAULT_LEVEL"),
        vec!["Level".to_string()]
    );
    assert_eq!(
        types_for_export(source, "DEFAULT_SHAPE"),
        vec!["Shape".to_string()]
    );
    assert_eq!(
        types_for_export(source, "WRAPPED"),
        vec!["Level".to_string()]
    );
}

#[test]
fn signature_references_ignore_value_arguments_of_wrapper_calls() {
    let source = r"
declare function make<T>(value: T): T;
interface Hidden { value: number }
const local = (input: Hidden) => input.value;
export const made = make(local);
";

    assert!(types_for_export(source, "made").is_empty());
}
