//! Map fragments: the text form of the clipboard, shared with Tiled. A fragment is a map with
//! the source map's grid and nothing else, its tilesets, and one tile layer per copied layer in
//! CSV (so an agent can read and write it) or one object layer.

use std::path::Path;

use bt_core::{LayerDataFormat, Map};

use crate::error::Error;
use crate::loader::{Loaded, Loader};
use crate::unfinish::Unfinisher;
use crate::warning::deduplicated;
use crate::write::Rendered;
use crate::xml::XmlPrinter;

impl Loader {
    /// Reads a fragment from text found anywhere (Tiled's clipboard, a chat window): the map's
    /// text, with whatever surrounds it (a code fence, blank lines) dropped. `path` is where its
    /// relative references are considered to be.
    pub fn parse_fragment(&mut self, text: &str, path: &Path) -> Result<Loaded<Map>, Error> {
        let start = text
            .find("<?xml")
            .or_else(|| text.find("<map"))
            .ok_or_else(|| Error::format(path, "no map in the text: it has no <map>"))?;
        let end = text
            .rfind("</map>")
            .map(|end| end + "</map>".len())
            .ok_or_else(|| Error::format(path, "no map in the text: it has no </map>"))?;
        let map_text = text
            .get(start..end)
            .filter(|_| start < end)
            .ok_or_else(|| Error::format(path, "no map in the text"))?;
        self.parse_map(map_text, path)
    }
}

/// A map's fragment text, as Tiled puts maps on the clipboard: XML, tile layers in CSV,
/// references as absolute paths so that the text can be pasted anywhere.
pub fn fragment_text(map: &Map, loader: Option<&Loader>) -> Rendered {
    let mut map = map.clone();
    map.file.layer_data = LayerDataFormat::Csv;
    map.file.compression_level = None;
    let mut unfinisher = Unfinisher::with_absolute_references(loader);
    let raw = unfinisher.map_of(&map, bt_core::CompatibilityVersion::Latest.version_string());
    Rendered {
        text: XmlPrinter::new(false).print_map(&raw),
        warnings: deduplicated(unfinisher.warnings.into_list()),
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Cell, TileId, TilesetSlot};
    use bt_test_data::Scratch;
    use glam::IVec2;

    use super::*;

    #[test]
    fn fragments_paste_from_anywhere() {
        let scratch = Scratch::new("bt-tmx-fragment").unwrap();
        std::fs::write(
            scratch.join("t.tsx"),
            r#"<tileset version="1.10" name="t" tilewidth="8" tileheight="8" tilecount="0" columns="0"/>"#,
        )
        .unwrap();
        let text = r#"Here you go:
```xml
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="2" height="1" tilewidth="8" tileheight="8" infinite="0" nextlayerid="2" nextobjectid="1">
 <tileset firstgid="1" source="t.tsx"/>
 <layer id="1" name="ground" width="2" height="1">
  <data encoding="csv">
3,0
</data>
 </layer>
</map>
```"#;
        let mut loader = Loader::default();
        let map = loader
            .parse_fragment(text, &scratch.join("here.tmx"))
            .unwrap()
            .value;
        let cell = map.layers[0].as_tile_layer().unwrap().cell(IVec2::ZERO);
        assert_eq!(cell, Cell::new(TilesetSlot::new(0).unwrap(), TileId(2)));
        let fragment = fragment_text(&map, Some(&loader)).text;
        let tileset = scratch.join("t.tsx").display().to_string();
        assert!(
            fragment.contains(&format!("source=\"{tileset}\"")),
            "{fragment}"
        );
        let again = loader
            .parse_fragment(&fragment, Path::new("/elsewhere/x.tmx"))
            .unwrap()
            .value;
        assert_eq!(again.layers, map.layers);
    }
}
