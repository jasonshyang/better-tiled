//! The one model every command's text is written from (`10-agents.md`, §9). A command returns a
//! typed outcome; [`Outcome::report`] lays it out as sections of lines, and [`Report::text`] is
//! what the reader sees: sections holding problems first, two spaces per level of indent, a long
//! section cut with "and N more". With `--json` the outcome itself is written, with a `schema`
//! field naming its shape ([`to_json`]), and read back the same way ([`from_json`]), which is how
//! an edit made in the editor reaches `bt`. The editor, `bt` and `bt --json` therefore describe
//! one run in one way.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// What a line is to its reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    /// Something to fix, naming where (layer, cells, ids) and what; written `problem: …`. What
    /// to do about it is the guide's, never the line's.
    Problem,
    /// Something that may keep the output from being what it seems, such as a warning from
    /// reading the map; written `warning: …`.
    Warning,
    /// How a request was carried out, when that is worth knowing; written `note: …`.
    Note,
    /// What is there; written as it is.
    Fact,
}

impl LineKind {
    /// The words a line of this kind starts with in text.
    fn marker(self) -> &'static str {
        match self {
            LineKind::Problem => "problem: ",
            LineKind::Warning => "warning: ",
            LineKind::Note => "note: ",
            LineKind::Fact => "",
        }
    }
}

/// A line of a report, with the lines that detail it indented under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// What the line is to its reader.
    pub kind: LineKind,
    /// The line's words, without the marker of its kind.
    pub text: String,
    /// Lines that detail this one, indented under it.
    pub details: Vec<Line>,
}

impl Line {
    /// A line of the given kind with no details.
    pub fn new(kind: LineKind, text: impl Into<String>) -> Line {
        Line {
            kind,
            text: text.into(),
            details: Vec::new(),
        }
    }

    /// A problem line.
    pub fn problem(text: impl Into<String>) -> Line {
        Line::new(LineKind::Problem, text)
    }

    /// A warning line.
    pub fn warning(text: impl Into<String>) -> Line {
        Line::new(LineKind::Warning, text)
    }

    /// A note line.
    pub fn note(text: impl Into<String>) -> Line {
        Line::new(LineKind::Note, text)
    }

    /// A fact line.
    pub fn fact(text: impl Into<String>) -> Line {
        Line::new(LineKind::Fact, text)
    }

    /// This line with `detail` added under it.
    pub fn with_detail(mut self, detail: Line) -> Line {
        self.details.push(detail);
        self
    }

    /// This line with `details` added under it.
    pub fn with_details(mut self, details: impl IntoIterator<Item = Line>) -> Line {
        self.details.extend(details);
        self
    }

    /// Whether this line or a line under it is a problem.
    pub fn has_problem(&self) -> bool {
        self.kind == LineKind::Problem || self.details.iter().any(Line::has_problem)
    }

    /// Writes the line and its details at `depth` levels of indent.
    fn write_text(&self, depth: usize, out: &mut String) {
        write_indent(depth, out);
        out.push_str(self.kind.marker());
        out.push_str(&self.text);
        out.push('\n');
        for detail in &self.details {
            detail.write_text(depth + 1, out);
        }
    }
}

/// Lines under an optional heading. A section with a heading indents its lines under it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Section {
    heading: Option<String>,
    lines: Vec<Line>,
    limit: Option<usize>,
}

impl Section {
    /// A section without a heading: its lines are not indented.
    pub fn new() -> Section {
        Section::default()
    }

    /// A section under `heading`.
    pub fn headed(heading: impl Into<String>) -> Section {
        Section {
            heading: Some(heading.into()),
            ..Section::default()
        }
    }

    /// This section showing at most `limit` lines in text, then how many more there are. The
    /// outcome keeps them all; only the text is cut.
    pub fn limited(mut self, limit: usize) -> Section {
        self.limit = Some(limit);
        self
    }

