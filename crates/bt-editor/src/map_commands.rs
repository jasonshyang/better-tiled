//! The Map menu's commands on the map's size: Resize Map with the values of its dialog, Crop to
//! Selection and Autocrop, each one step; the selection moves with the map's contents (Tiled's
//! `MainWindow::resizeMap` and `MapDocument::resizeMap`, `reference/tiled/src/tiled/mainwindow.cpp`
//! and `tiled/mapdocument.cpp`).

use bt_core::Map;
use bt_edit::{ResizeReport, autocrop, crop_to, resize_map};
use glam::{IVec2, UVec2};

use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::notice::Notice;

/// Why Resize Map cannot change `map`, if it cannot: an infinite map has no size to change (its
/// tile layers grow wherever tiles are painted).
pub(crate) fn resize_refusal(map: &Map) -> Option<String> {
    map.infinite
        .then(|| "Resize Map works on maps of a fixed size: this map is infinite".to_owned())
}

/// Why a map cannot become `size` cells, if it cannot.
fn size_problem(size: UVec2) -> Option<String> {
    if size.x == 0 || size.y == 0 {
        return Some("The map must be at least one tile wide and high".to_owned());
    }
    if i32::try_from(size.x.max(size.y)).is_err() {
        return Some("The map is larger than a map can be".to_owned());
    }
    None
}

impl Editor {
    /// Resize Map: the map becomes `size` cells with its contents moved by `offset` cells, as
    /// one step; with `remove_objects_outside`, objects that end up outside it go. Refused, with
    /// the reason, for an infinite map and for a size no map can have.
    pub(crate) fn resize_map_to(
        &mut self,
        doc: DocumentId,
        size: UVec2,
        offset: IVec2,
        remove_objects_outside: bool,
    ) -> Vec<Effect> {
        let Some(map) = self.map_of(doc) else {
            return vec![warn(EditError::NoSuchDocument(doc))];
        };
        if let Some(problem) = resize_refusal(map).or_else(|| size_problem(size)) {
            return vec![Effect::Notify(Notice::error(problem))];
        }
        self.resize_command(doc, "Resize Map", |map| {
            resize_map(map, size, offset, remove_objects_outside)
        })
    }

    /// Crop to Selection: the map cut down to the selected cells' bounding rectangle.
    pub(crate) fn crop_to_selection(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(bounds) = self
            .documents
            .get(doc)
            .and_then(|d| d.selection().tiles.bounds())
        else {
            return Vec::new();
        };
        self.resize_command(doc, "Crop to Selection", |map| crop_to(map, bounds))
    }

    /// Autocrop: the map cut down to the cells its tiles use.
    pub(crate) fn autocrop_map(&mut self, doc: DocumentId) -> Vec<Effect> {
        self.resize_command(doc, "Autocrop", autocrop)
    }

