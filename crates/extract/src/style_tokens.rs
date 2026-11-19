//! A token walk of stylesheet source before lightningcss parses it. The walk
//! uses cssparser, the tokenizer that lightningcss uses, so escapes, `url(`
//! tokens, bad strings and block ends read as they do in the parse.

use cssparser::{ParseError, Parser, ParserInput, Token};
use lightningcss::traits::Parse;
use lightningcss::values::{calc::Calc, percentage::Percentage};

use crate::nesting::MAX_COLOR_FUNCTION_DEPTH;

/// Nesting depth of CSS math functions past which lightningcss does not parse
/// a stylesheet.
///
/// When the innermost value of nested math functions is not of the type that
/// lightningcss expects, the parse time doubles at each level. Real
/// stylesheets nest a few math functions at most, for example
/// `clamp(1rem, calc(1rem + max(1vw, 2px)), 2rem)`.
const MAX_MATH_FUNCTION_DEPTH: usize = 6;

/// The CSS functions that lightningcss tries to parse as a color first.
const COLOR_FUNCTIONS: &[&str] = &[
    "rgb",
    "rgba",
    "hsl",
    "hsla",
    "hwb",
    "lab",
    "lch",
    "oklab",
    "oklch",
    "color",
    "color-mix",
    "light-dark",
    "alpha",
];

/// The CSS math functions that lightningcss parses as a calculation.
const MATH_FUNCTIONS: &[&str] = &[
    "calc", "min", "max", "clamp", "round", "rem", "mod", "sin", "cos", "tan", "asin", "acos",
    "atan", "atan2", "pow", "log", "sqrt", "exp", "hypot", "abs", "sign",
];

/// The properties and descriptors where lightningcss parses a bare
/// percentage, so a number-only math function such as `calc(1 / 2)` makes it
/// panic. `initial-value` is the `@property` descriptor, whose `syntax` can be
/// `<percentage>`. Keyframe selectors parse a bare percentage too, and they
/// stand outside any declaration.
const PERCENTAGE_ONLY_PROPERTIES: &[&str] = &[
    "font-stretch",
    "text-size-adjust",
    "-webkit-text-size-adjust",
    "-moz-text-size-adjust",
    "-ms-text-size-adjust",
    "initial-value",
];

/// The levels that `atan2` adds to the math nesting. lightningcss tries five
/// value types at each `atan2`, so each nested `atan2` costs about ten times
/// the one inside it, where the other math functions cost about twice.
const ATAN2_LEVELS: usize = 3;

/// Whether lightningcss parses `source` without a panic and without
/// exponential time. Returns false when color functions nest deeper than
/// [`MAX_COLOR_FUNCTION_DEPTH`], when math functions nest deeper than
/// [`MAX_MATH_FUNCTION_DEPTH`], or when a math function makes lightningcss
/// panic, see [`math_function_panics`]. The walk recurses once per block, so
/// call it on a stack that the parse of `source` fits on.
pub fn style_parse_is_safe(source: &str) -> bool {
    if !may_open_checked_function(source) {
        return true;
    }
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    walk(&mut parser, Context::default(), true).is_ok()
}

/// Whether `source` can hold a color or a math function, so the walk can
/// find something. A function token is its name right before a `(`. An
/// escape can spell any name, so a source with a `\` always gets the walk.
fn may_open_checked_function(source: &str) -> bool {
    let bytes = source.as_bytes();
    bytes.contains(&b'\\')
        || bytes.iter().enumerate().any(|(open, &byte)| {
            byte == b'(' && {
                let start = bytes[..open]
                    .iter()
                    .rposition(|&before| !is_name_byte(before))
                    .map_or(0, |index| index + 1);
                let name = &source[start..open];
                is_one_of(name, COLOR_FUNCTIONS) || is_one_of(name, MATH_FUNCTIONS)
            }
        })
}

/// Whether `byte` can be a part of a CSS name.
const fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') || byte >= 0x80
}

/// The open color and math functions around a token, and the declaration
/// that it is a part of.
#[derive(Clone, Copy, Default)]
struct Context {
    colors: usize,
    math: usize,
    /// Whether a math function is open and no color function opened inside
    /// it. lightningcss parses such a math function only as a part of the
    /// math function around it, so only that one needs a check.
    in_math: bool,
    /// Whether the token is in the value of a declaration whose property
    /// takes a number where it takes a percentage, so a number-only math
    /// function does not make lightningcss panic.
    number_allowed: bool,
}

