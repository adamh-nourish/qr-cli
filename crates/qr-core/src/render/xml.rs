//! A minimal typed XML writer.
//!
//! Element and attribute names are `&'static str`, so only names written in
//! this crate's source can appear in the output. Every attribute value and
//! text node goes through [`escape::xml`](super::escape::xml), so no value can
//! open or close markup. Elements are closed in order by [`XmlWriter::end`]
//! and [`XmlWriter::finish`], so the output is always well-formed.

use std::fmt::{self, Write as _};

use super::escape;

/// Builds an XML document as a string.
#[derive(Debug, Default)]
pub(crate) struct XmlWriter {
    out: String,
    /// Elements opened and not yet closed, innermost last.
    open: Vec<&'static str>,
    /// Whether the innermost start tag still awaits its closing `>`, so
    /// attributes may be added and an empty element can self-close.
    in_start_tag: bool,
}

impl XmlWriter {
    /// Starts a document with the XML declaration.
    pub(crate) fn new() -> Self {
        Self {
            out: String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"),
            ..Self::default()
        }
    }

    /// Opens element `name`.
    pub(crate) fn start(&mut self, name: &'static str) -> &mut Self {
        self.close_start_tag();
        self.out.push('<');
        self.out.push_str(name);
        self.open.push(name);
        self.in_start_tag = true;
        self
    }

    /// Adds an attribute to the element just opened. Ignored, rather than
    /// written out of place, once the element has content.
    pub(crate) fn attr(&mut self, name: &'static str, value: impl fmt::Display) -> &mut Self {
        if self.in_start_tag {
            let _ = write!(self.out, " {name}=\"{}\"", escape::xml(&value.to_string()));
        }
        self
    }

    /// Writes escaped text content.
    pub(crate) fn text(&mut self, text: &str) -> &mut Self {
        self.close_start_tag();
        self.out.push_str(&escape::xml(text));
        self
    }

    /// Writes a line break between elements, for readability.
    pub(crate) fn newline(&mut self) -> &mut Self {
        self.close_start_tag();
        self.out.push('\n');
        self
    }

    /// Closes the innermost open element.
    pub(crate) fn end(&mut self) -> &mut Self {
        if let Some(name) = self.open.pop() {
            if self.in_start_tag {
                self.out.push_str("/>");
                self.in_start_tag = false;
            } else {
                self.out.push_str("</");
                self.out.push_str(name);
                self.out.push('>');
            }
        }
        self
    }

    /// Closes every open element and returns the document, ending in `\n`.
    pub(crate) fn finish(mut self) -> String {
        while !self.open.is_empty() {
            self.end();
        }
        self.out.push('\n');
        self.out
    }

    fn close_start_tag(&mut self) {
        if self.in_start_tag {
            self.out.push('>');
            self.in_start_tag = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";

    #[test]
    fn writes_nested_elements_and_self_closes_empty_ones() {
        let mut w = XmlWriter::new();
        w.start("a").attr("k", 1).start("b").end().newline();
        w.start("c").text("x<y").end();
        assert_eq!(
            w.finish(),
            format!("{DECL}<a k=\"1\"><b/>\n<c>x&lt;y</c></a>\n")
        );
    }

    #[test]
    fn attribute_values_are_escaped() {
        let mut w = XmlWriter::new();
        w.start("e").attr("v", "\"><script>alert(1)</script>");
        assert_eq!(
            w.finish(),
            format!("{DECL}<e v=\"&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;\"/>\n")
        );
    }

    #[test]
    fn late_attributes_and_extra_ends_are_ignored() {
        let mut w = XmlWriter::new();
        w.start("e").text("t").attr("late", "x").end().end();
        assert_eq!(w.finish(), format!("{DECL}<e>t</e>\n"));
    }
}
