//! What `bt export` wrote (`10-agents.md`, §10.8): a map or tileset in another of Tiled's
//! formats, with Tiled's export options.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::report::{Line, Outcome, Report};

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
}

impl ExportFormat {
    /// The format's name in reports.
    fn name(self) -> &'static str {
        match self {
            ExportFormat::Xml => "Tiled's XML",
            ExportFormat::Json => "Tiled's JSON",
            ExportFormat::JavaScript => "Tiled's JavaScript",
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
}
