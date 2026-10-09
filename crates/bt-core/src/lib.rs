//! The data model of better-tiled: maps, tilesets, objects, properties, terrain sets, worlds and
//! projects, with the grid geometry of every Tiled orientation.
//!
//! The crate is data and arithmetic on data: no pixels, no file access, no undo and no global
//! state. The file format crate builds its values, the renderer and the exports read them, the
//! editing crates change them, and undo is built on cloning them: tile layers keep their cells
//! in shared 16×16 chunks and object layers share their object lists, so a clone copies only
//! the tree.

mod cell;
mod chunk;
mod color;
mod custom_types;
mod error;
mod file_format;
mod file_ref;
mod flip;
mod geometry;
mod ids;
mod image_layer;
mod image_ref;
mod layer;
mod map;
mod object;
mod object_layer;
mod project;
mod property;
mod template;
mod text;
mod tile;
mod tile_layer;
mod tileset;
mod validate;
mod wang;
mod world;

pub use cell::{Cell, PlacedCell};
pub use chunk::{CHUNK_SIZE, Chunk, ChunkPos};
pub use color::Color;
pub use custom_types::{
    CircularMember, ClassType, ClassUsage, CustomType, CustomTypeKind, CustomTypes, EnumStorage,
    EnumType, UsageName,
};
pub use error::Error;
pub use file_format::{ExportSettings, FileFormat, LayerDataFormat, LineEnding};
pub use file_ref::FileRef;
pub use flip::Flip;
pub use geometry::{DrawOrderKey, GridLine, Hex, HexParams, MapGeometry};
pub use ids::{CustomTypeId, LayerId, ObjectId, TileId, TilesetSlot};
pub use image_layer::ImageLayer;
pub use image_ref::{EmbeddedImage, ImageRef, ImageSource};
pub use layer::{BlendMode, EffectiveLayer, Layer, LayerKind};
pub use map::{Map, Orientation, RenderOrder, StaggerAxis, StaggerIndex, TilesetRef};
pub use object::{MapObject, ObjectOverrides, Shape};
pub use object_layer::{DrawOrder, ObjectLayer};
pub use project::{Command, CompatibilityVersion, Project};
pub use property::{Properties, Property, Value};
pub use template::ObjectTemplate;
pub use text::{HAlign, TextData, VAlign};
pub use tile::{CollisionGroup, Frame, TileData};
pub use tile_layer::{LayerChunk, TileBounds, TileLayer};
pub use tileset::{
    Alignment, FillMode, GridOrientation, TileImage, TileRenderSize, Tileset, TilesetGrid,
    TilesetImage, Transformations,
};
pub use validate::Violation;
pub use wang::{Lane, WangColor, WangId, WangSet, WangSetType};
pub use world::{World, WorldMap, WorldPattern};
