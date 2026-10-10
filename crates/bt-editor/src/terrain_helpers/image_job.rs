//! Reading a tileset's pixels for the labelling helpers, on a worker: the editor never decodes an
//! image on the thread that owns it. A [`TilesetImageJob`] decodes the tileset's image and looks
//! for autotile layouts in it, or labels one terrain from one tile of it; its
//! [`TilesetImageRead`] comes back as [`crate::Intent::TilesetImageRead`].

use std::collections::BTreeMap;
use std::sync::Arc;

use bt_core::{ImageRef, ImageSource, TileId, Tileset, TilesetImage, WangSetType};
use bt_image::{ColorKey, DecodedImage};
use bt_terrain::TerrainColor;
use bt_terrain::autolabel::{Method, Proposal, label_with};
use bt_terrain::layouts::{Fit, Samples, detect};
use bt_utils::IRect;
use glam::UVec2;

use crate::document::DocumentId;
use crate::editor::Editor;
use crate::effect::Effect;
use crate::store::TilesetHome;

/// What the layouts of a tileset's image depend on: the image and how it is cut into tiles.
/// Results read from another cut are stale.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ImageCut {
    image: ImageRef,
    tile_size: UVec2,
    margin: u32,
    spacing: u32,
    columns: u32,
    tile_count: u32,
}

impl ImageCut {
    /// The cut of an atlas tileset; `None` for a collection of images, which has no grid.
    pub(crate) fn of(tileset: &Tileset) -> Option<ImageCut> {
        let TilesetImage::Atlas {
            image,
            columns,
            tile_count,
        } = &tileset.image
        else {
            return None;
        };
        Some(ImageCut {
            image: image.clone(),
            tile_size: tileset.tile_size(),
            margin: tileset.margin,
            spacing: tileset.spacing,
            columns: *columns,
            tile_count: *tile_count,
        })
    }
}

/// Auto-label's question: terrain `color` of set `set`, of `kind`, labelled from tile
/// `reference`, which is all of it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AutoLabelRequest {
    pub(crate) set: usize,
    pub(crate) kind: WangSetType,
    pub(crate) color: TerrainColor,
    pub(crate) reference: TileId,
}

/// What a job reads the pixels for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ImagePurpose {
    /// Layout…: the blocks of autotile layouts in the image.
    FindLayouts,
    /// Auto-label: the tiles one terrain is on, from one tile of it.
    AutoLabel(AutoLabelRequest),
}

/// A read of a tileset's image for a labelling helper, to run on a worker with
/// [`TilesetImageJob::run`].
#[derive(Clone, Debug)]
pub struct TilesetImageJob {
    doc: DocumentId,
    home: TilesetHome,
    tileset: Arc<Tileset>,
    cut: ImageCut,
    purpose: ImagePurpose,
}

/// What a [`TilesetImageJob`] read, for [`crate::Intent::TilesetImageRead`].
#[derive(Debug)]
pub struct TilesetImageRead {
    doc: DocumentId,
    home: TilesetHome,
    cut: ImageCut,
    findings: ImageFindings,
}

/// What a job found in the pixels, for the purpose it read them for; each the reason the image
/// could not be read instead.
#[derive(Debug)]
enum ImageFindings {
    /// The blocks of layouts.
    Layouts(Result<FoundLayouts, String>),
    /// The labels proposed for the terrain asked about.
    AutoLabel {
        /// The question.
        request: AutoLabelRequest,
        /// The proposal.
        proposal: Result<Proposal, String>,
    },
}

/// The blocks of autotile layouts found in a tileset's image (Layout…), with what the tiles look
/// like near each lane, which Label All tells terrains apart by.
#[derive(Clone, Debug)]
pub struct FoundLayouts {
    pub(crate) samples: Samples,
    pub(crate) fits: Vec<Fit>,
}

impl FoundLayouts {
    /// The blocks found, best first, none overlapping another (`bt_terrain::layouts::detect`).
    pub fn fits(&self) -> &[Fit] {
        &self.fits
    }
}

