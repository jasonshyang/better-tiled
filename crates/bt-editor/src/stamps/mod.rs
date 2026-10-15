//! Saved and quick stamps: the stamp library writes files immediately; choosing a stamp
//! changes the brush, and painting it remains an ordinary undoable brush stroke.

mod convert;
mod store;

use std::path::PathBuf;

use crate::{Editor, Effect, Notice};
use convert::{brush_of, stamp_of};
use store::Operation;
pub use store::{SavedStamp, StampJob, StampStore, StampsLoaded};

/// A stamp-library command.
#[derive(Clone, Debug)]
pub enum StampEdit {
    /// Reload the current folder.
    Reload,
    /// Use all variations, or just the selected one.
    Choose {
        /// The saved file.
        path: PathBuf,
        /// One variation, or the weighted stamp.
        variation: Option<usize>,
    },
    /// Save the current brush under a new name.
    Save(String),
    /// Change the display name, preserving its file identity and references.
    Rename {
        /// The saved file.
        path: PathBuf,
        /// Its new display name.
        name: String,
    },
    /// Delete the saved stamp.
    Delete(PathBuf),
    /// Append the current brush's variations.
    AddVariation(PathBuf),
    /// Set a saved variation's weight.
    Probability {
        /// The saved file.
        path: PathBuf,
        /// The variation index.
        variation: usize,
        /// Its new non-negative weight.
        probability: f64,
    },
    /// Store the brush in a quick-stamp slot (0–8).
    SaveQuick(usize),
    /// Choose a quick stamp (0–8).
    ChooseQuick(usize),
    /// Append the brush to a quick stamp (0–8).
    ExtendQuick(usize),
}

impl Editor {
    pub(crate) fn set_stamp_folder(&mut self, folder: PathBuf) -> Vec<Effect> {
        self.stamps.folder = folder;
        self.stamps.busy = false;
        self.edit_stamp(StampEdit::Reload)
    }
    /// The saved-stamp library.
    pub fn stamps(&self) -> &StampStore {
        &self.stamps
    }

    /// Whether the current painting tool has a brush, or a selection tool has selected tiles.
    pub fn can_save_stamp(&self) -> bool {
        let Some(doc) = self.current().filter(|doc| self.map_of(*doc).is_some()) else {
            return false;
        };
        if matches!(
            self.tools.tool(),
            crate::ToolKind::Brush | crate::ToolKind::BucketFill | crate::ToolKind::ShapeFill
        ) {
            self.tools.brush().is_some_and(|brush| {
                brush
                    .variations()
                    .iter()
                    .any(|variation| !variation.clip.is_empty())
            })
        } else {
            self.document(doc)
                .is_some_and(|doc| !doc.selection().tiles.is_empty())
        }
    }

    pub(crate) fn edit_stamp(&mut self, edit: StampEdit) -> Vec<Effect> {
        match self.try_edit_stamp(edit) {
            Ok(effects) => effects,
            Err(error) => vec![Effect::Notify(Notice::error(error))],
        }
    }

    fn try_edit_stamp(&mut self, edit: StampEdit) -> Result<Vec<Effect>, String> {
        match edit {
            StampEdit::Choose { path, variation } => {
                let saved = self
                    .stamps
                    .entries
                    .iter()
                    .find(|saved| saved.path == path)
                    .ok_or("The saved stamp no longer exists")?;
                let target = self
                    .current()
                    .and_then(|doc| self.map_of(doc))
                    .map(AsRef::as_ref);
                self.set_brush(brush_of(&saved.stamp, variation, target)?);
                return Ok(Vec::new());
            }
            StampEdit::ChooseQuick(index) => {
                let stamp = self
                    .tools
                    .quick_stamps
                    .get(index)
                    .and_then(Option::as_ref)
                    .ok_or("That quick stamp is empty")?;
                let target = self
                    .current()
                    .and_then(|doc| self.map_of(doc))
                    .map(AsRef::as_ref);
                self.set_brush(brush_of(stamp, None, target)?);
                return Ok(Vec::new());
            }
            StampEdit::SaveQuick(index) | StampEdit::ExtendQuick(index) => {
                if index >= 9 {
                    return Err("Quick stamp slots are 1–9".into());
                }
                let mut stamp = self.capture_stamp(format!("Quick stamp {}", index + 1))?;
                if matches!(edit, StampEdit::ExtendQuick(_))
                    && let Some(existing) =
                        self.tools.quick_stamps.get(index).and_then(Option::as_ref)
                {
                    stamp.variations.splice(0..0, existing.variations.clone());
                }
                stamp.quick_stamp_index = Some(index as u32);
                let path = self.stamps.folder.join(format!("__quick_{index}.stamp"));
                let text = bt_tmx::WriteRequest::for_stamp(&stamp, &path)
                    .render()
                    .map_err(|error| error.to_string())?
                    .text;
                let slot = self
                    .tools
                    .quick_stamps
                    .get_mut(index)
                    .ok_or("Quick stamp slots are 1–9")?;
                *slot = Some(stamp);
                self.tools.quick_stamps_generation += 1;
                self.session
                    .quick_stamps
                    .retain(|saved| saved.slot != index);
                self.session.quick_stamps.push(crate::SessionStamp {
                    slot: index,
                    path,
                    text,
                });
                self.session_changed();
                return Ok(Vec::new());
            }
            _ => {}
        }
        if self.stamps.busy {
            return Err("The stamp library is being updated".into());
        }
        let operation = match edit {
            StampEdit::Reload => Operation::Load,
            StampEdit::Save(name) => {
                check_name(&name)?;
                let stamp = self.capture_stamp(name.clone())?;
                let path = self.stamps.folder.join(format!("{name}.stamp"));
                if path.exists() {
                    return Err(format!("{} already exists", path.display()));
                }
                Operation::Write(SavedStamp { path, stamp })
            }
            StampEdit::Rename { path, name } => {
                check_name(&name)?;
                let mut saved = self.saved_stamp(&path)?;
                saved.stamp.name = name;
                Operation::Write(saved)
            }
            StampEdit::Delete(path) => {
                self.saved_stamp(&path)?;
                Operation::Delete(path)
            }
            StampEdit::AddVariation(path) => {
                let mut saved = self.saved_stamp(&path)?;
                saved
                    .stamp
                    .variations
                    .extend(self.capture_stamp(String::new())?.variations);
                Operation::Write(saved)
            }
            StampEdit::Probability {
                path,
                variation,
                probability,
            } => {
                if !probability.is_finite() || probability < 0.0 {
                    return Err("A probability must be finite and non-negative".into());
                }
                let mut saved = self.saved_stamp(&path)?;
                saved
                    .stamp
                    .variations
                    .get_mut(variation)
                    .ok_or("The stamp no longer has that variation")?
                    .probability = probability;
                brush_of(&saved.stamp, None, None)?;
                Operation::Write(saved)
            }
            _ => return Ok(Vec::new()),
        };
        self.stamps.request_generation += 1;
        self.stamps.busy = true;
        Ok(vec![Effect::Stamps(
            StampJob::new(
                &self.stamps,
                std::sync::Arc::new(self.custom_types().clone()),
            )
            .operation(operation),
        )])
    }

