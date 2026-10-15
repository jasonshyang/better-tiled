//! Template links in ordinary object layers and tiles' collision groups share dependency and
//! propagation rules. The root owns embedded tilesets; external tilesets own their shapes.
use super::{AssetId, AssetStore, Root};
use bt_core::{ObjectTemplate, Tileset};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};

fn tileset_paths(tileset: &Tileset, paths: &mut BTreeSet<PathBuf>) {
    for tile in tileset.tiles.values() {
        if let Some(group) = &tile.collision {
            paths.extend(
                group
                    .layer
                    .objects
                    .iter()
                    .filter_map(|object| object.template.as_ref().map(|file| file.path.clone())),
            );
        }
    }
}
fn refresh_collision(
    tileset: &mut Tileset,
    path: &Path,
    before: &ObjectTemplate,
    after: &ObjectTemplate,
) -> Result<bool, bt_edit::EditError> {
    let old = before
        .object
        .as_ref()
        .ok_or(bt_edit::EditError::TemplateBaseRequired)?;
    let base = after
        .object
        .as_ref()
        .ok_or(bt_edit::EditError::TemplateBaseRequired)?;
    let mut changed = false;
    for tile in tileset.tiles.values_mut() {
        let Some(group) = &mut tile.collision else {
            continue;
        };
        // Do not clone a list or reject a tile base unless this group actually refers to it.
        if !group.layer.objects.iter().any(|object| {
            object
                .template
                .as_ref()
                .is_some_and(|file| file.path == path)
        }) {
            continue;
        }
        if base.tile.is_some() {
            return Err(bt_edit::EditError::TileTemplateInCollision);
        }
        for object in Arc::make_mut(&mut group.layer.objects) {
            if object
                .template
                .as_ref()
                .is_some_and(|file| file.path == path)
            {
                let previous = object.clone();
                bt_edit::refresh_template_object(object, old, base);
                changed |= *object != previous;
            }
        }
    }
    Ok(changed)
}
impl Root {
    pub(crate) fn template_paths(&self) -> BTreeSet<PathBuf> {
        let mut paths = BTreeSet::new();
        match self {
            Self::Map(map) => {
                map.for_each_layer(|layer, _| {
                    if let Some(objects) = layer.as_object_layer() {
                        paths.extend(objects.objects.iter().filter_map(|object| {
                            object.template.as_ref().map(|file| file.path.clone())
                        }));
                    }
                });
                for reference in &map.tilesets {
                    tileset_paths(&reference.tileset, &mut paths);
                }
            }
            Self::Tileset(tileset) => tileset_paths(tileset, &mut paths),
            _ => {}
        }
        paths
    }
    pub(crate) fn refresh_template_instances(
        &mut self,
        path: &Path,
        before: &ObjectTemplate,
        after: &ObjectTemplate,
    ) -> Result<bool, bt_edit::EditError> {
        match self {
            Self::Map(map) => {
                let map = Arc::make_mut(map);
                let mut changed =
                    !bt_edit::refresh_template_instances(map, path, before, after)?.is_empty();
                for reference in &mut map.tilesets {
                    if reference.file.is_none() {
                        changed |= refresh_collision(
                            Arc::make_mut(&mut reference.tileset),
                            path,
                            before,
                            after,
                        )?;
                    }
                }
                Ok(changed)
            }
            Self::Tileset(tileset) => {
                refresh_collision(Arc::make_mut(tileset), path, before, after)
            }
            _ => Ok(false),
        }
    }
}
impl AssetStore {
    /// Dependencies in write order, with templates used by collision groups before tilesets.
    /// A malformed cycle is visited once; it cannot recurse forever during save or diagnostics.
    pub(crate) fn dependencies_of(&self, owner: AssetId) -> Vec<AssetId> {
        fn visit(
            store: &AssetStore,
            id: AssetId,
            seen: &mut std::collections::HashSet<AssetId>,
            out: &mut Vec<AssetId>,
        ) {
            if !seen.insert(id) {
                return;
            }
            let Some(asset) = store.get(id) else { return };
            let mut direct = match asset.root() {
                Root::Map(map) => store.tilesets_of(map),
                Root::Template(template) => template
                    .tilesets
                    .iter()
                    .filter_map(|reference| reference.file.as_ref())
                    .filter_map(|file| store.by_path(&file.path))
                    .collect(),
                _ => Vec::new(),
            };
            direct.extend(
                asset
                    .root()
                    .template_paths()
                    .iter()
                    .filter_map(|path| store.by_path(path)),
            );
            for dependency in direct {
                visit(store, dependency, seen, out);
                if !out.contains(&dependency) {
                    out.push(dependency);
                }
            }
        }
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        visit(self, owner, &mut seen, &mut out);
        out.retain(|id| *id != owner);
        out
    }
}
