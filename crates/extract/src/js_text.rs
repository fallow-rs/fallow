//! Conversion of JavaScript string values to Rust text.

use std::borrow::Cow;

use oxc_ast::StaticPropertyName;
use oxc_str::JSStr;

/// The text of a JavaScript string value.
///
/// A well-formed value is borrowed. A value with a lone surrogate is owned, with
/// each lone surrogate written as `\uXXXX` (as `JSStr::display` does). Oxc 0.152
/// also stored a lone surrogate as escaped text (`\u{FFFD}XXXX`), so the text of
/// every well-formed value is the same as before.
pub trait JsText<'a> {
    /// Return the value as UTF-8 text.
    fn text(self) -> Cow<'a, str>;
}

impl<'a> JsText<'a> for JSStr<'a> {
    fn text(self) -> Cow<'a, str> {
        self.as_str()
            .map_or_else(|| Cow::Owned(self.display().to_string()), Cow::Borrowed)
    }
}

impl<'b> JsText<'b> for &'b StaticPropertyName<'_> {
    fn text(self) -> Cow<'b, str> {
        self.as_js_str().text()
    }
}

/// The text of a static property name, as [`JsText::text`] gives it, without a
/// borrow of the name.
#[must_use]
pub fn static_name_text(name: StaticPropertyName<'_>) -> Cow<'_, str> {
    match name {
        StaticPropertyName::Borrowed(name) => name.text(),
        StaticPropertyName::Owned(name) => Cow::Owned(name.into_string()),
    }
}
