//! The tileset editor through the editor: Edit Tileset shows a map's embedded tileset in place
//! of the map, its tiles are selected and animated as undoable steps, the map's commands wait
//! until the map is back, and a tileset file opens in a tab of its own and saves to its file.

#![cfg(test)]

mod common;

use bt_core::{Frame, TileId, TilesetSlot};
use bt_editor::{ActionId, Editor, Enabled, Intent, TilesetHome, TilesetMode, ToolKind};
use bt_test_data::Scratch;
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_tmx::WriteRequest;
use glam::UVec2;

use common::{apply, editor_in};

fn frames(tiles: &[u32]) -> Vec<Frame> {
    tiles
        .iter()
        .map(|&id| Frame {
            tile: TileId(id),
            duration_ms: 100,
        })
        .collect()
}

#[test]
fn an_embedded_tileset_is_edited_in_place_of_its_map() {
    let atlas = Atlas::builder().build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 3))
        .tileset(atlas.tileset)
        .tile_layer("ground", "1 1 1 1\n1 1 1 1\n1 1 1 1")
        .build()
        .unwrap();
    let mut editor = Editor::new();
    let doc = editor.open_new_map(map);
    let slot = TilesetSlot::new(0).unwrap();
    let asset = editor.document(doc).unwrap().asset();

    editor.apply(Intent::EditTileset { doc, slot });
    let editing = editor.document(doc).unwrap().tileset_editing().unwrap();
    assert_eq!(editing.home(), TilesetHome::Embedded { map: asset, slot });
    assert_eq!(
        editor.enabled(ActionId::BrushTool),
        Enabled::Disabled(Some(
            "Brush works on the map: the tileset editor is open".to_owned()
        ))
    );

    // Tiles are chosen, and one is animated as an undoable step on the map.
    editor.apply(Intent::SelectTilesetTiles {
        doc,
        tiles: vec![TileId(1), TileId(2)],
        current: Some(TileId(1)),
    });
    let animate = |editor: &mut Editor, tiles: &[u32]| {
        editor.apply(Intent::SetTileAnimation {
            doc,
            tile: TileId(1),
            frames: frames(tiles),
        })
    };
    animate(&mut editor, &[1, 2, 3]);
    let tileset = editor.edited_tileset(doc).unwrap();
    assert_eq!(
        tileset.tile(TileId(1)).unwrap().animation,
        frames(&[1, 2, 3])
    );
    assert_eq!(
        editor.map_of(doc).unwrap().tilesets[0]
            .tileset
            .tile(TileId(1)),
        tileset.tile(TileId(1)),
        "the map holds the change"
    );
    assert!(editor.has_unsaved());
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(
        editor.edited_tileset(doc).unwrap().tile(TileId(1)),
        None,
        "an atlas keeps no data for a tile without any"
    );
    let refused = animate(&mut editor, &[1, 99]);
    assert!(!refused.is_empty(), "a frame of a missing tile is refused");
    assert_eq!(editor.edited_tileset(doc).unwrap().tile(TileId(1)), None);

    // Collision shapes are edited with the object tools; back on the map the tile tools return.
    editor.apply(Intent::Run(ActionId::BrushTool));
    editor.apply(Intent::SetTilesetMode {
        doc,
        mode: TilesetMode::Collision,
    });
    assert_eq!(editor.tools().tool(), ToolKind::SelectObjects);
    editor.apply(Intent::StopEditingTileset { doc });
    assert!(editor.document(doc).unwrap().tileset_editing().is_none());
    assert_eq!(editor.tools().tool().family(), bt_editor::ToolFamily::Tile);
    assert_eq!(editor.enabled(ActionId::BrushTool), Enabled::Enabled);
}

#[test]
fn a_tileset_file_opens_in_its_own_tab_and_saves_there() {
    let scratch = Scratch::new("bt-editor-tileset-document").unwrap();
    let atlas = Atlas::builder()
        .name("tiles")
        .build_in(scratch.path())
        .unwrap();
    let path = scratch.join("tiles.tsx");
    WriteRequest::for_tileset(&atlas.tileset, &path)
        .write()
        .unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path.clone()));
    let doc = editor.current().expect("the tileset is open");
    let editing = editor.document(doc).unwrap().tileset_editing().unwrap();
    assert!(matches!(editing.home(), TilesetHome::Asset(_)));
    assert!(editor.map_of(doc).is_none(), "a tileset's tab has no map");
    assert_eq!(
        editor.enabled(ActionId::ResizeMap),
        Enabled::Disabled(Some(
            "Resize Map works on the map: the tileset editor is open".to_owned()
        ))
    );

    apply(
        &mut editor,
        Intent::SetTileAnimation {
            doc,
            tile: TileId(0),
            frames: frames(&[0, 1]),
        },
    );
    assert!(editor.has_unsaved());
    apply(&mut editor, Intent::Run(ActionId::Save));
    assert!(!editor.has_unsaved());
    let saved = bt_tmx::Loader::builder()
        .build()
        .load_tileset(&path)
        .unwrap()
        .value;
    assert_eq!(saved.tile(TileId(0)).unwrap().animation, frames(&[0, 1]));

    // Opening it again shows the same tab.
    let tabs = editor.documents().in_order().count();
    apply(&mut editor, Intent::Open(path));
    assert_eq!(editor.documents().in_order().count(), tabs);
}

