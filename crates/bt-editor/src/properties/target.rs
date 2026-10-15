//! The things whose custom properties are shown and changed: their own properties and what
//! they inherit, with where it comes from (libtiled's `Object::inheritedProperties`,
//! `reference/tiled/src/libtiled/object.cpp`, as `bt_core::CustomTypes::object_properties`
//! orders it).

use std::fmt;

use bt_core::{
    ClassUsage, CustomTypeId, CustomTypes, LayerId, Map, ObjectId, ObjectTemplate, Properties,
    Property, TileId, Tileset,
};

use super::error::PropertyError;
use super::lookup::{NO_PROPERTIES, class_members};
use super::target_id::TargetId;

/// Where an inherited value comes from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    /// The members of the target's class, by its name.
    Class(String),
    /// The properties of a tile object's tile.
    Tile,
    /// The properties of a template instance's template.
    Template,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Origin::Class(name) => write!(f, "class “{name}”"),
            Origin::Tile => f.write_str("the tile"),
            Origin::Template => f.write_str("the template"),
        }
    }
}

/// An inherited value, and where it comes from.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Found<'s> {
    /// The value.
    pub(crate) property: &'s Property,
    /// Where it comes from.
    pub(crate) origin: &'s Origin,
}

/// Properties a target inherits, and where they come from.
#[derive(Clone, Debug)]
pub(crate) struct Inherited<'a> {
    /// Where they come from.
    pub(crate) origin: Origin,
    /// The properties.
    pub(crate) properties: &'a Properties,
}

/// One thing whose custom properties are shown and changed: which thing it is, the properties
/// it keeps, and what it inherits, lowest precedence first. A template instance keeps its
/// template's values as well as its own (`bt_core::MapObject::properties`): a kept value equal
/// to the template's is the template's, not the instance's own.
#[derive(Clone, Debug)]
pub struct Target<'a> {
    id: TargetId,
    stored: &'a Properties,
    inherited: Vec<Inherited<'a>>,
}

impl<'a> Target<'a> {
    /// A standalone property collection, with no inherited base.
    pub fn properties(properties: &'a Properties) -> Target<'a> {
        Target {
            id: TargetId::Properties,
            stored: properties,
            inherited: Vec::new(),
        }
    }

    /// The map: its properties, under its class's members.
    pub fn map(map: &'a Map, types: &'a CustomTypes) -> Target<'a> {
        Target::of_class(
            TargetId::Map,
            &map.properties,
            &map.class,
            ClassUsage::MAP,
            types,
        )
    }

