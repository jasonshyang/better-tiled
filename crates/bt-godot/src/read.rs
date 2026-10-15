//! Reads Godot's text resources (`.tscn` and `.tres`) far enough to see what a file holds: the
//! `;` comment lines before its header, the header, and every section with its attributes and
//! `key = value` lines, values kept as written. Helpers read the few kinds of value the export
//! looks at (strings, numbers, `ExtResource("…")`, `SubResource("…")`, `PackedInt32Array(…)`);
//! there is no full Variant parser. Also the FNV-1a hash and Godot's uid text, which the
//! export's header and derived uids use.

// The contract reads files back with this; a few readers (the format, editable paths,
// sub-resource values) wait for the level-scene check and the shared TileSet.
#![cfg_attr(not(test), allow(dead_code))]

use std::borrow::Cow;
use std::ops::Range;

/// A text resource as read. Everything borrows from the text.
#[derive(Debug)]
pub struct Resource<'t> {
    /// The `;` lines before the header, without the `;`.
    pub comments: Vec<&'t str>,
    /// `[gd_scene …]` or `[gd_resource …]`. Its span starts where the export's hash starts.
    pub header: Section<'t>,
    /// Every section after the header, in order.
    pub sections: Vec<Section<'t>>,
}

/// A `[kind attr=value …]` tag and the `key = value` lines under it.
#[derive(Debug)]
pub struct Section<'t> {
    /// `gd_scene`, `gd_resource`, `ext_resource`, `sub_resource`, `resource`, `node`,
    /// `connection`, `editable`, …
    pub kind: &'t str,
    /// The tag's attributes in order, values as written.
    pub attrs: Vec<(&'t str, &'t str)>,
    /// The `key = value` lines in order, values as written (a value can span lines).
    pub props: Vec<(Cow<'t, str>, &'t str)>,
    /// The tag's line, from 1.
    pub line: usize,
    /// The section's bytes, from its `[` through the end of its last line.
    pub span: Range<usize>,
}

/// A `[node …]` tag.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Node {
    pub name: String,
    /// `type`, which Godot leaves out on instances and on nodes an instanced or inherited scene
    /// already has.
    pub class: Option<String>,
    /// The parent's path from the root (`.` for the root's children); none on the root.
    pub parent: Option<String>,
    /// The id of the `ext_resource` the node instances.
    pub instance: Option<String>,
    pub instance_placeholder: Option<String>,
    pub owner: Option<String>,
    /// Godot 4.6 and later.
    pub unique_id: Option<i32>,
    /// Godot 4.6 and later, on nodes under an instanced or inherited node: the `unique_id`s from
    /// below the root down to the parent, which find the parent again after a rename.
    pub parent_id_path: Vec<i32>,
    pub index: Option<i32>,
    pub groups: Vec<String>,
}

/// A `[connection …]` tag. `binds` and `unbinds` stay in the section's attributes.
#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub signal: String,
    pub from: String,
    pub to: String,
    pub method: String,
    pub flags: Option<i64>,
}

/// An `[ext_resource …]` tag.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtResource {
    /// `type`.
    pub class: String,
    pub path: String,
    pub uid: Option<String>,
    pub id: String,
}

/// A `[sub_resource …]` tag.
#[derive(Debug, Clone, PartialEq)]
pub struct SubResource {
    /// `type`.
    pub class: String,
    pub id: String,
}

/// Reads a whole text resource. A file Godot couldn't load either (an unclosed string or
/// bracket, a node without a name, …) is an error naming its line.
pub fn parse(text: &str) -> Result<Resource<'_>, String> {
    let mut reader = Reader::new(text);
    let mut resource = reader.head()?;
    while let Some(section) = reader.section()? {
        check(&section)?;
        resource.sections.push(section);
    }
    Ok(resource)
}

/// Reads only the comments and the header, leaving `sections` empty: enough for a file's uid or
/// the export's header without reading the rest.
pub fn parse_header(text: &str) -> Result<Resource<'_>, String> {
    Reader::new(text).head()
}

impl<'t> Resource<'t> {
    pub fn format(&self) -> Option<i64> {
        self.header.attr("format").and_then(int)
    }

    pub fn load_steps(&self) -> Option<i64> {
        self.header.attr("load_steps").and_then(int)
    }

    /// The uid text (`uid://…`).
    pub fn uid(&self) -> Option<String> {
        self.header.attr("uid").and_then(string)
    }

    /// The class of a `.tres` (`[gd_resource type=…]`).
    pub fn class(&self) -> Option<String> {
        self.header.attr("type").and_then(string)
    }

    pub fn nodes(&self) -> impl Iterator<Item = (Node, &Section<'t>)> {
        self.sections.iter().filter_map(|s| Some((s.node()?, s)))
    }

    /// The node without a parent.
    pub fn root(&self) -> Option<(Node, &Section<'t>)> {
        self.nodes().find(|(node, _)| node.parent.is_none())
    }

    pub fn connections(&self) -> impl Iterator<Item = Connection> {
        self.sections.iter().filter_map(Section::connection)
    }

    /// The paths of the `[editable]` sections: instances whose children the scene may change.
    pub fn editable(&self) -> impl Iterator<Item = String> {
        self.sections.iter().filter_map(Section::editable)
    }

    pub fn ext_resources(&self) -> impl Iterator<Item = ExtResource> {
        self.sections.iter().filter_map(Section::ext_resource)
    }

    pub fn sub_resources(&self) -> impl Iterator<Item = (SubResource, &Section<'t>)> {
        self.sections
            .iter()
            .filter_map(|s| Some((s.sub_resource()?, s)))
    }

    /// The `[resource]` section of a `.tres`.
    pub fn resource(&self) -> Option<&Section<'t>> {
        self.sections.iter().find(|s| s.kind == "resource")
    }
}

impl<'t> Section<'t> {
    pub fn attr(&self, key: &str) -> Option<&'t str> {
        self.attrs.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }

    /// A property's value as written; the last one when a key repeats, as Godot reads it.
    pub fn get(&self, key: &str) -> Option<&'t str> {
        self.props
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| *v)
    }

    /// A property's value as written, the first one when a key repeats: the export writes its
    /// own keys before a node's properties, which in scenes exported before they got keys of
    /// their own could repeat one (names.rs).
    pub fn first(&self, key: &str) -> Option<&'t str> {
        self.props.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
    }

    // A parsed resource has had every typed section checked, so these only fail on kind.
    pub fn node(&self) -> Option<Node> {
        (self.kind == "node")
            .then(|| Node::read(self).ok())
            .flatten()
    }

    pub fn connection(&self) -> Option<Connection> {
        (self.kind == "connection")
            .then(|| Connection::read(self).ok())
            .flatten()
    }

    pub fn editable(&self) -> Option<String> {
        (self.kind == "editable")
            .then(|| required(self, "path", string, "a string").ok())
            .flatten()
    }

    pub fn ext_resource(&self) -> Option<ExtResource> {
        (self.kind == "ext_resource")
            .then(|| ExtResource::read(self).ok())
            .flatten()
    }

    pub fn sub_resource(&self) -> Option<SubResource> {
        (self.kind == "sub_resource")
            .then(|| SubResource::read(self).ok())
            .flatten()
    }
}

