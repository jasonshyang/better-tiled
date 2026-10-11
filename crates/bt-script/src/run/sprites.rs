//! `place` and `scatter` (`10-agents.md`, §8.4): sprites stand where their feet are put. `place
//! … at` puts feet at the points; `in SHAPE` one per cell at the middle of its bottom edge, moved
//! by up to `jitter` sideways and half of it up so it stays in the cell; `rect` stretches one
//! sprite to a box, on orthogonal maps. `scatter` shuffles the shape's cells with the
//! operation's generator, takes `count` or `density × cells` of them, keeps feet at least
//! `spacing` apart from each other and from the sprites already on the layers they go on, and
//! skips cells where a group's other sprites would stand outside the shape. A group places its
//! first sprite at the foot and the others at their offsets, mirrored when flipped; the line's
//! name, `like` and `key=value` go to the first. Names of pieces place pieces
//! (`super::pieces`).

use std::collections::HashSet;
use std::sync::Arc;

use bt_core::{Flip as TileFlip, LayerId, MapObject, ObjectId, Orientation, tile_object_box};
use bt_edit::add_object;
use bt_utils::rng::Rng;
use glam::{DVec2, IVec2};

use super::error::RunFailure;
use super::pieces::{pick, short_of_room, shuffle, wanted_of};
use super::runner::Runner;
use crate::feet::standing_cell;
use crate::names::{GroupPart, Piece};
use crate::script::{Amount, Area, Flip, Named, ObjectOptions, Placement, parse_extent};

/// What the names of a `place` or `scatter` stand for: groups of sprites (a sprite alone is a
/// group of one), or pieces; never both.
pub(super) enum Placeables {
    Groups(Vec<Vec<GroupPart>>),
    Pieces(Vec<Arc<Piece>>),
}

