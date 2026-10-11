//! What `bt export` wrote (`10-agents.md`, §10.8): a map or tileset in another of Tiled's
//! formats, with Tiled's export options, or a map as the resolved JSON.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::report::{Line, Outcome, Report};
use crate::resolved::RESOLVED_FORMAT;

/// The formats `bt export` writes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// Tiled's XML: `.tmx`, `.tsx`.
    Xml,
    /// Tiled's JSON: `.tmj`, `.tsj`, `.json`.
    Json,
    /// A JSON map wrapped in a JavaScript call: `.js`.
    JavaScript,
    /// The resolved JSON for other programs (`--resolved`): `.json`.
    Resolved,
    /// A plain picture of the map, at its own size, without rulers: `.png`.
    Picture,
}

impl ExportFormat {
    /// The format's name in reports.
    fn name(self) -> String {
        match self {
            ExportFormat::Xml => "Tiled's XML".to_owned(),
            ExportFormat::Json => "Tiled's JSON".to_owned(),
            ExportFormat::JavaScript => "Tiled's JavaScript".to_owned(),
            ExportFormat::Resolved => format!("the resolved JSON ({RESOLVED_FORMAT})"),
            ExportFormat::Picture => "a picture of the map".to_owned(),
        }
    }
}

/// What `bt export` wrote.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportOutcome {
    /// The map or tileset exported.
    pub input: PathBuf,
    /// The file written.
    pub out: PathBuf,
    /// Its format.
    pub format: ExportFormat,
    /// Tiled's export options that applied, by their option names.
    pub options: Vec<String>,
    /// What reading and writing warned about.
    pub warnings: Vec<String>,
}

impl Outcome for ExportOutcome {
    const SCHEMA: &'static str = "bt.export/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        for warning in &self.warnings {
            report.add_line(Line::warning(warning.clone()));
        }
        let options = if self.options.is_empty() {
            String::new()
        } else {
            format!(" ({})", self.options.join(", "))
        };
        report.add_line(Line::fact(format!(
            "wrote {} as {}{options}",
            self.out.display(),
            self.format.name()
        )));
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{from_json, to_json};

    #[test]
    fn an_export_says_what_it_wrote_and_warnings_first() {
        let outcome = ExportOutcome {
            input: PathBuf::from("a.tmx"),
            out: PathBuf::from("a.tmj"),
            format: ExportFormat::Json,
            options: vec!["--embed-tilesets".into()],
            warnings: vec!["a.tmx: tile 9 of tiles is missing".into()],
        };
        assert_eq!(
            outcome.report().text(),
            "warning: a.tmx: tile 9 of tiles is missing\nwrote a.tmj as Tiled's JSON \
             (--embed-tilesets)\n"
        );
        let json = to_json(&outcome).unwrap();
        assert!(json.starts_with(r#"{"schema":"bt.export/1""#));
        assert_eq!(from_json::<ExportOutcome>(&json).unwrap(), outcome);
    }

    #[test]
    fn a_resolved_export_names_its_format() {
        let outcome = ExportOutcome {
            input: PathBuf::from("a.tmx"),
            out: PathBuf::from("a.json"),
            format: ExportFormat::Resolved,
            options: Vec::new(),
            warnings: Vec::new(),
        };
        assert_eq!(
            outcome.report().text(),
            "wrote a.json as the resolved JSON (bt resolved map 1)\n"
        );
        assert!(
            to_json(&outcome)
                .unwrap()
                .contains(r#""format":"resolved""#)
        );
    }
}
