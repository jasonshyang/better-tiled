//! The scene's `TileSet` (docs/design/12-godot-export.md): every tile of the map's atlas tilesets, and of the image
//! collections a tile layer is painted with, with their texture origins, probabilities,
//! collision and animations. Sources are numbered in tileset order: an atlas tileset's source,
//! then the sources of its animated tiles (animation.rs); an image collection's tiles one source
//! each, by id. Each source is named after what it holds. A TileSet shared by the maps of one
//! grid (shared.rs) is a `.tres` of its own, which numbers its sources by those names.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write;
use std::path::Path;

use bt_core::{Cell, Layer, LayerKind, Map, TileRenderSize, Tileset};
use glam::{DVec2, IVec2};

use super::animation::{self, Animation};
use super::grid::Flipped;
use super::report::{Element, Note};
use super::{RULE_TILES, collision, shared, terrain, text, tiledata};
use crate::build::Build;
use bt_utils::paths::{canonical_or_clean as normalized, reference_path as relative_path};

/// The id of the scene's one `TileSet`.
pub(super) const TILESET_ID: &str = "TileSet_0";

/// A tile of an atlas source.
pub(super) struct TileDef {
    /// The Tiled tile it is: its tileset's slot in the map, and its id there.
    pub slot: usize,
    pub id: u32,
    pub origin: IVec2,
    /// Tiled's probability, which Godot's terrain and scatter painting weigh tiles by too.
    pub probability: f64,
    pub polygons: Vec<collision::TileShape>,
    /// The animation of an animated tile in an animation source; `None` shows its first frame.
    pub animation: Option<Animation>,
    /// Alternatives 1, 2, … for flipped cells Godot would draw elsewhere (grid.rs).
    pub flipped: Vec<Flipped>,
}

pub(super) struct AtlasSource {
    /// Its source id: its place in the TileSet, or a shared TileSet's number for its name.
    pub id: usize,
    /// Its `resource_name`: its tileset's name (`source_names`), then `#<id>` for a tile of an
    /// image collection or `#anim<n>` for the n-th animation source of an atlas.
    pub name: String,
    /// Its image's `res://` path.
    pub texture: String,
    pub margins: IVec2,
    pub separation: IVec2,
    pub region: IVec2,
    pub tiles: BTreeMap<(i32, i32), TileDef>,
}

#[derive(Default)]
pub(super) struct TileSetBuilder {
    pub sources: Vec<AtlasSource>,
    /// What each tileset's sources are named after, by slot (`source_names`).
    pub names: Vec<String>,
    /// A Tiled tile (slot, id) → the source and atlas cell its cells point at.
    cells: HashMap<(usize, u32), (usize, IVec2)>,
    built: bool,
    /// How many physics layers the tiles' collision shapes use.
    pub physics: usize,
    /// The tiles' custom data and Godot settings (tiledata.rs).
    pub data: tiledata::Data,
    /// The terrain sets, and the Wang tiles' terrain (terrain.rs).
    pub terrain: terrain::Terrains,
    /// The `.tres` the TileSet goes in when the maps of its grid share one (shared.rs).
    pub shared: Option<shared::Shared>,
    /// The tiles without an image that cells (or tile objects) use, by slot, and the warning
    /// saying so.
    no_image: HashMap<(usize, bool), (BTreeSet<u32>, String)>,
}

/// What the sources of each of the map's tilesets are named after, by slot: the tileset file's
/// path from the Godot project or, for a tileset inside the map, the map's path, `::` and the
/// tileset's name. They stay the same from one export to the next, so a shared TileSet can find
/// its sources again by them. `%` and `#` are written `%25` and `%23`, so a source's suffix is
/// always after its last `#`. Takes the map as loaded: the resolved map has its tilesets embedded.
pub(super) fn source_names(map: &Map, map_path: &Path, project: &Path) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    map.tilesets
        .iter()
        .map(|t| {
            let name = match &t.file {
                Some(file) => escaped(&relative_path(project, &file.path)),
                None => format!(
                    "{}::{}",
                    escaped(&relative_path(project, map_path)),
                    escaped(&t.tileset.name)
                ),
            };
            // The same file twice, or two tilesets of one name in the map: numbered from the second.
            let n = seen.entry(name.clone()).or_default();
            *n += 1;
            if *n == 1 { name } else { format!("{name}~{n}") }
        })
        .collect()
}

