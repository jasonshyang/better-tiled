//! What the Layers panel asks of the editor: several layers selected with one of them current,
//! layers dragged into and out of groups as one step, a drag of the opacity slider as one step,
//! and the commands not built yet saying so.

#![cfg(test)]

use bt_core::{Layer, LayerId, LayerKind, Map};
use bt_editor::{ActionId, DocumentId, Editor, Effect, Enabled, Intent, LayerLocation, ToolKind};
use bt_test_data::fixtures::MapBuilder;
use glam::UVec2;

/// `1:ground 5:walls[2:floor 3:things] 4:sky`, all tile layers but the object layer “things”;
/// the sky current.
fn editor() -> (Editor, DocumentId) {
    let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 3))
        .tile_layer("ground", "")
        .tile_layer("floor", "")
        .object_layer("things", Vec::new())
        .tile_layer("sky", "")
        .build()
        .unwrap();
    let mut layers = std::mem::take(&mut map.layers).into_iter();
    let (Some(ground), Some(floor), Some(things), Some(sky)) =
        (layers.next(), layers.next(), layers.next(), layers.next())
    else {
        panic!("four layers");
    };
    let walls = Layer::new(LayerId(5), "walls", LayerKind::Group(vec![floor, things]));
    map.layers = vec![ground, walls, sky];
    map.next_layer_id = 6;
    let mut editor = Editor::new();
    let doc = editor.open_new_map(map);
    (editor, doc)
}

/// The tree as `id:name`, bottom first, a group's layers in brackets after it.
fn tree(editor: &Editor, doc: DocumentId) -> String {
    fn walk(layers: &[Layer]) -> String {
        layers
            .iter()
            .map(|layer| match &layer.kind {
                LayerKind::Group(children) => {
                    format!("{}:{}[{}]", layer.id.0, layer.name, walk(children))
                }
                _ => format!("{}:{}", layer.id.0, layer.name),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
    walk(&editor.map_of(doc).unwrap().layers)
}

fn map(editor: &Editor, doc: DocumentId) -> &Map {
    editor.map_of(doc).unwrap()
}

fn labels(editor: &Editor, doc: DocumentId) -> Vec<String> {
    editor
        .document(doc)
        .unwrap()
        .history()
        .entries()
        .into_iter()
        .filter(|entry| entry.done)
        .map(|entry| entry.label)
        .collect()
}

#[test]
fn several_layers_are_selected_with_one_current() {
    let (mut editor, doc) = editor();
    let effects = editor.apply(Intent::SelectLayers {
        doc,
        current: LayerId(3),
        layers: vec![LayerId(4), LayerId(1)],
    });
    assert!(effects.is_empty(), "{effects:?}");
    let selection = editor.document(doc).unwrap().selection().clone();
    assert_eq!(selection.current_layer, Some(LayerId(3)));
    assert_eq!(
        selection.layers,
        [4, 1, 3].map(LayerId),
        "the current one too"
    );
    assert_eq!(
        editor.tools().tool(),
        ToolKind::SelectObjects,
        "the tool follows the current layer"
    );

    // The layer commands act on all of them.
    editor.apply(Intent::Run(ActionId::ToggleSelectedLayers));
    let shown: Vec<bool> = [1, 2, 3, 4]
        .map(|id| map(&editor, doc).layer(LayerId(id)).unwrap().visible)
        .to_vec();
    assert_eq!(shown, [false, true, false, false]);

    // A layer the map does not have changes nothing.
    let effects = editor.apply(Intent::SelectLayers {
        doc,
        current: LayerId(1),
        layers: vec![LayerId(9)],
    });
    assert!(
        matches!(&effects[..], [Effect::Notify(n)] if n.text == "the map has no layer 9"),
        "{effects:?}"
    );
    assert_eq!(
        editor.document(doc).unwrap().selection().layers,
        [4, 1, 3].map(LayerId)
    );
}

#[test]
fn layers_are_dragged_into_and_out_of_groups_as_one_step() {
    let (mut editor, doc) = editor();
    let before = editor.document(doc).unwrap().selection().clone();
    editor.apply(Intent::MoveLayers {
        doc,
        layers: vec![LayerId(4)],
        to: LayerLocation {
            parent: Some(LayerId(5)),
            index: 2,
        },
    });
    assert_eq!(
        tree(&editor, doc),
        "1:ground 5:walls[2:floor 3:things 4:sky]"
    );
    assert_eq!(
        *editor.document(doc).unwrap().selection(),
        before,
        "the selection stays"
    );

    // The floor goes with its group, which goes to the bottom.
    editor.apply(Intent::MoveLayers {
        doc,
        layers: vec![LayerId(5), LayerId(2)],
        to: LayerLocation {
            parent: None,
            index: 0,
        },
    });
    assert_eq!(
        tree(&editor, doc),
        "5:walls[2:floor 3:things 4:sky] 1:ground"
    );
    assert_eq!(labels(&editor, doc), ["Drag Layer", "Drag Layer"]);
    editor.apply(Intent::Run(ActionId::Undo));
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(
        tree(&editor, doc),
        "1:ground 5:walls[2:floor 3:things] 4:sky"
    );

    // A group cannot go into itself.
    let effects = editor.apply(Intent::MoveLayers {
        doc,
        layers: vec![LayerId(5)],
        to: LayerLocation {
            parent: Some(LayerId(5)),
            index: 0,
        },
    });
    assert!(matches!(&effects[..], [Effect::Notify(_)]), "{effects:?}");
    assert_eq!(
        tree(&editor, doc),
        "1:ground 5:walls[2:floor 3:things] 4:sky"
    );
}

#[test]
fn a_drag_of_the_opacity_slider_is_one_step() {
    let (mut editor, doc) = editor();
    let opacity = |editor: &Editor, id: u32| map(editor, doc).layer(LayerId(id)).unwrap().opacity;
    let set = |editor: &mut Editor, opacity: f64, done: bool| {
        editor.apply(Intent::SetLayerOpacity {
            doc,
            layers: vec![LayerId(1), LayerId(4)],
            opacity,
            done,
        })
    };
    for value in [0.8, 0.5] {
        set(&mut editor, value, false);
    }
    set(&mut editor, 0.25, true);
    assert_eq!((opacity(&editor, 1), opacity(&editor, 4)), (0.25, 0.25));
    assert_eq!(opacity(&editor, 2), 1.0);
    assert_eq!(labels(&editor, doc), ["Change Layer Opacity"]);
    // The next drag is a step of its own.
    set(&mut editor, 0.75, true);
    assert_eq!(labels(&editor, doc).len(), 2);
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(opacity(&editor, 4), 0.25);
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(opacity(&editor, 4), 1.0);

    let effects = set(&mut editor, 1.5, true);
    assert!(matches!(&effects[..], [Effect::Notify(_)]), "{effects:?}");
    assert_eq!(opacity(&editor, 4), 1.0);
}

#[test]
fn commands_still_to_come_say_so() {
    let (editor, _) = editor();
    assert_eq!(
        editor.enabled(ActionId::LayerProperties),
        Enabled::Disabled(Some("Layer Properties is not built yet".to_owned()))
    );
}
