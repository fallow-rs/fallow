//! The JSX view of the nesting scan, see the module docs of `nesting.rs`.

use super::{JsxContainer, JsxPart, Scanner, is_identifier_byte, is_identifier_start};

/// The JSX view of the scan.
impl Scanner<'_> {
    /// Whether the `<` at `pos - 1` opens a JSX element: it stands where an
    /// expression starts, and a tag name or the `>` of a fragment follows.
    /// In a TSX file, `<T,>` and `<T extends U>` open the type parameters of
    /// an arrow function instead.
    pub(super) fn at_jsx_tag(&self) -> bool {
        if !(self.regex_allowed || self.after_keyword()) {
            return false;
        }
        match self.bytes.get(self.pos) {
            Some(b'>') => true,
            Some(&byte) if is_identifier_start(byte) => !self.opens_type_parameters(),
            _ => false,
        }
    }

    /// Whether the name after the `<` at `pos - 1` is followed by `,` or by
    /// `extends` and a type.
    fn opens_type_parameters(&self) -> bool {
        let name_end = self.jsx_name_end(self.pos);
        let rest = self.bytes[name_end..].trim_ascii_start();
        if rest.first() == Some(&b',') {
            return true;
        }
        rest.strip_prefix(b"extends").is_some_and(|after| {
            after.first().is_some_and(u8::is_ascii_whitespace)
                && !matches!(after.trim_ascii_start().first(), Some(b'=' | b'>' | b'/'))
        })
    }

    /// The end of the JSX tag name that starts at `start`.
    fn jsx_name_end(&self, start: usize) -> usize {
        self.bytes[start..]
            .iter()
            .position(|&byte| !is_jsx_name_byte(byte))
            .map_or(self.bytes.len(), |len| start + len)
    }

    /// Reads the JSX element whose `<` is at `pos - 1`, up to its end or to
    /// the first `{` container in it.
    pub(super) fn open_jsx_element(&mut self) {
        self.jsx_elements = 1;
        self.pos = self.jsx_name_end(self.pos);
        self.scan_jsx(JsxPart::Attributes);
    }

    /// Reads JSX from `pos` in `part` of the innermost open element. Returns
    /// at a `{` container, whose code the main scan reads, or after the last
    /// open element of the JSX expression closes. Element nesting is a count,
    /// so deep markup needs no deep call stack.
    fn scan_jsx(&mut self, mut part: JsxPart) {
        while let Some(&byte) = self.bytes.get(self.pos) {
            self.pos += 1;
            match (part, byte) {
                (_, b'{') => {
                    self.open_bracket(b'{');
                    self.jsx_containers.push(JsxContainer {
                        depth: self.depth,
                        part,
                        elements: self.jsx_elements,
                    });
                    self.jsx_elements = 0;
                    return;
                }
                (JsxPart::Attributes, b'"' | b'\'') => self.skip_jsx_string(byte),
                (JsxPart::Attributes, b'/') if self.bytes.get(self.pos) == Some(&b'>') => {
                    self.pos += 1;
                    if self.close_jsx_element() {
                        return;
                    }
                    part = JsxPart::Children;
                }
                (JsxPart::Attributes, b'>') => part = JsxPart::Children,
                (JsxPart::Children, b'<') if self.bytes.get(self.pos) == Some(&b'/') => {
                    while let Some(&byte) = self.bytes.get(self.pos) {
                        self.pos += 1;
                        if byte == b'>' {
                            break;
                        }
                    }
                    if self.close_jsx_element() {
                        return;
                    }
                }
                (JsxPart::Children, b'<') => {
                    self.jsx_elements += 1;
                    self.pos = self.jsx_name_end(self.pos);
                    part = JsxPart::Attributes;
                }
                _ => {}
            }
        }
    }

    /// Skips a JSX attribute string. It has no escapes and can span lines.
    fn skip_jsx_string(&mut self, quote: u8) {
        self.pos = self.bytes[self.pos..]
            .iter()
            .position(|&byte| byte == quote)
            .map_or(self.bytes.len(), |len| self.pos + len + 1);
    }

    /// Closes the innermost open element. Returns true when that ends the
    /// JSX expression, so the main scan reads code again after an operand.
    fn close_jsx_element(&mut self) -> bool {
        self.jsx_elements = self.jsx_elements.saturating_sub(1);
        if self.jsx_elements > 0 {
            return false;
        }
        self.regex_allowed = false;
        true
    }

    /// Ends the `{` container at the current depth and reads on in its
    /// element.
    pub(super) fn close_jsx_container(&mut self) {
        let Some(container) = self.jsx_containers.pop() else {
            return;
        };
        self.depth -= 1;
        self.close_type_arguments_above_depth();
        self.jsx_elements = container.elements;
        self.scan_jsx(container.part);
    }
}

/// Whether `byte` can be part of a JSX tag name, such as `a.b`, `a:b` or
/// `my-tag`.
const fn is_jsx_name_byte(byte: u8) -> bool {
    is_identifier_byte(byte) || matches!(byte, b'.' | b':' | b'-')
}
