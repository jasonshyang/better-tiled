//! The Properties panel's changes, each a transaction on what it inspects: on the map's asset,
//! or on the tileset's home for a tileset and its tiles (Tiled's `SetProperty`,
//! `RemoveProperty`, `RenameProperty` and the attribute commands of
//! `reference/tiled/src/tiled/propertieswidget.cpp`). The edits of one run of typing in a
//! field, or of dragging a colour, share a number and fold into one step.

use std::path::Path;

use bt_core::{Property, Value};
use bt_edit::TileRegion;

use super::inspected::Inspected;
use super::rows::NO_TYPES;
use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::history::{MergeKey, Tx};
use crate::properties::{
    Attribute, AttributeValue, Change, PropertySet, Step, set_layer_attribute, set_map_attribute,
    set_object_attribute, set_tile_attribute, set_tileset_attribute,
};
use crate::tileset_commands::drop_empty_tile_data;

impl Editor {
    /// Applies a change of custom properties to what `doc` inspects as `inspected`, as one
    /// step labelled as Tiled labels it; changes of one run (`merge`) fold into one step.
    pub(crate) fn change_properties(
        &mut self,
        doc: DocumentId,
        inspected: &Inspected,
        change: &Change,
        merge: Option<u64>,
    ) -> Vec<Effect> {
        let result = self.try_change_properties(doc, inspected, change, merge);
        result.err().map(warn).into_iter().collect()
    }

    fn try_change_properties(
        &mut self,
        doc: DocumentId,
        inspected: &Inspected,
        change: &Change,
        merge: Option<u64>,
    ) -> Result<bool, EditError> {
        let source = self.source_of(doc, inspected)?;
        let project = self.project_root();
        let types = project
            .as_deref()
            .map_or(&NO_TYPES, |project| &project.custom_types);
        let set = PropertySet::of(source.targets(inspected, types)?, types);
        let label = change.label();
        self.transact_in_run(doc, &label, inspected, merge, |tx| match inspected {
            Inspected::Map | Inspected::Layers(_) | Inspected::Objects(_) => {
                Ok(set.apply_to_map(tx.map_mut(source.place.map_asset()?)?, change)?)
            }
            Inspected::Tileset | Inspected::Tiles(_) => {
                let tileset = tx.tileset_at_mut(source.place.tileset_home()?)?;
                let changed = set.apply_to_tileset(tileset, change)?;
                drop_empty_tile_data(tileset, inspected.tiles());
                Ok(changed)
            }
        })
    }

    /// Sets a built-in attribute of what `doc` inspects as `inspected`, as one step; edits of
    /// one run (`merge`) fold into one step. A map made finite is cropped to its tiles, so its
    /// tile selection is cleared.
    pub(crate) fn set_attribute(
        &mut self,
        doc: DocumentId,
        inspected: &Inspected,
        attribute: Attribute,
        value: &AttributeValue,
        merge: Option<u64>,
    ) -> Vec<Effect> {
        let result = self.try_set_attribute(doc, inspected, attribute, value, merge);
        result.err().map(warn).into_iter().collect()
    }

    fn try_set_attribute(
        &mut self,
        doc: DocumentId,
        inspected: &Inspected,
        attribute: Attribute,
        value: &AttributeValue,
        merge: Option<u64>,
    ) -> Result<bool, EditError> {
        let source = self.source_of(doc, inspected)?;
        let label = format!("Change {}", attribute.label());
        let cropping = attribute == Attribute::Infinite && *value == AttributeValue::Bool(false);
        self.transact_in_run(doc, &label, inspected, merge, |tx| {
            let place = &source.place;
            let changed = match inspected {
                Inspected::Map => {
                    let changed =
                        set_map_attribute(tx.map_mut(place.map_asset()?)?, attribute, value)?;
                    if changed && cropping {
                        tx.selection.tiles = TileRegion::default();
                    }
                    changed
                }
                Inspected::Layers(layers) => {
                    set_layer_attribute(tx.map_mut(place.map_asset()?)?, layers, attribute, value)?
                }
                Inspected::Objects(objects) => set_object_attribute(
                    tx.map_mut(place.map_asset()?)?,
                    objects,
                    attribute,
                    value,
                )?,
                Inspected::Tileset => set_tileset_attribute(
                    tx.tileset_at_mut(place.tileset_home()?)?,
                    attribute,
                    value,
                )?,
                Inspected::Tiles(tiles) => {
                    let tileset = tx.tileset_at_mut(place.tileset_home()?)?;
                    let changed = set_tile_attribute(tileset, tiles, attribute, value)?;
                    drop_empty_tile_data(tileset, tiles);
                    changed
                }
            };
            Ok(changed)
        })
    }

    /// A file chosen for the custom property at `path` of what `doc` inspects: set relative to
    /// the folder of the file that holds it, as files refer to each other (absolute while that
    /// file has never been saved).
    pub(crate) fn property_file_picked(
        &mut self,
        doc: DocumentId,
        inspected: &Inspected,
        path: Vec<Step>,
        file: &Path,
    ) -> Vec<Effect> {
        let source = match self.source_of(doc, inspected) {
            Ok(source) => source,
            Err(error) => return vec![warn(error)],
        };
        let text = self
            .folder_of(&source)
            .and_then(|folder| bt_utils::paths::relative_path(folder, file))
            .unwrap_or_else(|| file.to_string_lossy().into_owned());
        let change = Change::set(path, Property::new(Value::File(text)));
        self.change_properties(doc, inspected, &change, None)
    }

    /// Runs `f` as a transaction labelled `label` in `doc`; the edits of one run (`merge`) of
    /// one field fold into one step.
    fn transact_in_run<R>(
        &mut self,
        doc: DocumentId,
        label: &str,
        inspected: &Inspected,
        merge: Option<u64>,
        f: impl FnOnce(&mut Tx<'_>) -> Result<R, EditError>,
    ) -> Result<R, EditError> {
        match merge {
            Some(run) => {
                let key = MergeKey(format!("{label} {inspected:?} run {run}"));
                self.transact_merging(doc, label, key, f)
            }
            None => self.transact(doc, label, f),
        }
    }
}
