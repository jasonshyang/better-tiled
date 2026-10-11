//! The map's own operations (`10-agents.md`, §8.8): its properties, its layers by name, and
//! `resize`, which resizes as `bt-edit` does and says what it leaves to do: the new strips of
//! cells, the tile layers that had a tile in every cell and are still empty there when the
//! script ends, and the objects that reach the old edges next to new cells.

use bt_core::{ClassUsage, LayerId, LayerKind};
use bt_edit::{
    LayerType, ObjectSpace, locate, move_layers, new_layer, object_bounds, remove_layers,
};
use bt_utils::IRect;
use glam::IVec2;

use super::error::RunFailure;
use super::runner::{NewStrips, ResizeStep, Runner};
use super::values::typed_value;
use crate::changes::object_kind;
use crate::feet::pixel_to_point;
use crate::script::{Assignment, LayerOp, LayerPlace};
use crate::words::region_text;

/// How close to an old edge, in cells, an object's side counts as reaching it.
const EDGE_TOLERANCE: f64 = 0.05;

/// The most objects a note lists by one.
const OBJECTS_LISTED: usize = 8;

impl Runner<'_> {
    /// `property key=value …`: the map's properties; an empty value removes one.
    pub(super) fn set_map_properties(
        &mut self,
        assignments: &[Assignment],
    ) -> Result<(), RunFailure> {
        for assignment in assignments {
            let key = assignment.key.as_str();
            if assignment.value.is_empty() {
                self.map.properties.remove(key);
                continue;
            }
            let like = self.map.properties.get(key).cloned().or_else(|| {
                self.loader
                    .custom_types()
                    .class_for(&self.map.class, ClassUsage::MAP)
                    .and_then(|class| class.members.get(key))
                    .cloned()
            });
            let property = typed_value(key, &assignment.value, like.as_ref())?;
            self.map.properties.insert(key.to_owned(), property);
        }
        Ok(())
    }

    /// `layer add | rename | delete | hide | show | lock | unlock | move …`.
    pub(super) fn layer_op(&mut self, op: &LayerOp) -> Result<(), RunFailure> {
        match op {
            LayerOp::Add {
                name,
                objects,
                place,
            } => self.add_layer(name, *objects, place.as_ref()),
            LayerOp::Rename { layer, name } => {
                if name.is_empty() {
                    return Err(RunFailure::refused("a layer needs a name"));
                }
                let id = self.names.layer(&self.map, layer)?;
                if let Some(layer) = self.map.layer_mut(id) {
                    layer.name = name.clone();
                }
                Ok(())
            }
            LayerOp::Delete { layer } => {
                let id = self.names.layer(&self.map, layer)?;
                self.unlocked(id)?;
                remove_layers(&mut self.map, &[id])?;
                Ok(())
            }
            LayerOp::SetVisible { layer, visible } => {
                let id = self.names.layer(&self.map, layer)?;
                if let Some(layer) = self.map.layer_mut(id) {
                    layer.visible = *visible;
                }
                Ok(())
            }
            LayerOp::SetLocked { layer, locked } => {
                let id = self.names.layer(&self.map, layer)?;
                if let Some(layer) = self.map.layer_mut(id) {
                    layer.locked = *locked;
                }
                Ok(())
            }
            LayerOp::Move { layer, place } => {
                let id = self.names.layer(&self.map, layer)?;
                self.move_layer(id, place)
            }
        }
    }

    /// `layer add NAME tiles|objects [above L|below L]`: on top of the map unless placed.
    fn add_layer(
        &mut self,
        name: &str,
        objects: bool,
        place: Option<&LayerPlace>,
    ) -> Result<(), RunFailure> {
        let kind = if objects {
            LayerType::Object
        } else {
            LayerType::Tile
        };
        let mut taken = false;
        self.map.for_each_layer(|layer, _| {
            taken |= layer.name == name && LayerType::of(layer) == kind;
        });
        if taken {
            return Err(RunFailure::refused(format!(
                "there is already a {} called {name}",
                if objects {
                    "object layer"
                } else {
                    "tile layer"
                }
            )));
        }
        let top = self.map.layers.last().map(|layer| layer.id);
        let id = new_layer(&mut self.map, kind, top)?;
        if let Some(layer) = self.map.layer_mut(id) {
            layer.name = name.to_owned();
        }
        if let Some(place) = place {
            self.move_layer(id, place)?;
        }
        Ok(())
    }

    /// Moves a layer just above or below another, in that one's group.
    fn move_layer(&mut self, id: LayerId, place: &LayerPlace) -> Result<(), RunFailure> {
        let other = self.names.layer(&self.map, &place.layer)?;
        if other == id {
            return Err(RunFailure::refused("a layer can't move next to itself"));
        }
        let location = locate(&self.map, other)?;
        let index = if place.above {
            location.index + 1
        } else {
            location.index
        };
        move_layers(&mut self.map, &[id], location.parent, index)?;
        Ok(())
    }

    /// `resize WxH [shift DX,DY]`.
    pub(super) fn resize(&mut self, size: IVec2, shift: IVec2) -> Result<(), RunFailure> {
        if self.map.infinite {
            return Err(RunFailure::refused("an infinite map has no size to change"));
        }
        let old = IVec2::new(
            i32::try_from(self.map.width).unwrap_or(i32::MAX),
            i32::try_from(self.map.height).unwrap_or(i32::MAX),
        );
        let old_cells = IRect::from_min_size(IVec2::ZERO, old);
        let mut full: Vec<LayerId> = Vec::new();
        self.map.for_each_layer(|layer, _| {
            if let LayerKind::Tile(tiles) = &layer.kind
                && old_cells.points().all(|p| !tiles.cell(p).is_empty())
            {
                full.push(layer.id);
            }
        });
        let new_size = size.as_uvec2();
        bt_edit::resize_map(&mut self.map, new_size, shift, false)?;
        self.resizes.push(ResizeStep {
            size: new_size,
            shift,
        });
        let strips = new_strips(old, size, shift);
        if strips.is_empty() {
            return Ok(());
        }
        let sides: Vec<String> = strips
            .iter()
            .map(|strip| format!("{} ({})", strip.side.word(), region_text(strip.cells)))
            .collect();
        self.note(format!("resize: new cells on the {}", sides.join(", ")));
        if !full.is_empty() {
            self.strips.push(NewStrips {
                line: self.line,
                strips: strips.iter().map(|strip| strip.cells).collect(),
                full,
            });
        }
        self.note_objects_at_old_edges(&strips, old, shift);
        Ok(())
    }

    /// Notes the objects reaching an old edge that has new cells beyond it (a wall at the end,
    /// the ground stretched across the level).
    fn note_objects_at_old_edges(&mut self, strips: &[Strip], old: IVec2, shift: IVec2) {
        let grid = self.map.geometry();
        let space = ObjectSpace::of_map(&self.map);
        let (old_min, old_max) = (shift.as_dvec2(), (shift + old).as_dvec2());
        let mut notes = Vec::new();
        for strip in strips {
            let mut reaching: Vec<String> = Vec::new();
            self.map.for_each_layer(|layer, _| {
                let Some(objects) = layer.as_object_layer() else {
                    return;
                };
                for object in objects.objects.iter() {
                    let bounds = object_bounds(&space, object);
                    let min = pixel_to_point(&grid, bounds.min);
                    let max = pixel_to_point(&grid, bounds.max);
                    let reaches = match strip.side {
                        Side::Left => min.x <= old_min.x + EDGE_TOLERANCE,
                        Side::Top => min.y <= old_min.y + EDGE_TOLERANCE,
                        Side::Right => max.x >= old_max.x - EDGE_TOLERANCE,
                        Side::Bottom => max.y >= old_max.y - EDGE_TOLERANCE,
                    };
                    if reaches {
                        reaching.push(format!(
                            "#{} {} on {}",
                            object.id,
                            object_kind(&self.names, object),
                            layer.name
                        ));
                    }
                }
            });
            if reaching.is_empty() {
                continue;
            }
            let count = reaching.len();
            let mut listed: Vec<String> = reaching.into_iter().take(OBJECTS_LISTED).collect();
            if count > OBJECTS_LISTED {
                listed.push(format!("and {} more", count - OBJECTS_LISTED));
            }
            let edge = match strip.side {
                Side::Left => format!("x={}", shift.x),
                Side::Top => format!("y={}", shift.y),
                Side::Right => format!("x={}", old_max.x),
                Side::Bottom => format!("y={}", old_max.y),
            };
            notes.push(format!(
                "resize: {} the old {} edge ({edge}): {}",
                if count == 1 {
                    "1 object reaches".to_owned()
                } else {
                    format!("{count} objects reach")
                },
                strip.side.word(),
                listed.join("; ")
            ));
        }
        for note in notes {
            self.note(note);
        }
    }
}