#[test]
fn collision_shapes_are_edited_with_the_object_tools_and_commands() {
    use bt_core::{MapObject, ObjectId};
    use bt_editor::{ObjectStroke, ObjectStrokeKind, SnapToggle};
    use glam::DVec2;

    let atlas = Atlas::builder().build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 2))
        .tileset(atlas.tileset)
        .tile_layer("ground", "1 1\n1 1")
        .build()
        .unwrap();
    let mut editor = Editor::new();
    let doc = editor.open_new_map(map);
    editor.apply(Intent::EditTileset {
        doc,
        slot: TilesetSlot::new(0).unwrap(),
    });
    editor.apply(Intent::SelectTilesetTiles {
        doc,
        tiles: vec![TileId(3)],
        current: Some(TileId(3)),
    });
    editor.apply(Intent::SetTilesetMode {
        doc,
        mode: TilesetMode::Collision,
    });
    let shapes = |editor: &Editor| -> Vec<(u32, DVec2)> {
        editor
            .edited_tileset(doc)
            .unwrap()
            .tile(TileId(3))
            .and_then(|data| data.collision.as_ref())
            .map(|group| {
                group
                    .layer
                    .objects
                    .iter()
                    .map(|o| (o.id.0, o.pos))
                    .collect()
            })
            .unwrap_or_default()
    };
    let selected = |editor: &Editor| editor.document(doc).unwrap().selection().objects.clone();

    // A shape inserted goes into the tile, with the tile's first id, and is selected.
    editor.apply(Intent::InsertObject {
        doc,
        object: Box::new(MapObject {
            pos: DVec2::new(2.0, 2.0),
            size: DVec2::new(8.0, 8.0),
            ..MapObject::default()
        }),
    });
    assert_eq!(shapes(&editor), [(1, DVec2::new(2.0, 2.0))]);
    assert_eq!(selected(&editor), [ObjectId(1)]);
    assert_eq!(
        editor.objects_at(doc, DVec2::new(5.0, 5.0), 1.0),
        [ObjectId(1)]
    );
    assert!(
        editor.map_of(doc).unwrap().layers.len() == 1,
        "the map gained no layer"
    );

    // A drag moves it as one step.
    for step in [
        ObjectStroke::Begin {
            doc,
            kind: ObjectStrokeKind::Move,
        },
        ObjectStroke::Move {
            delta: DVec2::new(4.0, 0.0),
            snap: SnapToggle::Keep,
        },
        ObjectStroke::End,
    ] {
        editor.apply(Intent::ObjectStroke(step));
    }
    assert_eq!(shapes(&editor), [(1, DVec2::new(6.0, 2.0))]);
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(shapes(&editor), [(1, DVec2::new(2.0, 2.0))]);

    // Copy and Paste in Place make a second shape; Select All and Delete leave the tile
    // without a collision group, as Tiled does.
    editor.apply(Intent::Run(ActionId::Copy));
    editor.apply(Intent::Run(ActionId::PasteInPlace));
    assert_eq!(
        shapes(&editor),
        [(1, DVec2::new(2.0, 2.0)), (2, DVec2::new(2.0, 2.0))]
    );
    editor.apply(Intent::Run(ActionId::SelectAll));
    assert_eq!(selected(&editor), [ObjectId(1), ObjectId(2)]);
    editor.apply(Intent::Run(ActionId::Delete));
    assert!(
        editor
            .edited_tileset(doc)
            .unwrap()
            .tile(TileId(3))
            .is_none(),
        "no shapes, no group, no data"
    );
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(shapes(&editor).len(), 2);
    assert_eq!(
        editor.enabled(ActionId::MoveObjectsToLayer),
        Enabled::Disabled(Some("Collision shapes stay with their tile".to_owned()))
    );
}
