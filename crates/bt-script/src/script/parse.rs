//! Reading a script (`10-agents.md`, §7): one operation per line, syntax only. A mistake names
//! its line and what was expected; an unknown operation gets "did you mean". Names stay as
//! written and resolve when the script runs.

use bt_utils::text::fuzzy_match;
use glam::{DVec2, IVec2};

use super::area::{take_area, take_shape};
use super::cursor::Cursor;
use super::op::{
    Amount, Assignment, Binding, CopySource, Drawing, Flip, Geometry, LayerOp, LayerPlace, Layers,
    LayersOp, Legend, LegendEntry, Located, ObjectOptions, ObjectsWhere, Op, Placement, Script,
    SpritePart, TerrainPart,
};
use super::refs::{LayerRef, Named, layer_ref_of, named_items, object_ref_of};
use super::tokens::{comment_of, words_of};
use super::values::{parse_cell, parse_extent, parse_radii, parse_size};

/// The operations, for "did you mean".
const OPERATIONS: [&str; 18] = [
    "seed", "define", "paint", "erase", "clear", "tile", "place", "scatter", "object", "set",
    "move", "delete", "copy", "automap", "property", "layer", "resize", "draw",
];

/// A mistake in a script: its line and what was expected there.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct ParseError {
    /// The line, from 1.
    pub line: usize,
    /// What is wrong.
    pub message: String,
}

/// Reads a script.
pub fn parse_script(text: &str) -> Result<Script, ParseError> {
    let lines: Vec<&str> = text.lines().collect();
    let label = lines
        .iter()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| comment_of(line))
        .filter(|comment| !comment.is_empty())
        .map(str::to_owned);
    let mut ops = Vec::new();
    let mut next = 0;
    while let Some(raw) = lines.get(next) {
        let number = next + 1;
        next += 1;
        let at = |message: String| ParseError {
            line: number,
            message,
        };
        let mut words = words_of(raw).map_err(at)?;
        if words.is_empty() {
            continue;
        }
        let seed = take_own_seed(&mut words).map_err(at)?;
        let op = if words.first().map(String::as_str) == Some("draw") {
            let drawing =
                read_drawing(&words, &lines, &mut next).map_err(|mistake| ParseError {
                    line: mistake.line.unwrap_or(number),
                    message: mistake.message,
                })?;
            Op::Draw(drawing)
        } else {
            let mut cursor = Cursor::new(&words);
            let op = take_op(&mut cursor).map_err(at)?;
            if let Some(rest) = cursor.peek() {
                return Err(at(format!("unexpected “{rest}”")));
            }
            op
        };
        ops.push(Located {
            line: number,
            text: raw.trim().to_owned(),
            op,
            seed,
        });
    }
    Ok(Script { label, ops })
}

/// Reads a definitions file (`--with FILE`): `define` lines and `seed` only.
pub fn parse_definitions(text: &str) -> Result<Script, ParseError> {
    let script = parse_script(text)?;
    if let Some(other) = script.ops.iter().find(|located| {
        !matches!(
            located.op,
            Op::Seed(_)
                | Op::DefineMaterial { .. }
                | Op::DefineGroup { .. }
                | Op::DefineTiles { .. }
                | Op::DefineArea { .. }
                | Op::DefinePiece { .. }
        )
    }) {
        return Err(ParseError {
            line: other.line,
            message: format!(
                "a definitions file holds define lines (and seed), not “{}”",
                other.text
            ),
        });
    }
    Ok(script)
}

/// A mistake inside a drawing: at one of its lines, or (without one) at its `draw` line.
struct DrawingMistake {
    line: Option<usize>,
    message: String,
}

/// Takes `seed N` off the end of an operation: that operation's own randomness. `automap
/// layers seed N` is an operation of its own, not a seed.
fn take_own_seed(words: &mut Vec<String>) -> Result<Option<u64>, String> {
    let n = words.len();
    if n < 3 || words.get(n - 2).map(String::as_str) != Some("seed") {
        return Ok(None);
    }
    if words.len() == 4
        && words.first().map(String::as_str) == Some("automap")
        && words.get(1).map(String::as_str) == Some("layers")
    {
        return Ok(None);
    }
    let Some(value) = words.last() else {
        return Ok(None);
    };
    let seed = value
        .parse::<u64>()
        .map_err(|_| format!("the seed is a whole number, not “{value}”"))?;
    words.truncate(n - 2);
    Ok(Some(seed))
}