    /// This section with `line` added.
    pub fn with_line(mut self, line: Line) -> Section {
        self.lines.push(line);
        self
    }

    /// Adds a line at the end.
    pub fn add_line(&mut self, line: Line) {
        self.lines.push(line);
    }

    /// Adds lines at the end.
    pub fn add_lines(&mut self, lines: impl IntoIterator<Item = Line>) {
        self.lines.extend(lines);
    }

    /// The heading, if any.
    pub fn heading(&self) -> Option<&str> {
        self.heading.as_deref()
    }

    /// The lines, all of them.
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// Whether the section has neither a heading nor lines.
    pub fn is_empty(&self) -> bool {
        self.heading.is_none() && self.lines.is_empty()
    }

    /// Whether a line of the section, or a line under one, is a problem.
    pub fn has_problem(&self) -> bool {
        self.lines.iter().any(Line::has_problem)
    }

    fn write_text(&self, out: &mut String) {
        let depth = match &self.heading {
            Some(heading) => {
                out.push_str(heading);
                out.push('\n');
                1
            }
            None => 0,
        };
        let shown = self.limit.unwrap_or(self.lines.len());
        for line in self.lines.iter().take(shown) {
            line.write_text(depth, out);
        }
        let hidden = self.lines.len().saturating_sub(shown);
        if hidden > 0 {
            write_indent(depth, out);
            out.push_str(&format!("… and {hidden} more\n"));
        }
    }
}

/// A command's outcome laid out for reading.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    sections: Vec<Section>,
}

impl Report {
    /// A report with nothing in it.
    pub fn new() -> Report {
        Report::default()
    }

    /// Adds a section at the end; an empty one is left out.
    pub fn add_section(&mut self, section: Section) {
        if !section.is_empty() {
            self.sections.push(section);
        }
    }

    /// Adds a line at the end, in a section without a heading.
    pub fn add_line(&mut self, line: Line) {
        match self.sections.last_mut() {
            Some(last) if last.heading.is_none() && last.limit.is_none() => last.add_line(line),
            _ => self.sections.push(Section::new().with_line(line)),
        }
    }

    /// The sections, in the order they were added.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Every line, depth first: each line before the lines under it.
    pub fn lines(&self) -> Vec<&Line> {
        fn visit<'l>(line: &'l Line, out: &mut Vec<&'l Line>) {
            out.push(line);
            for detail in &line.details {
                visit(detail, out);
            }
        }
        let mut out = Vec::new();
        for section in &self.sections {
            for line in &section.lines {
                visit(line, &mut out);
            }
        }
        out
    }

    /// Whether any line is a problem.
    pub fn has_problems(&self) -> bool {
        self.sections.iter().any(Section::has_problem)
    }

    /// The report as text: the sections holding a problem first, the rest after, each group in
    /// the order the sections were added.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let (with_problems, without): (Vec<&Section>, Vec<&Section>) =
            self.sections.iter().partition(|s| s.has_problem());
        for section in with_problems.into_iter().chain(without) {
            section.write_text(&mut out);
        }
        out
    }
}

/// A command's typed outcome: what tests assert, what `--json` writes and what the live link
/// carries, laid out for reading by [`Outcome::report`].
pub trait Outcome: Serialize + DeserializeOwned {
    /// The name of the outcome's JSON shape, written as its `schema` field: `bt.<name>/<version>`,
    /// the version raised when a field changes incompatibly.
    const SCHEMA: &'static str;

    /// The outcome laid out for reading.
    fn report(&self) -> Report;
}

/// An outcome with the name of its shape, as JSON carries it.
#[derive(Serialize)]
struct Tagged<'o, O> {
    schema: &'static str,
    #[serde(flatten)]
    outcome: &'o O,
}

/// An outcome read back with the name of its shape.
#[derive(Deserialize)]
struct Untagged<O> {
    schema: String,
    #[serde(flatten)]
    outcome: O,
}

