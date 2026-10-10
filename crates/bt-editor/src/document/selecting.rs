//! Selecting: the current layer, everything the selected layers hold, nothing, or the rest. A
//! change of selection alone is not a step of the history.

use bt_core::{LayerKind, Map};
use bt_edit::{TileRegion, invert};
use bt_utils::IRect;

use super::document::{DocumentId, ViewToggles};
use super::selection::Selection;
use crate::editor::Editor;
use crate::effect::Effect;
use crate::tools::ToolFamily;

impl Editor {
    /// Makes `layer` current and the only selected layer; the selected tiles and objects stay,
    /// as in Tiled. The tool follows the layer's family.
    pub(crate) fn select_layer(&mut self, doc: DocumentId, layer: bt_core::LayerId) {
        if let Some(document) = self.documents.get_mut(doc) {
            document.selection = document.selection.with_current_layer(layer);
        }
        let family = self.current_family(doc);
        self.tools.follow_layer(family);
        self.session_changed();
    }

    /// Selects tiles: `region` combined with the selection as `mode` says, kept to the map's
    /// cells on a finite map.
    pub(crate) fn select_tiles(
        &mut self,
        doc: DocumentId,
        region: &TileRegion,
        mode: bt_edit::SelectionMode,
    ) {
        let Some(map) = self.map_of(doc).cloned() else {
            return;
        };
        let region = if map.infinite {
            region.clone()
        } else {
            region.clipped(map.tile_bounding_rect())
        };
        self.change_selection(doc, |selection| {
            selection.tiles = mode.apply(&selection.tiles, &region);
        });
    }

    /// The family of tools `doc` takes: the object tools for a tileset editor's collision
    /// shapes, none for its other modes, else the current layer's.
    pub(crate) fn current_family(&self, doc: DocumentId) -> Option<ToolFamily> {
        if let Some(editing) = self.documents.get(doc)?.tileset_editing() {
            return (editing.mode() == crate::document::TilesetMode::Collision)
                .then_some(ToolFamily::Object);
        }
        let id = self.documents.get(doc)?.selection().current_layer?;
        Some(match self.map_of(doc)?.layer(id)?.kind {
            LayerKind::Tile(_) => ToolFamily::Tile,
            LayerKind::Object(_) => ToolFamily::Object,
            LayerKind::Image(_) | LayerKind::Group(_) => ToolFamily::Any,
        })
    }

    /// Select All: the cells of every selected unlocked tile layer (the map on a finite map, the
    /// allocated chunks on an infinite one) and the objects of every selected unlocked object
    /// layer (Tiled's `selectAll`).
    pub(crate) fn select_all(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(map) = self.map_of(doc).cloned() else {
            return Vec::new();
        };
        self.change_selection(doc, |selection| {
            let mut tiles = TileRegion::new();
            let mut objects = Vec::new();
            for id in &selection.layers {
                let unlocked = map.effective_layer(*id).is_some_and(|l| !l.locked);
                let Some(layer) = map.layer(*id).filter(|_| unlocked) else {
                    continue;
                };
                match &layer.kind {
                    LayerKind::Tile(cells) => {
                        tiles = tiles.union(&TileRegion::from_rect(layer_area(&map, cells)));
                    }
                    LayerKind::Object(group) => objects.extend(group.objects.iter().map(|o| o.id)),
                    LayerKind::Image(_) | LayerKind::Group(_) => {}
                }
            }
            selection.tiles = tiles;
            selection.objects = objects;
        });
        Vec::new()
    }

    /// Select None: no tiles and no objects.
    pub(crate) fn select_none(&mut self, doc: DocumentId) -> Vec<Effect> {
        self.change_selection(doc, |selection| {
            selection.tiles = TileRegion::new();
            selection.objects.clear();
        });
        Vec::new()
    }

    /// Invert Selection on the current layer: the cells of a tile layer's area that are not
    /// selected, or the objects of an object layer that are not.
    pub(crate) fn invert_selection(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(map) = self.map_of(doc).cloned() else {
            return Vec::new();
        };
        self.change_selection(doc, |selection| {
            let Some(layer) = selection.current_layer.and_then(|id| map.layer(id)) else {
                return;
            };
            match &layer.kind {
                LayerKind::Tile(cells) => {
                    selection.tiles = invert(&selection.tiles, layer_area(&map, cells));
                }
                LayerKind::Object(group) => {
                    selection.objects = group
                        .objects
                        .iter()
                        .map(|o| o.id)
                        .filter(|id| !selection.objects.contains(id))
                        .collect();
                }
                LayerKind::Image(_) | LayerKind::Group(_) => {}
            }
        });
        Vec::new()
    }

