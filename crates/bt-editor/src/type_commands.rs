//! The Custom Types editor's side of the editor (`docs/design/05-editor-core.md`, §18): its
//! changes are transactions on the project asset, recorded in the document it was opened from,
//! and the project's file is written when that document is saved. With no project open, the
//! types go to an untitled project that lives for the run, as Tiled's does when no project is
//! loaded (`reference/tiled/src/tiled/projectmanager.cpp`).

use bt_core::CustomTypeId;

use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::history::{MergeKey, Tx};
use crate::properties::{PropertyError, PropertySet, Shown, Target, TypeEdit};

impl Editor {
    /// The members of the project's class `id`, as the Custom Types editor shows them.
    pub fn member_rows(&self, id: CustomTypeId) -> Result<Vec<Shown>, PropertyError> {
        let types = self.custom_types();
        Ok(PropertySet::of(vec![Target::members(types, id)?], types).rows())
    }

    /// Changes the project's custom types as one step in `doc`; the edits of one run (`merge`,
    /// typing a member's default) fold into one step. With no project open, an untitled one
    /// takes them.
    pub(crate) fn edit_custom_types(
        &mut self,
        doc: DocumentId,
        edit: &TypeEdit,
        merge: Option<u64>,
    ) -> Vec<Effect> {
        if self.documents.get(doc).is_none() {
            return vec![warn(EditError::NoSuchDocument(doc))];
        }
        let project = self.project_for_edits();
        let label = edit.label();
        let apply = |tx: &mut Tx<'_>| -> Result<bool, EditError> {
            Ok(edit.apply_to(&mut tx.project_mut(project)?.custom_types)?)
        };
        let result = match merge {
            Some(run) => {
                let key = MergeKey(format!("{label} custom types run {run}"));
                self.transact_merging(doc, &label, key, apply)
            }
            None => self.transact(doc, &label, apply),
        };
        result.err().map(warn).into_iter().collect()
    }
}