impl TilesetImageJob {
    /// A read of `tileset`'s image (which lives at `home`, edited in `doc`) for `purpose`;
    /// `None` for a collection of images, which has no one image to read.
    pub(crate) fn new(
        doc: DocumentId,
        home: TilesetHome,
        tileset: Arc<Tileset>,
        purpose: ImagePurpose,
    ) -> Option<TilesetImageJob> {
        let cut = ImageCut::of(&tileset)?;
        Some(TilesetImageJob {
            doc,
            home,
            tileset,
            cut,
            purpose,
        })
    }

    /// The document that asked.
    pub fn doc(&self) -> DocumentId {
        self.doc
    }

    /// The image and its cut, as the job reads them.
    pub(crate) fn cut(&self) -> &ImageCut {
        &self.cut
    }

    /// Decodes the image and reads it; for a worker thread.
    pub fn run(&self) -> TilesetImageRead {
        let image = decode(&self.cut.image).map_err(|error| error.to_string());
        let findings = match self.purpose {
            ImagePurpose::FindLayouts => ImageFindings::Layouts(image.map(|image| {
                let samples = Samples::from_tileset(&self.tileset, &image);
                let fits = detect(&samples);
                FoundLayouts { samples, fits }
            })),
            ImagePurpose::AutoLabel(request) => ImageFindings::AutoLabel {
                request,
                proposal: image.and_then(|image| self.auto_label(&image, request)),
            },
        };
        TilesetImageRead {
            doc: self.doc,
            home: self.home,
            cut: self.cut.clone(),
            findings,
        }
    }

    /// `bt_terrain::autolabel::label_with`, choosing its method, over every tile of the image.
    fn auto_label(
        &self,
        image: &DecodedImage,
        request: AutoLabelRequest,
    ) -> Result<Proposal, String> {
        let tiles: BTreeMap<TileId, IRect> = self
            .tileset
            .tile_ids()
            .into_iter()
            .filter_map(|id| Some((id, self.tileset.tile_image(id)?.rect)))
            .collect();
        let reference = tiles
            .get(&request.reference)
            .copied()
            .ok_or_else(|| format!("the tileset has no tile {}", request.reference.0))?;
        Ok(label_with(
            Method::Auto,
            image,
            request.kind,
            self.tileset.tile_size(),
            reference,
            request.color,
            &tiles,
        ))
    }
}

impl Editor {
    /// Takes in what a read of a tileset's pixels found, for the helper that asked.
    pub(crate) fn finish_reading_tileset_image(&mut self, read: TilesetImageRead) -> Vec<Effect> {
        let TilesetImageRead {
            doc,
            home,
            cut,
            findings,
        } = read;
        match findings {
            ImageFindings::Layouts(found) => self.finish_finding_layouts(doc, home, cut, found),
            ImageFindings::AutoLabel { request, proposal } => {
                self.finish_auto_labelling(doc, home, request, proposal)
            }
        }
    }
}

/// Decodes an image of the model, with its colour drawn as transparent.
fn decode(image: &ImageRef) -> Result<DecodedImage, bt_image::Error> {
    let key = image.transparent.map(|c| ColorKey {
        r: c.r,
        g: c.g,
        b: c.b,
    });
    match &image.source {
        ImageSource::File(file) => bt_image::decode_file(&file.path, key),
        ImageSource::Embedded(embedded) => {
            bt_image::decode_bytes(&embedded.bytes, &embedded.format, key)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cut_is_the_image_and_how_it_is_cut_into_tiles() {
        let atlas = |tile_width: u32, margin: u32| Tileset {
            tile_width,
            tile_height: 16,
            margin,
            image: TilesetImage::Atlas {
                image: ImageRef::file(bt_core::FileRef::new("a.png", "/a.png"), UVec2::splat(64)),
                columns: 4,
                tile_count: 16,
            },
            ..Tileset::default()
        };
        let cut = ImageCut::of(&atlas(16, 0)).unwrap();
        assert!(cut.matches(&atlas(16, 0)));
        assert!(!cut.matches(&atlas(8, 0)), "another tile size");
        assert!(!cut.matches(&atlas(16, 1)), "another margin");
        assert_eq!(ImageCut::of(&Tileset::default()), None, "a collection");
    }
}