/// Why an outcome could not be read back from JSON.
#[derive(Debug, thiserror::Error)]
pub enum JsonError {
    /// The text is not the JSON of an outcome.
    #[error("not an outcome: {0}")]
    Malformed(#[from] serde_json::Error),
    /// The JSON is of another shape, or another version of it.
    #[error("the outcome is a {found}, not a {expected}")]
    OtherSchema {
        /// The shape expected.
        expected: &'static str,
        /// The shape found.
        found: String,
    },
}

/// The outcome as one line of JSON, with its `schema` field first.
pub fn to_json<O: Outcome>(outcome: &O) -> Result<String, serde_json::Error> {
    serde_json::to_string(&Tagged {
        schema: O::SCHEMA,
        outcome,
    })
}

/// The outcome as indented JSON, with its `schema` field first: what `--json` prints.
pub fn to_json_pretty<O: Outcome>(outcome: &O) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&Tagged {
        schema: O::SCHEMA,
        outcome,
    })
}

/// Reads back an outcome [`to_json`] wrote, refusing one of another shape.
pub fn from_json<O: Outcome>(text: &str) -> Result<O, JsonError> {
    let read: Untagged<O> = serde_json::from_str(text)?;
    if read.schema != O::SCHEMA {
        return Err(JsonError::OtherSchema {
            expected: O::SCHEMA,
            found: read.schema,
        });
    }
    Ok(read.outcome)
}

fn write_indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_holding_problems_come_first_and_kinds_are_marked() {
        let mut report = Report::new();
        report.add_section(
            Section::headed("ground")
                .with_line(Line::fact("12 cells changed").with_detail(Line::fact("water 0 → 12"))),
        );
        report.add_section(Section::new().with_line(Line::problem("walls: a seam at 4,5")));
        report.add_line(Line::note("the rules ran"));
        assert_eq!(
            report.text(),
            "problem: walls: a seam at 4,5\nnote: the rules ran\nground\n  12 cells changed\n    \
             water 0 → 12\n"
        );
        assert!(report.has_problems());
    }

    #[test]
    fn a_limited_section_says_how_many_more_it_holds() {
        let mut section = Section::headed("trees").limited(2);
        section.add_lines((1..=5).map(|n| Line::fact(format!("#{n}"))));
        let mut report = Report::new();
        report.add_section(section);
        assert_eq!(report.text(), "trees\n  #1\n  #2\n  … and 3 more\n");
        assert_eq!(report.lines().len(), 5, "the outcome keeps every line");
    }

    #[test]
    fn empty_sections_are_left_out_and_lines_join_the_last_plain_section() {
        let mut report = Report::new();
        report.add_section(Section::new());
        report.add_line(Line::fact("a"));
        report.add_line(Line::fact("b"));
        assert_eq!(report.sections().len(), 1);
        assert_eq!(report.text(), "a\nb\n");
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Counted {
        cells: usize,
    }

    impl Outcome for Counted {
        const SCHEMA: &'static str = "bt.counted/1";

        fn report(&self) -> Report {
            let mut report = Report::new();
            report.add_line(Line::fact(format!("{} cells", self.cells)));
            report
        }
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Other {
        cells: usize,
    }

    impl Outcome for Other {
        const SCHEMA: &'static str = "bt.other/1";

        fn report(&self) -> Report {
            Report::new()
        }
    }

    #[test]
    fn outcomes_round_trip_through_json_with_their_schema_first() {
        let outcome = Counted { cells: 3 };
        let json = to_json(&outcome).unwrap();
        assert_eq!(json, r#"{"schema":"bt.counted/1","cells":3}"#);
        assert_eq!(from_json::<Counted>(&json).unwrap(), outcome);
        assert!(matches!(
            from_json::<Other>(&json),
            Err(JsonError::OtherSchema { .. })
        ));
        assert!(matches!(
            from_json::<Counted>("{"),
            Err(JsonError::Malformed(_))
        ));
    }
}
