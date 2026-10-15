//! All diagnostic sources use the same selection and camera commands.
use super::IssueTarget;
use crate::{AssetId, AssetKind, Editor, Effect, Intent, SelectionMode, TilesetHome};
impl Editor {
    fn issue_document(&mut self, asset: AssetId) -> Option<crate::DocumentId> {
        self.store.get(asset)?;
        let existing = self
            .documents
            .in_order()
            .find(|doc| doc.asset() == asset)
            .map(|doc| doc.id());
        let doc = existing.unwrap_or_else(|| self.open_document(asset));
        self.documents.set_current(doc);
        Some(doc)
    }
    pub(crate) fn go_to_issue(&mut self, target: IssueTarget) -> Vec<Effect> {
        match target {
            IssueTarget::CollisionObject { home, tile, object } => {
                let Some(doc) = self.issue_document(home.asset()) else {
                    return Vec::new();
                };
                let mut effects = match home {
                    TilesetHome::Asset(_) => Vec::new(),
                    TilesetHome::Embedded { slot, .. } => {
                        self.apply(Intent::EditTileset { doc, slot })
                    }
                };
                effects.extend(self.apply(Intent::SelectTilesetTiles {
                    doc,
                    tiles: vec![tile],
                    current: Some(tile),
                }));
                effects.extend(self.apply(Intent::SetTilesetMode {
                    doc,
                    mode: crate::TilesetMode::Collision,
                }));
                effects.extend(self.apply(Intent::SelectObjects {
                    doc,
                    ids: vec![object],
                    mode: SelectionMode::Replace,
                }));
                effects.extend(self.apply(Intent::CenterOnObject { doc, object }));
                effects
            }
            IssueTarget::File(path) => self.request_open(&path),
            IssueTarget::Asset(asset) => {
                if self
                    .store
                    .get(asset)
                    .is_some_and(|asset| asset.kind() == AssetKind::World)
                {
                    return self
                        .current()
                        .map(|doc| Effect::Show(crate::Dialog::WorldProperties(doc)))
                        .into_iter()
                        .collect();
                }
                self.issue_document(asset);
                Vec::new()
            }
            IssueTarget::Layer { map, layer } => {
                let Some(doc) = self.issue_document(map) else {
                    return Vec::new();
                };
                let mut effects = self.apply(Intent::StopEditingTileset { doc });
                effects.extend(self.apply(Intent::SelectLayer { doc, layer }));
                effects
            }
            IssueTarget::Object { map, object } => {
                let Some(doc) = self.issue_document(map) else {
                    return Vec::new();
                };
                let Some(layer) = self.map_of(doc).and_then(|map| map.object_layer_of(object))
                else {
                    return Vec::new();
                };
                let mut effects = self.apply(Intent::StopEditingTileset { doc });
                effects.extend(self.apply(Intent::SelectLayer { doc, layer }));
                effects.extend(self.apply(Intent::SelectObjects {
                    doc,
                    ids: vec![object],
                    mode: SelectionMode::Replace,
                }));
                effects.extend(self.apply(Intent::CenterOnObject { doc, object }));
                effects
            }
            IssueTarget::Tileset { home, tile } => {
                let Some(doc) = self.issue_document(home.asset()) else {
                    return Vec::new();
                };
                let mut effects = match home {
                    TilesetHome::Asset(_) => Vec::new(),
                    TilesetHome::Embedded { slot, .. } => {
                        self.apply(Intent::EditTileset { doc, slot })
                    }
                };
                if let Some(tile) = tile {
                    effects.extend(self.apply(Intent::SelectTilesetTiles {
                        doc,
                        tiles: vec![tile],
                        current: Some(tile),
                    }));
                }
                effects
            }
        }
    }
}