fn escaped(name: &str) -> String {
    name.replace('%', "%25").replace('#', "%23")
}

impl Build<'_> {
    /// A cell's atlas source, atlas coordinates and transform flags.
    pub(super) fn cell(&mut self, cell: Cell) -> Option<(u16, IVec2, u16)> {
        let map = self.map;
        let slot = cell.slot()?.index();
        if self.is_rule_slot(slot) {
            self.warn_once(crate::Code::RuleTiles, RULE_TILES.to_owned(), Vec::new());
            return None;
        }
        map.tilesets.get(slot)?;
        self.tileset_contents();
        let Some(&(source, atlas)) = self.tileset.cells.get(&(slot, cell.tile().0)) else {
            self.no_image(slot, cell.tile().0, false);
            return None;
        };
        // Flipped tall tiles get an alternative that keeps them where Tiled draws them (grid.rs).
        let source_data = self.tileset.sources.get(source)?;
        let size = source_data.region;
        let id = source_data.id;
        Some((
            id as u16,
            atlas,
            self.alternative(source, atlas, slot, cell.tile().0, size, cell.flip()),
        ))
    }

    /// Notes that the tile `tile` of the tileset in `slot`, used by cells (or with `objects`, by
    /// tile objects), has no image: one warning for each tileset's, which grows with each tile.
    pub(super) fn no_image(&mut self, slot: usize, tile: u32, objects: bool) {
        let (ids, said) = self.tileset.no_image.entry((slot, objects)).or_default();
        if !ids.insert(tile) {
            return;
        }
        let ids: Vec<u32> = ids.iter().copied().collect();
        let (tiles, them) = match &ids[..] {
            [one] => (format!("tile {one} has no image"), "it"),
            _ => (
                format!("{} tiles have no image ({})", ids.len(), id_runs(&ids)),
                "them",
            ),
        };
        let left_out = if objects {
            format!("tile objects showing {them} show nothing")
        } else {
            format!("the cells showing {them} are left out")
        };
        let message = format!(
            "tileset {}: {tiles}; {left_out}",
            self.map
                .tilesets
                .get(slot)
                .map(|t| t.tileset.name.as_str())
                .unwrap_or("missing tileset")
        );
        let elements: Vec<Element> = ids
            .iter()
            .map(|&id| Element::Tile { tileset: slot, id })
            .collect();
        let before = std::mem::replace(said, message.clone());
        match self
            .notes
            .iter_mut()
            .find(|n| n.code == crate::Code::TileImageMissing && n.message == before)
        {
            Some(note) => (note.message, note.elements) = (message, elements),
            None => self.notes.push(Note::warning(
                crate::Code::TileImageMissing,
                message,
                elements,
            )),
        }
    }

    /// Makes every source of the TileSet, the first time it is needed. A shared TileSet also
    /// holds the image collections it has from other maps, and the tilesets of other maps
    /// (which the map's tilesets are extended with).
    pub(super) fn tileset_contents(&mut self) {
        if std::mem::replace(&mut self.tileset.built, true) {
            return;
        }
        let notes = self.notes.len();
        let map = self.map;
        let mut painted = HashSet::new();
        painted_slots(&map.layers, &mut painted);
        for (slot, tileset) in map.tilesets.iter().enumerate() {
            if self.is_rule_slot(slot) {
                continue;
            }
            let ts = &tileset.tileset;
            let name = self
                .tileset
                .names
                .get(slot)
                .cloned()
                .unwrap_or_else(|| escaped(&ts.name));
            if !ts.is_collection() {
                self.atlas_sources(ts, slot, name);
            } else if painted.contains(&slot)
                || self
                    .tileset
                    .shared
                    .as_ref()
                    .is_some_and(|s| s.registered(&name))
            {
                self.collection_sources(ts, slot, name);
            }
        }
        self.number_sources();
        let kept = self
            .tileset
            .shared
            .as_ref()
            .map_or(0, shared::Shared::kept_physics);
        self.tileset.physics = collision::layers_used(
            self.tileset
                .sources
                .iter()
                .flat_map(|s| s.tiles.values())
                .flat_map(|t| &t.polygons),
        )
        .max(kept);
        self.terrains();
        self.tile_data();
        self.area_notes();
        self.own_notes(notes);
    }

    /// The `res://` path of a tileset's image, as the scene uses it (or a shared TileSet, by its
    /// path from the tileset's folder).
    fn tileset_texture(&mut self, slot: usize, image: &bt_core::ImageRef) -> String {
        let Some(shared) = &self.tileset.shared else {
            // Registered now, so the scene's images keep the order they had.
            let res = self.images.res_image(image);
            self.scene.ext_resource("Texture2D", res.clone());
            return res;
        };
        let dir = self
            .tileset
            .data
            .dirs
            .get(slot)
            .map_or_else(|| self.map_dir.clone(), |dir| normalized(dir));
        self.images.res_image_for(image, &shared.res, &dir)
    }

    /// What a tile layer's `tile_set` is: the TileSet in the scene, or the shared one.
    pub(super) fn tile_set(&mut self) -> String {
        let Some(shared) = &self.tileset.shared else {
            return format!("SubResource(\"{TILESET_ID}\")");
        };
        let (res, uid) = (shared.res.clone(), shared.uid.clone());
        let id = self.scene.ext_resource("TileSet", res.clone());
        if let Some(ext) = self.scene.ext.iter_mut().find(|e| e.path == res) {
            ext.uid = Some(uid);
        }
        id
    }

    /// An atlas tileset's sources: every tile, then its animated tiles at their first frame's
    /// cell in sources of their own, as few as hold them without overlapping.
    fn atlas_sources(&mut self, ts: &Tileset, slot: usize, name: String) {
        let bt_core::TilesetImage::Atlas { image, .. } = &ts.image else {
            return;
        };
        let texture = self.tileset_texture(slot, image);
        let columns = ts.columns();
        let at = |id: u32| IVec2::new((id % columns) as i32, (id / columns) as i32);
        let size = IVec2::new(ts.tile_width as i32, ts.tile_height as i32);
        let source = |name: String| AtlasSource {
            id: 0,
            name,
            texture: texture.clone(),
            margins: IVec2::splat(ts.margin as i32),
            separation: IVec2::splat(ts.spacing as i32),
            region: size,
            tiles: BTreeMap::new(),
        };
        let base = self.tileset.sources.len();
        let mut plain = source(name.clone());
        for id in 0..ts.tile_count() {
            plain
                .tiles
                .insert(at(id).into(), self.tile_def(ts, slot, id, size));
            self.tileset.cells.insert((slot, id), (base, at(id)));
        }
        self.tileset.sources.push(plain);

        // Each animated tile, by id: the cells it covers, and its animation when Godot can step
        // through it (otherwise it covers its first frame's cell, and shows that).
        let mut animated = Vec::new();
        let mut first_frames = Vec::new();
        for (&bt_core::TileId(id), data) in &ts.tiles {
            let frames = &data.animation;
            if id >= ts.tile_count()
                || frames.is_empty()
                || frames.len() == 1 && frames.first().is_some_and(|f| f.tile.0 == id)
            {
                continue;
            }
            let cells: Vec<IVec2> = frames.iter().map(|f| at(f.tile.0)).collect();
            let valid = frames.iter().all(|f| f.tile.0 < ts.tile_count());
            match animation::solve(&cells).filter(|_| valid) {
                Some(layout) => animated.push((id, cells, Some(Animation::new(layout, frames)))),
                None => {
                    first_frames.push(id);
                    // A first frame that isn't a tile of the tileset: the tile shows itself.
                    if frames.first().is_some_and(|f| f.tile.0 < ts.tile_count()) {
                        animated.push((id, cells.first().copied().into_iter().collect(), None));
                    }
                }
            }
        }
        // A shared TileSet's animated tiles stay in the sources its file has them in while they fit
        // there: the cells of scenes exported before name those.
        let heads: Vec<shared::AnimatedTile> = animated
            .iter()
            .filter_map(|(_, cells, a)| {
                Some((
                    *cells.first()?,
                    a.iter().flat_map(animation::lines).collect(),
                ))
            })
            .collect();
        let was = self
            .tileset
            .shared
            .as_ref()
            .map(|s| s.animation_sources(&name, &heads))
            .unwrap_or_default();
        let packed = animation::pack(
            &animated
                .iter()
                .map(|(_, cells, _)| cells.clone())
                .collect::<Vec<_>>(),
            &was,
        );
        let mut sources: BTreeMap<usize, AtlasSource> = BTreeMap::new();
        let mut placed = Vec::new();
        for ((id, cells, animation), n) in animated.into_iter().zip(packed) {
            let Some(&head) = cells.first() else {
                self.failures
                    .push(format!("animated tile {id} has no first frame"));
                continue;
            };
            let mut def = self.tile_def(ts, slot, id, size);
            def.animation = animation;
            sources
                .entry(n)
                .or_insert_with(|| source(format!("{name}#anim{}", n + 1)))
                .tiles
                .insert(head.into(), def);
            placed.push((id, n, head));
        }
        // A source left empty isn't made (a shared TileSet keeps it as it was).
        let place: HashMap<usize, usize> = sources
            .keys()
            .enumerate()
            .map(|(i, &n)| (n, base + 1 + i))
            .collect();
        for (id, n, head) in placed {
            if let Some(&source) = place.get(&n) {
                self.tileset.cells.insert((slot, id), (source, head));
            } else {
                self.failures
                    .push(format!("animation source {n} was not packed"));
            }
        }
        self.tileset.sources.extend(sources.into_values());
        self.first_frames(ts, slot, &first_frames, "animates only frames evenly spaced along a row or column of the image, or filling whole rows");
    }

    /// An image collection's sources: one per tile, by id, on the part of an image the tile
    /// shows first. An animated tile animates when its frames are parts of one size evenly spaced
    /// in one image (its source's grid is then theirs), and shows its first frame otherwise.
    fn collection_sources(&mut self, ts: &Tileset, slot: usize, name: String) {
        let mut first_frames = Vec::new();
        for (&bt_core::TileId(id), data) in &ts.tiles {
            let (mut shown, mut spacing, mut animation) = (id, IVec2::ZERO, None);
            if let Some(first) = data.animation.first() {
                if data.animation.len() > 1 {
                    match animation::strip(ts, &data.animation) {
                        Some((between, layout)) => {
                            (spacing, animation) =
                                (between, Some(Animation::new(layout, &data.animation)))
                        }
                        None => first_frames.push(id),
                    }
                }
                if ts.tile_image(first.tile).is_some() {
                    shown = first.tile.0;
                }
            }
            // A tile without an image has no source; its cells are left out.
            let Some(bt_core::TileImage { image, rect }) = ts.tile_image(bt_core::TileId(shown))
            else {
                continue;
            };
            let texture = self.tileset_texture(slot, image);
            let mut def = self.tile_def(ts, slot, id, IVec2::new(rect.w, rect.h));
            def.animation = animation;
            self.tileset
                .cells
                .insert((slot, id), (self.tileset.sources.len(), IVec2::ZERO));
            self.tileset.sources.push(AtlasSource {
                id: 0,
                name: format!("{name}#{id}"),
                texture,
                margins: IVec2::new(rect.x, rect.y),
                separation: spacing,
                region: IVec2::new(rect.w, rect.h),
                tiles: BTreeMap::from([((0, 0), def)]),
            });
        }
        self.first_frames(
            ts,
            slot,
            &first_frames,
            "animates only frames of one size evenly spaced in one image",
        );
    }

    /// One warning for a tileset's animated tiles that Godot shows the first frame of.
    fn first_frames(&mut self, ts: &Tileset, slot: usize, ids: &[u32], why: &str) {
        if ids.is_empty() {
            return;
        }
        let tiles = if ids.len() == 1 {
            "1 animated tile".to_owned()
        } else {
            format!("{} animated tiles", ids.len())
        };
        let message = format!(
            "tileset {}: Godot shows the first frame of {tiles} ({}): it {why}",
            ts.name,
            id_runs(ids)
        );
        self.warn_once(
            crate::Code::AnimationLayout,
            message,
            ids.iter()
                .map(|&id| Element::Tile { tileset: slot, id })
                .collect(),
        );
    }

    /// A tile's texture origin (Tiled draws tiles from the cell's bottom-left corner, Godot
    /// centers them on the cell), probability and collision polygons.
    fn tile_def(&mut self, ts: &Tileset, slot: usize, id: u32, size: IVec2) -> TileDef {
        let cell = self.grid.cell_box();
        if ts.tile_render_size == TileRenderSize::Grid && size != cell {
            self.warn_once(crate::Code::TileRenderSize, format!("tileset {}: tiles drawn at the grid's size are drawn at their own size in Godot", ts.name), vec![Element::Tileset(slot)]);
        }
        let twice = IVec2::new(cell.x - size.x, size.y - cell.y);
        if twice.x % 2 != 0 || twice.y % 2 != 0 {
            self.warn_once(
                crate::Code::HalfPixelOrigin,
                format!("tileset {}: tiles {}×{} px don't center on {}×{} px cells; Godot draws them half a pixel off", ts.name, size.x, size.y, cell.x, cell.y),
                vec![Element::Tileset(slot)],
            );
        }
        let origin = IVec2::new(twice.x.div_euclid(2), twice.y.div_euclid(2)) - ts.tile_offset;
        let mut def = TileDef {
            slot,
            id,
            origin,
            probability: 1.0,
            polygons: Vec::new(),
            animation: None,
            flipped: Vec::new(),
        };
        let Some(data) = ts.tile(bt_core::TileId(id)) else {
            return def;
        };
        def.probability = data.probability;
        // Collision: Godot's polygons are around the tile's center.
        def.polygons = self.tile_shapes(
            ts,
            slot,
            id,
            size.as_dvec2(),
            size.as_dvec2() / 2.0 + origin.as_dvec2(),
        );
        def
    }

    /// The `[sub_resource]` sections of the TileSet in the scene: its atlas sources, then the
    /// TileSet.
    pub(super) fn tileset_sections(&mut self) -> String {
        self.tileset_contents();
        let textures: Vec<String> = self
            .tileset
            .sources
            .iter()
            .map(|s| s.texture.clone())
            .collect();
        let textures: Vec<String> = textures
            .into_iter()
            .map(|res| self.scene.ext_resource("Texture2D", res))
            .collect();
        let mut out = collision::resources(&self.tileset, BTreeMap::new());
        for (source, texture) in self.tileset.sources.iter().zip(&textures) {
            self.source_section(&mut out, source, texture);
        }
        let _ = write!(
            out,
            "\n[sub_resource type=\"TileSet\" id=\"{TILESET_ID}\"]\n"
        );
        for (key, value) in self.tileset_lines() {
            let _ = writeln!(out, "{key} = {value}");
        }
        for source in &self.tileset.sources {
            let _ = writeln!(
                out,
                "sources/{0} = SubResource(\"TileSetAtlasSource_{0}\")",
                source.id
            );
        }
        out
    }

    /// An atlas source's section, its image being `texture` (`ExtResource("…")`).
    pub(super) fn source_section(&self, out: &mut String, source: &AtlasSource, texture: &str) {
        let (map, grid, tileset) = (self.map, &self.grid, &self.tileset);
        let _ = write!(
            out,
            "\n[sub_resource type=\"TileSetAtlasSource\" id=\"TileSetAtlasSource_{}\"]\nresource_name = {}\ntexture = {texture}\n",
            source.id,
            text::string(&source.name)
        );
        if source.margins != IVec2::ZERO {
            let _ = writeln!(out, "margins = {}", text::vector2i(source.margins));
        }
        if source.separation != IVec2::ZERO {
            let _ = writeln!(out, "separation = {}", text::vector2i(source.separation));
        }
        let _ = writeln!(
            out,
            "texture_region_size = {}",
            text::vector2i(source.region)
        );
        for (&(x, y), def) in &source.tiles {
            for (key, value) in def.animation.iter().flat_map(animation::lines) {
                let _ = writeln!(out, "{x}:{y}/{key} = {value}");
            }
            let own: Vec<(String, String)> =
                terrain::tile_lines(map, grid, tileset, source.id, def.slot, def.id)
                    .into_iter()
                    .chain(tiledata::tile_lines(map, tileset, def.slot, def.id))
                    .collect();
            tile_lines(out, (x, y), def, &own);
            for (key, value) in self.flipped_lines(def) {
                let _ = writeln!(out, "{x}:{y}/{key} = {value}");
            }
        }
    }

    /// The TileSet's own lines, before its `sources/N`: its grid, then its physics, terrain and
    /// custom data layers (Godot reads a tile's physics and custom data only when its layers come
    /// before the sources).
    pub(super) fn tileset_lines(&self) -> Vec<(String, String)> {
        let (map, grid, tileset) = (self.map, &self.grid, &self.tileset);
        let mut lines = grid.tileset_lines();
        lines.push(("tile_size".to_owned(), text::vector2i(grid.tile_size())));
        lines.extend(collision::tileset_lines(
            self.setup.physics_layers.as_deref().unwrap_or_default(),
            tileset.physics,
        ));
        lines.extend(collision::area_lines(&self.setup, tileset));
        lines.extend(terrain::tileset_lines(map, grid, tileset));
        lines.extend(tiledata::tileset_lines(map, tileset));
        lines
    }
}

