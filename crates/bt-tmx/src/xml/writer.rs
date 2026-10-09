//! XML laid out exactly as `QXmlStreamWriter` writes it with auto-formatting and a one-space
//! indent: every element on its own line, text inline with its end tag, empty elements as
//! `<x/>`.

/// Writes XML the way Tiled's `QXmlStreamWriter` does.
pub(crate) struct XmlWriter {
    out: String,
    stack: Vec<&'static str>,
    in_start: bool,
    in_empty: bool,
    wrote_something: bool,
    last_was_start: bool,
    minimize: bool,
}

impl XmlWriter {
    /// A document started with its declaration; `minimize` drops line breaks and indentation
    /// (Tiled's "Minimize output").
    pub(crate) fn new(minimize: bool) -> XmlWriter {
        XmlWriter {
            out: String::from(r#"<?xml version="1.0" encoding="UTF-8"?>"#),
            stack: Vec::new(),
            in_start: false,
            in_empty: false,
            wrote_something: false,
            last_was_start: false,
            minimize,
        }
    }

    /// Closes a pending start tag; returns whether characters were written since the last
    /// element boundary (`QXmlStreamWriterPrivate::finishStartElement`).
    fn finish_start_element(&mut self, contents: bool) -> bool {
        let had_something_written = self.wrote_something;
        self.wrote_something = contents;
        if !self.in_start {
            return had_something_written;
        }
        if self.in_empty {
            self.out.push_str("/>");
            self.stack.pop();
            self.last_was_start = false;
        } else {
            self.out.push('>');
        }
        self.in_start = false;
        self.in_empty = false;
        had_something_written
    }

    fn push_indent(&mut self, level: usize) {
        if self.minimize {
            return;
        }
        self.out.push('\n');
        for _ in 0..level {
            self.out.push(' ');
        }
    }

    /// Starts an element.
    pub(crate) fn start_element(&mut self, name: &'static str) {
        if !self.finish_start_element(false) {
            self.push_indent(self.stack.len());
        }
        self.stack.push(name);
        self.out.push('<');
        self.out.push_str(name);
        self.in_start = true;
        self.last_was_start = true;
    }

    /// An element without contents (`writeEmptyElement`); attributes may follow.
    pub(crate) fn empty_element(&mut self, name: &'static str) {
        self.start_element(name);
        self.in_empty = true;
    }

    /// An attribute of the element just started.
    pub(crate) fn attribute(&mut self, name: &str, value: &str) {
        self.out.push(' ');
        self.out.push_str(name);
        self.out.push_str("=\"");
        escape_into(&mut self.out, value, true);
        self.out.push('"');
    }

    /// Text content.
    pub(crate) fn characters(&mut self, text: &str) {
        self.finish_start_element(true);
        escape_into(&mut self.out, text, false);
    }

    /// Ends the innermost open element.
    pub(crate) fn end_element(&mut self) {
        if self.stack.is_empty() {
            return;
        }
        if self.in_start && !self.in_empty {
            self.out.push_str("/>");
            self.last_was_start = false;
            self.in_start = false;
            self.stack.pop();
            return;
        }
        if !self.finish_start_element(false) && !self.last_was_start {
            self.push_indent(self.stack.len().saturating_sub(1));
        }
        let Some(name) = self.stack.pop() else {
            return;
        };
        self.last_was_start = false;
        self.out.push_str("</");
        self.out.push_str(name);
        self.out.push('>');
    }

    /// Closes every open element and ends the document with a line break.
    pub(crate) fn finish(mut self) -> String {
        while !self.stack.is_empty() {
            self.end_element();
        }
        self.out.push('\n');
        self.out
    }
}

/// `QXmlStreamWriterPrivate::writeEscaped`: attributes escape tabs and line breaks; characters
/// XML forbids are dropped.
fn escape_into(out: &mut String, text: &str, in_attribute: bool) {
    for c in text.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\t' if in_attribute => out.push_str("&#9;"),
            '\n' if in_attribute => out.push_str("&#10;"),
            '\r' if in_attribute => out.push_str("&#13;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if u32::from(c) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_follows_qxmlstreamwriter() {
        let mut w = XmlWriter::new(false);
        w.start_element("map");
        w.attribute("name", "a<b> & \"c\"\n\td");
        w.start_element("tileset");
        w.attribute("firstgid", "1");
        w.end_element();
        w.start_element("layer");
        w.start_element("data");
        w.attribute("encoding", "csv");
        w.characters("\n1,2,\n3,4\n");
        w.end_element();
        w.end_element();
        w.start_element("objectgroup");
        w.start_element("object");
        w.empty_element("ellipse");
        w.end_element();
        w.start_element("object");
        w.start_element("text");
        w.characters("");
        w.end_element();
        w.end_element();
        w.end_element();
        w.start_element("properties");
        w.start_element("property");
        w.attribute("name", "p");
        w.characters("x > y\nz");
        w.end_element();
        w.end_element();
        assert_eq!(
            w.finish(),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <map name=\"a&lt;b&gt; &amp; &quot;c&quot;&#10;&#9;d\">\n \
             <tileset firstgid=\"1\"/>\n \
             <layer>\n  \
             <data encoding=\"csv\">\n1,2,\n3,4\n</data>\n \
             </layer>\n \
             <objectgroup>\n  \
             <object>\n   \
             <ellipse/>\n  \
             </object>\n  \
             <object>\n   \
             <text></text>\n  \
             </object>\n \
             </objectgroup>\n \
             <properties>\n  \
             <property name=\"p\">x &gt; y\nz</property>\n \
             </properties>\n\
             </map>\n"
        );
    }

    #[test]
    fn minimized_output_has_no_whitespace() {
        let mut w = XmlWriter::new(true);
        w.start_element("map");
        w.start_element("layer");
        w.end_element();
        assert_eq!(
            w.finish(),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><map><layer/></map>\n"
        );
    }
}