/// Reads one operation.
fn take_op(cursor: &mut Cursor<'_>) -> Result<Op, String> {
    let word = cursor.take_word("an operation")?;
    match word {
        "seed" => Ok(Op::Seed(cursor.take_whole("a seed (a whole number)")?)),
        "define" => take_define(cursor),
        "paint" | "erase" => {
            let terrain = cursor.take_word("a terrain")?.to_owned();
            let mut layer = None;
            let area = take_area(cursor, |c, w| take_layer_option(c, w, &mut layer))?;
            Ok(if word == "paint" {
                Op::Paint {
                    terrain,
                    area,
                    layer,
                }
            } else {
                Op::Erase {
                    terrain,
                    area,
                    layer,
                }
            })
        }
        "clear" => {
            let mut layers = None;
            let area = take_area(cursor, |c, w| {
                if w == "layer" || w == "layers" {
                    c.take();
                    layers = Some(take_layers(c)?);
                    return Ok(true);
                }
                Ok(false)
            })?;
            let layers = layers.ok_or(
                "say which layers to clear: clear SHAPE layer L (or layers L,M, or layers all)",
            )?;
            Ok(Op::Clear { area, layers })
        }
        "tile" | "tiles" => {
            let tiles = named_items(&cursor.take_list("tiles TILESET:ID or a tile name")?)?;
            let mut layer = None;
            let area = take_area(cursor, |c, w| take_layer_option(c, w, &mut layer))?;
            Ok(Op::Tile { tiles, area, layer })
        }
        "copy" => {
            let source = take_copy_source(cursor, false)?;
            cursor.take_keyword("to")?;
            let to = cursor.take_cell("the top-left cell X,Y to copy to")?;
            let mut source = source;
            if cursor.take_if("objects") {
                source.objects = true;
            }
            Ok(Op::Copy { source, to })
        }
        "place" => take_place(cursor),
        "scatter" => take_scatter(cursor),
        "object" => take_object(cursor),
        "set" => {
            let mut targets = vec![object_ref_of(cursor.take_word("an object (#ID or name)")?)];
            while let Some(word) = cursor.peek().filter(|w| assignment_of(w).is_none()) {
                cursor.take();
                targets.push(object_ref_of(word));
            }
            let mut assignments = Vec::new();
            while let Some(word) = cursor.take() {
                assignments.push(
                    assignment_of(word)
                        .ok_or_else(|| format!("expected key=value, got “{word}”"))?,
                );
            }
            if assignments.is_empty() {
                return Err("set needs key=value".to_owned());
            }
            Ok(Op::Set {
                targets,
                assignments,
            })
        }
        "move" => {
            let mut targets = vec![object_ref_of(cursor.take_word("an object (#ID or name)")?)];
            while let Some(word) = cursor.peek().filter(|w| *w != "to" && *w != "by") {
                cursor.take();
                targets.push(object_ref_of(word));
            }
            match cursor.take() {
                Some("to") => match targets.as_slice() {
                    [target] => Ok(Op::MoveTo {
                        target: target.clone(),
                        to: cursor.take_point("a point X,Y")?,
                    }),
                    _ => Err("`to` moves one object; move several with `by DX,DY`".to_owned()),
                },
                Some("by") => Ok(Op::MoveBy {
                    targets,
                    by: cursor.take_point("a distance DX,DY")?,
                }),
                _ => Err("write move OBJECT to X,Y or move OBJECT … by DX,DY".to_owned()),
            }
        }
        "delete" => take_delete(cursor),
        "automap" => {
            if cursor.take_if("layers") {
                return take_automap_layers(cursor).map(Op::AutomapLayers);
            }
            let (rules, bindings) = take_rules_and_bindings(cursor, true)?;
            Ok(Op::Automap { rules, bindings })
        }
        "property" | "properties" => {
            let mut assignments = Vec::new();
            while let Some(word) = cursor.take() {
                assignments.push(
                    assignment_of(word)
                        .ok_or_else(|| format!("expected key=value, got “{word}”"))?,
                );
            }
            if assignments.is_empty() {
                return Err("property needs key=value".to_owned());
            }
            Ok(Op::Property { assignments })
        }
        "layer" => take_layer_op(cursor).map(Op::Layer),
        "resize" => {
            let size = parse_size(cursor.take_word("a size WxH")?)?;
            let shift = if cursor.take_if("shift") {
                cursor.take_cell("a shift DX,DY")?
            } else {
                IVec2::ZERO
            };
            Ok(Op::Resize { size, shift })
        }
        other => {
            let close = fuzzy_match(other, &OPERATIONS);
            let hint = if close.is_empty() {
                String::new()
            } else {
                format!(" (did you mean {}?)", close.join(", "))
            };
            Err(format!("unknown operation “{other}”{hint}"))
        }
    }
}

/// `define NAME = …`.
fn take_define(cursor: &mut Cursor<'_>) -> Result<Op, String> {
    let name = cursor.take_word("a name")?.to_owned();
    cursor.take_keyword("=")?;
    match cursor.peek() {
        Some("sprite") => {
            let mut parts = Vec::new();
            loop {
                cursor.take_keyword("sprite")?;
                let sprite = take_one_named(cursor, "a sprite name")?;
                let mut offset = DVec2::ZERO;
                let mut layer = None;
                loop {
                    if cursor.take_if("at") {
                        offset = cursor.take_point("an offset DX,DY from the first sprite")?;
                    } else if cursor.take_if("layer") {
                        layer = Some(layer_ref_of(cursor.take_word("a layer name")?));
                    } else {
                        break;
                    }
                }
                parts.push(SpritePart {
                    sprite,
                    offset,
                    layer,
                });
                if !cursor.take_if("+") {
                    break;
                }
            }
            Ok(Op::DefineGroup { name, parts })
        }
        Some("tile" | "tiles") => {
            cursor.take();
            let tiles = named_items(&cursor.take_list("tiles TILESET:ID")?)?;
            let layer = take_layer_if_written(cursor)?;
            Ok(Op::DefineTiles { name, tiles, layer })
        }
        Some("area") => {
            cursor.take();
            Ok(Op::DefineArea {
                name,
                area: take_area(cursor, |_, _| Ok(false))?,
            })
        }
        Some("copy") => {
            cursor.take();
            let mut source = take_copy_source(cursor, true)?;
            let mut minus = Vec::new();
            let mut foot = None;
            while let Some(word) = cursor.take() {
                match word {
                    "layer" | "layers" => source.layers = take_layers(cursor)?,
                    "objects" => source.objects = true,
                    "foot" => foot = Some(cursor.take_cell("the foot cell X,Y")?),
                    "minus" => minus.push(take_shape(cursor)?),
                    other => {
                        return Err(format!(
                            "unexpected “{other}”: write define NAME = copy MAP REGION [minus \
                             SHAPE] [layers L,…] [objects] [foot X,Y]"
                        ));
                    }
                }
            }
            Ok(Op::DefinePiece {
                name,
                source,
                minus,
                foot,
            })
        }
        _ => Ok(Op::DefineMaterial {
            name,
            parts: take_terrain_parts(cursor)?,
        }),
    }
}