    /// A layer of the map: its properties, under its class's members.
    pub fn layer(
        map: &'a Map,
        id: LayerId,
        types: &'a CustomTypes,
    ) -> Result<Target<'a>, PropertyError> {
        let layer = map.layer(id).ok_or(PropertyError::NoSuchLayer(id))?;
        Ok(Target::of_class(
            TargetId::Layer(id),
            &layer.properties,
            &layer.class,
            ClassUsage::LAYER,
            types,
        ))
    }

    /// An object of the map: its properties, over its template's (`template`, for a template
    /// instance), over its tile's (for a tile object), over its class's members. Its class is
    /// its own, else its template's, else its tile's.
    pub fn object(
        map: &'a Map,
        id: ObjectId,
        template: Option<&'a ObjectTemplate>,
        types: &'a CustomTypes,
    ) -> Result<Target<'a>, PropertyError> {
        let object = map.object(id).ok_or(PropertyError::NoSuchObject(id))?;
        let template = template
            .filter(|_| object.template.is_some())
            .and_then(|t| t.object.as_ref());
        let tile = object.tile.and_then(|cell| map.tile_data(cell));
        let class = [
            Some(object.class.as_str()),
            template.map(|t| t.class.as_str()),
            tile.map(|t| t.class.as_str()),
        ]
        .into_iter()
        .flatten()
        .find(|class| !class.is_empty())
        .unwrap_or("");
        let mut target = Target::of_class(
            TargetId::Object(id),
            &object.properties,
            class,
            ClassUsage::OBJECT,
            types,
        );
        if let Some(tile) = tile {
            target.inherited.push(Inherited {
                origin: Origin::Tile,
                properties: &tile.properties,
            });
        }
        if let Some(template) = template {
            target.inherited.push(Inherited {
                origin: Origin::Template,
                properties: &template.properties,
            });
        }
        Ok(target)
    }

    /// The tileset: its properties, under its class's members.
    pub fn tileset(tileset: &'a Tileset, types: &'a CustomTypes) -> Target<'a> {
        Target::of_class(
            TargetId::Tileset,
            &tileset.properties,
            &tileset.class,
            ClassUsage::TILESET,
            types,
        )
    }

    /// A tile of the tileset: its properties (none for a tile without data), under its class's
    /// members.
    pub fn tile(
        tileset: &'a Tileset,
        id: TileId,
        types: &'a CustomTypes,
    ) -> Result<Target<'a>, PropertyError> {
        if !tileset.has_tile(id) {
            return Err(PropertyError::NoSuchTile(id));
        }
        let tile = tileset.tile(id);
        let stored = tile.map_or(&NO_PROPERTIES, |tile| &tile.properties);
        let class = tile.map_or("", |tile| tile.class.as_str());
        Ok(Target::of_class(
            TargetId::Tile(id),
            stored,
            class,
            ClassUsage::TILE,
            types,
        ))
    }

    /// A terrain set of the tileset: its properties, under its class's members.
    pub fn terrain_set(
        tileset: &'a Tileset,
        set: usize,
        types: &'a CustomTypes,
    ) -> Result<Target<'a>, PropertyError> {
        let found = tileset
            .wang_sets
            .get(set)
            .ok_or(PropertyError::NoSuchTerrainSet(set))?;
        Ok(Target::of_class(
            TargetId::TerrainSet(set),
            &found.properties,
            &found.class,
            ClassUsage::WANG_SET,
            types,
        ))
    }

    /// A terrain of a terrain set of the tileset: its properties, under its class's members.
    pub fn terrain(
        tileset: &'a Tileset,
        set: usize,
        color: u8,
        types: &'a CustomTypes,
    ) -> Result<Target<'a>, PropertyError> {
        let terrain = tileset
            .wang_sets
            .get(set)
            .ok_or(PropertyError::NoSuchTerrainSet(set))?
            .colors
            .get(usize::from(color).wrapping_sub(1))
            .ok_or(PropertyError::NoSuchTerrain { set, color })?;
        Ok(Target::of_class(
            TargetId::Terrain { set, color },
            &terrain.properties,
            &terrain.class,
            ClassUsage::WANG_COLOR,
            types,
        ))
    }

    /// The members of the project's class `id`, with their defaults (the Custom Types editor's
    /// class); they inherit nothing.
    pub fn members(types: &'a CustomTypes, id: CustomTypeId) -> Result<Target<'a>, PropertyError> {
        Ok(Target {
            id: TargetId::Members(id),
            stored: class_members(types, id)?,
            inherited: Vec::new(),
        })
    }

    /// Which thing it is.
    pub fn id(&self) -> TargetId {
        self.id
    }

    /// A target keeping `stored`, under the members of `class` when it names a class for
    /// `usage`.
    fn of_class(
        id: TargetId,
        stored: &'a Properties,
        class: &str,
        usage: ClassUsage,
        types: &'a CustomTypes,
    ) -> Target<'a> {
        let inherited = types
            .class_for(class, usage)
            .map(|found| Inherited {
                origin: Origin::Class(class.to_owned()),
                properties: &found.members,
            })
            .into_iter()
            .collect();
        Target {
            id,
            stored,
            inherited,
        }
    }

    /// The properties it keeps (for a template instance, with the template's).
    pub(crate) fn stored(&self) -> &'a Properties {
        self.stored
    }

    /// Its own value of `name` among `stored` (its kept properties, or a copy of them): a kept
    /// value, unless it is its template's.
    pub(crate) fn own_in<'s>(&self, stored: &'s Properties, name: &str) -> Option<&'s Property> {
        stored
            .get(name)
            .filter(|kept| self.template_value(name) != Some(*kept))
    }

    /// The value it inherits for `name`, and where from: the highest precedence that has it.
    pub(crate) fn inherited_value(&self, name: &str) -> Option<Found<'_>> {
        self.inherited.iter().rev().find_map(|inherited| {
            inherited.properties.get(name).map(|property| Found {
                property,
                origin: &inherited.origin,
            })
        })
    }

    /// Its template's value of `name`, for a template instance.
    pub(crate) fn template_value(&self, name: &str) -> Option<&'a Property> {
        self.inherited
            .iter()
            .find(|inherited| inherited.origin == Origin::Template)
            .and_then(|inherited| inherited.properties.get(name))
    }

    /// The names of everything it has, its own and inherited.
    pub(crate) fn names(&self) -> impl Iterator<Item = &'a String> {
        self.stored
            .keys()
            .chain(self.inherited.iter().flat_map(|i| i.properties.keys()))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{
        Cell, ClassType, CustomType, FileRef, Layer, LayerKind, MapObject, ObjectLayer,
        ObjectOverrides, TileData, TilesetRef, TilesetSlot, Value,
    };

    use super::*;

    fn int(value: i64) -> Property {
        Property::new(Value::Int(value))
    }

    fn properties(list: &[(&str, Property)]) -> Properties {
        list.iter()
            .map(|(name, property)| ((*name).to_owned(), property.clone()))
            .collect()
    }

    #[test]
    fn objects_inherit_from_class_tile_and_template_in_that_order() {
        let mut types = CustomTypes::new();
        types.add(CustomType::class(
            "Enemy",
            ClassType {
                members: properties(&[("hp", int(10)), ("speed", int(1))]),
                ..ClassType::default()
            },
        ));
        let mut tileset = Tileset::default();
        tileset.tiles.insert(
            TileId(0),
            TileData {
                class: "Enemy".into(),
                properties: properties(&[("hp", int(20)), ("art", int(1))]),
                ..TileData::default()
            },
        );
        let template = ObjectTemplate {
            object: Some(MapObject {
                properties: properties(&[("hp", int(30))]),
                ..MapObject::default()
            }),
            ..ObjectTemplate::default()
        };
        let instance = MapObject {
            id: ObjectId(1),
            tile: Some(Cell::new(TilesetSlot::new(0).unwrap(), TileId(0))),
            template: Some(FileRef::new("enemy.tx", "/enemy.tx")),
            overrides: ObjectOverrides::empty(),
            // The template's value, kept, and an own one.
            properties: properties(&[("hp", int(30)), ("speed", int(5))]),
            ..MapObject::default()
        };
        let map = Map {
            tilesets: vec![TilesetRef::embedded(Arc::new(tileset))],
            layers: vec![Layer::new(
                LayerId(1),
                "things",
                LayerKind::Object(ObjectLayer {
                    objects: Arc::new(vec![instance]),
                    ..ObjectLayer::default()
                }),
            )],
            ..Map::default()
        };
        let target = Target::object(&map, ObjectId(1), Some(&template), &types).unwrap();
        assert_eq!(target.id(), TargetId::Object(ObjectId(1)));
        assert_eq!(target.own_in(target.stored(), "hp"), None, "the template's");
        assert_eq!(target.own_in(target.stored(), "speed"), Some(&int(5)));
        let inherited = |name| target.inherited_value(name).unwrap();
        assert_eq!(inherited("hp").property, &int(30));
        assert_eq!(inherited("hp").origin, &Origin::Template);
        assert_eq!(inherited("art").origin, &Origin::Tile);
        assert_eq!(inherited("speed").property, &int(1));
        assert_eq!(
            inherited("speed").origin,
            &Origin::Class("Enemy".into()),
            "the class comes from the tile"
        );
        assert_eq!(
            Target::object(&map, ObjectId(9), None, &types).unwrap_err(),
            PropertyError::NoSuchObject(ObjectId(9))
        );
        let without_template = Target::object(&map, ObjectId(1), None, &types).unwrap();
        assert_eq!(
            without_template.own_in(without_template.stored(), "hp"),
            Some(&int(30)),
            "without its template at hand, a kept value is the instance's own"
        );
    }

    #[test]
    fn tiles_without_data_have_no_properties_and_missing_things_are_errors() {
        let tileset = Tileset {
            image: bt_core::TilesetImage::Atlas {
                image: bt_core::ImageRef::file(
                    FileRef::new("a.png", "/a.png"),
                    glam::UVec2::new(32, 32),
                ),
                columns: 2,
                tile_count: 4,
            },
            ..Tileset::default()
        };
        let types = CustomTypes::new();
        let tile = Target::tile(&tileset, TileId(3), &types).unwrap();
        assert!(tile.stored().is_empty());
        assert_eq!(
            Target::tile(&tileset, TileId(4), &types).unwrap_err(),
            PropertyError::NoSuchTile(TileId(4))
        );
        assert_eq!(
            Target::layer(&Map::default(), LayerId(2), &types).unwrap_err(),
            PropertyError::NoSuchLayer(LayerId(2))
        );
        assert_eq!(Origin::Class("Enemy".into()).to_string(), "class “Enemy”");
    }
}
