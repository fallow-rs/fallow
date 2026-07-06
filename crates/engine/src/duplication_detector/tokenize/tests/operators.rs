use super::*;

#[test]
fn tokenize_all_binary_operators() {
    let code = r"
const a = 1 + 2;
const b = 3 - 4;
const c = 5 * 6;
const d = 7 / 8;
const e = 9 % 10;
const f = 2 ** 3;
const g = a == b;
const h = a != b;
const i = a === b;
const j = a !== b;
const k = a < b;
const l = a > b;
const m = a <= b;
const n = a >= b;
const o = a & b;
const p = a | b;
const q = a ^ b;
const r = a << b;
const s = a >> b;
const t = a >>> b;
const u = a instanceof Object;
const v = 'key' in obj;
";
    let tokens = tokenize(code);
    let ops: Vec<&OperatorType> = tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Operator(op) => Some(op),
            _ => None,
        })
        .collect();
    assert!(ops.contains(&&OperatorType::Add));
    assert!(ops.contains(&&OperatorType::Sub));
    assert!(ops.contains(&&OperatorType::Mul));
    assert!(ops.contains(&&OperatorType::Div));
    assert!(ops.contains(&&OperatorType::Mod));
    assert!(ops.contains(&&OperatorType::Exp));
    assert!(ops.contains(&&OperatorType::Eq));
    assert!(ops.contains(&&OperatorType::NEq));
    assert!(ops.contains(&&OperatorType::StrictEq));
    assert!(ops.contains(&&OperatorType::StrictNEq));
    assert!(ops.contains(&&OperatorType::Lt));
    assert!(ops.contains(&&OperatorType::Gt));
    assert!(ops.contains(&&OperatorType::LtEq));
    assert!(ops.contains(&&OperatorType::GtEq));
    assert!(ops.contains(&&OperatorType::BitwiseAnd));
    assert!(ops.contains(&&OperatorType::BitwiseOr));
    assert!(ops.contains(&&OperatorType::BitwiseXor));
    assert!(ops.contains(&&OperatorType::ShiftLeft));
    assert!(ops.contains(&&OperatorType::ShiftRight));
    assert!(ops.contains(&&OperatorType::UnsignedShiftRight));
    assert!(ops.contains(&&OperatorType::Instanceof));
    assert!(ops.contains(&&OperatorType::In));
}

#[test]
fn tokenize_logical_operators() {
    let tokens = tokenize("const x = a && b || c ?? d;");
    let ops: Vec<&OperatorType> = tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Operator(op) => Some(op),
            _ => None,
        })
        .collect();
    assert!(ops.contains(&&OperatorType::And));
    assert!(ops.contains(&&OperatorType::Or));
    assert!(ops.contains(&&OperatorType::NullishCoalescing));
}

#[test]
fn tokenize_assignment_operators() {
    let code = r"
x = 1;
x += 1;
x -= 1;
x *= 1;
x /= 1;
x %= 1;
x **= 1;
x &&= true;
x ||= true;
x ??= 1;
x &= 1;
x |= 1;
x ^= 1;
x <<= 1;
x >>= 1;
x >>>= 1;
";
    let tokens = tokenize(code);
    let ops: Vec<&OperatorType> = tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Operator(op) => Some(op),
            _ => None,
        })
        .collect();
    assert!(ops.contains(&&OperatorType::Assign));
    assert!(ops.contains(&&OperatorType::AddAssign));
    assert!(ops.contains(&&OperatorType::SubAssign));
    assert!(ops.contains(&&OperatorType::MulAssign));
    assert!(ops.contains(&&OperatorType::DivAssign));
    assert!(ops.contains(&&OperatorType::ModAssign));
    assert!(ops.contains(&&OperatorType::ExpAssign));
    assert!(ops.contains(&&OperatorType::AndAssign));
    assert!(ops.contains(&&OperatorType::OrAssign));
    assert!(ops.contains(&&OperatorType::NullishAssign));
    assert!(ops.contains(&&OperatorType::BitwiseAndAssign));
    assert!(ops.contains(&&OperatorType::BitwiseOrAssign));
    assert!(ops.contains(&&OperatorType::BitwiseXorAssign));
    assert!(ops.contains(&&OperatorType::ShiftLeftAssign));
    assert!(ops.contains(&&OperatorType::ShiftRightAssign));
    assert!(ops.contains(&&OperatorType::UnsignedShiftRightAssign));
}

#[test]
fn tokenize_unary_operators() {
    let code = "const a = +x; const b = -x; const c = !x; const d = ~x;";
    let tokens = tokenize(code);
    let ops: Vec<&OperatorType> = tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Operator(op) => Some(op),
            _ => None,
        })
        .collect();
    assert!(
        ops.contains(&&OperatorType::Add),
        "Should have unary plus (mapped to Add)"
    );
    assert!(
        ops.contains(&&OperatorType::Sub),
        "Should have unary minus (mapped to Sub)"
    );
    assert!(ops.contains(&&OperatorType::Not), "Should have logical not");
    assert!(
        ops.contains(&&OperatorType::BitwiseNot),
        "Should have bitwise not"
    );
}

#[test]
fn tokenize_typeof_void_delete_as_keywords() {
    let tokens = tokenize("typeof x; void 0; delete obj.key;");
    let has_typeof = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Typeof)));
    let has_void = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Void)));
    let has_delete = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Delete)));
    assert!(has_typeof, "typeof should be a keyword token");
    assert!(has_void, "void should be a keyword token");
    assert!(has_delete, "delete should be a keyword token");
}

#[test]
fn tokenize_prefix_and_postfix_update() {
    let tokens = tokenize("++x; x--;");
    let first_increment_idx = tokens
        .iter()
        .position(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Increment)));
    let has_decrement = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Decrement)));
    assert!(
        first_increment_idx.is_some(),
        "Should have increment operator"
    );
    assert!(has_decrement, "Should have decrement operator");

    let first_x_idx = tokens
        .iter()
        .position(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "x"))
        .unwrap();
    assert!(
        first_increment_idx.unwrap() < first_x_idx,
        "Prefix ++ should appear before identifier"
    );
}