    /// Changes a document's selection outside any transaction; carried tiles are let go when
    /// the selected cells are no longer theirs.
    fn change_selection(&mut self, doc: DocumentId, change: impl FnOnce(&mut Selection)) {
        if let Some(document) = self.documents.get_mut(doc) {
            change(&mut document.selection);
            document.selection.drop_stale_carry();
        }
    }

    /// Flips one of the current document's View switches.
    pub(crate) fn toggle_view(&mut self, toggle: impl FnOnce(&mut ViewToggles)) {
        if let Some(doc) = self.documents.current()
            && let Some(document) = self.documents.get_mut(doc)
        {
            toggle(&mut document.view);
        }
    }
}

/// A tile layer's area: the map's cells on a finite map, the allocated chunks on an infinite
/// one.
fn layer_area(map: &Map, cells: &bt_core::TileLayer) -> IRect {
    if map.infinite {
        cells.chunk_bounds().unwrap_or_default()
    } else {
        map.tile_bounding_rect()
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{LayerId, MapObject, ObjectId};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use crate::actions::ActionId;
    use crate::editor::Editor;
    use crate::intent::Intent;
    use crate::tools::ToolKind;

    fn editor_with_map() -> (Editor, crate::document::DocumentId) {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 3))
            .tileset(atlas.tileset)
            .tile_layer("ground", "1 1 1 1\n1 1 1 1\n1 1 1 1")
            .object_layer("things", vec![MapObject::default(), MapObject::default()])
            .build()
            .unwrap();
        let mut editor = Editor::new();
        let doc = editor.open_new_map(map);
        (editor, doc)
    }

    #[test]
    fn select_all_takes_the_selected_layers_contents_and_invert_the_rest() {
        let (mut editor, doc) = editor_with_map();
        editor.apply(Intent::SelectLayer {
            doc,
            layer: LayerId(1),
        });
        editor.apply(Intent::Run(ActionId::SelectAll));
        let selection = editor.document(doc).unwrap().selection().clone();
        assert_eq!(selection.tiles.len(), 12);
        assert!(
            selection.objects.is_empty(),
            "the object layer is not selected"
        );
        editor.apply(Intent::Run(ActionId::InvertSelection));
        assert!(editor.document(doc).unwrap().selection().tiles.is_empty());

        editor.apply(Intent::SelectLayer {
            doc,
            layer: LayerId(2),
        });
        editor.apply(Intent::Run(ActionId::SelectAll));
        let selection = editor.document(doc).unwrap().selection().clone();
        assert_eq!(selection.objects, [ObjectId(1), ObjectId(2)]);
        editor.apply(Intent::Run(ActionId::SelectNone));
        assert!(editor.document(doc).unwrap().selection().objects.is_empty());
    }

    #[test]
    fn the_tool_follows_the_current_layers_family() {
        let (mut editor, doc) = editor_with_map();
        editor.apply(Intent::SelectLayer {
            doc,
            layer: LayerId(1),
        });
        editor.apply(Intent::Run(ActionId::EraserTool));
        assert_eq!(editor.tools().tool(), ToolKind::Eraser);
        editor.apply(Intent::SelectLayer {
            doc,
            layer: LayerId(2),
        });
        assert_eq!(editor.tools().tool(), ToolKind::SelectObjects);
        // The Brush's key on an object layer says why it does nothing.
        let effects = editor.apply(Intent::Run(ActionId::BrushTool));
        assert_eq!(editor.tools().tool(), ToolKind::SelectObjects);
        assert!(
            matches!(&effects[..], [crate::effect::Effect::Notify(n)] if n.text == "Brush works on tile layers: “things” is an object layer"),
            "{effects:?}"
        );
        editor.apply(Intent::SelectLayer {
            doc,
            layer: LayerId(1),
        });
        assert_eq!(editor.tools().tool(), ToolKind::Eraser);
    }
}
