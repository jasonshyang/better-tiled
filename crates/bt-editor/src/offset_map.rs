//! Offset Map with the values of its dialog: the contents of the chosen layers shift by whole
//! cells, within the whole map or the selection's bounds, wrapping when asked, as one step
//! (Tiled's `OffsetMapDialog` and `MainWindow::offsetMap`,
//! `reference/tiled/src/tiled/offsetmapdialog.cpp` and `tiled/mainwindow.cpp`). As in Tiled the
//! layers are chosen among those that are not groups; unlike Tiled, locked layers stay as they
//! are and are named, since a lock means the layer is not to be edited.

use bt_core::{LayerId, LayerKind, Map};
use bt_edit::offset_map;
use glam::IVec2;

use crate::document::{DocumentId, Selection};
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::layer_commands::locked_layers_notice;
use crate::notice::Notice;

/// Which layers Offset Map moves, as its Layers box offers them (Tiled's `LayerSelection`).
/// Groups are never among them: their layers are.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum LayerChoice {
    /// Every layer shown by its own visibility (Tiled's default).
    #[default]
    AllVisible,
    /// Every layer.
    All,
    /// The layers selected in the Layers panel.
    Selected,
}

/// What Offset Map does: how far the contents of which layers move, within what, and whether
/// what is pushed out comes back on the other side.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MapOffset {
    /// How far, in cells.
    pub cells: IVec2,
    /// Whether tiles pushed out at the left or the right come back on the other side.
    pub wrap_x: bool,
    /// Whether tiles pushed out at the top or the bottom come back on the other side.
    pub wrap_y: bool,
    /// Within the bounds of the selected cells rather than the whole map.
    pub within_selection: bool,
    /// The layers whose contents move.
    pub layers: LayerChoice,
}

/// The layers `choice` names in `map`, groups left out, depth first (Tiled's
/// `OffsetMapDialog::affectedLayers`).
fn chosen_layers(map: &Map, selection: &Selection, choice: LayerChoice) -> Vec<LayerId> {
    let mut chosen = Vec::new();
    map.for_each_layer(|layer, _| {
        let wanted = match choice {
            LayerChoice::AllVisible => layer.visible,
            LayerChoice::All => true,
            LayerChoice::Selected => selection.layers.contains(&layer.id),
        };
        if wanted && !matches!(layer.kind, LayerKind::Group(_)) {
            chosen.push(layer.id);
        }
    });
    chosen
}

/// Why `choice` names no layer to move.
fn nothing_chosen(choice: LayerChoice) -> &'static str {
    match choice {
        LayerChoice::AllVisible => "Offset Map has no layer to move: none is shown",
        LayerChoice::All => "Offset Map has no layer to move: the map has none",
        LayerChoice::Selected => {
            "Offset Map has no layer to move: select layers (a group's layers move, not the group)"
        }
    }
}

impl Editor {
    /// Offset Map: the contents of the layers `offset` chooses move as it says, as one step;
    /// locked layers stay and are named. Says why when nothing can move.
    pub(crate) fn offset_map_contents(
        &mut self,
        doc: DocumentId,
        offset: MapOffset,
    ) -> Vec<Effect> {
        let Some(document) = self.documents.get(doc) else {
            return vec![warn(EditError::NoSuchDocument(doc))];
        };
        let asset = document.asset();
        let selection = document.selection();
        let Some(map) = self.store.map(asset) else {
            return vec![warn(EditError::NoSuchDocument(doc))];
        };
        let area = if offset.within_selection {
            match selection.tiles.bounds() {
                Some(bounds) => Some(bounds),
                None => {
                    return vec![Effect::Notify(Notice::info(
                        "Offset Map works within the selection: no tiles are selected",
                    ))];
                }
            }
        } else {
            None
        };
        let chosen = chosen_layers(map, selection, offset.layers);
        if chosen.is_empty() {
            return vec![Effect::Notify(Notice::info(nothing_chosen(offset.layers)))];
        }
        let locked = |id: &LayerId| map.effective_layer(*id).is_none_or(|layer| layer.locked);
        let moving: Vec<LayerId> = chosen.iter().copied().filter(|id| !locked(id)).collect();
        let stayed: Vec<LayerId> = chosen.iter().copied().filter(locked).collect();
        let mut effects: Vec<Effect> = locked_layers_notice(
            map,
            &stayed,
            "its contents stayed where they were",
            "their contents stayed where they were",
        )
        .into_iter()
        .collect();
        if moving.is_empty() {
            return effects;
        }
        let result = self.transact(doc, "Offset Map", |tx| {
            let map = tx.map_mut(asset)?;
            offset_map(
                map,
                &moving,
                area,
                offset.cells,
                offset.wrap_x,
                offset.wrap_y,
            )
            .map_err(EditError::from)?;
            Ok::<_, EditError>(())
        });
        effects.extend(result.err().map(warn));
        effects
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Cell, Layer};
    use bt_edit::{SelectionMode, TileRegion};
    use bt_test_data::fixtures::{Atlas, MapBuilder, grid_layer};
    use bt_utils::IRect;
    use glam::UVec2;

    use super::*;
    use crate::actions::ActionId;
    use crate::effect::Dialog;
    use crate::intent::Intent;