impl Node {
    /// The node's path from the root, `.` for the root itself: what `parent` says for its
    /// children and what node paths start from.
    pub fn path(&self) -> String {
        match self.parent.as_deref() {
            None => ".".to_owned(),
            Some(".") => self.name.clone(),
            Some(parent) => format!("{parent}/{}", self.name),
        }
    }

    fn read(section: &Section) -> Result<Node, String> {
        let mut node = Node {
            name: required(section, "name", string, "a string")?,
            ..Node::default()
        };
        for &(key, raw) in &section.attrs {
            let text = || attribute(section, key, raw, string, "a string");
            match key {
                "type" => node.class = Some(text()?),
                "parent" => node.parent = Some(text()?),
                "owner" => node.owner = Some(text()?),
                "instance" => {
                    node.instance = Some(attribute(
                        section,
                        key,
                        raw,
                        ext_resource,
                        "an ExtResource",
                    )?)
                }
                "instance_placeholder" => node.instance_placeholder = Some(text()?),
                "unique_id" => {
                    node.unique_id = Some(attribute(section, key, raw, int32, "a 32-bit number")?)
                }
                "parent_id_path" => {
                    node.parent_id_path =
                        attribute(section, key, raw, packed_int32s, "a PackedInt32Array")?
                }
                // Godot writes the index as a string.
                "index" => {
                    node.index = Some(attribute(
                        section,
                        key,
                        raw,
                        |raw| string(raw).as_deref().map_or(int32(raw), int32),
                        "a number",
                    )?)
                }
                "groups" => {
                    node.groups = attribute(section, key, raw, strings, "a list of strings")?
                }
                _ => {}
            }
        }
        Ok(node)
    }
}

impl Connection {
    fn read(section: &Section) -> Result<Connection, String> {
        let text = |key| required(section, key, string, "a string");
        let flags = section
            .attr("flags")
            .map(|raw| attribute(section, "flags", raw, int, "a number"))
            .transpose()?;
        Ok(Connection {
            signal: text("signal")?,
            from: text("from")?,
            to: text("to")?,
            method: text("method")?,
            flags,
        })
    }
}

impl ExtResource {
    fn read(section: &Section) -> Result<ExtResource, String> {
        let uid = section
            .attr("uid")
            .map(|raw| attribute(section, "uid", raw, string, "a string"))
            .transpose()?;
        Ok(ExtResource {
            class: required(section, "type", string, "a string")?,
            path: required(section, "path", string, "a string")?,
            uid,
            id: required(section, "id", id, "an id")?,
        })
    }
}

impl SubResource {
    fn read(section: &Section) -> Result<SubResource, String> {
        Ok(SubResource {
            class: required(section, "type", string, "a string")?,
            id: required(section, "id", id, "an id")?,
        })
    }
}

/// Checks a typed section the way Godot would when loading it.
fn check(section: &Section) -> Result<(), String> {
    match section.kind {
        "node" => Node::read(section).map(drop),
        "connection" => Connection::read(section).map(drop),
        "editable" => required(section, "path", string, "a string").map(drop),
        "ext_resource" => ExtResource::read(section).map(drop),
        "sub_resource" => SubResource::read(section).map(drop),
        _ => Ok(()),
    }
}

fn attribute<T>(
    section: &Section,
    key: &str,
    raw: &str,
    read: impl FnOnce(&str) -> Option<T>,
    what: &str,
) -> Result<T, String> {
    read(raw).ok_or_else(|| {
        format!(
            "line {}: {key}={raw} in [{}] isn't {what}",
            section.line, section.kind
        )
    })
}

fn required<T>(
    section: &Section,
    key: &str,
    read: impl FnOnce(&str) -> Option<T>,
    what: &str,
) -> Result<T, String> {
    let raw = section
        .attr(key)
        .ok_or_else(|| format!("line {}: [{}] has no {key}", section.line, section.kind))?;
    attribute(section, key, raw, read, what)
}

/// A string literal, unescaped the way Godot reads it: `\n`, `\t`, `\r`, `\b`, `\f`, `\uXXXX`
/// (UTF-16, pairs included), `\UXXXXXX`, and any other character after `\` as itself. A
/// StringName (`&"…"`) counts as a string. None for anything else.
pub fn string(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let raw = raw.strip_prefix('&').unwrap_or(raw);
    if !raw.starts_with('"') || string_end(raw.as_bytes(), 0)? != raw.len() {
        return None;
    }
    let mut out = String::with_capacity(raw.len() - 2);
    let mut chars = raw[1..raw.len() - 1].chars();
    // A UTF-16 lead surrogate waiting for its trail.
    let mut lead: Option<u32> = None;
    while let Some(c) = chars.next() {
        let code = if c == '\\' {
            match chars.next()? {
                'b' => 8,
                't' => 9,
                'n' => 10,
                'f' => 12,
                'r' => 13,
                u @ ('u' | 'U') => {
                    let hex: String = chars.by_ref().take(if u == 'u' { 4 } else { 6 }).collect();
                    if hex.len() != if u == 'u' { 4 } else { 6 }
                        || !hex.bytes().all(|b| b.is_ascii_hexdigit())
                    {
                        return None;
                    }
                    u32::from_str_radix(&hex, 16).ok()?
                }
                other => other as u32,
            }
        } else {
            c as u32
        };
        match (code, lead) {
            (0xd800..=0xdbff, None) => lead = Some(code),
            (0xdc00..=0xdfff, Some(high)) => {
                out.push(char::from_u32(
                    0x10000 + ((high - 0xd800) << 10) + (code - 0xdc00),
                )?);
                lead = None;
            }
            (_, None) => out.push(char::from_u32(code)?),
            (_, Some(_)) => return None,
        }
    }
    lead.is_none().then_some(out)
}

pub fn int(raw: &str) -> Option<i64> {
    raw.trim().parse().ok()
}

fn int32(raw: &str) -> Option<i32> {
    raw.trim().parse().ok()
}

/// The id in `ExtResource("id")`.
pub fn ext_resource(raw: &str) -> Option<String> {
    call(raw, "ExtResource").and_then(id)
}

