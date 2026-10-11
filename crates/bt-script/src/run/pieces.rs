//! Pieces (`10-agents.md`, §8.4): tiles and objects copied from a map once and placed by a foot
//! cell, by default the middle of the piece's bottom row. Its tiles go on the layers of the same
//! names (its empty cells leave the map's as they are), its objects on theirs, which are made
//! when missing; `scatter` places a piece only where all of it is on the map. `copy` is a piece
//! put down once by its top-left cell.

use std::collections::HashSet;

use bt_core::{Layer, LayerId, LayerKind, Map};
use bt_edit::{
    ClipLayer, LayerType, PasteTarget, TileClip, TileRegion, copy, copy_objects, new_layer, paste,
    paste_objects,
};
use bt_utils::IRect;
use bt_utils::text::and_list;
use glam::{DVec2, IVec2, UVec2};

use super::error::RunFailure;
use super::runner::Runner;
use crate::feet::standing_cell;
use crate::names::{Names, Piece, PieceObjects};
use crate::script::{Amount, Area, CopySource, LayerRef, Layers, Shape};
use crate::shapes::{map_cells, shape_cells};
use crate::words::{cell_text, region_text};

impl Runner<'_> {
    /// `define NAME = copy MAP REGION [minus SHAPE] [layers L,…] [objects] [foot X,Y]`.
    pub(super) fn define_piece(
        &mut self,
        name: &str,
        source: &CopySource,
        minus: &[Shape],
        foot: Option<IVec2>,
    ) -> Result<(), RunFailure> {
        let piece = self.take_piece(source, minus, foot)?;
        Ok(self.names.define_piece(name, piece)?)
    }

    /// A piece of the map `source` names (`.` for this one, as the script has left it): the
    /// tiles of its region but the cells of `minus`, on its layers (every tile layer when it
    /// names none), and with `objects` the objects standing in those cells; its foot cell is
    /// `foot` (in the map copied from), by default the middle of the region's bottom row.
    pub(super) fn take_piece(
        &mut self,
        source: &CopySource,
        minus: &[Shape],
        foot: Option<IVec2>,
    ) -> Result<Piece, RunFailure> {
        let map = self.source_map(&source.map)?;
        let region = source.region;
        let foot = foot.unwrap_or(IVec2::new(
            region.x + (region.w - 1) / 2,
            region.y + region.h - 1,
        ));
        if !region_holds(region, foot) {
            return Err(RunFailure::refused(format!(
                "the foot {} is outside the piece {}",
                cell_text(foot),
                region_text(region)
            )));
        }
        let names = Names::of(&map);
        let mut cells: HashSet<IVec2> = region.points().collect();
        for shape in minus {
            for cell in shape_cells(&map, &names, &self.context, shape)? {
                cells.remove(&cell);
            }
        }
        let chosen = chosen_layers(&map, &names, &source.layers)?;
        let tile_layers: Vec<LayerId> = chosen
            .iter()
            .filter(|id| map.layer(**id).is_some_and(|l| l.as_tile_layer().is_some()))
            .copied()
            .collect();
        let clip = copy(
            &map,
            &tile_layers,
            &TileRegion::from_cells(cells.iter().copied()),
        )?;
        let tiles = rebased(clip, region);
        let mut objects = Vec::new();
        if source.objects {
            let object_layers: Vec<&Layer> = chosen
                .iter()
                .filter_map(|&id| map.layer(id))
                .filter(|layer| layer.as_object_layer().is_some())
                .collect();
            for layer in object_layers {
                let Some(list) = layer.as_object_layer() else {
                    continue;
                };
                let mut ids = Vec::new();
                for object in list.objects.iter() {
                    match self.feet.standing_point(&map, object) {
                        Ok(point) if cells.contains(&standing_cell(point)) => ids.push(object.id),
                        Ok(_) => {}
                        Err(error) => self.warn(format!(
                            "#{} isn't copied: where it stands can't be told ({error})",
                            object.id
                        )),
                    }
                }
                if !ids.is_empty() {
                    objects.push(PieceObjects {
                        layer: layer.name.clone(),
                        objects: copy_objects(&map, &ids)?,
                    });
                }
            }
        }
        Ok(Piece {
            tiles,
            objects,
            foot: foot - region.min(),
        })
    }

    /// The map a script names: this one as the script has left it (`.`), or a file from the
    /// map's folder, read through the map's loader.
    fn source_map(&mut self, written: &str) -> Result<Map, RunFailure> {
        if written == "." {
            return Ok(self.map.clone());
        }
        let path = self.resolve(written);
        self.loader
            .load_map(&path)
            .map(|loaded| loaded.value)
            .map_err(|error| RunFailure::ReadMap {
                path: path.display().to_string(),
                reason: error.to_string(),
            })
    }

    /// Puts a piece down with the top-left of its region at `top_left`: its tiles on the layers
    /// of their names (made when missing), its objects on theirs (made when missing). `verb`
    /// names the operation in notes.
    pub(super) fn put_piece(
        &mut self,
        piece: &Piece,
        top_left: IVec2,
        verb: &str,
    ) -> Result<(), RunFailure> {
        let mut targets = Vec::new();
        for layer in &piece.tiles.layers {
            match layer_named(&self.map, &layer.name, false) {
                Some(id) => {
                    self.unlocked(id)?;
                    targets.push(PasteTarget::Layer(id));
                }
                None => targets.push(PasteTarget::NewLayer),
            }
        }
        let mut made: Vec<String> = Vec::new();
        if !piece.tiles.is_empty() {
            let report = paste(
                &mut self.map,
                &piece.tiles,
                top_left,
                &targets,
                &TileRegion::new(),
            )?;
            made.extend(
                report
                    .created_layers
                    .iter()
                    .filter_map(|&id| self.map.layer(id))
                    .map(|layer| layer.name.clone()),
            );
        }
        let offset = (top_left - piece.tiles.origin).as_dvec2();
        for group in &piece.objects {
            let target = match layer_named(&self.map, &group.layer, true) {
                Some(id) => id,
                None => {
                    let top = self.map.layers.last().map(|layer| layer.id);
                    let id = new_layer(&mut self.map, LayerType::Object, top)?;
                    if let Some(layer) = self.map.layer_mut(id) {
                        layer.name = group.layer.clone();
                    }
                    made.push(group.layer.clone());
                    id
                }
            };
            self.unlocked(target)?;
            let pasted = paste_objects(&mut self.map, target, &group.objects, DVec2::ZERO)?;
            if offset != DVec2::ZERO {
                for id in pasted.changed {
                    let place = self.object_place(id)?;
                    self.place_object_at(id, place + offset)?;
                }
            }
        }
        if !made.is_empty() {
            let noun = if made.len() == 1 { "layer" } else { "layers" };
            self.note(format!("{verb}: new {noun} {}", and_list(&made)));
        }
        Ok(())
    }

    /// Places pieces by their foot cells.
    pub(super) fn place_pieces(
        &mut self,
        pieces: &[std::sync::Arc<Piece>],
        feet: &[IVec2],
    ) -> Result<(), RunFailure> {
        let mut rng = self.rng();
        for &foot in feet {
            let Some(piece) = pick(pieces, &mut rng) else {
                continue;
            };
            self.put_piece(&piece, foot - piece.foot, "place")?;
        }
        Ok(())
    }

    /// `scatter PIECE,… in SHAPE [density D|count N] [spacing S]`: feet at random cells of the
    /// shape, `spacing` cells apart, where all of the piece is on the map.
    pub(super) fn scatter_pieces(
        &mut self,
        pieces: &[std::sync::Arc<Piece>],
        area: &Area,
        amount: Amount,
        spacing: f64,
    ) -> Result<(), RunFailure> {
        let mut rng = self.rng();
        let mut cells = self.cells_noted(area, "scatter")?;
        shuffle(&mut cells, &mut rng);
        let wanted = wanted_of(amount, cells.len());
        let bounds = map_cells(&self.map);
        let infinite = self.map.infinite;
        let mut placed: Vec<IVec2> = Vec::new();
        for cell in cells {
            if placed.len() >= wanted {
                break;
            }
            if spacing > 0.0
                && placed
                    .iter()
                    .any(|other| (*other - cell).as_dvec2().length() < spacing)
            {
                continue;
            }
            let Some(piece) = pick(pieces, &mut rng) else {
                continue;
            };
            let top_left = cell - piece.foot;
            let footprint = IRect::from_min_size(top_left, piece.tiles.size);
            if !infinite && footprint.intersection(bounds) != footprint {
                continue;
            }
            self.put_piece(&piece, top_left, "scatter")?;
            placed.push(cell);
        }
        if placed.len() < wanted {
            self.note(short_of_room("scatter", placed.len(), wanted, spacing));
        }
        Ok(())
    }
}

