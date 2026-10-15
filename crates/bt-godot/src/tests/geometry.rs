use bt_core::{Map, MapObject, ObjectFrame};
use glam::DVec2;
pub(super) fn frame(map: &Map, object: &MapObject, offset: DVec2) -> Frame {
    Frame {
        inner: ObjectFrame::of(&map.geometry(), object),
        offset,
    }
}
pub(super) struct Frame {
    inner: ObjectFrame,
    offset: DVec2,
}
impl Frame {
    pub fn to_world(&self, p: DVec2) -> DVec2 {
        self.inner.to_screen(p) + self.offset
    }
}
pub(super) use bt_core::rotated as rotate;
pub(super) fn local_box(map: &Map, o: &MapObject) -> (DVec2, DVec2) {
    let outline = bt_core::object_local_outline(
        &map.geometry(),
        o,
        o.tile.and_then(|c| c.slot()).and_then(|s| map.tileset(s)),
    );
    let bounds = bt_utils::DRect::from_points(outline.points().iter().copied()).unwrap();
    (bounds.min, bounds.max)
}
