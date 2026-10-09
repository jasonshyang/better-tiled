//! Object templates: an object without id or position, and the tileset of its tile.

use bt_core::ObjectTemplate;
use glam::UVec2;

use super::context::Finisher;
use super::map::GidSpace;
use super::object::{ObjectPlace, UnknownGids};
use crate::raw::RawTemplate;
use crate::warning::WarningKind;

impl Finisher<'_> {
    /// A template; its tileset's global ids are its own.
    pub(crate) fn template_of(&mut self, raw: RawTemplate) -> ObjectTemplate {
        let mut space = GidSpace::default();
        for raw_tileset in raw.tilesets {
            let finished = self.tileset_ref_of(raw_tileset, UVec2::splat(32));
            space.add_tileset(finished.tileset_ref, finished.placeholder, false);
        }
        let object = raw.object.map(|raw| {
            let mut unknown = UnknownGids::default();
            let object = self.object_of(raw, &mut ObjectPlace::Template(&mut space), &mut unknown);
            if unknown.count > 0 {
                self.record(
                    WarningKind::UnknownGid,
                    format!(
                        "the template's tile {} belongs to no tileset; it is left out",
                        unknown.first
                    ),
                );
            }
            object
        });
        if object.is_none() {
            self.record(WarningKind::InvalidValue, "the template has no object");
        }
        space.settle_placeholder_ranges();
        ObjectTemplate {
            object,
            tilesets: space.tilesets,
            file: Default::default(),
        }
    }
}