/// Whether `region` holds `cell`.
fn region_holds(region: IRect, cell: IVec2) -> bool {
    cell.cmpge(region.min()).all() && cell.cmplt(region.max()).all()
}

/// The layers of `map` a piece takes: those named (any kind), or every layer.
fn chosen_layers(map: &Map, names: &Names, layers: &Layers) -> Result<Vec<LayerId>, RunFailure> {
    match layers {
        Layers::All => {
            let mut ids = Vec::new();
            map.for_each_layer(|layer, _| ids.push(layer.id));
            Ok(ids)
        }
        Layers::Only(refs) => refs
            .iter()
            .map(|layer: &LayerRef| Ok(names.layer(map, layer)?))
            .collect(),
    }
}

/// A clip made of cells of `region` with its origin at the region's top-left and its size the
/// region's, whatever cells were left out.
fn rebased(clip: TileClip, region: IRect) -> TileClip {
    let shift = clip.origin - region.min();
    let layers = clip
        .layers
        .into_iter()
        .map(|layer| {
            let mut cells = bt_core::TileLayer::infinite(UVec2::ZERO);
            for placed in layer.cells.cells() {
                // A layer without bounds takes every cell.
                let _changed = cells.set_cell(placed.at + shift, placed.cell);
            }
            ClipLayer { cells, ..layer }
        })
        .collect();
    TileClip {
        origin: region.min(),
        size: region.size(),
        layers,
        tilesets: clip.tilesets,
    }
}

