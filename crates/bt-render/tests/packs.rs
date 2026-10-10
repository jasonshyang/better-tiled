//! The purchased packs' maps against their own pictures. The Highlands pack's biome map comes
//! with a PNG of each chamber, drawn by the pack folder's own compositor from the same maps. It
//! leaves out the design layer's shape objects, so they are left out here too, and it draws an
//! enemy at each spawn point, which the maps only mark: the area around a spawn is not compared.
//! The preview's compositor is not Tiled: where two props of a top-down layer overlap, it stacks
//! a few pixels differently from Tiled's order by y (chamber 1), so a few pixels may differ.

#![cfg(test)]

mod common;

use std::sync::Arc;

use bt_core::{LayerKind, Map};
use bt_render::{ObjectStyle, Picture, Render};
use common::{device, differing_share, keep};
use glam::DVec2;

const BIOME: &str = "assets/Epic RPG World - Highlands V1.4.1/Highlands Biome Map";

const CHAMBERS: [&str; 8] = [
    "c1 - Cloudbreak Landing",
    "c2 - Frozen Mere",
    "c3 - Pinewatch Hollow",
    "c4 - Shattered Terraces",
    "c5 - Rune Circle",
    "c6 - Star Arena",
    "c7 - Hearth Camp",
    "c8 - Frostspire Gate",
];

#[test]
fn the_highlands_chambers_match_their_previews() {
    let Some(folder) = bt_test_data::test_data(BIOME) else {
        return;
    };
    let Some(device) = device() else { return };
    for (index, name) in CHAMBERS.iter().enumerate() {
        let path = folder.join("tiled").join(format!("{name}.tmx"));
        let loaded = bt_tmx::Loader::default().load_map(&path).unwrap();
        assert!(loaded.warnings.is_empty(), "{name}: {:?}", loaded.warnings);
        let spawns = spawn_points(&loaded.value);
        let picture = Render::of(Arc::new(loaded.value))
            .objects(ObjectStyle::default().shapes(false))
            .run(device)
            .unwrap();
        let preview = image::open(folder.join("previews").join(format!("c{}.png", index + 1)))
            .unwrap()
            .to_rgba8();
        let mut expected = Picture {
            width: preview.width(),
            height: preview.height(),
            rgba: preview.into_raw(),
        };
        let mut picture = picture;
        for spawn in &spawns {
            leave_out(&mut picture, *spawn);
            leave_out(&mut expected, *spawn);
        }
        keep(&picture, name);
        let share = differing_share(&picture, &expected, 3);
        eprintln!("{name}: {:.4} % differ", share * 100.0);
        assert!(
            share < 0.001,
            "{name}: {:.3} % of pixels differ from its preview",
            share * 100.0
        );
    }
}

/// An enemy's spawn: where the design layer marks it and how large the preview draws it.
#[derive(Copy, Clone)]
struct Spawn {
    at: DVec2,
    scale: f64,
}

/// The spawns the design layer marks.
fn spawn_points(map: &Map) -> Vec<Spawn> {
    let mut points = Vec::new();
    map.for_each_layer(|layer, _| {
        if let LayerKind::Object(objects) = &layer.kind {
            points.extend(
                objects
                    .objects
                    .iter()
                    .filter(|o| o.class == "spawn")
                    .map(|o| {
                        let scale = o
                            .properties
                            .get("scale")
                            .and_then(|p| match &p.value {
                                bt_core::Value::String(text) => text.parse().ok(),
                                bt_core::Value::Float(value) => Some(*value),
                                bt_core::Value::Int(value) => Some(*value as f64),
                                _ => None,
                            })
                            .unwrap_or(1.0);
                        Spawn { at: o.pos, scale }
                    }),
            );
        }
    });
    points
}

/// Clears the area an enemy drawn at a spawn point can cover, in both pictures.
fn leave_out(picture: &mut Picture, spawn: Spawn) {
    let reach = |pixels: f64| pixels * spawn.scale.max(1.0);
    let x0 = (spawn.at.x - reach(96.0)).max(0.0) as u32;
    let x1 = ((spawn.at.x + reach(96.0)) as u32).min(picture.width);
    let y0 = (spawn.at.y - reach(240.0)).max(0.0) as u32;
    let y1 = ((spawn.at.y + reach(48.0)) as u32).min(picture.height);
    for y in y0..y1 {
        for x in x0..x1 {
            let at = ((y * picture.width + x) * 4) as usize;
            picture.rgba[at..at + 4].copy_from_slice(&[0, 0, 0, 0]);
        }
    }
}