/// The id in `SubResource("id")`.
pub fn sub_resource(raw: &str) -> Option<String> {
    call(raw, "SubResource").and_then(id)
}

pub fn packed_int32s(raw: &str) -> Option<Vec<i32>> {
    items(call(raw, "PackedInt32Array")?)?
        .into_iter()
        .map(int32)
        .collect()
}

/// `["a", "b"]` or `PackedStringArray("a", "b")`.
pub fn strings(raw: &str) -> Option<Vec<String>> {
    let raw = raw.trim();
    let inner =
        call(raw, "PackedStringArray").or_else(|| raw.strip_prefix('[')?.strip_suffix(']'))?;
    items(inner)?.into_iter().map(string).collect()
}

/// A resource id: a string, or a number in files from Godot 3.
fn id(raw: &str) -> Option<String> {
    string(raw).or_else(|| Some(int(raw)?.to_string()))
}

/// The arguments of `name(…)`.
fn call<'r>(raw: &'r str, name: &str) -> Option<&'r str> {
    raw.trim()
        .strip_prefix(name)?
        .trim_start()
        .strip_prefix('(')?
        .strip_suffix(')')
}

/// The items of a comma-separated list, split outside strings and brackets.
fn items(list: &str) -> Option<Vec<&str>> {
    let bytes = list.as_bytes();
    let (mut out, mut depth, mut start, mut i) = (Vec::new(), 0usize, 0, 0);
    while i < bytes.len() {
        match *bytes.get(i)? {
            b'"' => {
                i = string_end(bytes, i)?;
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.checked_sub(1)?,
            b',' if depth == 0 => {
                out.push(list[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    let last = list[start..].trim();
    if !last.is_empty() || !out.is_empty() {
        out.push(last);
    }
    (depth == 0 && out.iter().all(|item| !item.is_empty())).then_some(out)
}

/// Where the string literal opening at `open` ends: just past its closing quote.
fn string_end(bytes: &[u8], open: usize) -> Option<usize> {
    let mut i = open + 1;
    loop {
        i += bytes
            .get(i..)?
            .iter()
            .position(|&b| b == b'"' || b == b'\\')?;
        if bytes.get(i) == Some(&b'"') {
            return Some(i + 1);
        }
        i += 2;
    }
}

/// FNV-1a, 64 bits: the export's check that a file is as it wrote it, and the seed of the uids
/// it derives.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bt_utils::hash::fnv1a_64(bytes)
}

/// FNV-1a, 32 bits: the seed of the `unique_id`s the export derives for nodes that have no Tiled
/// id of their own (names.rs).
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    bt_utils::hash::fnv1a_32(bytes)
}

/// The digits of a uid: Godot counts 25 letters (`'z' - 'a'`) and 9 digits (`'9' - '0'`).
const UID_BASE: u64 = 34;
const UID_LETTERS: u8 = 25;

/// A uid as Godot writes it (`ResourceUID::id_to_text`): `uid://` and the id in base 34, digits
/// a–y then 0–8, most significant first.
pub fn uid_to_text(id: u64) -> String {
    let mut digits = Vec::new();
    let mut rest = id;
    loop {
        let d = (rest % UID_BASE) as u8;
        digits.push(if d < UID_LETTERS {
            b'a' + d
        } else {
            b'0' + d - UID_LETTERS
        });
        rest /= UID_BASE;
        if rest == 0 {
            break;
        }
    }
    digits.reverse();
    format!("uid://{}", String::from_utf8_lossy(&digits))
}

/// The id of a uid's text, read as `ResourceUID::text_to_id` reads it (which also takes the z and
/// 9 its own text never holds, overflowing like it): None where Godot finds no id.
pub fn text_to_uid(text: &str) -> Option<u64> {
    let digits = text.strip_prefix("uid://").filter(|d| !d.is_empty())?;
    let mut id: u64 = 0;
    for c in digits.bytes() {
        let d = match c {
            b'a'..=b'z' => c - b'a',
            b'0'..=b'9' => c - b'0' + UID_LETTERS,
            _ => return None,
        };
        id = id.wrapping_mul(UID_BASE).wrapping_add(d as u64);
    }
    Some(id & (u64::MAX >> 1))
}

type Attrs<'t> = Vec<(&'t str, &'t str)>;

/// Walks the text. Positions only point at ASCII delimiters, so slices stay on char boundaries.
struct Reader<'t> {
    text: &'t str,
    pos: usize,
    /// The lines counted up to `counted`, so that section lines come cheaply.
    line: usize,
    counted: usize,
}

impl<'t> Reader<'t> {
    fn new(text: &'t str) -> Self {
        let pos = if text.starts_with('\u{feff}') { 3 } else { 0 };
        Reader {
            text,
            pos,
            line: 1,
            counted: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn line_at(&mut self, pos: usize) -> usize {
        let count = |s: &str| s.bytes().filter(|&b| b == b'\n').count();
        if pos < self.counted {
            return 1 + count(&self.text[..pos]);
        }
        self.line += count(&self.text[self.counted..pos]);
        self.counted = pos;
        self.line
    }

    fn error(&mut self, pos: usize, what: &str) -> String {
        format!("line {}: {what}", self.line_at(pos))
    }

    fn line_end(&self) -> usize {
        self.text[self.pos..]
            .find('\n')
            .map_or(self.text.len(), |i| self.pos + i)
    }

    fn skip_while(&mut self, f: impl Fn(u8) -> bool) {
        while self.peek().is_some_and(&f) {
            self.pos += 1;
        }
    }

    fn word(&mut self) -> &'t str {
        let start = self.pos;
        self.skip_while(|b| b.is_ascii_alphanumeric() || b == b'_');
        &self.text[start..self.pos]
    }

    fn skip_space_and_comments(&mut self) {
        loop {
            self.skip_while(|b| b.is_ascii_whitespace());
            if self.peek() != Some(b';') {
                return;
            }
            self.pos = self.line_end();
        }
    }

    /// Steps past the rest of the line (blanks and a comment) and returns where it ends; stops
    /// short if something else follows on the line.
    fn finish_line(&mut self) -> usize {
        self.skip_while(|b| b == b' ' || b == b'\t' || b == b'\r');
        if self.peek() == Some(b';') {
            self.pos = self.line_end();
        }
        if self.peek() == Some(b'\n') {
            self.pos += 1;
        }
        self.pos
    }

    /// The comments and the header.
    fn head(&mut self) -> Result<Resource<'t>, String> {
        let mut comments = Vec::new();
        loop {
            self.skip_while(|b| b.is_ascii_whitespace());
            match self.peek() {
                Some(b';') => {
                    let end = self.line_end();
                    comments.push(self.text[self.pos + 1..end].trim_end_matches('\r'));
                    self.pos = end;
                }
                Some(b'[') => break,
                None => return Err("the file has no [gd_scene] or [gd_resource] header".to_owned()),
                Some(_) => {
                    return Err(
                        self.error(self.pos, "expected the [gd_scene] or [gd_resource] header")
                    );
                }
            }
        }
        let (start, line) = (self.pos, self.line_at(self.pos));
        let (kind, attrs) = self.tag()?;
        if kind != "gd_scene" && kind != "gd_resource" {
            return Err(format!(
                "line {line}: the file starts with [{kind}], not [gd_scene] or [gd_resource]"
            ));
        }
        let end = self.finish_line();
        Ok(Resource {
            comments,
            header: Section {
                kind,
                attrs,
                props: Vec::new(),
                line,
                span: start..end,
            },
            sections: Vec::new(),
        })
    }

    /// The next section, if any.
    fn section(&mut self) -> Result<Option<Section<'t>>, String> {
        self.skip_space_and_comments();
        match self.peek() {
            None => return Ok(None),
            Some(b'[') => {}
            Some(_) => return Err(self.error(self.pos, "a value before the first section")),
        }
        let (start, line) = (self.pos, self.line_at(self.pos));
        let (kind, attrs) = self.tag()?;
        let mut end = self.finish_line();
        let mut props = Vec::new();
        loop {
            self.skip_space_and_comments();
            if matches!(self.peek(), None | Some(b'[')) {
                break;
            }
            let key = self.key()?;
            props.push((key, self.value(false)?));
            end = self.finish_line();
        }
        Ok(Some(Section {
            kind,
            attrs,
            props,
            line,
            span: start..end,
        }))
    }

    /// `[kind attr=value …]`.
    fn tag(&mut self) -> Result<(&'t str, Attrs<'t>), String> {
        let start = self.pos;
        self.pos += 1;
        let kind = self.word();
        if kind.is_empty() {
            return Err(self.error(start, "a section without a name"));
        }
        let mut attrs = Vec::new();
        loop {
            self.skip_while(|b| b.is_ascii_whitespace());
            match self.peek() {
                Some(b']') => {
                    self.pos += 1;
                    return Ok((kind, attrs));
                }
                None => return Err(self.error(start, &format!("[{kind} never closes"))),
                Some(_) => {}
            }
            let key_start = self.pos;
            let key = self.word();
            if key.is_empty() {
                let c = self.text[self.pos..].chars().next().unwrap_or(' ');
                return Err(self.error(key_start, &format!("unexpected '{c}' in [{kind}]")));
            }
            self.skip_while(|b| b == b' ' || b == b'\t');
            if self.peek() != Some(b'=') {
                return Err(self.error(key_start, &format!("{key} in [{kind}] has no value")));
            }
            self.pos += 1;
            attrs.push((key, self.value(true)?));
        }
    }

    /// The key of a `key = value` line, past its `=`. Godot quotes keys with spaces and the like.
    fn key(&mut self) -> Result<Cow<'t, str>, String> {
        let start = self.pos;
        let key = if self.peek() == Some(b'"') {
            self.pos = string_end(self.text.as_bytes(), start)
                .ok_or_else(|| self.error(start, "a string that never ends"))?;
            let key = string(&self.text[start..self.pos])
                .ok_or_else(|| self.error(start, "a key that isn't a valid string"))?;
            self.skip_while(|b| b == b' ' || b == b'\t');
            if self.peek() != Some(b'=') {
                return Err(self.error(start, "expected key = value"));
            }
            Cow::Owned(key)
        } else {
            let rest = &self.text[start..self.line_end()];
            let eq = rest
                .find('=')
                .ok_or_else(|| self.error(start, "expected key = value"))?;
            self.pos = start + eq;
            Cow::Borrowed(rest[..eq].trim())
        };
        self.pos += 1;
        Ok(key)
    }

    /// A value as written, after its `=`: to the end of the line (in a tag, to the next blank
    /// or the `]`), or further while a string or bracket is open.
    fn value(&mut self, in_tag: bool) -> Result<&'t str, String> {
        self.skip_while(|b| b == b' ' || b == b'\t');
        let start = self.pos;
        let bytes = self.text.as_bytes();
        let mut depth = 0usize;
        while let Some(&b) = bytes.get(self.pos) {
            match b {
                b'"' => {
                    self.pos = string_end(bytes, self.pos)
                        .ok_or_else(|| self.error(self.pos, "a string that never ends"))?;
                    continue;
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' if depth > 0 => depth -= 1,
                b']' if in_tag => break,
                b')' | b']' | b'}' => {
                    return Err(self.error(self.pos, &format!("an unmatched '{}'", b as char)));
                }
                // A comment inside a bracket.
                b';' if depth > 0 => {
                    self.pos = self.line_end();
                    continue;
                }
                b';' | b'\n' if depth == 0 => break,
                b' ' | b'\t' | b'\r' if depth == 0 && in_tag => break,
                _ => {}
            }
            self.pos += 1;
        }
        if depth > 0 {
            return Err(self.error(start, "a value whose brackets never close"));
        }
        let raw = self.text[start..self.pos].trim_end();
        if raw.is_empty() {
            return Err(self.error(start, "a missing value"));
        }
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Our own export, with the header the export writes from docs/design/12-godot-export.md on.
    const OURS: &str = r#"; Generated by better-tiled from ../maps/features.tmx. Edit the map, or a scene that
; inherits this one: changes made here are replaced by the next export.
; better-tiled-export 1 fnv1a64=9c1e04d2a7b3f051
[gd_scene format=3 uid="uid://cyncsb"]

[ext_resource type="Texture2D" path="res://maps/tiles.png" id="1"]

[sub_resource type="TileSetAtlasSource" id="TileSetAtlasSource_0"]
texture = ExtResource("1")
texture_region_size = Vector2i(32, 32)
0:0/0 = 0
1:0/0 = 0

[sub_resource type="TileSetAtlasSource" id="TileSetAtlasSource_1"]
texture = ExtResource("1")
texture_region_size = Vector2i(32, 32)
0:0/animation_frames_count = 2
0:0/animation_frame_0/duration = 0.1
0:0/animation_frame_1/duration = 0.25
0:0/0 = 0

[sub_resource type="TileSet" id="TileSet_0"]
tile_size = Vector2i(32, 32)
sources/0 = SubResource("TileSetAtlasSource_0")
sources/1 = SubResource("TileSetAtlasSource_1")

[sub_resource type="RectangleShape2D" id="RectangleShape2D_2"]
size = Vector2(64, 32)

[node name="features" type="Node2D" unique_id=1]
texture_filter = 1
metadata/bt_source = "../maps/features.tmx"
metadata/intent = "a test room"

[node name="cells" type="TileMapLayer" parent="." unique_id=1073741825]
tile_map_data = PackedByteArray(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
tile_set = SubResource("TileSet_0")
metadata/layer_id = 1

[node name="objects" type="Node2D" parent="." unique_id=1073741830]
y_sort_enabled = true
metadata/layer_id = 6

[node name="crate" type="Sprite2D" parent="objects" unique_id=536870913 groups=["Box"]]
position = Vector2(16, 80)
metadata/target = NodePath("../spawn")
metadata/class = "Box"
metadata/id = 1

[node name="zone" type="Area2D" parent="objects" unique_id=536870915]
position = Vector2(64, 0)
rotation = 0.5235987755982988
metadata/id = 3

[node name="Shape" type="CollisionShape2D" parent="objects/zone"]
position = Vector2(32, 16)
shape = SubResource("RectangleShape2D_2")

[node name="sign" type="Label" parent="objects" unique_id=536870918]
text = "say \"hi\"\\\n"
metadata/id = 6
"#;

    #[test]
    fn our_own_export_reads_back() {
        let r = parse(OURS).unwrap();
        assert_eq!(
            r.comments[2],
            " better-tiled-export 1 fnv1a64=9c1e04d2a7b3f051"
        );
        assert_eq!(r.comments.len(), 3);
        assert_eq!(
            (
                r.header.kind,
                r.format(),
                r.uid().as_deref(),
                r.load_steps(),
                r.class()
            ),
            ("gd_scene", Some(3), Some("uid://cyncsb"), None, None)
        );
        assert!(
            OURS[r.header.span.start..].starts_with("[gd_scene format=3"),
            "the hash starts at the header"
        );
        assert_eq!(
            &OURS[r.header.span.clone()],
            "[gd_scene format=3 uid=\"uid://cyncsb\"]\n"
        );
        assert_eq!(
            r.ext_resources().collect::<Vec<_>>(),
            [ExtResource {
                class: "Texture2D".into(),
                path: "res://maps/tiles.png".into(),
                uid: None,
                id: "1".into()
            }]
        );

        let subs: Vec<_> = r
            .sub_resources()
            .map(|(sub, s)| (sub.id, s.props.len()))
            .collect();
        assert_eq!(
            subs,
            [
                ("TileSetAtlasSource_0".into(), 4),
                ("TileSetAtlasSource_1".into(), 6),
                ("TileSet_0".into(), 3),
                ("RectangleShape2D_2".into(), 1)
            ]
        );
        let (_, tileset) = r
            .sub_resources()
            .find(|(sub, _)| sub.class == "TileSet")
            .unwrap();
        assert_eq!(
            sub_resource(tileset.get("sources/1").unwrap()).as_deref(),
            Some("TileSetAtlasSource_1")
        );
        assert_eq!(
            r.sections[2].get("0:0/animation_frame_1/duration"),
            Some("0.25")
        );
        assert_eq!(
            ext_resource(r.sections[1].get("texture").unwrap()).as_deref(),
            Some("1")
        );
        assert_eq!(
            &OURS[r.sections[4].span.clone()],
            "[sub_resource type=\"RectangleShape2D\" id=\"RectangleShape2D_2\"]\nsize = Vector2(64, 32)\n"
        );

        let nodes: Vec<(String, Option<i32>)> =
            r.nodes().map(|(n, _)| (n.path(), n.unique_id)).collect();
        assert_eq!(
            nodes,
            [
                (".".into(), Some(1)),
                ("cells".into(), Some(1073741825)),
                ("objects".into(), Some(1073741830)),
                ("objects/crate".into(), Some(536870913)),
                ("objects/zone".into(), Some(536870915)),
                ("objects/zone/Shape".into(), None),
                ("objects/sign".into(), Some(536870918))
            ]
        );
        let (root, root_section) = r.root().unwrap();
        assert_eq!(
            (root.name.as_str(), root.class.as_deref()),
            ("features", Some("Node2D"))
        );
        assert_eq!(
            root_section
                .get("metadata/bt_source")
                .and_then(string)
                .as_deref(),
            Some("../maps/features.tmx")
        );
        let (crate_, section) = r.nodes().find(|(n, _)| n.name == "crate").unwrap();
        assert_eq!(
            (crate_.groups, crate_.parent.as_deref()),
            (vec!["Box".to_owned()], Some("objects"))
        );
        assert_eq!(
            (
                section.get("metadata/target"),
                section.get("metadata/id").and_then(int)
            ),
            (Some("NodePath(\"../spawn\")"), Some(1))
        );
        let (_, sign) = r.nodes().find(|(n, _)| n.name == "sign").unwrap();
        assert_eq!(
            sign.get("text").and_then(string).as_deref(),
            Some("say \"hi\"\\\n")
        );
        assert_eq!(r.connections().count() + r.editable().count(), 0);
        assert!(r.resource().is_none());
    }

    /// Godot 4.7.2's re-save of a level scene that inherits an export, painted and edited in
    /// Godot (copied as written).
    const RESAVED: &str = r#"[gd_scene format=4]

[ext_resource type="PackedScene" path="res://desert.tscn" id="1_dcxic"]
[ext_resource type="Script" path="res://door.gd" id="2_oanav"]

[node name="Level" unique_id=1174528209 instance=ExtResource("1_dcxic")]

[node name="Ground" parent="." index="0" unique_id=1073741825]
tile_map_data = PackedByteArray("AAAAAAAAAAAAAAAAAAABAAAAAAABAAAAAAACAAAAAAAAAAAAAAADAAAAAAABAAAAAAAEAAAAAAAAAAAAAAAFAAAAAAABAAAAAAA=")

[node name="Door" parent="Objects" parent_id_path=PackedInt32Array(1073741826) index="0" unique_id=536870919]
position = Vector2(40, 16)
script = ExtResource("2_oanav")
message = "Locked.
Find the \"key\"."

[node name="Shape" parent="Objects/Door" index="0"]
disabled = true

[node name="Player" type="Node2D" parent="Objects" parent_id_path=PackedInt32Array(1073741826) index="1" unique_id=228638984 groups=["players"]]
position = Vector2(64, 64)

[connection signal="body_entered" from="Objects/Door" to="." method="queue_free"]
"#;

    #[test]
    fn godot_resaves_read() {
        let r = parse(RESAVED).unwrap();
        assert_eq!((r.format(), r.uid(), r.comments.len()), (Some(4), None, 0));
        let (root, _) = r.root().unwrap();
        assert_eq!(
            (
                root.name.as_str(),
                root.class,
                root.instance.as_deref(),
                root.unique_id
            ),
            ("Level", None, Some("1_dcxic"), Some(1174528209))
        );
        let ext: Vec<_> = r.ext_resources().map(|e| (e.class, e.id)).collect();
        assert_eq!(
            ext,
            [
                ("PackedScene".into(), "1_dcxic".into()),
                ("Script".into(), "2_oanav".into())
            ]
        );

        let (ground, section) = r.nodes().find(|(n, _)| n.name == "Ground").unwrap();
        assert_eq!(
            (
                ground.class,
                ground.index,
                ground.unique_id,
                ground.parent_id_path
            ),
            (None, Some(0), Some(1073741825), vec![])
        );
        assert!(
            section
                .get("tile_map_data")
                .unwrap()
                .starts_with("PackedByteArray(\"AAAAAAAAAAAAAAAAAAAB")
                && section.get("tile_map_data").unwrap().ends_with("AAA=\")")
        );
        let (door, section) = r.nodes().find(|(n, _)| n.name == "Door").unwrap();
        assert_eq!(
            (door.path(), door.parent_id_path, door.unique_id),
            ("Objects/Door".into(), vec![1073741826], Some(536870919))
        );
        assert_eq!(
            section.get("script").and_then(ext_resource).as_deref(),
            Some("2_oanav")
        );
        assert_eq!(
            section.get("message").and_then(string).as_deref(),
            Some("Locked.\nFind the \"key\"."),
            "a string over two lines"
        );
        assert_eq!(section.props.len(), 3);
        let (shape, _) = r.nodes().find(|(n, _)| n.name == "Shape").unwrap();
        assert_eq!(
            (shape.unique_id, shape.index),
            (None, Some(0)),
            "Godot gives a node of the base no id of its own"
        );
        let (player, _) = r.nodes().find(|(n, _)| n.name == "Player").unwrap();
        assert_eq!(
            (player.class.as_deref(), player.groups, player.index),
            (Some("Node2D"), vec!["players".to_owned()], Some(1))
        );
        assert_eq!(
            r.connections().collect::<Vec<_>>(),
            [Connection {
                signal: "body_entered".into(),
                from: "Objects/Door".into(),
                to: ".".into(),
                method: "queue_free".into(),
                flags: None
            }]
        );
    }

    /// Values Godot 4.7.2 wrote for a node it was given metadata of every kind, and connections
    /// with binds and flags (copied as written; the string holds a tab).
    const GODOT_VALUES: &str = "[gd_scene format=3]

[node name=\"Level\" type=\"Node2D\" unique_id=1869977825]

[node name=\"Added _q_\" type=\"Node2D\" parent=\".\" unique_id=1817809565 groups=[\"b group\", \"enemies\"]]
metadata/note = \"two
lines \\\\ \\\"x\\\" é \t tab ; semi [bracket\"
metadata/dict = {
\"k\": [1, 2],
\"s\": \"v\"
}
metadata/bytes = PackedByteArray(1, 2, 3, 250)
metadata/sn = &\"name\"
metadata/np = NodePath(\"../Map\")
metadata/big = 1e+300
metadata/arr = Array[Vector2]([Vector2(1, 2)])

[node name=\"T\" type=\"Timer\" parent=\"Added _q_\" unique_id=1144672780]

[connection signal=\"body_entered\" from=\"Map/Objects/Door\" to=\"Added _q_\" method=\"set_meta\" binds= [\"hit\", 1]]
[connection signal=\"timeout\" from=\"Added _q_/T\" to=\"Added _q_\" method=\"queue_free\" flags=3]
";

    #[test]
    fn values_over_several_lines_stay_whole() {
        let r = parse(GODOT_VALUES).unwrap();
        let (added, section) = r.nodes().find(|(n, _)| n.name == "Added _q_").unwrap();
        assert_eq!(added.groups, ["b group", "enemies"]);
        let keys: Vec<&str> = section.props.iter().map(|(k, _)| k.as_ref()).collect();
        assert_eq!(
            keys,
            [
                "metadata/note",
                "metadata/dict",
                "metadata/bytes",
                "metadata/sn",
                "metadata/np",
                "metadata/big",
                "metadata/arr"
            ]
        );
        assert_eq!(
            section.get("metadata/note").and_then(string).as_deref(),
            Some("two\nlines \\ \"x\" é \t tab ; semi [bracket")
        );
        assert_eq!(
            section.get("metadata/dict"),
            Some("{\n\"k\": [1, 2],\n\"s\": \"v\"\n}")
        );
        assert_eq!(
            section.get("metadata/sn").and_then(string).as_deref(),
            Some("name")
        );
        assert_eq!(
            section.get("metadata/arr"),
            Some("Array[Vector2]([Vector2(1, 2)])")
        );
        let connections: Vec<_> = r.connections().map(|c| (c.method, c.flags)).collect();
        assert_eq!(
            connections,
            [("set_meta".into(), None), ("queue_free".into(), Some(3))]
        );
        assert_eq!(r.sections.last().unwrap().attr("flags"), Some("3"));
        let binds = r
            .sections
            .iter()
            .find(|s| s.kind == "connection")
            .unwrap()
            .attr("binds");
        assert_eq!(
            binds,
            Some("[\"hit\", 1]"),
            "Godot writes a blank after binds="
        );
    }

    /// Tiled 1.12's TileMap export (format = 2), shortened.
    const TILED: &str = r#"[gd_scene load_steps=4 format=3]


[ext_resource type="Texture2D" path="res://examples/hexmini.png" id="hex mini"]

[sub_resource type="TileSetAtlasSource" id="TileSetAtlasSource_0"]
texture = ExtResource("hex mini")
texture_region_size = Vector2i(18, 18)
0:0/0 = 0
1:0/0 = 0

[sub_resource type="TileSet" id="TileSet_0"]
tile_shape = 3
tile_layout = 5
tile_size = Vector2i(14, 12)
sources/0 = SubResource("TileSetAtlasSource_0")

[node name="hexagonal-mini" type="Node2D"]

[node name="TileMap" type="TileMap" parent="."]
tile_set = SubResource("TileSet_0")
format = 2
layer_0/name = "Ground"
layer_0/tile_data = PackedInt32Array(0, 65536, 1, 1, 0, 0, 65538, 131072, 0)
layer_1/name = "Empty"
layer_1/tile_data = PackedInt32Array()
"#;

    #[test]
    fn tileds_tilemaps_read() {
        let r = parse(TILED).unwrap();
        assert_eq!((r.format(), r.load_steps()), (Some(3), Some(4)));
        assert_eq!(r.ext_resources().next().unwrap().id, "hex mini");
        let tilemaps: Vec<_> = r
            .nodes()
            .filter(|(n, _)| n.class.as_deref() == Some("TileMap"))
            .collect();
        assert_eq!(tilemaps.len(), 1);
        let (tilemap, section) = &tilemaps[0];
        assert_eq!(
            (tilemap.path(), section.get("format")),
            ("TileMap".into(), Some("2"))
        );
        assert_eq!(
            section.get("layer_0/tile_data").and_then(packed_int32s),
            Some(vec![0, 65536, 1, 1, 0, 0, 65538, 131072, 0])
        );
        assert_eq!(
            section.get("layer_1/tile_data").and_then(packed_int32s),
            Some(vec![])
        );
        assert_eq!(
            section.get("layer_0/name").and_then(string).as_deref(),
            Some("Ground")
        );
        assert_eq!(r.sections[0].line, 4, "blank lines count");
    }

    /// A level scene written by hand: an inheritor with an editable instance and connections.
    const INHERITOR: &str = r#"; A level made by hand.
[gd_scene load_steps=4 format=3 uid="uid://b6x62a7vjuo44"]

[ext_resource type="PackedScene" uid="uid://cyncsb" path="res://levels/features.tscn" id="1"]
[ext_resource type="Script" path="res://door.gd" id="2"]
[ext_resource type="PackedScene" path="res://chest.tscn" id="3"]

[node name="Level" instance=ExtResource("1")]

[node name="crate" parent="objects"]
position = Vector2(40, 96) ; moved
script = ExtResource("2")
"metadata/max hp" = 3

; the chest the map's objects lack
[node name="Chest" parent="objects" instance=ExtResource("3")]

[node name="Lid" parent="objects/Chest"]
visible = false

[node name="Player" type="CharacterBody2D" parent="objects" groups=["players", "saved"]]

[connection signal="body_entered" from="objects/zone" to="." method="_on_zone_body_entered" flags=3 binds=[1, "x"]]
[connection signal="opened" from="objects/Chest" to="objects/Player" method="loot"]

[editable path="objects/Chest"]
"#;

    #[test]
    fn hand_written_inheritors_read() {
        let r = parse(INHERITOR).unwrap();
        assert_eq!(
            (r.comments.clone(), r.uid().as_deref(), r.load_steps()),
            (
                vec![" A level made by hand."],
                Some("uid://b6x62a7vjuo44"),
                Some(4)
            )
        );
        let base = r.ext_resources().next().unwrap();
        assert_eq!(
            (base.class.as_str(), base.uid.as_deref(), base.path.as_str()),
            (
                "PackedScene",
                Some("uid://cyncsb"),
                "res://levels/features.tscn"
            )
        );
        let (root, _) = r.root().unwrap();
        assert_eq!(
            (root.class.as_deref(), root.instance.as_deref(), root.path()),
            (None, Some("1"), ".".into())
        );

        let (crate_, section) = r.nodes().find(|(n, _)| n.name == "crate").unwrap();
        assert_eq!(
            (crate_.class, crate_.unique_id, crate_.parent_id_path),
            (None, None, vec![])
        );
        assert_eq!(
            section.get("position"),
            Some("Vector2(40, 96)"),
            "the comment isn't part of the value"
        );
        assert_eq!(section.get("metadata/max hp"), Some("3"), "a quoted key");
        let (chest, _) = r.nodes().find(|(n, _)| n.name == "Chest").unwrap();
        assert_eq!(
            (chest.instance.as_deref(), chest.path()),
            (Some("3"), "objects/Chest".into())
        );
        let paths: Vec<String> = r.nodes().map(|(n, _)| n.path()).collect();
        assert_eq!(
            paths,
            [
                ".",
                "objects/crate",
                "objects/Chest",
                "objects/Chest/Lid",
                "objects/Player"
            ]
        );
        let (player, _) = r.nodes().last().unwrap();
        assert_eq!(player.groups, ["players", "saved"]);

        let connections: Vec<Connection> = r.connections().collect();
        assert_eq!(
            connections[0],
            Connection {
                signal: "body_entered".into(),
                from: "objects/zone".into(),
                to: ".".into(),
                method: "_on_zone_body_entered".into(),
                flags: Some(3)
            }
        );
        assert_eq!(
            (connections[1].to.as_str(), connections[1].flags),
            ("objects/Player", None)
        );
        assert_eq!(r.editable().collect::<Vec<_>>(), ["objects/Chest"]);
        let crate_section = r
            .sections
            .iter()
            .find(|s| s.attr("name") == Some("\"crate\""))
            .unwrap();
        assert!(
            INHERITOR[crate_section.span.clone()].ends_with("= 3\n"),
            "the span ends with the last value, not at the comment after it"
        );
    }

    #[test]
    fn tres_files_read() {
        let tres = "[gd_resource type=\"TileSet\" load_steps=2 format=3 uid=\"uid://a\"]\n\n[ext_resource type=\"Texture2D\" path=\"res://t.png\" id=\"1_huc3s\"]\n\n[sub_resource type=\"TileSetAtlasSource\" id=\"TileSetAtlasSource_x7k2p\"]\nresource_name = \"desert\"\ntexture = ExtResource(\"1_huc3s\")\n\n[resource]\nsources/0 = SubResource(\"TileSetAtlasSource_x7k2p\")\n";
        let r = parse(tres).unwrap();
        assert_eq!(
            (r.header.kind, r.class().as_deref(), r.uid().as_deref()),
            ("gd_resource", Some("TileSet"), Some("uid://a"))
        );
        let resource = r.resource().unwrap();
        assert_eq!(
            resource.get("sources/0").and_then(sub_resource).as_deref(),
            Some("TileSetAtlasSource_x7k2p")
        );
        let (sub, section) = r.sub_resources().next().unwrap();
        assert_eq!(
            (
                sub.class.as_str(),
                section.get("resource_name").and_then(string).as_deref()
            ),
            ("TileSetAtlasSource", Some("desert"))
        );
        assert_eq!(r.nodes().count(), 0);
        // Godot 3 numbered its resources.
        assert_eq!(
            (
                ext_resource("ExtResource( 1 )").as_deref(),
                sub_resource("SubResource(\"a)\")").as_deref()
            ),
            (Some("1"), Some("a)"))
        );
    }

    #[test]
    fn the_header_alone_needs_no_more_than_the_header() {
        let r = parse_header("\u{feff};one\r\n;two\n\n[gd_scene format=3 uid=\"uid://cyncsb\"]\r\n[node name=\"broken").unwrap();
        assert_eq!(
            (r.uid().as_deref(), r.sections.len(), r.comments),
            (Some("uid://cyncsb"), 0, vec!["one", "two"])
        );
        assert!(parse("[gd_scene format=3]\n[node name=\"broken").is_err());
    }

    #[test]
    fn files_godot_couldnt_load_are_errors_with_their_line() {
        let errors = [
            ("", "no [gd_scene]"),
            ("hello\n[gd_scene]", "line 1: expected the [gd_scene]"),
            ("[node name=\"a\"]\n", "line 1: the file starts with [node]"),
            (
                "[gd_scene format=3]\nformat = 3\n",
                "line 2: a value before the first section",
            ),
            (
                "[gd_scene format=3]\n\n[node name=\"a\" type=\"Node2D\"]\ntext = \"open\n",
                "line 4: a string that never ends",
            ),
            (
                "[gd_scene format=3]\n[node name=\"a\"]\npoints = PackedVector2Array(1, 2\n",
                "line 3: a value whose brackets never close",
            ),
            (
                "[gd_scene format=3]\n[node name=\"a\"]\nvalue = 1)\n",
                "line 3: an unmatched ')'",
            ),
            (
                "[gd_scene format=3]\n[node name=\"a\"]\njust words\n",
                "line 3: expected key = value",
            ),
            (
                "[gd_scene format=3]\n[node name=\"a\"]\nkey =\n",
                "line 3: a missing value",
            ),
            (
                "[gd_scene format=3]\n[node type=\"Node2D\"]\n",
                "line 2: [node] has no name",
            ),
            (
                "[gd_scene format=3]\n[node name=\"a\" unique_id=x]\n",
                "line 2: unique_id=x in [node] isn't a 32-bit number",
            ),
            (
                "[gd_scene format=3]\n[node name=\"a\" unique_id=4294967296]\n",
                "isn't a 32-bit number",
            ),
            (
                "[gd_scene format=3]\n[node name=\"a\" parent_id_path=PackedInt32Array(1, x)]\n",
                "isn't a PackedInt32Array",
            ),
            (
                "[gd_scene format=3]\n[connection signal=\"a\" from=\".\" to=\".\"]\n",
                "[connection] has no method",
            ),
            (
                "[gd_scene format=3]\n[editable]\n",
                "[editable] has no path",
            ),
            (
                "[gd_scene format=3]\n[ext_resource type=\"Script\" id=\"1\"]\n",
                "[ext_resource] has no path",
            ),
            (
                "[gd_scene format=3]\n[sub_resource type=\"Theme\"]\n",
                "[sub_resource] has no id",
            ),
            (
                "[gd_scene format=3]\n\n[node name=\"a\"",
                "line 3: [node never closes",
            ),
            (
                "[gd_scene format=3]\n[node \"a\"]",
                "line 2: unexpected '\"' in [node]",
            ),
            (
                "[gd_scene format=3]\n[node name]",
                "line 2: name in [node] has no value",
            ),
            (
                "[gd_scene format=3]\n[]",
                "line 2: a section without a name",
            ),
        ];
        for (text, error) in errors {
            let got = parse(text).map(|_| ()).unwrap_err();
            assert!(got.contains(error), "{text:?}: {got}");
        }
    }

    #[test]
    fn strings_unescape_like_godot() {
        assert_eq!(
            string(r#""tab\there\nand \"q\" \\ \a""#).as_deref(),
            Some("tab\there\nand \"q\" \\ a")
        );
        assert_eq!(
            string(r#""\u00e9\U01F600\ud83d\ude00\b\f\r""#).as_deref(),
            Some("é😀😀\u{8}\u{c}\r")
        );
        assert_eq!(string("&\"name\"").as_deref(), Some("name"));
        for bad in [
            "12",
            "\"open",
            "\"a\" x",
            "\"\\u12\"",
            "\"\\uzzzz\"",
            "\"\\ud83d\"",
            "\"\\ude00\"",
            "\"\\ud83dx\"",
            "NodePath(\"a\")",
        ] {
            assert_eq!(string(bad), None, "{bad}");
        }
        // What the export writes reads back as it was.
        for s in [
            "",
            "say \"hi\"",
            "a\\b",
            "tab\there",
            "line\nbreak\r",
            "é ü 😀",
            "[x] ; y",
        ] {
            assert_eq!(string(&super::super::text::string(s)).as_deref(), Some(s));
        }
    }

    #[test]
    fn lists_split_outside_strings_and_brackets() {
        assert_eq!(
            strings(r#"["a, b", "c]"]"#),
            Some(vec!["a, b".to_owned(), "c]".to_owned()])
        );
        assert_eq!(
            strings("PackedStringArray(\"x\", &\"y\")"),
            Some(vec!["x".to_owned(), "y".to_owned()])
        );
        assert_eq!(strings("[]"), Some(vec![]));
        assert_eq!(strings("[1]"), None);
        assert_eq!(
            packed_int32s("PackedInt32Array(1, -2, 2147483647)"),
            Some(vec![1, -2, i32::MAX])
        );
        assert_eq!(packed_int32s("PackedInt32Array(1,, 2)"), None);
        assert_eq!(packed_int32s("PackedInt64Array(1)"), None);
        assert_eq!(
            items("Vector2(1, 2), [3, 4]"),
            Some(vec!["Vector2(1, 2)", "[3, 4]"])
        );
        assert_eq!((items("(1"), items("1)")), (None, None));
    }

    #[test]
    fn uids_are_written_and_read_like_godot() {
        assert_eq!(uid_to_text(123456789), "uid://cyncsb");
        assert_eq!(text_to_uid("uid://cyncsb"), Some(123456789));
        assert_eq!(
            (
                uid_to_text(0),
                uid_to_text(24),
                uid_to_text(25),
                uid_to_text(33),
                uid_to_text(34)
            ),
            (
                "uid://a".into(),
                "uid://y".into(),
                "uid://0".into(),
                "uid://8".into(),
                "uid://ba".into()
            )
        );
        for id in [
            0,
            1,
            24,
            25,
            33,
            34,
            123456789,
            0x1234_5678_9abc_def0,
            (1 << 63) - 1,
        ] {
            assert_eq!(text_to_uid(&uid_to_text(id)), Some(id), "{id}");
        }
        assert_eq!(uid_to_text((1 << 63) - 1), "uid://d4n4ub6itg400");
        // Godot reads z as y + 1 and 9 as 8 + 1, which its own text never holds.
        assert_eq!(
            (text_to_uid("uid://z"), text_to_uid("uid://9")),
            (Some(25), Some(34))
        );
        for bad in [
            "uid://",
            "uid://<invalid>",
            "uid://Abc",
            "res://cyncsb",
            "cyncsb",
        ] {
            assert_eq!(text_to_uid(bad), None, "{bad}");
        }
    }

    #[test]
    fn fnv1a_matches_the_reference_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
        assert_eq!(fnv1a32(b""), 0x811c_9dc5);
        assert_eq!(fnv1a32(b"a"), 0xe40c_292c);
        assert_eq!(fnv1a32(b"foobar"), 0xbf9c_f968);
    }
}