impl Runner<'_> {
    /// `place NAME,… at X,Y … | in SHAPE | rect X,Y WxH` with its options.
    pub(super) fn place(
        &mut self,
        names: &[Named],
        placement: &Placement,
        flip: Flip,
        jitter: f64,
        options: &ObjectOptions,
    ) -> Result<(), RunFailure> {
        match self.placeables(names)? {
            Placeables::Pieces(pieces) => {
                refuse_sprite_options_for_pieces(flip, options)?;
                let feet: Vec<IVec2> = match placement {
                    Placement::At(points) => points.iter().map(|p| p.floor().as_ivec2()).collect(),
                    Placement::In(area) => self.cells_noted(area, "place")?,
                    Placement::Rect { .. } => {
                        return Err(RunFailure::refused(
                            "a piece goes by its foot cell: place PIECE at X,Y",
                        ));
                    }
                };
                self.place_pieces(&pieces, &feet)
            }
            Placeables::Groups(groups) => match placement {
                Placement::Rect { min, size } => {
                    self.place_rect(&groups, *min, *size, flip, options)
                }
                Placement::At(points) => {
                    let mut rng = self.rng();
                    for &foot in points {
                        self.place_one(&groups, foot, flip, options, &mut rng)?;
                    }
                    Ok(())
                }
                Placement::In(area) => {
                    let cells = self.cells_noted(area, "place")?;
                    let mut rng = self.rng();
                    let feet: Vec<DVec2> = cells
                        .iter()
                        .map(|&cell| foot_in_cell(cell, jitter, &mut rng))
                        .collect();
                    for foot in feet {
                        self.place_one(&groups, foot, flip, options, &mut rng)?;
                    }
                    Ok(())
                }
            },
        }
    }

    /// `scatter NAME,… in SHAPE …`.
    #[expect(
        clippy::too_many_arguments,
        reason = "the parts of one parsed operation, passed on as they are"
    )]
    pub(super) fn scatter(
        &mut self,
        names: &[Named],
        area: &Area,
        amount: Amount,
        spacing: f64,
        jitter: f64,
        flip: Flip,
        options: &ObjectOptions,
    ) -> Result<(), RunFailure> {
        let groups = match self.placeables(names)? {
            Placeables::Pieces(pieces) => {
                refuse_sprite_options_for_pieces(flip, options)?;
                return self.scatter_pieces(&pieces, area, amount, spacing);
            }
            Placeables::Groups(groups) => groups,
        };
        let mut rng = self.rng();
        let mut cells = self.cells_noted(area, "scatter")?;
        let allowed: HashSet<IVec2> = cells.iter().copied().collect();
        shuffle(&mut cells, &mut rng);
        let wanted = wanted_of(amount, cells.len());
        let mut feet = self.feet_on_layers_of(&groups, options)?;
        let mut placed = 0;
        for cell in cells {
            if placed >= wanted {
                break;
            }
            let foot = foot_in_cell(cell, jitter, &mut rng);
            if spacing > 0.0 && feet.iter().any(|other| other.distance(foot) < spacing) {
                continue;
            }
            let Some(parts) = pick(&groups, &mut rng) else {
                continue;
            };
            let flipped = flipped(flip, &mut rng);
            let fits = parts
                .iter()
                .skip(1)
                .all(|part| allowed.contains(&standing_cell(foot + offset_of(part, flipped))));
            if !fits {
                continue;
            }
            self.place_group(&parts, foot, flipped, options)?;
            feet.push(foot);
            placed += 1;
        }
        if placed < wanted {
            self.note(short_of_room("scatter", placed, wanted, spacing));
        }
        Ok(())
    }

    /// What the names stand for.
    pub(super) fn placeables(&self, names: &[Named]) -> Result<Placeables, RunFailure> {
        let mut groups = Vec::new();
        let mut pieces = Vec::new();
        for named in names {
            match named {
                Named::Name(name) => match self.names.piece(name) {
                    Some(piece) => pieces.push(piece),
                    None => groups.push(self.names.group(name)?),
                },
                Named::Tiles(_) => {
                    groups.push(vec![GroupPart {
                        sprite: self.sprite_named(named)?,
                        offset: DVec2::ZERO,
                        layer: None,
                    }]);
                }
            }
        }
        match (groups.is_empty(), pieces.is_empty()) {
            (_, true) => Ok(Placeables::Groups(groups)),
            (true, false) => Ok(Placeables::Pieces(pieces)),
            (false, false) => Err(RunFailure::refused(
                "pieces (define … = copy) and sprites can't be mixed in one operation",
            )),
        }
    }

    /// Places one of `groups`, at random, with its first sprite's foot at `foot`.
    fn place_one(
        &mut self,
        groups: &[Vec<GroupPart>],
        foot: DVec2,
        flip: Flip,
        options: &ObjectOptions,
        rng: &mut Rng,
    ) -> Result<(), RunFailure> {
        let Some(parts) = pick(groups, rng) else {
            return Ok(());
        };
        let flipped = flipped(flip, rng);
        self.place_group(&parts, foot, flipped, options)?;
        Ok(())
    }

    /// Places a group: its first sprite's foot at `foot`, the others at their offsets (mirrored
    /// when flipped); the line's name, `like` and `key=value` go to the first.
    pub(super) fn place_group(
        &mut self,
        parts: &[GroupPart],
        foot: DVec2,
        flipped: bool,
        options: &ObjectOptions,
    ) -> Result<Vec<ObjectId>, RunFailure> {
        let mut ids = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            let first = index == 0;
            let given = part
                .layer
                .as_ref()
                .or(if first { options.layer.as_ref() } else { None });
            let layer = self.sprite_layer(part.sprite.tile, given)?;
            self.unlocked(layer)?;
            let tile = if flipped {
                part.sprite.tile.with_flip(TileFlip::H)
            } else {
                part.sprite.tile
            };
            let mut object = MapObject {
                tile: Some(tile),
                size: part.sprite.size.as_dvec2(),
                ..MapObject::default()
            };
            if first {
                if let Some(size) = options.assignments.iter().find(|a| a.key == "size") {
                    object.size = self.sprite_size(&size.value)?;
                }
                if let Some(name) = &options.name {
                    object.name = name.clone();
                }
                if let Some(like) = &options.like {
                    self.take_after(&mut object, like)?;
                }
            }
            object.pos =
                self.feet
                    .position_for_foot(&self.map, &object, foot + offset_of(part, flipped))?;
            let id = add_object(&mut self.map, layer, object, None)?;
            if first {
                let rest: Vec<_> = options
                    .assignments
                    .iter()
                    .filter(|a| a.key != "size")
                    .cloned()
                    .collect();
                self.assign(id, &rest)?;
            }
            ids.push(id);
        }
        Ok(ids)
    }

    /// `place SPRITE rect X,Y WxH`: one sprite stretched to the box, on orthogonal maps.
    fn place_rect(
        &mut self,
        groups: &[Vec<GroupPart>],
        min: DVec2,
        size: DVec2,
        flip: Flip,
        options: &ObjectOptions,
    ) -> Result<(), RunFailure> {
        if self.map.orientation != Orientation::Orthogonal {
            return Err(RunFailure::refused(
                "place … rect is for orthogonal maps: place sprites by their feet (at X,Y, with \
                 size=WxH)",
            ));
        }
        if options.assignments.iter().any(|a| a.key == "size") {
            return Err(RunFailure::refused("place … rect gives the size already"));
        }
        let mut rng = self.rng();
        let Some(parts) = pick(groups, &mut rng) else {
            return Ok(());
        };
        let [part] = parts.as_slice() else {
            return Err(RunFailure::refused(
                "place … rect stretches one sprite to a box, not sprites placed together",
            ));
        };
        let layer = self.sprite_layer(
            part.sprite.tile,
            part.layer.as_ref().or(options.layer.as_ref()),
        )?;
        self.unlocked(layer)?;
        let tile = if flipped(flip, &mut rng) {
            part.sprite.tile.with_flip(TileFlip::H)
        } else {
            part.sprite.tile
        };
        let cell = DVec2::new(
            f64::from(self.map.tile_width),
            f64::from(self.map.tile_height),
        );
        let mut object = MapObject {
            tile: Some(tile),
            size: size * cell,
            name: options.name.clone().unwrap_or_default(),
            ..MapObject::default()
        };
        if let Some(like) = &options.like {
            self.take_after(&mut object, like)?;
        }
        let geometry = self.map.geometry();
        let drawn = tile_object_box(&geometry, &object, self.map.tileset_of(tile))
            .ok_or_else(|| RunFailure::refused("the sprite has no tile"))?;
        object.pos = min * cell - drawn.min;
        let id: ObjectId = add_object(&mut self.map, layer, object, None)?;
        self.assign(id, &options.assignments)
    }

    /// A sprite's size in pixels from `WxH` in tiles.
    pub(super) fn sprite_size(&self, written: &str) -> Result<DVec2, RunFailure> {
        let size =
            parse_extent(written).map_err(|why| RunFailure::refused(format!("size: {why}")))?;
        Ok(size
            * DVec2::new(
                f64::from(self.map.tile_width),
                f64::from(self.map.tile_height),
            ))
    }

    /// Where the sprites already on the layers `groups` go on stand: what `scatter` keeps its
    /// distance from.
    fn feet_on_layers_of(
        &mut self,
        groups: &[Vec<GroupPart>],
        options: &ObjectOptions,
    ) -> Result<Vec<DVec2>, RunFailure> {
        let mut layers: Vec<LayerId> = Vec::new();
        for parts in groups {
            if let Some(first) = parts.first() {
                let layer = self.sprite_layer(
                    first.sprite.tile,
                    first.layer.as_ref().or(options.layer.as_ref()),
                )?;
                if !layers.contains(&layer) {
                    layers.push(layer);
                }
            }
        }
        let mut sprites: Vec<MapObject> = Vec::new();
        for &layer in &layers {
            if let Some(objects) = self.map.layer(layer).and_then(|l| l.as_object_layer()) {
                sprites.extend(objects.objects.iter().filter(|o| o.tile.is_some()).cloned());
            }
        }
        let mut feet = Vec::new();
        for sprite in sprites {
            match self.feet.standing_point(&self.map, &sprite) {
                Ok(foot) => feet.push(foot),
                Err(error) => self.warn(format!(
                    "scatter keeps no distance from #{}: where it stands can't be told ({error})",
                    sprite.id
                )),
            }
        }
        Ok(feet)
    }
}

