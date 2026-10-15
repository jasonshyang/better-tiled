//! Resource identities are derived from model references, including groups and tile collections.

use super::model::{IssueTarget, LocateTarget, ResourceKind};
use crate::{AssetId, AssetStore, Root, TilesetHome};
use bt_core::{ImageRef, ImageSource, LayerKind, Tileset, TilesetImage, TilesetSlot};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(super) struct Resource {
    pub(super) path: Option<PathBuf>,
    pub(super) image_key: Option<bt_image::ImageKey>,
    pub(super) kind: ResourceKind,
    pub(super) target: IssueTarget,
    pub(super) fix: Option<LocateTarget>,
    pub(super) image: Option<ImageRef>,
}

fn image_resource(image: &ImageRef, target: IssueTarget, fix: LocateTarget) -> Option<Resource> {
    let path = match &image.source {
        ImageSource::File(file) if file.source.starts_with(":/") => return None,
        ImageSource::File(file) => Some(file.path.clone()),
        ImageSource::Embedded(_) => None,
    };
    Some(Resource {
        path,
        image_key: Some(bt_image::ImageKey::of(&bt_image::image_request(image))),
        kind: ResourceKind::Image,
        target,
        fix: Some(fix),
        image: Some(image.clone()),
    })
}
fn tileset_resources(tileset: &Tileset, home: TilesetHome, resources: &mut Vec<Resource>) {
    if let TilesetImage::Atlas { image, .. } = &tileset.image
        && let Some(resource) = image_resource(
            image,
            IssueTarget::Tileset { home, tile: None },
            LocateTarget::TilesetImage { home, tile: None },
        )
    {
        resources.push(resource);
    }
    for (id, tile) in &tileset.tiles {
        if let Some(group) = &tile.collision {
            for object in group.layer.objects.iter() {
                if let Some(file) = &object.template {
                    resources.push(Resource {
                        path: Some(file.path.clone()),
                        image_key: None,
                        kind: ResourceKind::Template,
                        target: IssueTarget::CollisionObject {
                            home,
                            tile: *id,
                            object: object.id,
                        },
                        fix: Some(LocateTarget::CollisionTemplate {
                            home,
                            tile: *id,
                            object: object.id,
                        }),
                        image: None,
                    });
                }
            }
        }
        if let Some(image) = &tile.image
            && let Some(resource) = image_resource(
                image,
                IssueTarget::Tileset {
                    home,
                    tile: Some(*id),
                },
                LocateTarget::TilesetImage {
                    home,
                    tile: Some(*id),
                },
            )
        {
            resources.push(resource);
        }
    }
}

pub(super) fn resources_of(store: &AssetStore, scope: &[AssetId]) -> Vec<Resource> {
    let mut resources = Vec::new();
    for id in scope {
        let Some(asset) = store.get(*id) else {
            continue;
        };
        if let Some(path) = asset.path() {
            resources.push(Resource {
                path: Some(path.to_path_buf()),
                image_key: None,
                kind: ResourceKind::File,
                target: IssueTarget::Asset(*id),
                fix: None,
                image: None,
            });
        }
        match asset.root() {
            Root::Map(map) => {
                for (index, reference) in map.tilesets.iter().enumerate() {
                    let Some(slot) = TilesetSlot::new(index) else {
                        continue;
                    };
                    if let Some(file) = &reference.file {
                        if !file.source.starts_with(":/") {
                            resources.push(Resource {
                                path: Some(file.path.clone()),
                                image_key: None,
                                kind: ResourceKind::Tileset,
                                target: IssueTarget::Asset(*id),
                                fix: Some(LocateTarget::Tileset { owner: *id, slot }),
                                image: None,
                            });
                        }
                    } else {
                        tileset_resources(
                            &reference.tileset,
                            TilesetHome::Embedded { map: *id, slot },
                            &mut resources,
                        );
                    }
                }
                map.for_each_layer(|layer, _| match &layer.kind {
                    LayerKind::Image(image) => {
                        if let Some(image) = &image.image
                            && let Some(resource) = image_resource(
                                image,
                                IssueTarget::Layer {
                                    map: *id,
                                    layer: layer.id,
                                },
                                LocateTarget::LayerImage {
                                    map: *id,
                                    layer: layer.id,
                                },
                            )
                        {
                            resources.push(resource);
                        }
                    }
                    LayerKind::Object(objects) => {
                        for object in objects.objects.iter() {
                            if let Some(file) = &object.template {
                                resources.push(Resource {
                                    path: Some(file.path.clone()),
                                    image_key: None,
                                    kind: ResourceKind::Template,
                                    target: IssueTarget::Object {
                                        map: *id,
                                        object: object.id,
                                    },
                                    fix: Some(LocateTarget::Template {
                                        map: *id,
                                        object: object.id,
                                    }),
                                    image: None,
                                });
                            }
                        }
                    }
                    _ => {}
                });
            }
            Root::Tileset(tileset) => {
                tileset_resources(tileset, TilesetHome::Asset(*id), &mut resources)
            }
            Root::Template(template) => {
                for (index, reference) in template.tilesets.iter().enumerate() {
                    let Some(slot) = TilesetSlot::new(index) else {
                        continue;
                    };
                    if let Some(file) = &reference.file {
                        resources.push(Resource {
                            path: Some(file.path.clone()),
                            image_key: None,
                            kind: ResourceKind::Tileset,
                            target: IssueTarget::Asset(*id),
                            fix: Some(LocateTarget::Tileset { owner: *id, slot }),
                            image: None,
                        });
                    } else {
                        tileset_resources(
                            &reference.tileset,
                            TilesetHome::Embedded { map: *id, slot },
                            &mut resources,
                        );
                    }
                }
            }
            Root::World(world) => {
                for (index, map) in world.maps.iter().enumerate() {
                    resources.push(Resource {
                        path: Some(map.file.path.clone()),
                        image_key: None,
                        kind: ResourceKind::Map,
                        target: IssueTarget::Asset(*id),
                        fix: Some(LocateTarget::WorldMap { world: *id, index }),
                        image: None,
                    });
                }
            }
            Root::Project(project) => {
                for file in project
                    .object_types_file
                    .iter()
                    .chain(project.automapping_rules_file.iter())
                {
                    resources.push(Resource {
                        path: Some(file.path.clone()),
                        image_key: None,
                        kind: ResourceKind::File,
                        target: IssueTarget::Asset(*id),
                        fix: None,
                        image: None,
                    });
                }
            }
        }
    }
    let referenced = resources
        .iter()
        .filter(|resource| resource.fix.is_some())
        .map(|resource| resource.path.clone())
        .collect::<std::collections::HashSet<_>>();
    resources.retain(|resource| {
        resource.kind != ResourceKind::File || !referenced.contains(&resource.path)
    });
    resources
}