/// A side of the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Left,
    Top,
    Right,
    Bottom,
}

impl Side {
    fn word(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Top => "top",
            Side::Right => "right",
            Side::Bottom => "bottom",
        }
    }
}

/// A strip of new cells on one side.
#[derive(Clone, Copy, Debug)]
struct Strip {
    side: Side,
    cells: IRect,
}

/// The new strips of cells a resize from `old` to `size` with the old cells moved by `shift`
/// makes, in the new map's cells.
fn new_strips(old: IVec2, size: IVec2, shift: IVec2) -> Vec<Strip> {
    let mut strips = Vec::new();
    if shift.x > 0 {
        strips.push(Strip {
            side: Side::Left,
            cells: IRect::from_min_size(IVec2::ZERO, IVec2::new(shift.x, size.y)),
        });
    }
    if shift.y > 0 {
        strips.push(Strip {
            side: Side::Top,
            cells: IRect::from_min_size(IVec2::ZERO, IVec2::new(size.x, shift.y)),
        });
    }
    let right = old.x + shift.x;
    if size.x > right {
        strips.push(Strip {
            side: Side::Right,
            cells: IRect::from_min_size(IVec2::new(right, 0), IVec2::new(size.x - right, size.y)),
        });
    }
    let bottom = old.y + shift.y;
    if size.y > bottom {
        strips.push(Strip {
            side: Side::Bottom,
            cells: IRect::from_min_size(IVec2::new(0, bottom), IVec2::new(size.x, size.y - bottom)),
        });
    }
    strips
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resize_says_which_strips_are_new() {
        let strips = new_strips(IVec2::new(12, 8), IVec2::new(14, 10), IVec2::new(0, 2));
        let found: Vec<(Side, IRect)> = strips.iter().map(|s| (s.side, s.cells)).collect();
        assert_eq!(
            found,
            [
                (
                    Side::Top,
                    IRect::from_min_size(IVec2::ZERO, IVec2::new(14, 2))
                ),
                (
                    Side::Right,
                    IRect::from_min_size(IVec2::new(12, 0), IVec2::new(2, 10))
                ),
            ]
        );
        assert!(new_strips(IVec2::new(4, 4), IVec2::new(3, 3), IVec2::ZERO).is_empty());
    }
}
