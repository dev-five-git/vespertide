//! Small helpers shared across the SVG ERD renderer.

#![expect(
    clippy::uninlined_format_args,
    reason = "long SVG template strings keep repeated named arguments explicit for readability"
)]

use super::style::{BG, FONT_FAMILY};

pub(super) fn render_empty() -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 320 120\" \
         width=\"320\" height=\"120\" font-family=\"{ff}\">\n\
         \x20 <rect x=\"0\" y=\"0\" width=\"320\" height=\"120\" fill=\"{bg}\"/>\n\
         \x20 <text x=\"160\" y=\"65\" fill=\"#50505d\" font-size=\"14\" \
         text-anchor=\"middle\">No tables to render</text>\n\
         </svg>\n",
        ff = FONT_FAMILY,
        bg = BG,
    )
}

pub(super) fn escape_xml(input: &str) -> std::borrow::Cow<'_, str> {
    if !input.contains(['&', '<', '>', '"', '\'']) {
        return std::borrow::Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    std::borrow::Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_empty_contains_placeholder_svg() {
        let svg = render_empty();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("No tables to render"));
    }

    #[test]
    fn escape_xml_escapes_every_special_character() {
        assert_eq!(escape_xml("&<>\"'x"), "&amp;&lt;&gt;&quot;&apos;x");
    }
}
