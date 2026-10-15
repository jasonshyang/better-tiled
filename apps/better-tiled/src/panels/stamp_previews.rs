//! Small stamp pictures use the headless map renderer on workers and upload on completion.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};

use crate::workers::Workers;
use bt_editor::{Editor, Intent, Notice, SavedStamp};
use bt_image::ImageStore;
use bt_render::{HeadlessDevice, MapImages, Render};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PreviewKey {
    path: PathBuf,
    variation: usize,
}

struct Finished {
    generation: u64,
    key: PreviewKey,
    result: Result<egui::TextureHandle, String>,
}

pub(super) struct StampPreviews {
    generation: u64,
    textures: HashMap<PreviewKey, Option<Result<egui::TextureHandle, String>>>,
    sender: mpsc::Sender<Finished>,
    results: mpsc::Receiver<Finished>,
}

impl Default for StampPreviews {
    fn default() -> Self {
        let (sender, results) = mpsc::channel();
        Self {
            generation: 0,
            textures: HashMap::new(),
            sender,
            results,
        }
    }
}

impl StampPreviews {
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        editor: &Editor,
        saved: &SavedStamp,
        variation: usize,
        workers: &mut Workers,
        out: &mut Vec<Intent>,
    ) {
        let generation = editor.stamps().generation();
        if self.generation != generation {
            self.generation = generation;
            self.textures.clear();
        }
        for finished in self.results.try_iter() {
            if finished.generation == generation {
                self.textures.insert(finished.key, Some(finished.result));
            }
        }
        let key = PreviewKey {
            path: saved.path.clone(),
            variation,
        };
        if !self.textures.contains_key(&key) {
            let Some(variation) = saved.stamp.variations.get(variation) else {
                return;
            };
            let map = Arc::new(variation.map.clone());
            let sender = self.sender.clone();
            let context = ui.ctx().clone();
            let wanted = key.clone();
            let spawned = workers.spawn("stamp preview", move || {
                let result = (|| -> Result<egui::TextureHandle, String> {
                    let longest = map.bounding_rect().w.max(map.bounding_rect().h).max(1);
                    let scale = (64.0 / f64::from(longest)).min(4.0);
                    let mut images = ImageStore::builder()
                        .map_images(Arc::new(MapImages::new(|path| {
                            let map = bt_tmx::Loader::default()
                                .load_map(path)
                                .map_err(|error| error.to_string())?
                                .value;
                            Ok(Render::of(Arc::new(map)))
                        })))
                        .build()
                        .map_err(|error| error.to_string())?;
                    let picture = Render::of(map)
                        .scale(scale)
                        .run_with_images(
                            HeadlessDevice::acquire().map_err(|error| error.to_string())?,
                            &mut images,
                        )
                        .map_err(|error| error.to_string())?;
                    Ok(context.load_texture(
                        format!("stamp {}", wanted.path.display()),
                        egui::ColorImage::from_rgba_unmultiplied(
                            [picture.width as usize, picture.height as usize],
                            &picture.rgba,
                        ),
                        egui::TextureOptions::NEAREST,
                    ))
                })();
                let _ = sender.send(Finished {
                    generation,
                    key: wanted,
                    result,
                });
                Vec::new()
            });
            match spawned {
                Ok(()) => {
                    self.textures.insert(key.clone(), None);
                }
                Err(error) => {
                    self.textures.insert(key.clone(), Some(Err(error.clone())));
                    out.push(Intent::Report(Notice::error(error)));
                }
            }
        }
        match self.textures.get(&key).and_then(Option::as_ref) {
            Some(Ok(texture)) => {
                ui.image((texture.id(), texture.size_vec2()));
            }
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, "Preview failed")
                    .on_hover_text(error);
            }
            None => {
                ui.spinner();
            }
        }
    }
}
