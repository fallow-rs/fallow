use super::*;

/// Source where every keyword, operator and punctuation token belongs to a
/// node that spans more than one line.
const MULTI_LINE_SOURCE: &str = "\
import {
  a,
} from './a';
interface Props {
  name: string;
}
enum Mode {
  On,
}
export default class Box extends Base {
  run(items) {
    for (const item of items) {
      if (
        item
      ) {
        continue;
      } else {
        break;
      }
    }
    for (const key in this) {
      key;
    }
    for (let i = 0; i < 1; i++) {
      i;
    }
    while (
      a
    ) {
      a;
    }
    switch (
      a
    ) {
      case 1: {
        break;
      }
      default: {
        break;
      }
    }
    try {
      throw new Error(
        'x',
      );
    } catch (e) {
      return typeof (
        e
      );
    }
  }
}
export const value = async (): Promise<void> => {
  const typed: string =
    a;
  const total =
    a +
    b;
  const pick = a
    ? b
    : c;
  const list = [
    ...a,
  ];
  const object = {
    a,
  } as Props;
  await (
    a, b
  );
  return function named() {
    return (
      <div>
        {a}
      </div>
    );
  };
};
";

/// Tokens whose text can itself span lines.
fn may_span_lines(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::StringLiteral(_) | TokenKind::TemplateLiteral | TokenKind::Boundary(_)
    )
}

/// The `function` and `async` tokens of a method have no text in the source.
/// The visitor emits the arrow of an arrow function before its return type,
/// so with a return type the arrow token has no text either.
fn may_have_no_text(token: &SourceToken) -> bool {
    matches!(
        token.kind,
        TokenKind::Keyword(KeywordType::Function | KeywordType::Async)
            | TokenKind::Operator(OperatorType::Arrow)
    ) && token.span.start == token.span.end
}

/// Keyword, operator and punctuation tokens. The visitor emits some names and
/// strings twice (a class name, a function name, the parameters and an import
/// source), so only these tokens are in source order.
fn is_structural(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Keyword(_) | TokenKind::Operator(_) | TokenKind::Punctuation(_)
    )
}

fn text_of(token: &SourceToken) -> &'static str {
    MULTI_LINE_SOURCE
        .get(token.span.start as usize..token.span.end as usize)
        .unwrap_or_default()
}

#[test]
fn every_token_has_one_line_of_its_own_text() {
    let tokens = tokenize_tsx(MULTI_LINE_SOURCE);
    assert!(tokens.len() > 100, "the fixture must tokenize");
    let bad: Vec<String> = tokens
        .iter()
        .filter(|token| !may_span_lines(&token.kind) && !may_have_no_text(token))
        .filter(|token| {
            let text = text_of(token);
            text.is_empty() || text.contains('\n') || text.trim() != text
        })
        .map(|token| format!("{:?} {:?}", token.kind, text_of(token)))
        .collect();
    assert!(
        bad.is_empty(),
        "each token must carry the span of its own text, not of its node: {bad:#?}"
    );
}

#[test]
fn keyword_tokens_cover_the_keyword_text() {
    let tokens = tokenize_tsx(MULTI_LINE_SOURCE);
    let bad: Vec<String> = tokens
        .iter()
        .filter_map(|token| match &token.kind {
            TokenKind::Keyword(_) if may_have_no_text(token) => None,
            TokenKind::Keyword(keyword) => {
                let expected = format!("{keyword:?}").to_lowercase();
                (text_of(token) != expected)
                    .then(|| format!("{expected} has text {:?}", text_of(token)))
            }
            _ => None,
        })
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn open_paren_tokens_cover_a_paren() {
    let bad: Vec<String> = tokenize_tsx(MULTI_LINE_SOURCE)
        .iter()
        .filter(|token| {
            matches!(
                token.kind,
                TokenKind::Punctuation(PunctuationType::OpenParen)
            )
        })
        .filter(|token| text_of(token) != "(")
        .map(|token| format!("{:?} at {}", text_of(token), token.span.start))
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn token_starts_follow_the_source_order() {
    let tokens: Vec<SourceToken> = tokenize_tsx(MULTI_LINE_SOURCE)
        .into_iter()
        .filter(|token| is_structural(&token.kind))
        .collect();
    let backwards: Vec<String> = tokens
        .windows(2)
        .filter(|pair| pair[1].span.start < pair[0].span.start)
        .map(|pair| {
            format!(
                "{:?} {:?} then {:?} {:?}",
                pair[0].kind,
                text_of(&pair[0]),
                pair[1].kind,
                text_of(&pair[1])
            )
        })
        .collect();
    assert!(backwards.is_empty(), "{backwards:#?}");
}
