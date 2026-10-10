//! One instance of each tool, kept across documents; the current one is the editor's choice.

use bt_editor::{DocumentId, ToolKind};

use super::magic_wand::LikeTiles;
use super::rect_select::RectSelect;
use super::tool::Tool;

/// The tools.
pub(crate) struct ToolBox {
    tools: Vec<Box<dyn Tool>>,
    /// The tool that last handled a gesture, and in which document, so switching either
    /// resets it.
    active: Option<Active>,
}

/// The tool in use and the document it works in.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Active {
    kind: ToolKind,
    doc: DocumentId,
}

impl Default for ToolBox {
    fn default() -> ToolBox {
        ToolBox {
            tools: vec![
                Box::new(RectSelect::default()),
                Box::new(LikeTiles::magic_wand()),
                Box::new(LikeTiles::same_tile()),
            ],
            active: None,
        }
    }
}

impl ToolBox {
    /// The tool of `kind`, active in `doc`; the tool active before (or the same tool in another
    /// document) drops what it was doing. `None` for a tool not built.
    pub(crate) fn activate(
        &mut self,
        kind: ToolKind,
        doc: DocumentId,
    ) -> Option<&mut (dyn Tool + 'static)> {
        let now = Active { kind, doc };
        if self.active != Some(now) {
            if let Some(previous) = self.active
                && let Some(tool) = self.tools.iter_mut().find(|t| t.kind() == previous.kind)
            {
                tool.reset();
            }
            self.active = Some(now);
        }
        self.tools
            .iter_mut()
            .find(|t| t.kind() == kind)
            .map(|tool| tool.as_mut())
    }
}
