//! Worker-side model validation and resource checks.
use super::resources::Resource;
use super::{Issue, IssueKind, IssueTarget, deduplicate_issues};
use crate::{AssetId, AssetStore, Root, Severity};
use bt_core::Violation;

impl Resource {
    pub(super) fn issue(&self, detail: impl std::fmt::Display) -> Issue {
        let kind = match &self.path {
            Some(path) => IssueKind::Resource {
                kind: self.kind,
                path: path.clone(),
            },
            None => match &self.image_key {
                Some(key) => IssueKind::EmbeddedImage(key.clone()),
                None => IssueKind::Worker,
            },
        };
        let name = self
            .path
            .as_deref()
            .map(bt_utils::paths::display_name)
            .unwrap_or_else(|| "Embedded image".into());
        let mut issue = Issue::new(kind, format!("{name}: {detail}"))
            .severity(Severity::Error)
            .target(self.target.clone());
        issue.fix = self.fix;
        issue
    }
}

pub(super) fn collect(store: &AssetStore, scope: &[AssetId], resources: &[Resource]) -> Vec<Issue> {
    let mut issues = Vec::new();
    for resource in resources {
        let result = match (&resource.path, &resource.image) {
            (Some(path), Some(_)) if !bt_image::is_map_file(path) => bt_image::probe_size(path)
                .map(|_| ())
                .map_err(image_problem),
            (Some(path), _) => std::fs::File::open(path)
                .map(|_| ())
                .map_err(|error| error.to_string()),
            (None, Some(image)) => match &image.source {
                bt_core::ImageSource::Embedded(embedded) => {
                    bt_image::probe_size_of_bytes(&embedded.bytes, &embedded.format)
                        .map(|_| ())
                        .map_err(image_problem)
                }
                _ => Err("The image has no file reference".into()),
            },
            _ => Err("The resource has no file reference".into()),
        };
        if let Err(error) = result {
            issues.push(resource.issue(error));
        }
    }
    for &id in scope {
        let Some(asset) = store.get(id) else { continue };
        if let Some(map) = store.editing_map(id) {
            for violation in map.validate() {
                let target = match &violation {
                    Violation::DuplicateLayerId(layer)
                    | Violation::BoundsMismatch(layer)
                    | Violation::UnknownTileset { layer, .. }
                    | Violation::TilesOutOfRange { layer, .. }
                    | Violation::CellsOutsideBounds { layer, .. } => IssueTarget::Layer {
                        map: id,
                        layer: *layer,
                    },
                    Violation::DuplicateObjectId(object)
                    | Violation::OverridesWithoutTemplate(object) => IssueTarget::Object {
                        map: id,
                        object: *object,
                    },
                    Violation::TooManyColors { slot, .. }
                    | Violation::UnknownColor { slot, .. } => store
                        .home_of_slot(id, *slot)
                        .map(|home| IssueTarget::Tileset { home, tile: None })
                        .unwrap_or(IssueTarget::Asset(id)),
                    _ => IssueTarget::Asset(id),
                };
                issues.push(
                    Issue::new(IssueKind::Map(violation.clone()), violation.to_string())
                        .target(target),
                );
            }
        }
        for warning in asset.warnings() {
            if let Some(path) = &warning.related_file {
                // The reference may have been edited since loading. Only live references count.
                for resource in resources
                    .iter()
                    .filter(|resource| resource.path.as_ref() == Some(path))
                {
                    if resource.image.is_some() {
                        continue;
                    } // Live image probes supersede header warnings.
                    if !issues.iter().any(|issue| {
                        issue.kind
                            == IssueKind::Resource {
                                kind: resource.kind,
                                path: path.clone(),
                            }
                            && issue.fix == resource.fix
                    }) {
                        issues.push(resource.issue(&warning.message));
                    }
                }
                continue;
            }
            let target = match (&warning.target, asset.root()) {
                (Some(bt_tmx::Target::Layer(layer)), _) => IssueTarget::Layer {
                    map: id,
                    layer: *layer,
                },
                (Some(bt_tmx::Target::Object(object)), _) => IssueTarget::Object {
                    map: id,
                    object: *object,
                },
                (Some(bt_tmx::Target::Tile { tile, .. }), Root::Tileset(_)) => {
                    IssueTarget::Tileset {
                        home: crate::TilesetHome::Asset(id),
                        tile: Some(*tile),
                    }
                }
                _ => IssueTarget::Asset(id),
            };
            issues.push(
                Issue::new(IssueKind::Format(warning.kind), warning.to_string()).target(target),
            );
        }
    }
    deduplicate_issues(issues)
}

/// The path belongs to the issue's typed identity; show the decoder detail without repeating it.
pub(super) fn image_problem(error: bt_image::Error) -> String {
    use bt_image::Error;
    match error {
        Error::Io { message, .. }
        | Error::Decode { message, .. }
        | Error::Write { message, .. } => message,
        Error::Unsupported { .. } => "Unsupported image format".into(),
        Error::Recursive { .. } => "The map image refers to itself".into(),
        Error::NoMapSource { .. } => "Map-image rendering is not configured".into(),
        Error::Stopped => "The image workers stopped".into(),
        Error::Exists { .. } => "A file already exists at the destination".into(),
    }
}
