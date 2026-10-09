//! The image store: decoded images by [`ImageKey`], decoded on worker threads so that a frame
//! never waits for one, kept until a byte budget asks for room.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use crate::decode::{DecodedImage, decode_bytes, decode_file, is_map_file};
use crate::error::Error;
use crate::key::{ImageInput, ImageKey, ImageRequest};

/// Draws a map file used as an image (of a tileset, a tile or an image layer). The application
/// gives one to the store, built on the headless renderer; the store has no other way to draw a
/// map.
pub trait MapImageSource: Send + Sync {
    /// The picture of the map at `path`. Images the map itself shows that are maps too are
    /// decoded through `nested`, which refuses a map that would show itself.
    fn render_map(&self, path: &Path, nested: &NestedImages<'_>) -> Result<DecodedImage, Error>;
}

/// Decodes the images a map drawn as an image needs, remembering which maps are being drawn so
/// that one showing itself is an error rather than an endless loop.
pub struct NestedImages<'a> {
    source: &'a dyn MapImageSource,
    drawing: Vec<PathBuf>,
}

impl NestedImages<'_> {
    /// Decodes an image the map being drawn shows.
    pub fn decode(&self, request: &ImageRequest) -> Result<DecodedImage, Error> {
        decode_request(request, Some(self.source), &self.drawing)
    }
}

/// Decodes one request; `drawing` holds the maps being drawn around it.
fn decode_request(
    request: &ImageRequest,
    source: Option<&dyn MapImageSource>,
    drawing: &[PathBuf],
) -> Result<DecodedImage, Error> {
    match &request.input {
        ImageInput::Bytes { bytes, format } => decode_bytes(bytes, format, request.transparent),
        ImageInput::File(path) if is_map_file(path) => {
            if drawing.iter().any(|map| map == path) {
                return Err(Error::Recursive { path: path.clone() });
            }
            let Some(source) = source else {
                return Err(Error::NoMapSource { path: path.clone() });
            };
            let nested = NestedImages {
                source,
                drawing: drawing
                    .iter()
                    .cloned()
                    .chain(std::iter::once(path.clone()))
                    .collect(),
            };
            let mut image = source.render_map(path, &nested)?;
            if let Some(key) = request.transparent {
                image.apply_color_key(key);
            }
            Ok(image)
        }
        ImageInput::File(path) => decode_file(path, request.transparent),
    }
}

/// What the store has for a key.
#[derive(Clone, Debug)]
pub enum ImagePoll {
    /// The image.
    Ready(Arc<DecodedImage>),
    /// Still being decoded.
    Pending,
    /// It could not be decoded.
    Failed(Error),
    /// Nobody asked for it (or it was evicted).
    NotRequested,
}

/// Sets up an [`ImageStore`].
pub struct ImageStoreBuilder {
    threads: usize,
    map_images: Option<Arc<dyn MapImageSource>>,
}

impl ImageStoreBuilder {
    /// How many worker threads decode (at least one; two by default).
    pub fn threads(mut self, threads: usize) -> ImageStoreBuilder {
        self.threads = threads.max(1);
        self
    }

    /// What draws maps used as images.
    pub fn map_images(mut self, source: Arc<dyn MapImageSource>) -> ImageStoreBuilder {
        self.map_images = Some(source);
        self
    }

    /// The store, with its workers started.
    pub fn build(self) -> std::io::Result<ImageStore> {
        let (job_sender, job_receiver) = mpsc::channel::<Job>();
        let (result_sender, results) = mpsc::channel::<Finished>();
        let queue = Arc::new(Mutex::new(job_receiver));
        let waker: SharedWaker = Arc::new(Mutex::new(None));
        let mut workers = Vec::with_capacity(self.threads);
        for n in 0..self.threads {
            let worker = Worker {
                queue: Arc::clone(&queue),
                results: result_sender.clone(),
                waker: Arc::clone(&waker),
                map_images: self.map_images.clone(),
            };
            workers.push(
                std::thread::Builder::new()
                    .name(format!("image decoder {n}"))
                    .spawn(move || worker.run())?,
            );
        }
        Ok(ImageStore {
            entries: HashMap::new(),
            jobs: Some(job_sender),
            results,
            workers,
            waker,
            tick: 0,
            map_images: self.map_images,
        })
    }
}

type Waker = Arc<dyn Fn() + Send + Sync>;
type SharedWaker = Arc<Mutex<Option<Waker>>>;

/// One image to decode.
struct Job {
    key: ImageKey,
    request: ImageRequest,
}

/// A decoded (or failed) image on its way back.
struct Finished {
    key: ImageKey,
    result: Result<DecodedImage, Error>,
}

/// A decoding thread.
struct Worker {
    queue: Arc<Mutex<Receiver<Job>>>,
    results: Sender<Finished>,
    waker: SharedWaker,
    map_images: Option<Arc<dyn MapImageSource>>,
}

