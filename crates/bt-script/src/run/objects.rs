//! Objects (`10-agents.md`, §8.5): markers and zones made with their shapes in tile units,
//! converted through the map's geometry; `set` of their own fields and properties; `move` by the
//! point they are placed by (a sprite's foot, a point, a shape's top-left); `delete` by name, by
//! the cells they stand in, or by what they stand on as `check` sees it.

use std::collections::HashSet;

use bt_core::{ClassUsage, LayerId, MapObject, ObjectId, Shape, rotated, tile_object_box};
use bt_edit::{add_object, object_mut, remove_objects};
use bt_utils::text::slug;
use glam::{DVec2, IVec2};

use super::choose::unlocked_in;
use super::error::RunFailure;
use super::pieces::{short_of_room, shuffle};
use super::runner::Runner;
use super::values::{typed_value, yes_or_no};
use crate::feet::{pixel_to_point, point_to_pixel, standing_cell};
use crate::ground::{GroundUnder, StandsOn};
use crate::names::{Ground, object_type};
use crate::script::{
    Assignment, Geometry, LayerRef, Named, ObjectOptions, ObjectRef, ObjectsWhere, parse_extent,
};

impl Runner<'_> {
    /// `object TYPE at X,Y | rect | ellipse | poly | path … | in SHAPE …`.
    pub(super) fn add_objects(
        &mut self,
        kind: &str,
        geometry: &Geometry,
        options: &ObjectOptions,
    ) -> Result<(), RunFailure> {
        let layer = self.typed_layer(kind, options.layer.as_ref())?;
        self.unlocked(layer)?;
        let Geometry::In {
            area,
            count,
            spacing,
        } = geometry
        else {
            self.make_object(layer, kind, geometry, options)?;
            return Ok(());
        };
        let mut rng = self.rng();
        let mut cells = self.cells_of(area)?;
        shuffle(&mut cells, &mut rng);
        let wanted = usize::try_from(*count).unwrap_or(usize::MAX);
        let mut placed: Vec<DVec2> = Vec::new();
        for cell in cells {
            if placed.len() >= wanted {
                break;
            }
            let point = cell.as_dvec2() + 0.5;
            if *spacing > 0.0 && placed.iter().any(|p| p.distance(point) < *spacing) {
                continue;
            }
            self.make_object(layer, kind, &Geometry::Point(point), options)?;
            placed.push(point);
        }
        if placed.len() < wanted {
            self.note(short_of_room(
                &format!("object {kind}"),
                placed.len(),
                wanted,
                *spacing,
            ));
        }
        Ok(())
    }

    /// One object of type `kind` on `layer`.
    pub(super) fn make_object(
        &mut self,
        layer: LayerId,
        kind: &str,
        geometry: &Geometry,
        options: &ObjectOptions,
    ) -> Result<ObjectId, RunFailure> {
        let grid = self.map.geometry();
        let pixel = |point: DVec2| point_to_pixel(&grid, point);
        let (pos, size, shape) = match geometry {
            Geometry::Point(point) => (pixel(*point), DVec2::ZERO, Shape::Point),
            Geometry::Rect { min, size } => (
                pixel(*min),
                pixel(*min + *size) - pixel(*min),
                Shape::Rectangle,
            ),
            Geometry::Ellipse { center, radii } => {
                let min = pixel(*center - *radii);
                (min, pixel(*center + *radii) - min, Shape::Ellipse)
            }
            Geometry::Polygon(points) | Geometry::Path(points) => {
                let Some(&first) = points.first() else {
                    return Err(RunFailure::refused("a shape needs its points"));
                };
                let origin = pixel(first);
                let local: Vec<DVec2> = points.iter().map(|p| pixel(*p) - origin).collect();
                let shape = if matches!(geometry, Geometry::Polygon(_)) {
                    Shape::Polygon(local)
                } else {
                    Shape::Polyline(local)
                };
                (origin, DVec2::ZERO, shape)
            }
            Geometry::In { .. } => {
                return Err(RunFailure::refused(
                    "objects in a shape are placed one point at a time",
                ));
            }
        };
        let mut object = MapObject {
            class: kind.to_owned(),
            name: options.name.clone().unwrap_or_default(),
            pos,
            size,
            shape,
            ..MapObject::default()
        };
        if let Some(like) = &options.like {
            self.take_after(&mut object, like)?;
            object.class = kind.to_owned();
        }
        let id = add_object(&mut self.map, layer, object, None)?;
        self.assign(id, &options.assignments)?;
        Ok(id)
    }

    /// `set OBJECT … key=value…`.
    pub(super) fn set_objects(
        &mut self,
        targets: &[ObjectRef],
        assignments: &[Assignment],
    ) -> Result<(), RunFailure> {
        let ids = self.objects_of_all(targets)?;
        self.unlocked_objects(&ids)?;
        for id in ids {
            self.assign(id, assignments)?;
        }
        Ok(())
    }

    /// `move OBJECT to X,Y`: one object, by the point it is placed by.
    pub(super) fn move_to(&mut self, target: &ObjectRef, to: DVec2) -> Result<(), RunFailure> {
        let ids = self.objects_of(target)?;
        let [id] = ids.as_slice() else {
            return Err(RunFailure::refused(format!(
                "`to` moves one object, and that names {}: move them with `by DX,DY`",
                ids.len()
            )));
        };
        self.unlocked_objects(&[*id])?;
        self.place_object_at(*id, to)
    }

    /// `move OBJECT … by DX,DY`.
    pub(super) fn move_by(&mut self, targets: &[ObjectRef], by: DVec2) -> Result<(), RunFailure> {
        let ids = self.objects_of_all(targets)?;
        self.unlocked_objects(&ids)?;
        for id in ids {
            let place = self.object_place(id)?;
            self.place_object_at(id, place + by)?;
        }
        Ok(())
    }

    /// `delete OBJECT …`.
    pub(super) fn delete_named(&mut self, targets: &[ObjectRef]) -> Result<(), RunFailure> {
        let ids = self.objects_of_all(targets)?;
        self.unlocked_objects(&ids)?;
        remove_objects(&mut self.map, &ids)?;
        Ok(())
    }

    /// `delete objects in SHAPE …` and `delete objects on TERRAIN,… …`: what stands there, of
    /// a type and on a layer when the line says.
    pub(super) fn delete_where(
        &mut self,
        place: &ObjectsWhere,
        kind: Option<&str>,
        layer: Option<&LayerRef>,
    ) -> Result<(), RunFailure> {
        let only = layer
            .map(|layer| self.names.object_layer(&self.map, layer))
            .transpose()?;
        let kind = kind.map(slug);
        let mut candidates: Vec<Standing> = Vec::new();
        let mut objects: Vec<(LayerId, MapObject)> = Vec::new();
        self.map.for_each_layer(|layer, _| {
            if only.is_some_and(|only| only != layer.id) {
                return;
            }
            if let Some(list) = layer.as_object_layer() {
                objects.extend(list.objects.iter().map(|o| (layer.id, o.clone())));
            }
        });
        for (layer, object) in objects {
            if let Some(kind) = &kind {
                let sprite = object
                    .tile
                    .and_then(|tile| self.names.sprite_of(tile))
                    .map(|sprite| sprite.name.clone());
                let of_type = slug(object_type(&self.map, &object)) == *kind;
                if !of_type && sprite.as_deref() != Some(kind.as_str()) {
                    continue;
                }
            }
            match self.feet.standing_point(&self.map, &object) {
                Ok(point) => candidates.push(Standing {
                    layer,
                    id: object.id,
                    cell: standing_cell(point),
                }),
                Err(error) => self.warn(format!(
                    "delete: where #{} stands can't be told ({error}), so it stays",
                    object.id
                )),
            }
        }
        let (found, what) = match place {
            ObjectsWhere::In(area) => {
                let cells: HashSet<IVec2> = self.cells_of(area)?.into_iter().collect();
                let found: Vec<ObjectId> = candidates
                    .iter()
                    .filter(|c| cells.contains(&c.cell))
                    .map(|c| c.id)
                    .collect();
                (found, "there".to_owned())
            }
            ObjectsWhere::On(grounds) => {
                let found = self.standing_on(&candidates, grounds)?;
                let written: Vec<String> = grounds.iter().map(named_text).collect();
                (found, format!("on {}", written.join(", ")))
            }
        };
        if found.is_empty() {
            self.note(format!("delete: no objects stand {what}"));
            return Ok(());
        }
        self.unlocked_objects(&found)?;
        remove_objects(&mut self.map, &found)?;
        Ok(())
    }

    /// The candidates standing on one of `grounds`, as `check` sees it on the map as it shows.
    fn standing_on(
        &mut self,
        candidates: &[Standing],
        grounds: &[Named],
    ) -> Result<Vec<ObjectId>, RunFailure> {
        self.refresh_shown();
        let shown = self.shown_now();
        let grounds = self.names.grounds(shown, grounds)?;
        let mut under = GroundUnder::new(shown, &self.names);
        let mut found = Vec::new();
        for candidate in candidates {
            let on = grounds.iter().any(|ground| match ground {
                Ground::Terrain(terrain) => matches!(
                    under.stands_on(candidate.layer, candidate.cell),
                    StandsOn::Terrain(t)
                        if t.slot == terrain.slot && t.set == terrain.set && t.color == terrain.color
                ),
                Ground::Tiles(group) => under
                    .top_tile_below(candidate.layer, candidate.cell)
                    .is_some_and(|tile| {
                        group
                            .tiles
                            .iter()
                            .any(|t| t.slot() == tile.slot() && t.tile() == tile.tile())
                    }),
                Ground::Area { cells, .. } => cells.contains(&candidate.cell),
            });
            if on {
                found.push(candidate.id);
            }
        }
        Ok(found)
    }

    /// Refuses when an object is on a locked layer.
    fn unlocked_objects(&self, ids: &[ObjectId]) -> Result<(), RunFailure> {
        for &id in ids {
            if let Some(layer) = self.map.object_layer_of(id) {
                unlocked_in(&self.map, layer)?;
            }
        }
        Ok(())
    }

    /// The point an object is placed by, in tile units: a sprite's foot, a point's position, a
    /// shape's top-left corner (a polygon's or path's first point).
    pub(super) fn object_place(&mut self, id: ObjectId) -> Result<DVec2, RunFailure> {
        let object = self
            .map
            .object(id)
            .cloned()
            .ok_or(bt_edit::EditError::NoSuchObject(id))?;
        if object.tile.is_some() {
            return Ok(self.feet.standing_point(&self.map, &object)?);
        }
        Ok(pixel_to_point(&self.map.geometry(), object.pos))
    }

    /// Moves an object so the point it is placed by is `point`.
    pub(super) fn place_object_at(&mut self, id: ObjectId, point: DVec2) -> Result<(), RunFailure> {
        let object = self
            .map
            .object(id)
            .cloned()
            .ok_or(bt_edit::EditError::NoSuchObject(id))?;
        let pos = if object.tile.is_some() {
            self.feet.position_for_foot(&self.map, &object, point)?
        } else {
            point_to_pixel(&self.map.geometry(), point)
        };
        object_mut(&mut self.map, id)?.pos = pos;
        Ok(())
    }

    /// Gives `object` the type and properties of the one `like` names (which may be one the
    /// rules placed: it is only read).
    pub(super) fn take_after(
        &mut self,
        object: &mut MapObject,
        like: &ObjectRef,
    ) -> Result<(), RunFailure> {
        let ids = match self.names.objects(&self.map, like, &self.context) {
            Ok(ids) => ids,
            Err(error) => {
                self.refresh_shown();
                self.names
                    .objects(self.shown_now(), like, &self.context)
                    .map_err(|_| error)?
            }
        };
        let [id] = ids.as_slice() else {
            return Err(RunFailure::refused(format!(
                "like takes one object, and that names {}",
                ids.len()
            )));
        };
        let model = match self.map.object(*id) {
            Some(model) => model.clone(),
            None => {
                self.refresh_shown();
                self.shown_now()
                    .object(*id)
                    .cloned()
                    .ok_or(bt_edit::EditError::NoSuchObject(*id))?
            }
        };
        object.class = model.class.clone();
        for (key, property) in &model.properties {
            object.properties.insert(key.clone(), property.clone());
        }
        Ok(())
    }

    /// Sets an object's fields (`name`, `type`, `visible`, `rotation`, `size`) and properties;
    /// an empty value removes a property.
    pub(super) fn assign(
        &mut self,
        id: ObjectId,
        assignments: &[Assignment],
    ) -> Result<(), RunFailure> {
        for assignment in assignments {
            let value = assignment.value.as_str();
            match assignment.key.as_str() {
                "name" => object_mut(&mut self.map, id)?.name = value.to_owned(),
                "type" | "class" => object_mut(&mut self.map, id)?.class = value.to_owned(),
                "visible" => {
                    object_mut(&mut self.map, id)?.visible = yes_or_no(value).ok_or_else(|| {
                        RunFailure::refused(format!("visible is true or false, not “{value}”"))
                    })?;
                }
                "rotation" => {
                    let degrees = value
                        .parse::<f64>()
                        .ok()
                        .filter(|d| d.is_finite())
                        .ok_or_else(|| {
                            RunFailure::refused(format!(
                                "rotation is a number of degrees, not “{value}”"
                            ))
                        })?;
                    object_mut(&mut self.map, id)?.rotation = degrees;
                }
                "size" => self.resize_object(id, value)?,
                key => self.assign_property(id, key, value)?,
            }
        }
        Ok(())
    }

    /// A property of an object: the type it has, else the project class's, else what it reads
    /// as; empty removes it.
    fn assign_property(&mut self, id: ObjectId, key: &str, value: &str) -> Result<(), RunFailure> {
        if value.is_empty() {
            object_mut(&mut self.map, id)?.properties.remove(key);
            return Ok(());
        }
        let object = self
            .map
            .object(id)
            .ok_or(bt_edit::EditError::NoSuchObject(id))?;
        let class = object_type(&self.map, object).to_owned();
        let like = object.properties.get(key).cloned().or_else(|| {
            self.loader
                .custom_types()
                .class_for(&class, ClassUsage::OBJECT)
                .and_then(|class| class.members.get(key))
                .cloned()
        });
        let property = typed_value(key, value, like.as_ref())?;
        object_mut(&mut self.map, id)?
            .properties
            .insert(key.to_owned(), property);
        Ok(())
    }

    /// `size=WxH` in tiles: a shape keeps its top-left corner, a sprite its bottom-left; points,
    /// polygons and paths have no size.
    fn resize_object(&mut self, id: ObjectId, written: &str) -> Result<(), RunFailure> {
        let object = self
            .map
            .object(id)
            .cloned()
            .ok_or(bt_edit::EditError::NoSuchObject(id))?;
        let grid = self.map.geometry();
        if object.tile.is_some() {
            let size = self.sprite_size(written)?;
            let tileset = object.tile.and_then(|tile| self.map.tileset_of(tile));
            let before = tile_object_box(&grid, &object, tileset);
            let mut resized = object.clone();
            resized.size = size;
            let after = tile_object_box(&grid, &resized, tileset);
            let pos = match (before, after) {
                (Some(before), Some(after)) => {
                    let bottom_left = |rect: bt_utils::DRect| {
                        rotated(DVec2::new(rect.min.x, rect.max.y), object.rotation)
                    };
                    let screen =
                        grid.pixel_to_screen(object.pos) + bottom_left(before) - bottom_left(after);
                    grid.screen_to_pixel(screen)
                }
                _ => object.pos,
            };
            let target = object_mut(&mut self.map, id)?;
            target.size = size;
            target.pos = pos;
            return Ok(());
        }
        if matches!(
            object.shape,
            Shape::Point | Shape::Polygon(_) | Shape::Polyline(_)
        ) {
            return Err(RunFailure::refused(
                "points, polygons and paths have no size (move their points instead)",
            ));
        }
        let size =
            parse_extent(written).map_err(|why| RunFailure::refused(format!("size: {why}")))?;
        let top_left = pixel_to_point(&grid, object.pos);
        let pixels = point_to_pixel(&grid, top_left + size) - point_to_pixel(&grid, top_left);
        object_mut(&mut self.map, id)?.size = pixels;
        Ok(())
    }
}

/// An object, the layer it is on and the cell it stands in.
struct Standing {
    layer: LayerId,
    id: ObjectId,
    cell: IVec2,
}

/// A ground as written.
fn named_text(named: &Named) -> String {
    match named {
        Named::Name(name) => name.clone(),
        Named::Tiles(ids) => super::definitions::tile_ids_text(ids),
    }
}
