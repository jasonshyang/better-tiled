//! The live link's side of the editor (`05-editor-core.md`, §17; `11-live-link.md`, §9–§10). A
//! script runs elsewhere, on a base the editor hands out ([`Editor::prepare_external_edit`]),
//! and its result comes back as one transaction labelled by the script, only while the map is
//! still at the base's version ([`Editor::apply_external_edit`]): the person's edits are never
//! overwritten and never interleaved with a script's. `bt undo` takes back the latest external
//! edit only while it is the latest change in the document ([`Editor::external_undo`]). Marks
//! are cells `bt` outlines on a document's canvas ([`Editor::set_marks`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_automap::RuleSet;
use bt_automap::layers::Setup;
use bt_core::{CustomTypes, Map, ObjectId};
use bt_edit::TileRegion;
use bt_utils::IRect;
use glam::IVec2;

use crate::automap::KnownTileset;
use crate::document::DocumentId;
use crate::editor::Editor;
use crate::effect::Effect;
use crate::error::EditError;
use crate::store::{AssetKind, NewAsset, Root};

/// How many colours marks take in turn; the window picks them, unlike the selection's.
pub const MARK_COLORS: u8 = 5;

/// What a script runs on: a document's map as the editor has it, with what the script needs
/// from the editor besides.
#[derive(Clone, Debug)]
pub struct ExternalBase {
    /// The document.
    pub doc: DocumentId,
    /// The map's version: an edit lands only while the map is still at it.
    pub version: u64,
    /// The map's file, when it has one.
    pub path: Option<PathBuf>,
    /// The map as painted: what a script edits.
    pub painted: Arc<Map>,
    /// The map as it shows and as its file gets it: with AutoMap layers on, what the rules make
    /// of what was painted; else the painted map.
    pub shown: Arc<Map>,
    /// The map's AutoMap layers, while they are on.
    pub setup: Option<Setup>,
    /// The rules the editor read for the map, and their file, when it has any.
    pub rules: Option<BaseRules>,
    /// The project's custom types.
    pub custom_types: Arc<CustomTypes>,
    /// The external tilesets the editor has open, with their unsaved changes: a script reading
    /// another file that uses one gets the editor's copy.
    pub tilesets: Vec<KnownTileset>,
    /// The selected cells (`selection` in a script).
    pub selected_cells: TileRegion,
    /// The selected objects (`selected` in a script).
    pub selected_objects: Vec<ObjectId>,
}

/// Rules the editor has read for a map.
#[derive(Clone, Debug)]
pub struct BaseRules {
    /// The rules file.
    pub file: PathBuf,
    /// The rules.
    pub rules: Arc<RuleSet>,
}

/// A script's result to land in a document.
#[derive(Clone, Debug)]
pub struct ExternalEdit {
    /// The document.
    pub doc: DocumentId,
    /// The map's version the script started from.
    pub base_version: u64,
    /// The map as painted after the script.
    pub painted: Map,
    /// The AutoMap layers after the script.
    pub setup: Option<Setup>,
    /// The step's name in the History panel.
    pub label: String,
}

/// Why a script's result did not land.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ExternalEditError {
    /// The document is no longer open.
    #[error("the map is no longer open in the editor")]
    NoSuchDocument,
    /// A stroke or a drag is under way: the edit waits for it to end.
    #[error("a stroke or drag is under way in the editor")]
    Busy,
    /// The map changed in the editor since the script started: it runs again on the new map.
    #[error("the map changed in the editor since the script started")]
    Conflict,
    /// The transaction failed.
    #[error(transparent)]
    Edit(#[from] EditError),
}

/// What `bt undo` did in the editor, and what the window must do after it.
#[derive(Clone, Debug)]
pub struct ExternalUndone {
    /// What was taken back.
    pub undo: ExternalUndo,
    /// What the window must do.
    pub effects: Vec<Effect>,
}

/// What `bt undo` did in the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalUndo {
    /// The edit with this label was taken back.
    Undone {
        /// The edit's label.
        label: String,
        /// How many earlier external edits of the document could still be taken back.
        earlier: usize,
    },
    /// The edit with this label had been undone in the editor already.
    AlreadyUndone {
        /// The edit's label.
        label: String,
    },
    /// No external edit of the document is left to take back.
    Nothing,
}

