use std::path::Path;

/// Path::new("foo").parent() == Some("") which is weird and not really what I want.
/// This does the same thing but also returns None if the parent is empty
pub fn dirname(path: &Path) -> Option<&Path> {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => Some(p),
        _ => None,
    }
}

// Printing things with ansi_term involves some weird Cow trait bounds that break things, so I want
// to use termcolor instead. lscolors::Style has a method to convert to an ansi_term::Style, but
// not to a termcolor::ColorSpec, so roll my own conversion with some extension traits.

pub trait ToColor {
    fn to_color(&self) -> termcolor::Color;
}

impl ToColor for lscolors::style::Color {
    fn to_color(&self) -> termcolor::Color {
        use lscolors::style::Color::*;
        use termcolor::Color;
        match *self {
            Black => Color::Black,
            Red => Color::Red,
            Green => Color::Green,
            Yellow => Color::Yellow,
            Blue => Color::Blue,
            Magenta => Color::Magenta,
            Cyan => Color::Cyan,
            White => Color::White,
            Fixed(x) => Color::Ansi256(x),
            RGB(r, g, b) => Color::Rgb(r, g, b),
        }
    }
}

pub trait ToColorSpec {
    fn to_color_spec(&self) -> termcolor::ColorSpec;
}

impl ToColorSpec for lscolors::Style {
    fn to_color_spec(&self) -> termcolor::ColorSpec {
        let mut cs = termcolor::ColorSpec::new();
        cs.set_fg(self.foreground.as_ref().map(|c| c.to_color()))
            .set_bg(self.background.as_ref().map(|c| c.to_color()))
            .set_bold(self.font_style.bold)
            .set_dimmed(self.font_style.dimmed)
            .set_italic(self.font_style.italic)
            .set_underline(self.font_style.underline);
        // note: no termcolor properties for blink, reverse, hidden, or strikethrough
        cs
    }
}