/// `MAP REGION [layers L,…]`: what `copy` takes. `options_later` leaves the layers to the
/// caller, which reads them among other options.
fn take_copy_source(cursor: &mut Cursor<'_>, options_later: bool) -> Result<CopySource, String> {
    let map = cursor
        .take_word("a map to copy from (or . for this map)")?
        .to_owned();
    let region = cursor.take_region()?;
    let mut layers = Layers::All;
    if !options_later && cursor.take_if_any(&["layer", "layers"]).is_some() {
        layers = take_layers(cursor)?;
    }
    Ok(CopySource {
        map,
        region,
        layers,
        objects: false,
    })
}

/// `place NAME,… at X,Y … | rect X,Y WxH | in SHAPE` and its options.
fn take_place(cursor: &mut Cursor<'_>) -> Result<Op, String> {
    let names = named_items(&cursor.take_list("a sprite, group or piece name")?)?;
    let mut flip = Flip::None;
    let mut jitter = 0.0;
    let mut object = ObjectOptions::default();
    let placement = match cursor.take() {
        Some("at") => Placement::At(cursor.take_points(1, "place … at")?),
        Some("rect") => {
            let min = cursor.take_point("the top-left X,Y")?;
            let size = parse_extent(cursor.take_word("a size WxH")?)?;
            Placement::Rect { min, size }
        }
        Some("in") => Placement::In(take_area(cursor, |c, w| {
            take_sprite_option(c, w, &mut flip, &mut jitter, &mut object, true)
        })?),
        _ => {
            return Err(
                "write place NAME at X,Y …, place NAME rect X,Y WxH or place NAME in SHAPE"
                    .to_owned(),
            );
        }
    };
    while let Some(word) = cursor.peek() {
        if !take_sprite_option(cursor, word, &mut flip, &mut jitter, &mut object, true)? {
            return Err(format!("unexpected “{word}”"));
        }
    }
    Ok(Op::Place {
        names,
        placement,
        flip,
        jitter,
        object,
    })
}