    /// Runs a resize as one step; the selected cells move with the map's contents.
    fn resize_command(
        &mut self,
        doc: DocumentId,
        label: &str,
        resize: impl FnOnce(&mut bt_core::Map) -> Result<ResizeReport, bt_edit::EditError>,
    ) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let worlds = self
            .path_of(doc)
            .map(|path| {
                self.worlds_for_path(&path)
                    .into_iter()
                    .map(|asset| WorldOwner {
                        asset,
                        path: path.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let result = self.transact(doc, label, |tx| {
            let map = tx.map_mut(asset)?;
            let geometry = map.geometry();
            let old_bounds = geometry.map_bounding_rect();
            let report = resize(map).map_err(EditError::from)?;
            let new_bounds = geometry.bounding_rect(bt_utils::IRect::from_min_size(
                IVec2::ZERO.saturating_sub(report.tile_offset),
                IVec2::new(map.width as i32, map.height as i32),
            ));
            // Worlds use the grid's projected bounding rectangle, not the object pixel offset.
            let delta = new_bounds.min() - old_bounds.min();
            let extent = map.geometry().map_bounding_rect().size();
            for world in &worlds {
                if let Some(entry) = tx
                    .world_mut(world.asset)?
                    .maps
                    .iter_mut()
                    .find(|entry| entry.file.path == world.path)
                {
                    entry.rect.x = entry.rect.x.saturating_add(delta.x);
                    entry.rect.y = entry.rect.y.saturating_add(delta.y);
                    entry.rect.w = extent.x;
                    entry.rect.h = extent.y;
                }
            }
            let moved = tx.selection.tiles.translated(report.tile_offset);
            tx.selection.tiles = moved;
            Ok::<_, EditError>(())
        });
        result.err().map(warn).into_iter().collect()
    }
}

struct WorldOwner {
    asset: crate::AssetId,
    path: std::path::PathBuf,
}

#[cfg(test)]
mod tests {
    use bt_core::{LayerId, MapObject, ObjectId};
    use bt_edit::{SelectionMode, TileRegion};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_utils::IRect;
    use glam::DVec2;

    use super::*;
    use crate::actions::{ActionId, Enabled};
    use crate::effect::Dialog;
    use crate::intent::Intent;

    /// A 4×3 map of 16-pixel cells: tiles at (0, 0) and (1, 0) on "ground", and on "things" an
    /// object in the first cell and one in the last.
    fn editor_with_map(infinite: bool) -> (Editor, DocumentId) {
        let atlas = Atlas::builder().name("tiles").build_embedded().unwrap();
        let object = |x: f64, y: f64| MapObject {
            pos: DVec2::new(x, y),
            size: DVec2::splat(8.0),
            ..MapObject::default()
        };
        let mut builder = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 3))
            .tileset(atlas.tileset.clone())
            .tile_layer("ground", "0 1 . .\n. . . .\n. . . .")
            .object_layer("things", vec![object(2.0, 2.0), object(50.0, 34.0)]);
        if infinite {
            builder = builder.infinite();
        }
        let mut editor = Editor::new();
        let doc = editor.open_new_map(builder.build().unwrap());
        editor.documents.set_current(doc);
        (editor, doc)
    }

    #[test]
    fn resize_map_shows_its_dialog_and_is_one_step() {
        let (mut editor, doc) = editor_with_map(false);
        let shown = editor.apply(Intent::Run(ActionId::ResizeMap));
        assert!(
            matches!(&shown[..], [Effect::Show(Dialog::ResizeMap(d))] if *d == doc),
            "{shown:?}"
        );
        editor.apply(Intent::SelectTiles {
            doc,
            region: TileRegion::from_rect(IRect::from_min_size(IVec2::ZERO, IVec2::ONE)),
            mode: SelectionMode::Replace,
        });
        let effects = editor.apply(Intent::ResizeMap {
            doc,
            size: UVec2::new(3, 4),
            offset: IVec2::new(-1, 1),
            remove_objects_outside: true,
        });
        assert!(effects.is_empty(), "{effects:?}");
        let map = editor.map_of(doc).unwrap();
        assert_eq!((map.width, map.height), (3, 4));
        // The first column went, with its tile and its object; the second tile moved.
        let ground = map.layer(LayerId(1)).unwrap().as_tile_layer().unwrap();
        assert!(!ground.cell(IVec2::new(0, 1)).is_empty());
        assert_eq!(
            IRect::from_min_size(IVec2::ZERO, IVec2::new(3, 4))
                .points()
                .filter(|p| !ground.cell(*p).is_empty())
                .count(),
            1
        );
        assert!(map.object(ObjectId(1)).is_none());
        assert_eq!(
            map.object(ObjectId(2)).unwrap().pos,
            DVec2::new(34.0, 50.0),
            "moved by the offset in pixels"
        );
        let selection = &editor.document(doc).unwrap().selection().tiles;
        assert_eq!(
            selection.bounds(),
            Some(IRect::from_min_size(IVec2::new(-1, 1), IVec2::ONE))
        );
        editor.apply(Intent::Run(ActionId::Undo));
        let map = editor.map_of(doc).unwrap();
        assert_eq!((map.width, map.height), (4, 3), "one step");
        assert!(map.object(ObjectId(1)).is_some());
    }

    #[test]
    fn objects_outside_stay_unless_asked_and_a_resize_to_nothing_is_refused() {
        let (mut editor, doc) = editor_with_map(false);
        editor.apply(Intent::ResizeMap {
            doc,
            size: UVec2::new(2, 2),
            offset: IVec2::ZERO,
            remove_objects_outside: false,
        });
        let map = editor.map_of(doc).unwrap();
        assert_eq!((map.width, map.height), (2, 2));
        assert!(map.object(ObjectId(2)).is_some(), "kept outside the map");
        let effects = editor.apply(Intent::ResizeMap {
            doc,
            size: UVec2::new(0, 2),
            offset: IVec2::ZERO,
            remove_objects_outside: false,
        });
        assert!(
            matches!(&effects[..], [Effect::Notify(notice)] if notice.text.contains("one tile")),
            "{effects:?}"
        );
        assert_eq!(editor.map_of(doc).unwrap().width, 2);
    }

    #[test]
    fn an_infinite_map_cannot_be_resized_and_says_why() {
        let (mut editor, doc) = editor_with_map(true);
        let reason = "Resize Map works on maps of a fixed size: this map is infinite";
        assert_eq!(
            editor.enabled(ActionId::ResizeMap),
            Enabled::Disabled(Some(reason.to_owned()))
        );
        let effects = editor.apply(Intent::ResizeMap {
            doc,
            size: UVec2::new(2, 2),
            offset: IVec2::ZERO,
            remove_objects_outside: false,
        });
        assert!(
            matches!(&effects[..], [Effect::Notify(notice)] if notice.text == reason),
            "{effects:?}"
        );
        assert_eq!(editor.map_of(doc).unwrap().width, 4);
    }
}
