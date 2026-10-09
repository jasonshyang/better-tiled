//! The syntaxes of Tiled's maps, tilesets and templates, and how a file's is found.

use std::path::Path;

/// How a map, tileset or template file is written.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// XML: `.tmx` maps, `.tsx` tilesets, `.tx` templates (and `.xml`).
    Xml,
    /// JSON: `.tmj` maps, `.tsj` tilesets, `.tj` templates (and `.json`).
    Json,
    /// A JSON map wrapped in a JavaScript call (`.js`); maps only.
    JavaScript,
}

impl Format {
    /// The format a file name's extension names (case-insensitively), if any.
    pub fn from_path(path: &Path) -> Option<Format> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "tmx" | "tsx" | "tx" | "xml" => Some(Format::Xml),
            "tmj" | "tsj" | "tj" | "json" => Some(Format::Json),
            "js" => Some(Format::JavaScript),
            _ => None,
        }
    }

    /// The format a file's contents show: a first non-blank `<` is XML, `{` is JSON, and text
    /// with a later line starting with `{` is a JavaScript-wrapped map.
    pub fn from_contents(text: &str) -> Option<Format> {
        let trimmed = text.trim_start_matches('\u{feff}').trim_start();
        match trimmed.chars().next()? {
            '<' => Some(Format::Xml),
            '{' => Some(Format::Json),
            _ if trimmed.contains("\n{") => Some(Format::JavaScript),
            _ => None,
        }
    }

    /// The format of a file: from its contents, else its extension, else XML.
    pub(crate) fn of_file(text: &str, path: &Path) -> Format {
        Format::from_contents(text)
            .or_else(|| Format::from_path(path))
            .unwrap_or(Format::Xml)
    }

    /// The usual extension of maps in this format.
    pub fn map_extension(self) -> &'static str {
        match self {
            Format::Xml => "tmx",
            Format::Json => "tmj",
            Format::JavaScript => "js",
        }
    }

    /// The usual extension of tilesets in this format.
    pub fn tileset_extension(self) -> &'static str {
        match self {
            Format::Xml => "tsx",
            Format::Json | Format::JavaScript => "tsj",
        }
    }

    /// The usual extension of templates in this format.
    pub fn template_extension(self) -> &'static str {
        match self {
            Format::Xml => "tx",
            Format::Json | Format::JavaScript => "tj",
        }
    }
}

/// The JSON inside a JavaScript-wrapped map: from the first line starting with `{`, without
/// the closing `);`, as Tiled's reader takes it.
pub(crate) fn unwrap_javascript(text: &str) -> &str {
    if text.trim_start().starts_with('{') {
        return text;
    }
    let Some(start) = text.find("\n{") else {
        return text;
    };
    let json = text.get(start..).unwrap_or(text).trim();
    let json = json.strip_suffix(';').unwrap_or(json).trim_end();
    json.strip_suffix(')').unwrap_or(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contents_decide_before_extensions() {
        assert_eq!(Format::from_path(Path::new("a/b.TMJ")), Some(Format::Json));
        assert_eq!(Format::from_path(Path::new("b.tx")), Some(Format::Xml));
        assert_eq!(
            Format::from_path(Path::new("b.js")),
            Some(Format::JavaScript)
        );
        assert_eq!(Format::from_path(Path::new("b.png")), None);
        assert_eq!(
            Format::from_contents("\u{feff}  <?xml version=\"1.0\"?>"),
            Some(Format::Xml)
        );
        assert_eq!(
            Format::from_contents("\n { \"type\":\"map\" }"),
            Some(Format::Json)
        );
        assert_eq!(
            Format::from_contents("(function(name,data){\n})(\"m\",\n{ \"a\":1});"),
            Some(Format::JavaScript)
        );
        assert_eq!(Format::from_contents(""), None);
        assert_eq!(Format::of_file("", Path::new("x.tsj")), Format::Json);
        assert_eq!(Format::of_file("{}", Path::new("x.tmx")), Format::Json);
        assert_eq!(Format::of_file("garbage", Path::new("x")), Format::Xml);
    }

    #[test]
    fn javascript_wrappers_come_off() {
        assert_eq!(
            unwrap_javascript("(function(){\n})(\"m\",\n{ \"a\":1});\n"),
            "{ \"a\":1}"
        );
        assert_eq!(unwrap_javascript("{}"), "{}");
    }
}
