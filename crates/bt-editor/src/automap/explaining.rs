//! The Explain tool's side of the editor: the cell chosen in each document, whose explanation
//! the AutoMap panel shows.

use glam::IVec2;

use crate::document::DocumentId;
use crate::editor::Editor;
use crate::effect::Effect;

impl Editor {
    /// The cell the Explain tool chose in a document.
    pub fn explained_cell(&self, doc: DocumentId) -> Option<IVec2> {
        self.automap.explaining.get(&doc).copied()
    }

    /// The Explain tool chose `cell`: the AutoMap panel explains it, with the map's rules read
    /// first when they are not yet.
    pub(crate) fn explain_cell(&mut self, doc: DocumentId, cell: IVec2) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        self.automap.explaining.insert(doc, cell);
        self.find_rules(asset, false)
    }
}