/// The lines of the tile at `x:y` itself (alternative 0): its texture origin, probability and
/// collision, then `own` (its terrain and custom data lines). Its flipped alternatives follow
/// (`flipped_lines`).
fn tile_lines(out: &mut String, (x, y): (i32, i32), def: &TileDef, own: &[(String, String)]) {
    let _ = writeln!(out, "{x}:{y}/0 = 0");
    if def.origin != IVec2::ZERO {
        let _ = writeln!(
            out,
            "{x}:{y}/0/texture_origin = {}",
            text::vector2i(def.origin)
        );
    }
    if def.probability != 1.0 {
        let _ = writeln!(
            out,
            "{x}:{y}/0/probability = {}",
            text::float(def.probability)
        );
    }
    for (key, value) in collision::tile_lines(&def.polygons, DVec2::ZERO)
        .iter()
        .chain(own)
    {
        let _ = writeln!(out, "{x}:{y}/0/{key} = {value}");
    }
}

/// The tilesets a tile layer is painted with, by slot (a `tilesetOnly` layer's too, not a
/// `noExport` one's).
fn painted_slots(layers: &[Layer], slots: &mut HashSet<usize>) {
    for layer in layers.iter().filter(|l| !super::layers::no_export(l)) {
        match &layer.kind {
            LayerKind::Tile(tiles) => slots.extend(
                tiles
                    .cells()
                    .filter_map(|p| p.cell.slot())
                    .map(|s| s.index()),
            ),
            LayerKind::Group(children) => painted_slots(children, slots),
            _ => {}
        }
    }
}

/// Ids as runs: `37, 41–43`, the first six of them and then `…` (the notes say how many tiles
/// there are, and their elements list them all).
pub(super) fn id_runs(ids: &[u32]) -> String {
    const SHOWN: usize = 6;
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for &id in ids {
        match runs.last_mut() {
            Some((_, end)) if *end + 1 == id => *end = id,
            _ => runs.push((id, id)),
        }
    }
    let mut text: Vec<String> = runs
        .iter()
        .take(SHOWN)
        .map(|&(a, b)| {
            if a == b {
                a.to_string()
            } else {
                format!("{a}–{b}")
            }
        })
        .collect();
    if runs.len() > SHOWN {
        text.push("…".to_owned());
    }
    text.join(", ")
}