impl Context {
    /// The context inside `block`.
    fn enter(self, block: Block) -> Self {
        Self {
            colors: self.colors + usize::from(block.color),
            math: self.math + block.math_levels,
            in_math: (self.in_math || block.is_math()) && !block.color,
            // A legacy `rgb(10%, 10%, ...)` reads each channel after a
            // percentage as a bare percentage.
            number_allowed: self.number_allowed && !block.rules && !block.legacy_rgb,
        }
    }

    const fn exceeds_caps(self) -> bool {
        self.colors > MAX_COLOR_FUNCTION_DEPTH || self.math > MAX_MATH_FUNCTION_DEPTH
    }
}

/// A function or a bracket that opens a nested block.
#[derive(Clone, Copy)]
struct Block {
    color: bool,
    /// The levels that the block adds to the math nesting, 0 for a block that
    /// is not a math function.
    math_levels: usize,
    legacy_rgb: bool,
    /// Whether the block is a `{}` rule block, which holds declarations.
    rules: bool,
}

impl Block {
    fn of(token: &Token<'_>) -> Option<Self> {
        match token {
            Token::Function(name) => Some(Self {
                color: is_one_of(name, COLOR_FUNCTIONS),
                math_levels: if name.eq_ignore_ascii_case("atan2") {
                    ATAN2_LEVELS
                } else {
                    usize::from(is_one_of(name, MATH_FUNCTIONS))
                },
                legacy_rgb: is_one_of(name, &["rgb", "rgba"]),
                rules: false,
            }),
            Token::ParenthesisBlock | Token::SquareBracketBlock => Some(Self::bracket(false)),
            Token::CurlyBracketBlock => Some(Self::bracket(true)),
            _ => None,
        }
    }

    const fn bracket(rules: bool) -> Self {
        Self {
            color: false,
            math_levels: 0,
            legacy_rgb: false,
            rules,
        }
    }

    const fn is_math(self) -> bool {
        self.math_levels > 0
    }
}

/// Walks every token of `parser`. A rule block (`rules`) holds declarations
/// as `name: value;`. Returns an error as soon as the stylesheet is not safe
/// to parse.
fn walk<'i>(
    parser: &mut Parser<'i, '_>,
    outer: Context,
    rules: bool,
) -> Result<(), ParseError<'i, ()>> {
    let mut context = outer;
    let mut property = None;
    loop {
        let start = parser.position();
        let Ok(token) = parser.next() else {
            return Ok(());
        };
        if rules && read_declaration(token, &mut property, &mut context) {
            continue;
        }
        property = None;
        let Some(block) = Block::of(token) else {
            continue;
        };
        let inner = context.enter(block);
        if inner.exceeds_caps() {
            return Err(parser.new_custom_error(()));
        }
        parser.parse_nested_block(|nested| walk(nested, inner, block.rules))?;
        if block.rules {
            context.number_allowed = false;
        }
        if block.is_math()
            && !context.in_math
            && math_function_panics(parser.slice_from(start), context.number_allowed)
        {
            return Err(parser.new_custom_error(()));
        }
    }
}

/// Reads a token of a `name: value;` declaration in a rule block into
/// `property` (whether the last name allows a number) and `context`. Returns
/// false for any other token.
fn read_declaration(token: &Token<'_>, property: &mut Option<bool>, context: &mut Context) -> bool {
    match token {
        Token::Ident(name) => *property = Some(!is_one_of(name, PERCENTAGE_ONLY_PROPERTIES)),
        Token::Colon => context.number_allowed = property.take().unwrap_or(false),
        Token::Semicolon => {
            context.number_allowed = false;
            *property = None;
        }
        _ => return false,
    }
    true
}

fn is_one_of(name: &str, names: &[&str]) -> bool {
    names.iter().any(|known| name.eq_ignore_ascii_case(known))
}

/// Whether lightningcss can panic when it parses the math function `text`
/// where a percentage is expected, for example as `opacity`, `font-stretch`
/// or a color channel. lightningcss expects a percentage math function to
/// reduce to one percentage, and it panics on any other result, such as the
/// one of `max(4%, 5)`, `calc(4% + 5)`, `abs(4%)` or `max(4, nan)`. This runs
/// the same math parse alone, which returns that result instead of a panic.
/// A number-only result such as the one of `calc(1 / 2)` panics only where a
/// number is not allowed (`number_allowed`), because elsewhere lightningcss
/// parses the number first.
fn math_function_panics(text: &str, number_allowed: bool) -> bool {
    match Calc::<Percentage>::parse_string(text) {
        Ok(Calc::Value(_)) | Err(_) => false,
        Ok(Calc::Number(_)) => !number_allowed,
        Ok(_) => true,
    }
}