impl Worker {
    fn run(self) {
        loop {
            let job = self
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .recv();
            let Ok(Job { key, request }) = job else {
                return;
            };
            let result = decode_request(&request, self.map_images.as_deref(), &[]);
            if self.results.send(Finished { key, result }).is_err() {
                return;
            }
            let waker = self
                .waker
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if let Some(waker) = waker {
                waker();
            }
        }
    }
}

/// An entry of the store.
struct Entry {
    state: EntryState,
    /// The tick of the last request or poll, for eviction.
    last_used: u64,
}

enum EntryState {
    Pending,
    Ready(Arc<DecodedImage>),
    Failed(Error),
}

/// Decoded images by key. One store serves a process: the renderer, the panels and the
/// headless pictures all read it.
pub struct ImageStore {
    entries: HashMap<ImageKey, Entry>,
    /// `None` once the store is shutting down.
    jobs: Option<Sender<Job>>,
    results: Receiver<Finished>,
    workers: Vec<JoinHandle<()>>,
    waker: SharedWaker,
    tick: u64,
    map_images: Option<Arc<dyn MapImageSource>>,
}

impl ImageStore {
    /// Sets up a store.
    pub fn builder() -> ImageStoreBuilder {
        ImageStoreBuilder {
            threads: 2,
            map_images: None,
        }
    }

    /// Sets what is called on a worker thread whenever an image is done (to ask a window to
    /// repaint, say).
    pub fn set_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        *self.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(waker));
    }

    /// Asks for an image and returns its key; decoding starts on a worker unless the store has
    /// it already or is decoding it.
    pub fn request(&mut self, request: ImageRequest) -> ImageKey {
        let key = ImageKey::of(&request);
        self.tick += 1;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = self.tick;
            return key;
        }
        let state = match &self.jobs {
            Some(jobs)
                if jobs
                    .send(Job {
                        key: key.clone(),
                        request,
                    })
                    .is_ok() =>
            {
                EntryState::Pending
            }
            _ => EntryState::Failed(Error::Stopped),
        };
        self.entries.insert(
            key.clone(),
            Entry {
                state,
                last_used: self.tick,
            },
        );
        key
    }

    /// What the store has for `key` now, taking in what the workers finished.
    pub fn poll(&mut self, key: &ImageKey) -> ImagePoll {
        self.take_finished();
        self.tick += 1;
        match self.entries.get_mut(key) {
            None => ImagePoll::NotRequested,
            Some(entry) => {
                entry.last_used = self.tick;
                match &entry.state {
                    EntryState::Pending => ImagePoll::Pending,
                    EntryState::Ready(image) => ImagePoll::Ready(Arc::clone(image)),
                    EntryState::Failed(error) => ImagePoll::Failed(error.clone()),
                }
            }
        }
    }

    /// The image for `request`, decoding it on this thread when the store does not have it:
    /// for headless pictures and tests, which have nothing better to do meanwhile.
    pub fn decode_now(&mut self, request: ImageRequest) -> Result<Arc<DecodedImage>, Error> {
        let key = self.request(request);
        loop {
            match self.poll(&key) {
                ImagePoll::Ready(image) => return Ok(image),
                ImagePoll::Failed(error) => return Err(error),
                ImagePoll::NotRequested => return Err(Error::Stopped),
                ImagePoll::Pending => match self.results.recv() {
                    Ok(finished) => self.record_finished(finished),
                    Err(_) => return Err(Error::Stopped),
                },
            }
        }
    }

    /// Evicts the least recently used images until the decoded pixels take at most
    /// `keep_bytes`. Images being decoded stay.
    pub fn trim(&mut self, keep_bytes: usize) {
        self.take_finished();
        let mut ready: Vec<(u64, ImageKey, usize)> = Vec::new();
        for (key, entry) in &self.entries {
            if let EntryState::Ready(image) = &entry.state {
                ready.push((entry.last_used, key.clone(), image.byte_size()));
            }
        }
        let mut total: usize = ready.iter().map(|(_, _, bytes)| bytes).sum();
        ready.sort_by_key(|(last_used, _, _)| *last_used);
        for (_, key, bytes) in ready {
            if total <= keep_bytes {
                break;
            }
            self.entries.remove(&key);
            total -= bytes;
        }
    }

    /// The memory the decoded pixels take.
    pub fn byte_size(&self) -> usize {
        self.entries
            .values()
            .map(|entry| match &entry.state {
                EntryState::Ready(image) => image.byte_size(),
                EntryState::Pending | EntryState::Failed(_) => 0,
            })
            .sum()
    }

    /// Whether maps used as images can be drawn.
    pub fn draws_maps(&self) -> bool {
        self.map_images.is_some()
    }

    /// Takes in every result the workers have sent.
    fn take_finished(&mut self) {
        while let Ok(finished) = self.results.try_recv() {
            self.record_finished(finished);
        }
    }

    fn record_finished(&mut self, finished: Finished) {
        let state = match finished.result {
            Ok(image) => EntryState::Ready(Arc::new(image)),
            Err(error) => EntryState::Failed(error),
        };
        // An entry evicted while it was being decoded is not brought back.
        if let Some(entry) = self.entries.get_mut(&finished.key) {
            entry.state = state;
        }
    }
}

