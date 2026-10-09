//! Saved brushes (`.stamp`): a name and variations, each a small map with a probability.

use std::path::Path;

use bt_core::Map;
use serde_json::{Map as JsonObject, Value as Json};

use crate::error::Error;
use crate::finish::Finisher;
use crate::json::{JsonReader, map_json, qt_compact_layout};
use crate::loader::{Loaded, Loader, absolute};
use crate::text::read_text;
use crate::unfinish::Unfinisher;
use crate::warning::{Warning, WarningKind, Warnings};
use crate::write::Rendered;

/// A saved brush.
#[derive(Clone, Debug, PartialEq)]
pub struct Stamp {
    /// The name.
    pub name: String,
    /// The number key it is on (Tiled's quick stamps), if any.
    pub quick_stamp_index: Option<u32>,
    /// The variations, one picked by probability each time the brush paints.
    pub variations: Vec<StampVariation>,
}

/// One variation of a [`Stamp`].
#[derive(Clone, Debug, PartialEq)]
pub struct StampVariation {
    /// Its weight among the variations.
    pub probability: f64,
    /// The tiles: a map with the source map's grid, its tilesets and one or more tile layers.
    pub map: Map,
}

impl Loader {
    /// Reads a stamp with the tilesets its variations use.
    pub fn load_stamp(&mut self, path: &Path) -> Result<Loaded<Stamp>, Error> {
        let path = absolute(path)?;
        let text = read_text(&path)?;
        self.parse_stamp(&text, &path)
    }

    /// Reads a stamp from text; `path` is where it is.
    pub fn parse_stamp(&mut self, text: &str, path: &Path) -> Result<Loaded<Stamp>, Error> {
        let path = absolute(path)?;
        let value = crate::json::parse_document(text, &path)?;
        let object = value
            .as_object()
            .ok_or_else(|| Error::format(&path, "not a stamp: not a JSON object"))?;
        let mut warnings = Warnings::new(&path);
        let mut stamp = Stamp {
            name: String::new(),
            quick_stamp_index: None,
            variations: Vec::new(),
        };
        let mut raw_variations = Vec::new();
        {
            let mut reader = JsonReader {
                warnings: &mut warnings,
            };
            reader.check_keys(
                object,
                "the stamp",
                &["name", "quickStampIndex", "variations"],
            );
            stamp.name = reader.text_of(object, "name");
            stamp.quick_stamp_index = reader
                .scalar(object, "quickStampIndex")
                .and_then(|v| v.to_int())
                .and_then(|i| u32::try_from(i).ok());
            for variation in reader.objects(object, "variations") {
                reader.check_keys(variation, "a stamp variation", &["probability", "map"]);
                let probability = reader
                    .scalar(variation, "probability")
                    .and_then(|v| v.to_real())
                    .unwrap_or(1.0);
                match reader.object(variation, "map") {
                    Some(map) => raw_variations.push(RawVariation {
                        probability,
                        map: reader.read_map(map),
                    }),
                    None => reader.warnings.record(
                        WarningKind::InvalidValue,
                        "a stamp variation has no map; it is dropped",
                    ),
                }
            }
        }
        let mut all: Vec<Warning> = warnings.into_list();
        for raw in raw_variations {
            let mut finisher = Finisher::new(self, &path);
            let map = finisher.map_of(raw.map);
            all.extend(finisher.into_warnings());
            stamp.variations.push(StampVariation {
                probability: raw.probability,
                map,
            });
        }
        Ok(Loaded {
            value: stamp,
            warnings: crate::warning::deduplicated(all),
        })
    }
}

/// A variation as read, before its map is finished.
struct RawVariation {
    probability: f64,
    map: crate::raw::RawMap,
}

/// A stamp's text, as Tiled writes it: compact JSON, each variation's map as a JSON map with
/// references relative to the stamp's folder.
pub(crate) fn stamp_text(stamp: &Stamp, path: &Path, loader: Option<&Loader>) -> Rendered {
    let mut document = JsonObject::new();
    let mut warnings = Vec::new();
    document.insert("name".into(), Json::String(stamp.name.clone()));
    if let Some(index) = stamp.quick_stamp_index {
        document.insert("quickStampIndex".into(), Json::from(index));
    }
    let variations = stamp
        .variations
        .iter()
        .map(|variation| {
            let mut unfinisher = Unfinisher::new(path, loader);
            let raw = unfinisher.map_of(
                &variation.map,
                bt_core::CompatibilityVersion::Latest.version_string(),
            );
            warnings.extend(unfinisher.warnings.into_list());
            let mut entry = JsonObject::new();
            entry.insert(
                "probability".into(),
                serde_json::Number::from_f64(variation.probability)
                    .map_or(Json::Null, Json::Number),
            );
            entry.insert("map".into(), map_json(&raw));
            Json::Object(entry)
        })
        .collect();
    document.insert("variations".into(), Json::Array(variations));
    Rendered {
        text: qt_compact_layout(&Json::Object(document)),
        warnings: crate::warning::deduplicated(warnings),
    }
}

#[cfg(test)]
mod tests {
    use bt_test_data::Scratch;

    use super::*;

    #[test]
    fn stamps_read_and_write_their_variation_maps() {
        let scratch = Scratch::new("bt-tmx-stamp").unwrap();
        let path = scratch.join("hill.stamp");
        let text = r#"{"name":"hill","quickStampIndex":2,"variations":[{"map":{"compressionlevel":-1,"height":1,"infinite":false,"layers":[{"data":[1,0],"height":1,"id":1,"name":"","opacity":1,"type":"tilelayer","visible":true,"width":2,"x":0,"y":0}],"nextlayerid":2,"nextobjectid":1,"orientation":"orthogonal","renderorder":"right-down","tiledversion":"1.12.2","tileheight":16,"tilesets":[{"columns":0,"firstgid":1,"margin":0,"name":"t","spacing":0,"tilecount":0,"tileheight":16,"tilewidth":16}],"tilewidth":16,"type":"map","version":"1.11","width":2},"probability":0.5}]}"#;
        let mut loader = Loader::default();
        let loaded = loader.parse_stamp(text, &path).unwrap();
        let stamp = loaded.value;
        assert_eq!(
            (stamp.name.as_str(), stamp.quick_stamp_index),
            ("hill", Some(2))
        );
        assert_eq!(stamp.variations[0].probability, 0.5);
        assert_eq!(stamp.variations[0].map.width, 2);
        assert_eq!(stamp_text(&stamp, &path, None).text, text);
    }
}
