//! Godot format, regeneration, mapping and headless behavior tests.
mod bodies;
mod contract;
mod front_door;
mod geometry;
mod grid;
mod grid_oracle;
mod images;
mod inheritors;
mod layers;
mod names;
mod navigation;
mod objects;
mod oracle;
mod safety;
mod shared;
mod support;
mod terrain;
mod tiledata;
mod tileset;

use crate::*;
use crate::report;
use bt_core::LayerKind;
use serde_json::Value;