impl Drop for ImageStore {
    fn drop(&mut self) {
        // Closing the queue ends each worker after its current image.
        self.jobs = None;
        for worker in self.workers.drain(..) {
            // A worker that panicked has nothing more to say.
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use bt_test_data::Scratch;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(width, height)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn bytes_request(bytes: Vec<u8>) -> ImageRequest {
        ImageRequest {
            input: ImageInput::Bytes {
                bytes: Arc::from(bytes),
                format: "png".to_owned(),
            },
            transparent: None,
        }
    }

    #[test]
    fn images_decode_on_workers_and_wake_the_owner() {
        let mut store = ImageStore::builder().threads(3).build().unwrap();
        let woken = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&woken);
        store.set_waker(move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        let keys: Vec<ImageKey> = (1..=5)
            .map(|n| store.request(bytes_request(png(n, 2))))
            .collect();
        let same = store.request(bytes_request(png(1, 2)));
        assert_eq!(
            same, keys[0],
            "a request for the same bytes is the same image"
        );
        for (n, key) in (1..=5).zip(&keys) {
            let image = loop {
                match store.poll(key) {
                    ImagePoll::Ready(image) => break image,
                    ImagePoll::Pending => std::thread::yield_now(),
                    other => panic!("{other:?}"),
                }
            };
            assert_eq!((image.width, image.height), (n, 2));
        }
        // A worker wakes the owner just after sending its image, so the last wake may lag.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while woken.load(Ordering::SeqCst) < 5 && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(woken.load(Ordering::SeqCst), 5);
        assert_eq!(
            store.byte_size(),
            (1..=5).map(|n| n * 2 * 4).sum::<u32>() as usize
        );
    }

    #[test]
    fn failures_are_kept_and_eviction_takes_the_oldest() {
        let scratch = Scratch::new("bt-image-store").unwrap();
        let mut store = ImageStore::builder().build().unwrap();
        let missing = ImageRequest {
            input: ImageInput::File(scratch.join("missing.png")),
            transparent: None,
        };
        assert!(matches!(store.decode_now(missing), Err(Error::Io { .. })));
        let old = store.decode_now(bytes_request(png(10, 10))).unwrap();
        let new = store.decode_now(bytes_request(png(20, 10))).unwrap();
        assert_eq!(store.byte_size(), old.byte_size() + new.byte_size());
        store.trim(new.byte_size());
        let old_key = ImageKey::of(&bytes_request(png(10, 10)));
        assert!(
            matches!(store.poll(&old_key), ImagePoll::NotRequested),
            "the least recently used went"
        );
        assert!(matches!(
            store.poll(&ImageKey::of(&bytes_request(png(20, 10)))),
            ImagePoll::Ready(_)
        ));
    }

    /// Draws every map as a 2×1 picture of the images it names in its file, one per line.
    struct LinesAsImages;

    impl MapImageSource for LinesAsImages {
        fn render_map(
            &self,
            path: &Path,
            nested: &NestedImages<'_>,
        ) -> Result<DecodedImage, Error> {
            let text = std::fs::read_to_string(path).map_err(|e| Error::from_io(path, &e))?;
            for line in text.lines() {
                let inner = ImageRequest {
                    input: ImageInput::File(path.with_file_name(line)),
                    transparent: None,
                };
                nested.decode(&inner)?;
            }
            Ok(DecodedImage {
                width: 2,
                height: 1,
                rgba: vec![255; 8],
            })
        }
    }

    #[test]
    fn maps_are_drawn_by_the_source_and_may_not_show_themselves() {
        let scratch = Scratch::new("bt-image-maps").unwrap();
        std::fs::write(scratch.join("tiles.png"), png(4, 4)).unwrap();
        std::fs::write(scratch.join("a.tmx"), "tiles.png\nb.tmx").unwrap();
        std::fs::write(scratch.join("b.tmx"), "tiles.png").unwrap();
        std::fs::write(scratch.join("loop.tmx"), "other.tmx").unwrap();
        std::fs::write(scratch.join("other.tmx"), "loop.tmx").unwrap();
        let map = |name: &str| ImageRequest {
            input: ImageInput::File(scratch.join(name)),
            transparent: None,
        };

        let mut plain = ImageStore::builder().build().unwrap();
        assert!(matches!(
            plain.decode_now(map("a.tmx")),
            Err(Error::NoMapSource { .. })
        ));
        assert!(!plain.draws_maps());

        let mut store = ImageStore::builder()
            .map_images(Arc::new(LinesAsImages))
            .build()
            .unwrap();
        assert_eq!(store.decode_now(map("a.tmx")).unwrap().width, 2);
        let error = store.decode_now(map("loop.tmx")).unwrap_err();
        assert_eq!(
            error,
            Error::Recursive {
                path: scratch.join("loop.tmx")
            }
        );
    }
}
