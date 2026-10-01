//! A small XML writer for the XML formats (MusicXML, TTML, SSML): indented
//! elements, escaped text and attributes. Mixed content (text with inline
//! elements) is built with [`tag`] and written with [`Xml::raw`].

/// An XML document being written.
pub struct Xml {
    out: String,
    open: Vec<&'static str>,
}

impl Xml {
    /// A document starting with the XML declaration and `prolog` (a DOCTYPE,
    /// or nothing).
    pub fn new(prolog: &str) -> Xml {
        let mut out = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        if !prolog.is_empty() {
            out.push('\n');
            out.push_str(prolog);
        }
        Xml { out, open: vec![] }
    }

    fn line(&mut self) {
        self.out.push('\n');
        for _ in 0..self.open.len() {
            self.out.push_str("  ");
        }
    }

    /// Open an element; [`Xml::close`] ends it.
    pub fn open(&mut self, name: &'static str, attrs: &[(&str, &str)]) {
        self.line();
        self.out.push_str(&start(name, attrs));
        self.out.push('>');
        self.open.push(name);
    }

    pub fn close(&mut self) {
        let name = self.open.pop().expect("an open element");
        self.line();
        self.out.push_str(&format!("</{name}>"));
    }

    /// An element holding text.
    pub fn leaf(&mut self, name: &str, attrs: &[(&str, &str)], text: &str) {
        self.raw(&tag(name, attrs, &escape(text)));
    }

    /// An element without content.
    pub fn empty(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.raw(&tag(name, attrs, ""));
    }

    /// Markup already escaped, on its own line.
    pub fn raw(&mut self, markup: &str) {
        self.line();
        self.out.push_str(markup);
    }

    pub fn finish(mut self) -> String {
        while !self.open.is_empty() {
            self.close();
        }
        self.out.push('\n');
        self.out
    }
}

/// An element as markup: `inner` is already escaped; empty, it is `<a/>`.
pub fn tag(name: &str, attrs: &[(&str, &str)], inner: &str) -> String {
    if inner.is_empty() {
        format!("{}/>", start(name, attrs))
    } else {
        format!("{}>{inner}</{name}>", start(name, attrs))
    }
}

fn start(name: &str, attrs: &[(&str, &str)]) -> String {
    let mut s = format!("<{name}");
    for (k, v) in attrs {
        s.push_str(&format!(" {k}=\"{}\"", escape(v)));
    }
    s
}

/// Text or an attribute value with `&`, `<`, `>` and `"` escaped.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_nested_escaped_markup() {
        let mut x = Xml::new("");
        x.open("a", &[("k", "1 < 2")]);
        x.leaf("b", &[], "R&B");
        x.empty("c", &[]);
        let s = x.finish();
        assert_eq!(
            s,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<a k=\"1 &lt; 2\">\n  <b>R&amp;B</b>\n  <c/>\n</a>\n"
        );
        roxmltree::Document::parse(&s).unwrap();
    }
}