/// A piece is placed as it is: the options that dress sprites are refused for it.
fn refuse_sprite_options_for_pieces(flip: Flip, options: &ObjectOptions) -> Result<(), RunFailure> {
    let dressed = flip != Flip::None
        || options.name.is_some()
        || options.layer.is_some()
        || options.like.is_some()
        || !options.assignments.is_empty();
    if dressed {
        return Err(RunFailure::refused(
            "a piece's tiles and objects go on the layers of their names, as they are: flip, \
             layer, name, like and key=value are for sprites",
        ));
    }
    Ok(())
}

/// Whether a placement is mirrored: as asked, or at random.
fn flipped(flip: Flip, rng: &mut Rng) -> bool {
    match flip {
        Flip::None => false,
        Flip::Flipped => true,
        Flip::Random => rng.next_f64() < 0.5,
    }
}

/// A group part's offset from the first sprite's foot, mirrored when flipped.
fn offset_of(part: &GroupPart, flipped: bool) -> DVec2 {
    if flipped {
        DVec2::new(-part.offset.x, part.offset.y)
    } else {
        part.offset
    }
}

/// A foot in a cell: the middle of its bottom edge, moved at random by up to half the jitter
/// either way sideways and up to half of it up, so it stays in the cell.
pub(super) fn foot_in_cell(cell: IVec2, jitter: f64, rng: &mut Rng) -> DVec2 {
    let mut foot = cell.as_dvec2() + DVec2::new(0.5, 1.0);
    if jitter > 0.0 {
        foot.x += (rng.next_f64() - 0.5) * jitter;
        foot.y -= rng.next_f64() * jitter * 0.5;
    }
    foot
}
