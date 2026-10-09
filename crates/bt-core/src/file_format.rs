//! How a file was written: what a byte-identical write needs that is not content.

use glam::UVec2;

use crate::file_ref::FileRef;

/// Everything about a map, tileset or template file that is "how it was written" rather than
/// "what it is", kept so that saving an unchanged value gives the same bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFormat {
    /// The format version as read (`"1.10"`); empty for a value never read from a file.
    pub version: String,
    /// The version of Tiled that wrote the file, as read; informational.
    pub tiled_version: String,
    /// The line ending of the file's first line, used for the whole file when it is written.
    pub line_ending: LineEnding,
    /// How tile layer data is encoded (maps only).
    pub layer_data: LayerDataFormat,
    /// The compression level the file asks for; `None` is the format's default.
    pub compression_level: Option<i32>,
    /// The chunk size infinite maps are written in.
    pub output_chunk_size: UVec2,
    /// Where File ▸ Export last wrote the value, so the next export can repeat it.
    pub export: ExportSettings,
}

impl Default for FileFormat {
    fn default() -> FileFormat {
        FileFormat {
            version: String::new(),
            tiled_version: String::new(),
            line_ending: LineEnding::Lf,
            layer_data: LayerDataFormat::Base64Zlib,
            compression_level: None,
            output_chunk_size: UVec2::new(16, 16),
            export: ExportSettings::default(),
        }
    }
}

/// Line endings.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineEnding {
    /// `\n`, as Tiled writes on macOS and Linux.
    #[default]
    Lf,
    /// `\r\n`, as Tiled writes on Windows.
    CrLf,
}

/// How tile layer data is encoded in a file.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum LayerDataFormat {
    /// One `<tile gid>` element per cell; read, never written.
    Xml,
    /// Decimal global ids separated by commas.
    Csv,
    /// Little-endian 32-bit global ids, base64-encoded.
    Base64,
    /// The same, gzip-compressed.
    Base64Gzip,
    /// The same, zlib-compressed; Tiled's default.
    #[default]
    Base64Zlib,
    /// The same, zstd-compressed.
    Base64Zstd,
}

/// The last export of a value, kept in its file the way Tiled keeps it
/// (`<editorsettings><export target="…" format="…"/>`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportSettings {
    /// The file exported to, if any.
    pub target: Option<FileRef>,
    /// Tiled's short name of the format (`tmx`, `json`, `tscn`, or another tool's); empty for
    /// none.
    pub format: String,
}

impl ExportSettings {
    /// Whether there is nothing to repeat.
    pub fn is_empty(&self) -> bool {
        self.target.is_none() && self.format.is_empty()
    }
}
