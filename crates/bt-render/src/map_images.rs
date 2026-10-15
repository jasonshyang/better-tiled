//! Map images use the same GPU device and a renderer of their own, so nested rendering never
//! waits on the outer picture's renderer lock. Loading remains the caller's job.

use std::path::Path;

use bt_image::{DecodedImage, Error, MapImageSource, NestedImages};

use crate::{HeadlessDevice, MapRenderer, NestedSupply, Render};

/// Connects a file loader to the image store's map-image hook without a file-format dependency
/// in the renderer. The callback supplies the map and its rendering options.
pub struct MapImages<F> {
    load: F,
}

impl<F> MapImages<F>
where
    F: Fn(&Path) -> Result<Render, String> + Send + Sync,
{
    /// Draws map files loaded by `load`.
    pub fn new(load: F) -> MapImages<F> {
        MapImages { load }
    }
}

impl<F> MapImageSource for MapImages<F>
where
    F: Fn(&Path) -> Result<Render, String> + Send + Sync,
{
    fn render_map(&self, path: &Path, nested: &NestedImages<'_>) -> Result<DecodedImage, Error> {
        let draw = || -> Result<DecodedImage, String> {
            let render = (self.load)(path)?;
            let gpu = HeadlessDevice::acquire().map_err(|error| error.to_string())?;
            let mut renderer =
                MapRenderer::builder(gpu.device(), gpu.queue(), crate::compositor::OFFSCREEN)
                    .build();
            let mut images = NestedSupply::new(nested);
            let picture = render
                .run_with(gpu.device(), gpu.queue(), &mut renderer, &mut images)
                .map_err(|error| error.to_string())?;
            if let Some(error) = images.failure() {
                return Err(error.to_string());
            }
            Ok(DecodedImage {
                width: picture.width,
                height: picture.height,
                rgba: picture.rgba,
            })
        };
        draw().map_err(|message| Error::Decode {
            source_name: path.display().to_string(),
            message,
        })
    }
}
