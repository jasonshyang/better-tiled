//! Where object positions snap: the snapping preference, changed for one gesture by the
//! modifiers held, as Tiled's `SnapHelper` does (`reference/tiled/src/tiled/snaphelper.cpp`):
//! Command turns grid snapping on (from none or whole pixels) or off (from the grid or the fine
//! grid), and Command with Shift switches to the fine grid, or from it back to the grid.

use bt_edit::{SnapMode, snap};
use glam::DVec2;

use crate::document::DocumentId;
use crate::editor::Editor;
use crate::settings::Snapping;

/// How the modifiers held change the snapping preference for one gesture.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum SnapToggle {
    /// The preference as it is.
    #[default]
    Keep,
    /// Grid snapping turned on, or off (Command).
    Grid,
    /// The fine grid turned on, or back to the grid (Command and Shift).
    FineGrid,
}

impl Editor {
    /// `pixel` snapped as the snapping preference, changed by `toggle`, says.
    pub fn snapped(&self, doc: DocumentId, pixel: DVec2, toggle: SnapToggle) -> DVec2 {
        match self.map_of(doc) {
            Some(map) => snap(&map.geometry(), pixel, self.snap_mode(toggle)),
            None => pixel,
        }
    }

    /// The snapping preference as the object commands take it, changed by `toggle`.
    pub(crate) fn snap_mode(&self, toggle: SnapToggle) -> SnapMode {
        let fine = SnapMode::FineGrid(self.settings.interface.fine_grid_divisions);
        let preference = match self.settings.behaviour.snapping {
            Snapping::None => SnapMode::None,
            Snapping::Grid => SnapMode::Grid,
            Snapping::FineGrid => fine,
            Snapping::Pixels => SnapMode::Pixels,
        };
        match (toggle, preference) {
            (SnapToggle::Keep, mode) => mode,
            (SnapToggle::Grid, SnapMode::None | SnapMode::Pixels) => SnapMode::Grid,
            (SnapToggle::Grid, SnapMode::Grid | SnapMode::FineGrid(_)) => SnapMode::None,
            (SnapToggle::FineGrid, SnapMode::FineGrid(_)) => SnapMode::Grid,
            (SnapToggle::FineGrid, SnapMode::None | SnapMode::Grid | SnapMode::Pixels) => fine,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn editor_snapping(snapping: Snapping) -> Editor {
        let mut settings = Settings::default();
        settings.behaviour.snapping = snapping;
        settings.interface.fine_grid_divisions = 4;
        let mut editor = Editor::new();
        editor.change_settings(settings);
        editor
    }

    #[test]
    fn the_modifiers_toggle_snapping_as_in_tiled() {
        let cases = [
            (Snapping::None, SnapToggle::Grid, SnapMode::Grid),
            (Snapping::Pixels, SnapToggle::Grid, SnapMode::Grid),
            (Snapping::Grid, SnapToggle::Grid, SnapMode::None),
            (Snapping::FineGrid, SnapToggle::Grid, SnapMode::None),
            (Snapping::None, SnapToggle::FineGrid, SnapMode::FineGrid(4)),
            (Snapping::Grid, SnapToggle::FineGrid, SnapMode::FineGrid(4)),
            (Snapping::FineGrid, SnapToggle::FineGrid, SnapMode::Grid),
            (Snapping::Pixels, SnapToggle::Keep, SnapMode::Pixels),
        ];
        for (preference, toggle, expected) in cases {
            assert_eq!(
                editor_snapping(preference).snap_mode(toggle),
                expected,
                "{preference:?} with {toggle:?}"
            );
        }
    }
}
