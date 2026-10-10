//! The rows the Properties panel shows for what a document inspects: the built-in attributes
//! and the custom properties (Tiled's `PropertiesWidget`, `reference/tiled/src/tiled/
//! propertieswidget.cpp`), read with the open project's custom types.

use std::sync::Arc;

use bt_core::{CustomTypes, Project};

use super::inspected::Inspected;
use crate::document::DocumentId;
use crate::editor::Editor;
use crate::error::EditError;
use crate::properties::{AttributeRow, PropertySet, Shown};

/// The custom types when no project is open: none.
pub(crate) static NO_TYPES: CustomTypes = CustomTypes { types: Vec::new() };

impl Editor {
    /// The open project's custom types; none when no project is open.
    pub fn custom_types(&self) -> &CustomTypes {
        self.files
            .project
            .and_then(|id| self.store.project(id))
            .map_or(&NO_TYPES, |project| &project.custom_types)
    }

    /// The open project, as the store has it.
    pub(crate) fn project_root(&self) -> Option<Arc<Project>> {
        self.files
            .project
            .and_then(|id| self.store.project(id))
            .cloned()
    }

    /// The built-in attributes of what `doc` inspects as `inspected`, in the order Tiled shows
    /// them, with Tiled's labels.
    pub fn attribute_rows(
        &self,
        doc: DocumentId,
        inspected: &Inspected,
    ) -> Result<Vec<AttributeRow>, EditError> {
        let source = self.source_of(doc, inspected)?;
        Ok(source.attribute_rows(inspected)?)
    }

    /// The custom properties of what `doc` inspects as `inspected`: every property any of them
    /// has, its own or inherited, with class values expanded to their members and lists to
    /// their items.
    pub fn property_rows(
        &self,
        doc: DocumentId,
        inspected: &Inspected,
    ) -> Result<Vec<Shown>, EditError> {
        let source = self.source_of(doc, inspected)?;
        let types = self.custom_types();
        let targets = source.targets(inspected, types)?;
        Ok(PropertySet::of(targets, types).rows())
    }
}
