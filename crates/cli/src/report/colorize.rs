//! The colors of the human text output, with project strings escaped first.
//!
//! A path or a name from the analyzed project can hold a terminal sequence,
//! for example `ESC [ 8 m`, which hides all later text. The human renderers
//! (the report, `audit` and the sink candidates) color most values, so this trait
//! escapes the control characters of each value (see
//! [`fallow_output::display_text`]) before it adds the color. The renderers
//! import this trait in place of `colored::Colorize`. The method names are the
//! same, so a call site does not change.

use colored::ColoredString;

/// The color methods that the human renderers use. Each method on a string
/// escapes the control characters of the string before it adds the color. On
/// a [`ColoredString`], the method adds one more style to a value that is
/// escaped already.
pub trait Colorize: Sized {
    /// The value, with its control characters escaped and no style.
    fn escaped(self) -> ColoredString;

    fn bold(self) -> ColoredString {
        colored::Colorize::bold(self.escaped())
    }

    fn dimmed(self) -> ColoredString {
        colored::Colorize::dimmed(self.escaped())
    }

    fn red(self) -> ColoredString {
        colored::Colorize::red(self.escaped())
    }

    fn green(self) -> ColoredString {
        colored::Colorize::green(self.escaped())
    }

    fn yellow(self) -> ColoredString {
        colored::Colorize::yellow(self.escaped())
    }

    fn cyan(self) -> ColoredString {
        colored::Colorize::cyan(self.escaped())
    }

    fn blue(self) -> ColoredString {
        colored::Colorize::blue(self.escaped())
    }

    fn bright_black(self) -> ColoredString {
        colored::Colorize::bright_black(self.escaped())
    }

    fn magenta(self) -> ColoredString {
        colored::Colorize::magenta(self.escaped())
    }
}

impl Colorize for &str {
    fn escaped(self) -> ColoredString {
        ColoredString::from(&*fallow_output::display_text(self))
    }
}

impl Colorize for ColoredString {
    fn escaped(self) -> ColoredString {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Colorize;

    #[test]
    fn a_terminal_sequence_in_a_value_is_escaped_before_the_color() {
        let colored = "src/hid\u{1b}[8mden.ts".bold();
        assert_eq!(&*colored, r"src/hid\u{001B}[8mden.ts");
        assert_eq!(&*"a\nb".dimmed().bold(), r"a\nb");
    }
}