/// `scatter NAME,… in SHAPE` and its options.
fn take_scatter(cursor: &mut Cursor<'_>) -> Result<Op, String> {
    let names = named_items(&cursor.take_list("a sprite, group or piece name")?)?;
    cursor.take_keyword("in")?;
    let mut flip = Flip::None;
    let mut jitter = 0.35;
    let mut object = ObjectOptions::default();
    let mut amount = Amount::Density(0.15);
    let mut spacing = 1.0;
    let area = take_area(cursor, |c, w| {
        if take_sprite_option(c, w, &mut flip, &mut jitter, &mut object, false)? {
            return Ok(true);
        }
        match w {
            "count" => {
                c.take();
                amount = Amount::Count(take_count(c)?);
            }
            "density" => {
                c.take();
                let density = c.take_number("a density between 0 and 1")?;
                if !(0.0..=1.0).contains(&density) {
                    return Err(format!("the density is between 0 and 1, not {density}"));
                }
                amount = Amount::Density(density);
            }
            "spacing" => {
                c.take();
                spacing = take_spacing(c)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    })?;
    Ok(Op::Scatter {
        names,
        area,
        amount,
        spacing,
        jitter,
        flip,
        object,
    })
}

/// `object TYPE …`.
fn take_object(cursor: &mut Cursor<'_>) -> Result<Op, String> {
    let kind = cursor.take_word("an object type")?.to_owned();
    let mut object = ObjectOptions::default();
    if cursor.take_if("in") {
        let mut count = 1;
        let mut spacing = 1.0;
        let area = take_area(cursor, |c, w| {
            match w {
                "count" => {
                    c.take();
                    count = take_count(c)?;
                }
                "spacing" => {
                    c.take();
                    spacing = take_spacing(c)?;
                }
                _ => return take_object_option(c, w, &mut object),
            }
            Ok(true)
        })?;
        return Ok(Op::Object {
            kind,
            geometry: Geometry::In {
                area,
                count,
                spacing,
            },
            object,
        });
    }
    let geometry = match cursor.take() {
        Some("at") => Geometry::Point(cursor.take_point("a point X,Y")?),
        Some("rect") => {
            let first = cursor.take_word("a rectangle X,Y WxH")?;
            if first.contains("..") {
                let region = super::values::parse_region(first)?;
                Geometry::Rect {
                    min: region.min().as_dvec2(),
                    size: region.size().as_dvec2(),
                }
            } else {
                Geometry::Rect {
                    min: super::values::parse_point(first)?,
                    size: parse_extent(cursor.take_word("a size WxH")?)?,
                }
            }
        }
        Some("ellipse" | "circle") => Geometry::Ellipse {
            center: cursor.take_point("the centre X,Y")?,
            radii: parse_radii(cursor.take_word("radii RXxRY")?)?,
        },
        Some("poly" | "polygon") => Geometry::Polygon(cursor.take_points(3, "a polygon")?),
        Some("path" | "line") => Geometry::Path(cursor.take_points(2, "a path")?),
        _ => {
            return Err(
                "write object TYPE at X,Y (or rect X,Y WxH, ellipse CX,CY RXxRY, poly \
                        X,Y …, path X,Y …, or in SHAPE count N)"
                    .to_owned(),
            );
        }
    };
    while let Some(word) = cursor.peek() {
        if !take_object_option(cursor, word, &mut object)? {
            return Err(format!(
                "unexpected “{word}” (properties are written key=value)"
            ));
        }
    }
    Ok(Op::Object {
        kind,
        geometry,
        object,
    })
}

/// `delete OBJECT …`, `delete objects in SHAPE …`, `delete objects on TERRAIN,… …`.
fn take_delete(cursor: &mut Cursor<'_>) -> Result<Op, String> {
    let filters = |c: &mut Cursor<'_>,
                   w: &str,
                   kind: &mut Option<String>,
                   layer: &mut Option<LayerRef>|
     -> Result<bool, String> {
        match w {
            "type" => {
                c.take();
                *kind = Some(c.take_word("a type")?.to_owned());
            }
            "layer" => {
                c.take();
                *layer = Some(layer_ref_of(c.take_word("a layer")?));
            }
            _ => return Ok(false),
        }
        Ok(true)
    };
    if cursor.peek() == Some("objects") && matches!(cursor.peek_ahead(1), Some("in" | "on")) {
        cursor.take();
        let mut kind = None;
        let mut layer = None;
        let place = if cursor.take_if("in") {
            ObjectsWhere::In(take_area(cursor, |c, w| {
                filters(c, w, &mut kind, &mut layer)
            })?)
        } else {
            cursor.take_keyword("on")?;
            let grounds = named_items(&cursor.take_list("a terrain")?)?;
            while let Some(word) = cursor.peek() {
                if !filters(cursor, word, &mut kind, &mut layer)? {
                    return Err(format!(
                        "unexpected “{word}” (delete objects on TERRAIN [type T] [layer L])"
                    ));
                }
            }
            ObjectsWhere::On(grounds)
        };
        return Ok(Op::DeleteObjects { place, kind, layer });
    }
    let mut targets = Vec::new();
    while let Some(word) = cursor.take() {
        targets.push(object_ref_of(word));
    }
    if targets.is_empty() {
        return Err(
            "write delete #ID …, delete objects in SHAPE [type T] [layer L] or delete \
                    objects on TERRAIN"
                .to_owned(),
        );
    }
    Ok(Op::Delete { targets })
}

/// `automap layers …`.
fn take_automap_layers(cursor: &mut Cursor<'_>) -> Result<LayersOp, String> {
    match cursor.peek() {
        Some("off") => {
            cursor.take();
            Ok(LayersOp::Off)
        }
        Some("seed") => {
            cursor.take();
            Ok(LayersOp::Seed(
                cursor.take_whole("a seed (a whole number)")?,
            ))
        }
        Some("add") => {
            cursor.take();
            let (_, bindings) = take_rules_and_bindings(cursor, false)?;
            if bindings.is_empty() {
                return Err(
                    "automap layers add: say which layers the run is on, e.g. automap \
                            layers add walls=plat2"
                        .to_owned(),
                );
            }
            Ok(LayersOp::Add { bindings })
        }
        _ => {
            let (rules, bindings) = take_rules_and_bindings(cursor, true)?;
            Ok(LayersOp::Start { rules, bindings })
        }
    }
}

/// `[RULES] [RULE-LAYER=MAP-LAYER …]`.
fn take_rules_and_bindings(
    cursor: &mut Cursor<'_>,
    rules_allowed: bool,
) -> Result<(Option<String>, Vec<Binding>), String> {
    let mut rules = None;
    let mut bindings = Vec::new();
    while let Some(word) = cursor.take() {
        match word.split_once('=') {
            Some((rule_layer, map_layer)) if !rule_layer.is_empty() && !map_layer.is_empty() => {
                bindings.push(Binding {
                    rule_layer: rule_layer.to_owned(),
                    map_layer: layer_ref_of(map_layer),
                });
            }
            None if rules_allowed && rules.is_none() => rules = Some(word.to_owned()),
            _ => {
                return Err(format!(
                    "unexpected “{word}”: write RULE-LAYER=MAP-LAYER (e.g. walls=plat2)"
                ));
            }
        }
    }
    Ok((rules, bindings))
}

/// `layer …`.
fn take_layer_op(cursor: &mut Cursor<'_>) -> Result<LayerOp, String> {
    let verb = cursor.take_word("add, rename, delete, hide, show, lock, unlock or move")?;
    let layer = |c: &mut Cursor<'_>| c.take_word("a layer").map(layer_ref_of);
    Ok(match verb {
        "add" => {
            let name = cursor.take_word("a layer name")?.to_owned();
            let objects = match cursor.take() {
                Some("tiles" | "tile") => false,
                Some("objects" | "object") => true,
                _ => {
                    return Err(
                        "write layer add NAME tiles (or objects) [above L | below L]".to_owned(),
                    );
                }
            };
            let place = match cursor.peek() {
                Some("above" | "below") => Some(take_layer_place(cursor)?),
                _ => None,
            };
            LayerOp::Add {
                name,
                objects,
                place,
            }
        }
        "rename" => LayerOp::Rename {
            layer: layer(cursor)?,
            name: cursor.take_word("the new name")?.to_owned(),
        },
        "delete" | "remove" => LayerOp::Delete {
            layer: layer(cursor)?,
        },
        "hide" | "show" => LayerOp::SetVisible {
            layer: layer(cursor)?,
            visible: verb == "show",
        },
        "lock" | "unlock" => LayerOp::SetLocked {
            layer: layer(cursor)?,
            locked: verb == "lock",
        },
        "move" => LayerOp::Move {
            layer: layer(cursor)?,
            place: take_layer_place(cursor)?,
        },
        other => {
            return Err(format!(
                "unknown layer operation “{other}”: add, rename, delete, hide, show, lock, \
                 unlock or move"
            ));
        }
    })
}

/// `above L` or `below L`.
fn take_layer_place(cursor: &mut Cursor<'_>) -> Result<LayerPlace, String> {
    let above = match cursor.take() {
        Some("above") => true,
        Some("below") => false,
        _ => return Err("write above L or below L".to_owned()),
    };
    Ok(LayerPlace {
        above,
        layer: layer_ref_of(cursor.take_word("a layer")?),
    })
}

/// `L,M` or `all`, after `layer`/`layers`.
fn take_layers(cursor: &mut Cursor<'_>) -> Result<Layers, String> {
    let names = cursor.take_list("a layer name (or all)")?;
    if names.iter().any(|name| name == "all") {
        return match names.as_slice() {
            [_] => Ok(Layers::All),
            _ => Err("write layers all, or name the layers".to_owned()),
        };
    }
    Ok(Layers::Only(
        names.iter().map(|name| layer_ref_of(name)).collect(),
    ))
}

/// `TERRAIN [layer L] [+ TERRAIN [layer L]] …`.
fn take_terrain_parts(cursor: &mut Cursor<'_>) -> Result<Vec<TerrainPart>, String> {
    let mut parts = Vec::new();
    loop {
        let terrain = cursor.take_word("a terrain")?.to_owned();
        let layer = take_layer_if_written(cursor)?;
        parts.push(TerrainPart { terrain, layer });
        if !cursor.take_if("+") {
            break;
        }
    }
    Ok(parts)
}

/// `layer L`, when it is next.
fn take_layer_if_written(cursor: &mut Cursor<'_>) -> Result<Option<LayerRef>, String> {
    if cursor.take_if("layer") {
        return Ok(Some(layer_ref_of(cursor.take_word("a layer name")?)));
    }
    Ok(None)
}

/// One name, or one tile by id.
fn take_one_named(cursor: &mut Cursor<'_>, what: &str) -> Result<Named, String> {
    let word = cursor.take_word(what)?;
    let mut named = named_items(&[word.to_owned()])?;
    match (named.pop(), named.is_empty()) {
        (Some(one), true) => Ok(one),
        _ => Err(format!("expected {what}")),
    }
}

/// `layer L` among an area's words.
fn take_layer_option(
    cursor: &mut Cursor<'_>,
    word: &str,
    layer: &mut Option<LayerRef>,
) -> Result<bool, String> {
    if word == "layer" {
        cursor.take();
        *layer = Some(layer_ref_of(cursor.take_word("a layer name")?));
        return Ok(true);
    }
    Ok(false)
}

/// The options of what `place` and `scatter` put down: `flip [random]`, `jitter J`, `layer L`,
/// `like OBJECT`, and for `place` also `name N` and `key=value`.
fn take_sprite_option(
    cursor: &mut Cursor<'_>,
    word: &str,
    flip: &mut Flip,
    jitter: &mut f64,
    object: &mut ObjectOptions,
    named: bool,
) -> Result<bool, String> {
    match word {
        "flip" | "flipped" => {
            cursor.take();
            *flip = if cursor.take_if("random") {
                Flip::Random
            } else {
                Flip::Flipped
            };
        }
        "jitter" => {
            cursor.take();
            let value = cursor.take_number("a jitter in cells between 0 and 1")?;
            if !(0.0..=1.0).contains(&value) {
                return Err(format!("the jitter is between 0 and 1, not {value}"));
            }
            *jitter = value;
        }
        "layer" | "like" => return take_object_option(cursor, word, object),
        "name" if named => return take_object_option(cursor, word, object),
        _ if named => return take_object_option(cursor, word, object),
        _ => return Ok(false),
    }
    Ok(true)
}

/// `name N`, `layer L`, `like OBJECT` and `key=value`.
fn take_object_option(
    cursor: &mut Cursor<'_>,
    word: &str,
    object: &mut ObjectOptions,
) -> Result<bool, String> {
    match word {
        "name" => {
            cursor.take();
            object.name = Some(cursor.take_word("a name")?.to_owned());
        }
        "layer" => {
            cursor.take();
            object.layer = Some(layer_ref_of(cursor.take_word("a layer name")?));
        }
        "like" => {
            cursor.take();
            object.like = Some(object_ref_of(cursor.take_word("an object (#ID or name)")?));
        }
        _ => match assignment_of(word) {
            Some(assignment) => {
                cursor.take();
                object.assignments.push(assignment);
            }
            None => return Ok(false),
        },
    }
    Ok(true)
}

/// `count N`: a whole number.
fn take_count(cursor: &mut Cursor<'_>) -> Result<u32, String> {
    let count = cursor.take_whole("a count (a whole number)")?;
    u32::try_from(count).map_err(|_| format!("{count} is too many"))
}

/// `spacing S`: cells, zero or more.
fn take_spacing(cursor: &mut Cursor<'_>) -> Result<f64, String> {
    let spacing = cursor.take_number("a spacing in cells")?;
    if spacing < 0.0 {
        return Err(format!("the spacing is 0 or more, not {spacing}"));
    }
    Ok(spacing)
}

/// `key=value`, when the word is one.
fn assignment_of(word: &str) -> Option<Assignment> {
    let (key, value) = word.split_once('=')?;
    (!key.is_empty() && !key.starts_with('@')).then(|| Assignment {
        key: key.to_owned(),
        value: value.to_owned(),
    })
}

/// Reads a `draw` block from its `draw X,Y` line (`words`) to its `end`, moving `next` past it.
fn read_drawing(
    words: &[String],
    lines: &[&str],
    next: &mut usize,
) -> Result<Drawing, DrawingMistake> {
    let at_draw = |message: String| DrawingMistake {
        line: None,
        message,
    };
    let origin = match words {
        [_, cell] => parse_cell(cell).map_err(at_draw)?,
        _ => {
            return Err(at_draw(
                "write draw X,Y (the top-left cell of the rows that follow)".to_owned(),
            ));
        }
    };
    let mut legend: Vec<LegendEntry> = Vec::new();
    let mut rows = Vec::new();
    let mut rows_line = 0;
    loop {
        let Some(line) = lines.get(*next) else {
            return Err(at_draw("the draw block has no `end`".to_owned()));
        };
        let number = *next + 1;
        *next += 1;
        let at_line = |message: String| DrawingMistake {
            line: Some(number),
            message,
        };
        if let Some(row) = line.trim_start().strip_prefix('|') {
            if rows.is_empty() {
                rows_line = number;
            }
            rows.push(row.trim_end().to_owned());
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "end" {
            break;
        }
        // `C = …`, where C may be any character, `#` too; a `#` line without `=` is a comment.
        let mut chars = trimmed.chars();
        let Some(symbol) = chars.next() else {
            continue;
        };
        let rest = words_of(chars.as_str()).map_err(at_line)?;
        if symbol == '#' && rest.first().map(String::as_str) != Some("=") {
            continue;
        }
        legend.push(read_legend(symbol, &rest).map_err(at_line)?);
    }
    if rows.is_empty() {
        return Err(at_draw(
            "the draw block has no rows (rows start with |)".to_owned(),
        ));
    }
    for entry in &legend {
        if !rows.iter().any(|row| row.contains(entry.symbol)) {
            return Err(at_draw(format!(
                "“{}” is in the legend but not in the rows",
                entry.symbol
            )));
        }
    }
    for (index, row) in rows.iter().enumerate() {
        if let Some(symbol) = row
            .chars()
            .find(|c| *c != '.' && *c != ' ' && !legend.iter().any(|e| e.symbol == *c))
        {
            return Err(DrawingMistake {
                line: Some(rows_line + index),
                message: format!(
                    "“{symbol}” is not in the legend (use . or a space for cells to leave as \
                     they are)"
                ),
            });
        }
    }
    Ok(Drawing {
        origin,
        legend,
        rows,
        rows_line,
    })
}

/// What a legend line can be, for messages.
fn legend_forms(symbol: char) -> String {
    format!(
        "`{symbol} = TERRAIN [layer L]`, `{symbol} = tile TILESET:ID [layer L]` (or a tile name), \
         `{symbol} = clear layer L`, `{symbol} = sprite NAME` or `{symbol} = object TYPE \
         [key=value…]`"
    )
}

/// `C = …` (the words after the character).
fn read_legend(symbol: char, words: &[String]) -> Result<LegendEntry, String> {
    if symbol == '.' || symbol == '|' {
        return Err(format!(
            "“{symbol}” can't be in the legend (. leaves a cell as it is, | starts a row)"
        ));
    }
    if words.first().map(String::as_str) != Some("=") {
        return Err(format!(
            "write the legend as {} (and end the block with `end`)",
            legend_forms(symbol)
        ));
    }
    let rest = words.get(1..).unwrap_or_default();
    let mut cursor = Cursor::new(rest);
    let meaning = match cursor.peek() {
        Some("clear") => {
            cursor.take();
            if cursor.take_if_any(&["layer", "layers"]).is_none() {
                return Err(format!(
                    "write `{symbol} = clear layer L` (the layers to empty)"
                ));
            }
            Legend::Clear(take_layers(&mut cursor)?)
        }
        Some("tile" | "tiles") => {
            cursor.take();
            let tiles = named_items(&cursor.take_list("tiles TILESET:ID")?)?;
            Legend::Tiles {
                tiles,
                layer: take_layer_if_written(&mut cursor)?,
            }
        }
        Some(word) if word.contains(':') => {
            let tiles = named_items(&cursor.take_list("tiles TILESET:ID")?)?;
            Legend::Tiles {
                tiles,
                layer: take_layer_if_written(&mut cursor)?,
            }
        }
        Some("sprite" | "sprites") => {
            cursor.take();
            let sprites = named_items(&cursor.take_list("a sprite name")?)?;
            Legend::Sprites {
                sprites,
                layer: take_layer_if_written(&mut cursor)?,
            }
        }
        Some("object") => {
            cursor.take();
            let kind = cursor.take_word("an object type")?.to_owned();
            let mut object = ObjectOptions::default();
            while let Some(word) = cursor.peek() {
                if word == "layer" || assignment_of(word).is_some() {
                    take_object_option(&mut cursor, word, &mut object)?;
                } else {
                    return Err(format!("unexpected “{word}”"));
                }
            }
            Legend::Object {
                kind,
                layer: object.layer,
                assignments: object.assignments,
            }
        }
        _ => Legend::Paint(take_terrain_parts(&mut cursor)?),
    };
    if let Some(rest) = cursor.peek() {
        return Err(format!(
            "unexpected “{rest}”: a legend line is {}",
            legend_forms(symbol)
        ));
    }
    Ok(LegendEntry { symbol, meaning })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::area::{Area, Nearness, Shape};
    use crate::script::refs::{IdRange, ObjectRef, TileIds};
    use bt_utils::IRect;

    fn op(line: &str) -> Op {
        parse_script(line)
            .unwrap_or_else(|e| panic!("{line}: {e}"))
            .ops
            .remove(0)
            .op
    }

    fn error(text: &str) -> String {
        parse_script(text).unwrap_err().to_string()
    }

    fn name(text: &str) -> Named {
        Named::Name(text.to_owned())
    }

    fn layer(text: &str) -> Option<LayerRef> {
        Some(LayerRef::Name(text.to_owned()))
    }

    #[test]
    fn terrain_operations_read_as_written() {
        assert_eq!(
            op(
                "paint frozen-lake ellipse 22.5,12.5 8x4.5 rough 0.1 on snow-ground avoid terrain1 layer ice"
            ),
            Op::Paint {
                terrain: "frozen-lake".into(),
                area: Area {
                    on: vec![name("snow-ground")],
                    avoid: vec![Nearness {
                        grounds: vec![name("terrain1")],
                        margin: 0
                    }],
                    ..Area::of(Shape::Ellipse {
                        center: DVec2::new(22.5, 12.5),
                        radii: DVec2::new(8.0, 4.5),
                        rough: Some(0.1),
                    })
                },
                layer: layer("ice"),
            }
        );
        assert_eq!(
            op("erase grass 3,3"),
            Op::Erase {
                terrain: "grass".into(),
                area: Area::of(Shape::Cells(vec![IVec2::new(3, 3)])),
                layer: None,
            }
        );
        assert_eq!(
            op("clear 1,1 2x2 layers ground, #4"),
            Op::Clear {
                area: Area::of(Shape::Rect(IRect::from_min_size(
                    IVec2::ONE,
                    IVec2::splat(2)
                ))),
                layers: Layers::Only(vec![LayerRef::Name("ground".into()), LayerRef::Id(4)]),
            }
        );
        assert!(matches!(
            op("clear all layer all"),
            Op::Clear {
                layers: Layers::All,
                ..
            }
        ));
        assert!(matches!(
            op("tile hex-mini:5 where plains near mountain margin 2 layer Ground"),
            Op::Tile { ref tiles, ref area, layer: Some(_) } if tiles.len() == 1 && area.near[0].margin == 2
        ));
    }

    #[test]
    fn sprites_and_objects_read_as_written() {
        assert_eq!(
            op("scatter pine-big, pine-small in all count 12 spacing 3 avoid water flip random"),
            Op::Scatter {
                names: vec![name("pine-big"), name("pine-small")],
                area: Area {
                    avoid: vec![Nearness {
                        grounds: vec![name("water")],
                        margin: 0
                    }],
                    ..Area::of(Shape::All)
                },
                amount: Amount::Count(12),
                spacing: 3.0,
                jitter: 0.35,
                flip: Flip::Random,
                object: ObjectOptions::default(),
            }
        );
        let Op::Place {
            placement, object, ..
        } = op("place house at 10.5,6 12.5,9 layer props name home owner=bob like #3")
        else {
            panic!()
        };
        assert_eq!(
            placement,
            Placement::At(vec![DVec2::new(10.5, 6.0), DVec2::new(12.5, 9.0)])
        );
        assert_eq!(
            object,
            ObjectOptions {
                name: Some("home".into()),
                layer: layer("props"),
                like: Some(ObjectRef::Id(3)),
                assignments: vec![Assignment {
                    key: "owner".into(),
                    value: "bob".into()
                }],
            }
        );
        assert!(matches!(
            op("place gate rect 2,3 4x1.5 flip"),
            Op::Place { placement: Placement::Rect { size, .. }, flip: Flip::Flipped, .. } if size == DVec2::new(4.0, 1.5)
        ));
        assert!(matches!(
            op("object hazard rect 13,8 18x10 kind=\"slick ice\""),
            Op::Object { geometry: Geometry::Rect { size, .. }, ref object, .. }
                if size == DVec2::new(18.0, 10.0) && object.assignments[0].value == "slick ice"
        ));
        assert!(matches!(
            op("object spawn in circle 22.5,26 5 count 3 spacing 2 on snow wave=1 layer design"),
            Op::Object { geometry: Geometry::In { count: 3, spacing, ref area }, ref object, .. }
                if spacing == 2.0 && area.on.len() == 1 && object.assignments.len() == 1 && object.layer.is_some()
        ));
        assert_eq!(
            op("move #4 #5 by 0,1"),
            Op::MoveBy {
                targets: vec![ObjectRef::Id(4), ObjectRef::Id(5)],
                by: DVec2::new(0.0, 1.0)
            }
        );
        assert_eq!(
            op("move entry to 3.5,4"),
            Op::MoveTo {
                target: ObjectRef::Name("entry".into()),
                to: DVec2::new(3.5, 4.0)
            }
        );
        assert_eq!(
            op("set selected #7 name=gate size=2x1"),
            Op::Set {
                targets: vec![ObjectRef::Selected, ObjectRef::Id(7)],
                assignments: vec![
                    Assignment {
                        key: "name".into(),
                        value: "gate".into()
                    },
                    Assignment {
                        key: "size".into(),
                        value: "2x1".into()
                    },
                ],
            }
        );
        assert_eq!(
            op("delete objects on frozen-lake layer props"),
            Op::DeleteObjects {
                place: ObjectsWhere::On(vec![name("frozen-lake")]),
                kind: None,
                layer: layer("props"),
            }
        );
        assert!(matches!(
            op("delete objects in @arena type spawn"),
            Op::DeleteObjects {
                place: ObjectsWhere::In(_),
                kind: Some(_),
                ..
            }
        ));
        assert_eq!(
            op("delete #12 last"),
            Op::Delete {
                targets: vec![ObjectRef::Id(12), ObjectRef::Last]
            }
        );
    }

    #[test]
    fn definitions_read_as_written() {
        assert!(matches!(
            op("define path = snow-ground layer ground + terrain1 layer dirt"),
            Op::DefineMaterial { ref parts, .. } if parts.len() == 2
        ));
        assert_eq!(
            op("define tree = sprite pine layer props + sprite shadow at 0,1.2 layer shadows"),
            Op::DefineGroup {
                name: "tree".into(),
                parts: vec![
                    SpritePart {
                        sprite: name("pine"),
                        offset: DVec2::ZERO,
                        layer: layer("props")
                    },
                    SpritePart {
                        sprite: name("shadow"),
                        offset: DVec2::new(0.0, 1.2),
                        layer: layer("shadows")
                    },
                ],
            }
        );
        assert_eq!(
            op("define mountain = tile hex-mini:10,14 layer Ground"),
            Op::DefineTiles {
                name: "mountain".into(),
                tiles: vec![Named::Tiles(TileIds {
                    tileset: "hex-mini".into(),
                    ranges: vec![
                        IdRange {
                            first: 10,
                            last: 10
                        },
                        IdRange {
                            first: 14,
                            last: 14
                        }
                    ],
                })],
                layer: layer("Ground"),
            }
        );
        assert!(matches!(
            op("define lake = area where water containing 12,10 13,10"),
            Op::DefineArea { ref area, .. } if area.containing.len() == 2
        ));
        assert_eq!(
            op(
                "define bridge = copy ../c2.tmx 21,29 3x5 minus 21,29 layers bridges,rails objects foot 22,33"
            ),
            Op::DefinePiece {
                name: "bridge".into(),
                source: CopySource {
                    map: "../c2.tmx".into(),
                    region: IRect::from_min_size(IVec2::new(21, 29), IVec2::new(3, 5)),
                    layers: Layers::Only(vec![
                        LayerRef::Name("bridges".into()),
                        LayerRef::Name("rails".into())
                    ]),
                    objects: true,
                },
                minus: vec![Shape::Cells(vec![IVec2::new(21, 29)])],
                foot: Some(IVec2::new(22, 33)),
            }
        );
        assert!(matches!(
            op("copy ../c2.tmx 21,29 3x5 layers bridges,bridge_rails to 20,30 objects"),
            Op::Copy { ref source, to } if source.objects && to == IVec2::new(20, 30)
                && matches!(source.layers, Layers::Only(ref l) if l.len() == 2)
        ));
    }

    #[test]
    fn rules_layers_and_the_map_read_as_written() {
        assert_eq!(
            op("automap rules.txt walls=ground"),
            Op::Automap {
                rules: Some("rules.txt".into()),
                bindings: vec![Binding {
                    rule_layer: "walls".into(),
                    map_layer: LayerRef::Name("ground".into())
                }],
            }
        );
        assert_eq!(
            op("automap layers seed 5"),
            Op::AutomapLayers(LayersOp::Seed(5))
        );
        assert_eq!(op("automap layers off"), Op::AutomapLayers(LayersOp::Off));
        assert!(matches!(
            op("automap layers add walls=plat2"),
            Op::AutomapLayers(LayersOp::Add { ref bindings }) if bindings.len() == 1
        ));
        assert!(matches!(
            op("automap layers walls=#2"),
            Op::AutomapLayers(LayersOp::Start { rules: None, ref bindings })
                if bindings[0].map_layer == LayerRef::Id(2)
        ));
        assert_eq!(
            op("layer add props objects above ground"),
            Op::Layer(LayerOp::Add {
                name: "props".into(),
                objects: true,
                place: Some(LayerPlace {
                    above: true,
                    layer: LayerRef::Name("ground".into())
                }),
            })
        );
        assert_eq!(
            op("layer hide #3"),
            Op::Layer(LayerOp::SetVisible {
                layer: LayerRef::Id(3),
                visible: false
            })
        );
        assert_eq!(
            op("resize 40x30 shift 2,0"),
            Op::Resize {
                size: IVec2::new(40, 30),
                shift: IVec2::new(2, 0)
            }
        );
        assert!(matches!(
            op("property weather=snow"),
            Op::Property { ref assignments } if assignments.len() == 1
        ));
    }

    #[test]
    fn seeds_comments_and_labels() {
        let script = parse_script(
            "# widen the lake\nseed 4\n\npaint water all seed 9 # a comment\nautomap layers seed 3",
        )
        .unwrap();
        assert_eq!(script.label.as_deref(), Some("widen the lake"));
        assert_eq!(script.ops.len(), 3);
        assert_eq!(script.ops[0].op, Op::Seed(4));
        assert_eq!((script.ops[1].line, script.ops[1].seed), (4, Some(9)));
        assert_eq!(script.ops[1].text, "paint water all seed 9 # a comment");
        assert_eq!(script.ops[2].op, Op::AutomapLayers(LayersOp::Seed(3)));
        assert_eq!(script.ops[2].seed, None);
        assert_eq!(parse_script("paint water all").unwrap().label, None);
    }

    #[test]
    fn drawings_read_their_legend_and_rows() {
        let script = parse_script(
            "draw 2,3\n  # = snow-ground layer ground\n  # a comment\n  T = sprite pine-big\n  \
             ~ = rules-sewers:1\n  x = clear layer ground\n  |..##~\n  |.#Tx\nend\nseed 4",
        )
        .unwrap();
        assert_eq!(script.ops.len(), 2);
        let Op::Draw(drawing) = &script.ops[0].op else {
            panic!()
        };
        assert_eq!(drawing.origin, IVec2::new(2, 3));
        assert_eq!(drawing.legend.len(), 4);
        assert_eq!(drawing.rows, ["..##~", ".#Tx"]);
        assert_eq!(drawing.rows_line, 7);
        assert!(matches!(
            drawing.legend[2].meaning,
            Legend::Tiles { layer: None, .. }
        ));
        assert!(matches!(drawing.legend[3].meaning, Legend::Clear(_)));
        assert_eq!(script.ops[1].line, 10);
        assert!(error("draw 0,0\n|#\nend").contains("not in the legend"));
        assert!(error("draw 0,0\n# = grass\n|..").contains("no `end`"));
        assert!(
            error("draw 0,0\n# = grass\nT = tree\n|#\nend")
                .contains("“T” is in the legend but not in the rows")
        );
        assert_eq!(
            error("draw 0,0\n# = grass\n|#\n|#?\nend"),
            "line 4: “?” is not in the legend (use . or a space for cells to leave as they are)"
        );
        assert!(error("draw 0,0\n# grass banana\n|#\nend").contains("not in the legend"));
        assert!(
            error("draw 0,0\nx = grass banana\n|x\nend").starts_with("line 2: unexpected “banana”")
        );
    }

    #[test]
    fn mistakes_say_where_and_what() {
        assert_eq!(
            error("seed 1\npaint grass ellipse 1,1"),
            "line 2: expected radii RXxRY"
        );
        assert!(error("pant grass all").contains("did you mean paint"));
        assert!(error("clear all").contains("say which layers"));
        assert!(error("paint grass all banana").contains("unexpected “banana”"));
        assert!(error("move #4 #5 to 1,1").contains("`to` moves one object"));
        assert!(error("scatter tree in all density 2").contains("between 0 and 1"));
        assert!(error("paint grass all seed x").contains("the seed is a whole number"));
        assert!(error("set #1").contains("set needs key=value"));
        assert!(error("tile 5 all").contains("without a tileset"));
    }

    #[test]
    fn definitions_files_hold_definitions_only() {
        assert!(parse_definitions("define tree = sprite pine\nseed 3").is_ok());
        assert_eq!(
            parse_definitions("define tree = sprite pine\npaint grass all")
                .unwrap_err()
                .line,
            2
        );
    }
}
