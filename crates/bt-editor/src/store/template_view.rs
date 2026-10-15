//! An immutable map view of a template base, matching Tiled's dummy-map editing view. It is
//! derived from the template asset; transactions convert edited views back before recording.

use bt_core::{Layer, LayerId, LayerKind, Map, ObjectId, ObjectLayer, ObjectTemplate};
use bt_edit::{EditError, ObjectSpace, object_bounds};
use glam::{DVec2, UVec2};

pub(super) fn of(template: &ObjectTemplate) -> Map {
    let mut objects = Vec::new();
    let mut size = UVec2::splat(32);
    if let Some(base) = &template.object {
        let mut object = base.clone();
        object.id = ObjectId(1);
        object.template = None;
        let bounds = object_bounds(&ObjectSpace::of_template(template), &object);
        object.pos += DVec2::ONE - bounds.min;
        size = (bounds.max - bounds.min)
            .ceil()
            .max(DVec2::splat(30.0))
            .as_uvec2()
            + UVec2::splat(2);
        objects.push(object);
    }
    Map {
        width: size.x,
        height: size.y,
        tile_width: 1,
        tile_height: 1,
        tilesets: template.tilesets.clone(),
        layers: vec![Layer::new(
            LayerId(1),
            "Template Base",
            LayerKind::Object(ObjectLayer {
                objects: objects.into(),
                ..ObjectLayer::default()
            }),
        )],
        next_layer_id: 2,
        next_object_id: 2,
        ..Map::default()
    }
}

pub(super) fn to_template(
    map: &Map,
    original: &ObjectTemplate,
) -> Result<ObjectTemplate, EditError> {
    let mut objects = Vec::new();
    map.for_each_layer(|layer, _| {
        if let Some(layer) = layer.as_object_layer() {
            objects.extend(layer.objects.iter());
        }
    });
    if objects.len() != 1 {
        return Err(EditError::TemplateBaseRequired);
    }
    let object = objects.first().ok_or(EditError::TemplateBaseRequired)?;
    let mut template = ObjectTemplate::from_map_object(map, object);
    template.file = original.file.clone();
    Ok(template)
}