    /// A 4×2 map of 16-pixel cells: "ground" (1) with a row of four tiles, "hidden" (2) with a
    /// tile and not shown, and the group "walls" (3) holding "wall" (4) with a tile; "ground"
    /// is current.
    fn editor_with_map() -> (Editor, DocumentId) {
        let atlas = Atlas::builder().name("tiles").build_embedded().unwrap();
        let mut hidden = Layer::new(
            LayerId(0),
            "hidden",
            LayerKind::Tile(grid_layer("0 . . .\n. . . .").unwrap()),
        );
        hidden.visible = false;
        let wall = Layer::new(
            LayerId(0),
            "wall",
            LayerKind::Tile(grid_layer("0 . . .\n. . . .").unwrap()),
        );
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 2))
            .tileset(atlas.tileset.clone())
            .tile_layer("ground", "0 1 2 3\n. . . .")
            .layer(hidden)
            .layer(Layer::new(
                LayerId(0),
                "walls",
                LayerKind::Group(vec![wall]),
            ))
            .build()
            .unwrap();
        let mut editor = Editor::new();
        let doc = editor.open_new_map(map);
        editor.documents.set_current(doc);
        editor.apply(Intent::SelectLayer {
            doc,
            layer: LayerId(1),
        });
        (editor, doc)
    }

    /// The cells of a layer's first row.
    fn first_row(editor: &Editor, doc: DocumentId, layer: u32) -> Vec<Cell> {
        let map = editor.map_of(doc).unwrap();
        let tiles = map.layer(LayerId(layer)).unwrap().as_tile_layer().unwrap();
        (0..4).map(|x| tiles.cell(IVec2::new(x, 0))).collect()
    }

    fn notices(effects: &[Effect]) -> Vec<String> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Notify(notice) => Some(notice.text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_shown_layers_wrap_around_the_whole_map_in_one_step() {
        let (mut editor, doc) = editor_with_map();
        let shown = editor.apply(Intent::Run(ActionId::OffsetMap));
        assert!(
            matches!(&shown[..], [Effect::Show(Dialog::OffsetMap(d))] if *d == doc),
            "{shown:?}"
        );
        let ground = first_row(&editor, doc, 1);
        let hidden = first_row(&editor, doc, 2);
        let wall = first_row(&editor, doc, 4);
        let effects = editor.apply(Intent::OffsetMap {
            doc,
            offset: MapOffset {
                cells: IVec2::new(1, 0),
                wrap_x: true,
                ..MapOffset::default()
            },
        });
        assert!(effects.is_empty(), "{effects:?}");
        let moved = first_row(&editor, doc, 1);
        assert_eq!(moved, [ground[3], ground[0], ground[1], ground[2]]);
        assert_eq!(first_row(&editor, doc, 2), hidden, "not shown");
        assert_eq!(first_row(&editor, doc, 4)[1], wall[0], "in a shown group");
        editor.apply(Intent::Run(ActionId::Undo));
        assert_eq!(first_row(&editor, doc, 1), ground, "one step");
        assert_eq!(first_row(&editor, doc, 4), wall);
    }

    #[test]
    fn selected_layers_move_within_the_selection_and_locked_ones_stay() {
        let (mut editor, doc) = editor_with_map();
        let ground = first_row(&editor, doc, 1);
        let hidden = first_row(&editor, doc, 2);
        let only_selected = MapOffset {
            cells: IVec2::new(1, 0),
            within_selection: true,
            layers: LayerChoice::Selected,
            ..MapOffset::default()
        };
        let effects = editor.apply(Intent::OffsetMap {
            doc,
            offset: only_selected,
        });
        assert_eq!(
            notices(&effects),
            ["Offset Map works within the selection: no tiles are selected"]
        );
        editor.apply(Intent::SelectTiles {
            doc,
            region: TileRegion::from_rect(IRect::from_min_size(IVec2::ZERO, IVec2::new(2, 1))),
            mode: SelectionMode::Replace,
        });
        editor.apply(Intent::SetLayerLocked {
            doc,
            layer: LayerId(2),
            locked: true,
        });
        editor.documents.get_mut(doc).unwrap().selection.layers = vec![LayerId(1), LayerId(2)];
        let effects = editor.apply(Intent::OffsetMap {
            doc,
            offset: only_selected,
        });
        assert_eq!(
            notices(&effects),
            ["“hidden” is locked: its contents stayed where they were"]
        );
        // Within the first two cells the first moves over the second, which is pushed out; the
        // others are outside the selection.
        assert_eq!(
            first_row(&editor, doc, 1),
            [Cell::default(), ground[0], ground[2], ground[3]]
        );
        assert_eq!(first_row(&editor, doc, 2), hidden);
    }

    #[test]
    fn a_selected_group_alone_moves_nothing_and_says_why() {
        let (mut editor, doc) = editor_with_map();
        editor.apply(Intent::SelectLayer {
            doc,
            layer: LayerId(3),
        });
        let wall = first_row(&editor, doc, 4);
        let effects = editor.apply(Intent::OffsetMap {
            doc,
            offset: MapOffset {
                cells: IVec2::new(1, 0),
                layers: LayerChoice::Selected,
                ..MapOffset::default()
            },
        });
        assert_eq!(notices(&effects), [nothing_chosen(LayerChoice::Selected)]);
        assert_eq!(first_row(&editor, doc, 4), wall);
    }
}