/// Why `bt undo` did not take an edit back.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ExternalUndoError {
    /// The document is no longer open.
    #[error("the map is no longer open in the editor")]
    NoSuchDocument,
    /// A stroke or a drag is under way.
    #[error("a stroke or drag is under way in the editor")]
    Busy,
    /// The person changed the map since the edit: their change is the latest.
    #[error(
        "“{latest}” was done in the editor after “{edit}”: undo it there first (⌘Z), or leave \
         both"
    )]
    NotLatest {
        /// The external edit.
        edit: String,
        /// The change made since.
        latest: String,
    },
    /// The undo failed.
    #[error(transparent)]
    Edit(#[from] EditError),
}

/// Cells `bt` outlines on a document's canvas, under a label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    /// The label, written at the first row's left end.
    pub label: String,
    /// The cells, sorted by row then column, each once.
    pub cells: Vec<IVec2>,
    /// Which of the [`MARK_COLORS`] colours it takes.
    pub color: u8,
}

impl Mark {
    /// The smallest rectangle holding its cells.
    pub fn bounds(&self) -> Option<IRect> {
        IRect::bounding(self.cells.iter().copied())
    }
}

/// Cells to mark under a label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkCells {
    /// The label; marking a label again moves that mark.
    pub label: String,
    /// The cells.
    pub cells: Vec<IVec2>,
}

/// A document's external edits and marks.
#[derive(Clone, Debug, Default)]
pub(crate) struct ExternalState {
    /// The external edits that landed, oldest first.
    edits: Vec<Landed>,
    marks: Vec<Mark>,
    next_color: u8,
}

/// An external edit that landed: the map's version it left.
#[derive(Clone, Debug)]
struct Landed {
    label: String,
    left: u64,
}

impl Editor {
    /// The document showing the map at `path` (a canonical path), if one does.
    pub fn document_at(&self, path: &Path) -> Option<DocumentId> {
        let asset = self.store.by_path(path)?;
        self.documents
            .in_order()
            .find(|document| document.asset() == asset)
            .map(|document| document.id())
    }

    /// What a script runs on in document `doc`; `None` when it shows no map.
    pub fn prepare_external_edit(&self, doc: DocumentId) -> Option<ExternalBase> {
        let document = self.documents.get(doc)?;
        let asset = self.store.get(document.asset())?;
        if asset.kind() != AssetKind::Map {
            return None;
        }
        let painted = Arc::clone(self.store.map(document.asset())?);
        let shown = Arc::clone(self.result_map(doc)?);
        let rules = self.automap.maps.get(&document.asset()).and_then(|rules| {
            Some(BaseRules {
                file: rules.file.clone()?,
                rules: rules.rules.clone()?,
            })
        });
        Some(ExternalBase {
            doc,
            version: asset.version(),
            path: asset.path().map(Path::to_path_buf),
            painted,
            shown,
            setup: document.automap.clone(),
            rules,
            custom_types: Arc::new(self.custom_types().clone()),
            tilesets: self.known_tilesets(),
            selected_cells: document.selection.tiles.clone(),
            selected_objects: document.selection.objects.clone(),
        })
    }

    /// Lands a script's result as one transaction, while the map is still at the version the
    /// script started from and no stroke or drag is under way. External tilesets the result
    /// uses that the editor does not have open are opened with it.
    pub fn apply_external_edit(
        &mut self,
        edit: ExternalEdit,
    ) -> Result<Vec<Effect>, ExternalEditError> {
        let asset = self
            .documents
            .get(edit.doc)
            .map(|d| d.asset())
            .ok_or(ExternalEditError::NoSuchDocument)?;
        if self.is_stroking() {
            return Err(ExternalEditError::Busy);
        }
        let version = self
            .store
            .get(asset)
            .map(|a| a.version())
            .ok_or(ExternalEditError::NoSuchDocument)?;
        if version != edit.base_version {
            return Err(ExternalEditError::Conflict);
        }
        let ExternalEdit {
            doc,
            painted,
            setup,
            label,
            ..
        } = edit;
        self.adopt_tilesets_of(&painted);
        self.transact(doc, &label, |tx| {
            *tx.map_mut(asset)? = painted;
            tx.automap = setup;
            Ok::<_, EditError>(())
        })?;
        let left = self.store.get(asset).map_or(version, |a| a.version());
        if left != version {
            self.external
                .entry(doc)
                .or_default()
                .edits
                .push(Landed { label, left });
        }
        Ok(self.after_changes())
    }

    /// Takes back the document's latest external edit, only while it is the latest change in
    /// the document.
    pub fn external_undo(&mut self, doc: DocumentId) -> Result<ExternalUndone, ExternalUndoError> {
        let undo = self.take_back_external(doc)?;
        Ok(ExternalUndone {
            undo,
            effects: self.after_changes(),
        })
    }

