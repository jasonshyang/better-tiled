//! A map's AutoMap record in its project file (`09-project-file.md`, §6.2): `bt edit` writes it
//! after editing a map with AutoMap layers (or removes it when the script stopped them) and keeps
//! the record as it was for `bt undo`, which puts it back.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use bt_automap::layers::{AutoLayers, Setup, painted_record};
use bt_project::{AutoMapRecord, EntryPlace, ProjectFile, paths};

use crate::folders::Folders;

/// The map's record as its project file keeps it now, as JSON; none when it keeps none.
pub(crate) fn record_json(map: &Path, folders: &Folders) -> anyhow::Result<Option<String>> {
    let Some(file) = ProjectFile::find(map, folders.home.as_deref())? else {
        return Ok(None);
    };
    let key = paths::key(&file, map)?;
    let document = ProjectFile::load(&file)?;
    document
        .automap(&key)
        .map(serde_json::to_string)
        .transpose()
        .with_context(|| format!("the AutoMap record in {} can't be read", file.display()))
}

/// Writes what was painted under the AutoMap layers `layers` (with `setup`) as the entry of the
/// map at `map` in its project file (`open_project` is the `.tiled-project` that applies to it);
/// the project file written.
pub(crate) fn write_layers_record(
    map: &Path,
    setup: &Setup,
    layers: &AutoLayers,
    open_project: Option<&Path>,
    folders: &Folders,
) -> anyhow::Result<PathBuf> {
    let place = EntryPlace::for_map(map, open_project, folders.home.as_deref())?;
    let rules = paths::key(&place.file, &setup.rules)?;
    let record = painted_record(setup, rules, layers);
    ProjectFile::edit(&place.file, &folders.locks()?, |document| {
        document.set_automap(&place.key, record);
    })?;
    Ok(place.file)
}

/// Puts the map's record back as `entry` (its JSON) gives it, or removes it when `entry` is
/// none; a project file is created only to hold a record.
pub(crate) fn put_back_record(
    map: &Path,
    entry: Option<&str>,
    folders: &Folders,
) -> anyhow::Result<()> {
    let place = EntryPlace::for_map(map, None, folders.home.as_deref())?;
    let record: Option<AutoMapRecord> = entry
        .map(serde_json::from_str)
        .transpose()
        .context("the kept AutoMap record is damaged")?;
    if record.is_none() && !place.file.exists() {
        return Ok(());
    }
    ProjectFile::edit(&place.file, &folders.locks()?, |document| match record {
        Some(record) => document.set_automap(&place.key, record),
        None => {
            document.remove_automap(&place.key);
        }
    })?;
    Ok(())
}