    fn saved_stamp(&self, path: &std::path::Path) -> Result<SavedStamp, String> {
        self.stamps
            .entries
            .iter()
            .find(|saved| saved.path == path)
            .cloned()
            .ok_or_else(|| "The saved stamp no longer exists".into())
    }

    fn capture_stamp(&self, name: String) -> Result<bt_tmx::Stamp, String> {
        let doc = self.current().ok_or("Open a map before saving a stamp")?;
        let map = self.map_of(doc).ok_or("Open a map before saving a stamp")?;
        if matches!(
            self.tools.tool(),
            crate::ToolKind::Brush | crate::ToolKind::BucketFill | crate::ToolKind::ShapeFill
        ) {
            let brush = self
                .tools
                .brush()
                .ok_or("Pick or capture a brush before saving a stamp")?;
            stamp_of(name, brush, map)
        } else {
            let selection = self
                .document(doc)
                .ok_or("The map is no longer open")?
                .selection();
            if selection.tiles.is_empty() {
                return Err("Select tiles or pick a brush before saving a stamp".into());
            }
            let clip = match &selection.carried {
                Some(carried) => carried.tiles.clip(map),
                None => bt_edit::copy(
                    map,
                    &crate::clipboard::clip_layers(map, selection),
                    &selection.tiles,
                ),
            }
            .map_err(|error| error.to_string())?;
            let brush =
                bt_edit::Brush::from_clip(clip.trimmed()).map_err(|error| error.to_string())?;
            stamp_of(name, &brush, map)
        }
    }

    pub(crate) fn stamps_loaded(&mut self, loaded: StampsLoaded) -> Vec<Effect> {
        match loaded {
            StampsLoaded::Quick {
                generation,
                stamps,
                notices,
            } => {
                if generation != self.tools.quick_stamps_generation {
                    return Vec::new();
                }
                self.tools.quick_stamps = *stamps;
                notices.into_iter().map(Effect::Notify).collect()
            }
            StampsLoaded::Library { generation, result } => {
                self.apply_library_result(generation, result)
            }
        }
    }

    fn apply_library_result(
        &mut self,
        generation: u64,
        result: Result<StampStore, String>,
    ) -> Vec<Effect> {
        if generation != self.stamps.request_generation {
            return Vec::new();
        }
        match result {
            Ok(store) => {
                self.stamps = store;
                Vec::new()
            }
            Err(error) => {
                self.stamps.busy = false;
                vec![Effect::Notify(Notice::error(error))]
            }
        }
    }

    pub(crate) fn restore_quick_stamps(&mut self) -> Vec<Effect> {
        self.tools.quick_stamps = Default::default();
        self.tools.quick_stamps_generation += 1;
        if self.session.quick_stamps.is_empty() {
            return Vec::new();
        }
        vec![Effect::Stamps(
            StampJob::new(
                &self.stamps,
                std::sync::Arc::new(self.custom_types().clone()),
            )
            .operation(Operation::RestoreQuick {
                generation: self.tools.quick_stamps_generation,
                records: self.session.quick_stamps.clone(),
            }),
        )]
    }
}

fn check_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
    {
        return Err(
            "A stamp name must be a non-empty file name without separators or reserved characters"
                .into(),
        );
    }
    Ok(())
}