/// The first layer of `map` of the kind called `name`: by its exact name, else by its slug.
fn layer_named(map: &Map, name: &str, objects: bool) -> Option<LayerId> {
    let fits = |layer: &Layer| match layer.kind {
        LayerKind::Tile(_) => !objects,
        LayerKind::Object(_) => objects,
        LayerKind::Image(_) | LayerKind::Group(_) => false,
    };
    let mut exact = None;
    let mut slugged = None;
    let wanted = bt_utils::text::slug(name);
    map.for_each_layer(|layer, _| {
        if !fits(layer) {
            return;
        }
        if exact.is_none() && layer.name == name {
            exact = Some(layer.id);
        }
        if slugged.is_none() && bt_utils::text::slug(&layer.name) == wanted {
            slugged = Some(layer.id);
        }
    });
    exact.or(slugged)
}

/// One of `items`, at random.
pub(super) fn pick<T: Clone>(items: &[T], rng: &mut bt_utils::rng::Rng) -> Option<T> {
    let count = u64::try_from(items.len()).ok()?;
    let index = usize::try_from(rng.below(count)?).ok()?;
    items.get(index).cloned()
}

/// Shuffles `cells` with `rng` (Fisher and Yates).
pub(super) fn shuffle(cells: &mut [IVec2], rng: &mut bt_utils::rng::Rng) {
    for i in (1..cells.len()).rev() {
        let bound = u64::try_from(i + 1).unwrap_or(u64::MAX);
        if let Some(j) = rng.below(bound).and_then(|j| usize::try_from(j).ok()) {
            cells.swap(i, j);
        }
    }
}

/// How many `scatter` places: the count, or the density times the cells.
pub(super) fn wanted_of(amount: Amount, cells: usize) -> usize {
    match amount {
        Amount::Count(count) => usize::try_from(count).unwrap_or(usize::MAX),
        Amount::Density(density) => {
            let wanted = (density * cells as f64).round();
            // A density is between 0 and 1, so this is at most the number of cells.
            if wanted > 0.0 { wanted as usize } else { 0 }
        }
    }
}

/// The note for a scatter that ran out of room.
pub(super) fn short_of_room(verb: &str, placed: usize, wanted: usize, spacing: f64) -> String {
    let apart = if spacing > 0.0 {
        format!(" {} cells apart", crate::words::number_text(spacing))
    } else {
        String::new()
    };
    format!("{verb} placed {placed} of {wanted} (no room left{apart})")
}