    /// [`Editor::external_undo`] without what follows a change.
    fn take_back_external(&mut self, doc: DocumentId) -> Result<ExternalUndo, ExternalUndoError> {
        let document = self
            .documents
            .get(doc)
            .ok_or(ExternalUndoError::NoSuchDocument)?;
        if self.is_stroking() {
            return Err(ExternalUndoError::Busy);
        }
        let asset = document.asset();
        let Some(landed) = self
            .external
            .get(&doc)
            .and_then(|s| s.edits.last())
            .cloned()
        else {
            return Ok(ExternalUndo::Nothing);
        };
        let history = &document.history;
        let left_it = |t: &crate::history::Transaction| {
            t.changes
                .iter()
                .any(|c| c.asset == asset && c.after_version == landed.left)
        };
        let done = history.undoable().is_some_and(left_it);
        let undone = history
            .entries
            .get(history.position..)
            .unwrap_or(&[])
            .iter()
            .any(left_it);
        let latest = history.undoable().map(|t| t.label.clone());
        let current = self.store.get(asset).map(|a| a.version());
        if done && current == Some(landed.left) {
            self.undo(doc)?;
            let state = self.external.entry(doc).or_default();
            state.edits.pop();
            return Ok(ExternalUndo::Undone {
                label: landed.label,
                earlier: state.edits.len(),
            });
        }
        if undone {
            self.external.entry(doc).or_default().edits.pop();
            return Ok(ExternalUndo::AlreadyUndone {
                label: landed.label,
            });
        }
        Err(ExternalUndoError::NotLatest {
            edit: landed.label,
            latest: latest.unwrap_or_else(|| "a change".to_owned()),
        })
    }

    /// Replaces a document's marks with `marks`, or with `add` adds them: a label marked again
    /// moves that mark and keeps its colour. No marks and no `add` clear them. The marks after.
    pub fn set_marks(&mut self, doc: DocumentId, marks: Vec<MarkCells>, add: bool) -> &[Mark] {
        let state = self.external.entry(doc).or_default();
        if !add {
            let kept: Vec<Mark> = state
                .marks
                .iter()
                .filter(|mark| marks.iter().any(|m| m.label == mark.label))
                .cloned()
                .collect();
            state.marks = kept;
        }
        for MarkCells { label, mut cells } in marks {
            cells.sort_by_key(|c| (c.y, c.x));
            cells.dedup();
            match state.marks.iter_mut().find(|mark| mark.label == label) {
                Some(mark) => mark.cells = cells,
                None => {
                    let color = state.next_color;
                    state.next_color = (state.next_color + 1) % MARK_COLORS;
                    state.marks.push(Mark {
                        label,
                        cells,
                        color,
                    });
                }
            }
        }
        if state.marks.is_empty() {
            state.next_color = 0;
        }
        &state.marks
    }

    /// A document's marks.
    pub fn marks(&self, doc: DocumentId) -> &[Mark] {
        self.external.get(&doc).map_or(&[], |state| &state.marks)
    }

    /// Whether a stroke or a drag is under way: an external edit or undo waits for it to end.
    pub fn is_stroking(&self) -> bool {
        self.gesture.is_some()
            || self.stroke.is_some()
            || self.object_stroke.is_some()
            || self.terrain_stroke.is_some()
            || self.label_stroke.is_some()
    }

    /// The external tilesets the editor has open.
    pub(crate) fn known_tilesets(&self) -> Vec<KnownTileset> {
        self.store
            .assets()
            .filter(|a| a.kind() == AssetKind::Tileset)
            .filter_map(|a| {
                Some(KnownTileset {
                    path: a.path()?.to_path_buf(),
                    tileset: Arc::clone(self.store.tileset(a.id())?),
                })
            })
            .collect()
    }

    /// Opens the external tilesets `map` uses that the editor does not have open, so they are
    /// watched and saved like the others.
    fn adopt_tilesets_of(&mut self, map: &Map) {
        for reference in &map.tilesets {
            let Some(file) = &reference.file else {
                continue;
            };
            if !file.path.is_absolute() || self.store.by_path(&file.path).is_some() {
                continue;
            }
            self.store.insert(NewAsset {
                root: Root::Tileset(Arc::clone(&reference.tileset)),
                path: Some(file.path.clone()),
                content_hash: None,
                warnings: Vec::new(),
            });
        }
    }

    /// Forgets a closed document's external edits and marks.
    pub(crate) fn forget_external(&mut self, doc: DocumentId) {
        self.external.remove(&doc);
    }
}

/// Each document's external edits and marks.
pub(crate) type ExternalStates = HashMap<DocumentId, ExternalState>;
