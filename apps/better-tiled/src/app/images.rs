//! Image attribute probing and PNG export, off the frame thread.

use std::path::PathBuf;
use std::sync::Arc;

use bt_core::{Color, FileRef, ImageRef};
use bt_editor::{DocumentId, Inspected, Intent, Notice};
use bt_image::{DecodedImage, ImageStore, MapImageSource, probe_size, replace_png};
use bt_render::{HeadlessDevice, MapImages, ObjectStyle, Render};

use super::App;

impl App {
    pub(super) fn start_attribute_image(
        &mut self,
        doc: DocumentId,
        inspected: Inspected,
        path: PathBuf,
    ) {
        let spawned = self.workers.spawn("attribute image", move || {
            let result = read_image_reference(&path)
                .map_err(|error| format!("Cannot use {}: {error}", path.display()));
            vec![Intent::AttributeImageRead {
                doc,
                inspected,
                result,
            }]
        });
        if let Err(error) = spawned {
            self.notify(Notice::error(error));
        }
    }

    pub(super) fn start_image_export(
        &mut self,
        doc: DocumentId,
        scale_percent: u32,
        background: bool,
        path: PathBuf,
    ) {
        let Some(map) = self.editor.shown_map(doc).cloned() else {
            self.notify(Notice::error("The map to export is no longer open"));
            return;
        };
        if !(1..=800).contains(&scale_percent) {
            self.notify(Notice::error("Image scale must be between 1% and 800%"));
            return;
        }
        let types = Arc::new(self.editor.custom_types().clone());
        let objects = ObjectStyle::default().custom_types(&types);
        let spawned = self.workers.spawn("image export", move || {
            let result = (|| -> Result<(), String> {
                let color = if background {
                    map.background_color.unwrap_or(Color::rgba(0, 0, 0, 0))
                } else {
                    Color::rgba(0, 0, 0, 0)
                };
                let mut images = ImageStore::builder()
                    .map_images(map_image_source(types))
                    .build()
                    .map_err(|error| error.to_string())?;
                let rendered = Render::of(map)
                    .scale(f64::from(scale_percent) / 100.0)
                    .background(color)
                    .objects(objects)
                    .run_with_images(
                        HeadlessDevice::acquire().map_err(|error| error.to_string())?,
                        &mut images,
                    )
                    .map_err(|error| error.to_string())?;
                replace_png(
                    &path,
                    &DecodedImage {
                        width: rendered.width,
                        height: rendered.height,
                        rgba: rendered.rgba,
                    },
                )
                .map_err(|error| error.to_string())
            })();
            let notice = match result {
                Ok(()) => Notice::info(format!("Exported image to {}", path.display())),
                Err(error) => Notice::error(format!("Cannot export image: {error}")),
            };
            vec![Intent::Report(notice)]
        });
        if let Err(error) = spawned {
            self.notify(Notice::error(error));
        }
    }
}

pub(super) fn map_image_source(types: Arc<bt_core::CustomTypes>) -> Arc<dyn MapImageSource> {
    Arc::new(MapImages::new(move |path| {
        let map = bt_tmx::Loader::builder()
            .custom_types(Arc::clone(&types))
            .build()
            .load_map(path)
            .map_err(|error| error.to_string())?
            .value;
        let background = map.background_color.unwrap_or(Color::rgba(0, 0, 0, 0));
        Ok(Render::of(Arc::new(map))
            .background(background)
            .objects(ObjectStyle::default().custom_types(&types)))
    }))
}

pub(super) fn read_image_reference(path: &std::path::Path) -> Result<ImageRef, String> {
    let size = if bt_image::is_map_file(path) {
        let map = bt_tmx::Loader::builder()
            .build()
            .load_map(path)
            .map_err(|error| error.to_string())?
            .value;
        map.bounding_rect().size().as_uvec2()
    } else {
        probe_size(path).map_err(|error| error.to_string())?
    };
    let path = std::path::absolute(path).map_err(|error| error.to_string())?;
    Ok(ImageRef::file(
        FileRef::new(path.to_string_lossy().into_owned(), path),
        size,
    ))
}
