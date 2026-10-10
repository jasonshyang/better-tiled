//! The checks that run without a person. `--screenshot` opens the files, waits until they are
//! loaded, every image is drawn and egui's animations are over, runs the commands `BT_SCREENSHOT_ACTIONS` names (by their
//! keymap names, separated by commas) and waits again, saves the window as a PNG and quits. `--bench` waits the
//! same way, then pans across the current map at each zoom from 1/32 to 64, prints the frame
//! times and the renderer's account at each, and quits.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bt_core::Map;
use bt_editor::ActionId;
use bt_render::RenderStats;
use glam::DVec2;

use crate::app::PanelTab;
use crate::canvas::{Canvas, MAX_ZOOM, MIN_ZOOM, ViewTransform};

/// How long a check waits for its files before it gives up.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// Frames in a row with nothing pending before the window counts as drawn.
const SETTLED_FRAMES: u32 = 3;
/// How long it must stay so too: longer than egui's animations (`Style::animation_time`,
/// 0.2 s), so a window a command opened has faded in.
const SETTLE_TIME: Duration = Duration::from_millis(300);
/// Frames drawn at each zoom before they are measured.
const WARMUP_FRAMES: usize = 15;
/// Frames measured at each zoom.
const MEASURED_FRAMES: usize = 240;
/// How many frames it takes the bench to pan one view width.
const FRAMES_PER_VIEW: f64 = 60.0;
/// A frame slower than this is logged (at debug level) with what it did.
const SLOW_FRAME: Duration = Duration::from_millis(12);
/// Frames closer together than this (a 500 Hz display) were not presented.
const UNPRESENTED_INTERVAL_MS: f64 = 2.0;

/// A check in progress.
pub(crate) enum DevRun {
    /// `--screenshot`.
    Screenshot(ScreenshotRun),
    /// `--bench`.
    Bench(BenchRun),
}

/// What a check waits on.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Readiness {
    /// Loads are running.
    pub(crate) busy: bool,
    /// Images are not drawn yet (the canvas's or the Tiles panel's).
    pub(crate) pending_images: bool,
    /// A document is open.
    pub(crate) has_document: bool,
    /// Files were given to open.
    pub(crate) expects_document: bool,
}

impl Readiness {
    fn ready(self) -> bool {
        !self.busy && !self.pending_images && (self.has_document || !self.expects_document)
    }
}

/// What a check asks of the window this frame.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DevStep {
    /// Keep drawing frames.
    Continue,
    /// Run these commands, then wait for the window to settle again.
    Run(Vec<ActionId>),
    /// Take a picture of the window.
    TakeScreenshot,
    /// The check is over: quit, having failed when there is a reason.
    Quit(Option<String>),
}

/// `--screenshot` in progress.
pub(crate) struct ScreenshotRun {
    path: PathBuf,
    started: Instant,
    settled: u32,
    /// When the frames in a row with nothing pending began.
    settled_since: Option<Instant>,
    requested: bool,
    /// Commands still to run before the picture.
    actions: Vec<ActionId>,
}

impl ScreenshotRun {
    /// A screenshot to `path`, after running `actions` once the window has settled.
    pub(crate) fn new(path: PathBuf, actions: Vec<ActionId>) -> ScreenshotRun {
        ScreenshotRun {
            path,
            started: Instant::now(),
            settled: 0,
            settled_since: None,
            requested: false,
            actions,
        }
    }

    /// The next step, given what the window is waiting on and a picture taken, if any.
    pub(crate) fn step(
        &mut self,
        now: Instant,
        readiness: Readiness,
        picture: Option<&egui::ColorImage>,
    ) -> DevStep {
        if let Some(picture) = picture {
            return DevStep::Quit(save_png(picture, &self.path).err());
        }
        if now.duration_since(self.started) > READY_TIMEOUT {
            let what = if self.requested {
                // A window that cannot present (a locked screen, another Space) draws nothing to
                // take a picture of.
                "the picture of the window did not arrive (is the screen locked, or the window hidden?)"
            } else {
                "the window was not ready"
            };
            return DevStep::Quit(Some(format!(
                "--screenshot: {what} within {} s ({readiness:?})",
                READY_TIMEOUT.as_secs()
            )));
        }
        if self.requested {
            return DevStep::Continue;
        }
        if readiness.ready() {
            self.settled += 1;
            self.settled_since.get_or_insert(now);
        } else {
            self.settled = 0;
            self.settled_since = None;
        }
        let settled_for = self.settled_since.map(|since| now.duration_since(since));
        if self.settled >= SETTLED_FRAMES && settled_for.is_some_and(|t| t >= SETTLE_TIME) {
            if !self.actions.is_empty() {
                self.settled = 0;
                self.settled_since = None;
                return DevStep::Run(std::mem::take(&mut self.actions));
            }
            self.requested = true;
            return DevStep::TakeScreenshot;
        }
        DevStep::Continue
    }
}

/// The commands `BT_SCREENSHOT_ACTIONS` names, by their keymap names separated by commas; a name
/// that is no command stops the run.
pub(crate) fn screenshot_actions(value: Option<&str>) -> Result<Vec<ActionId>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            ActionId::from_name(name)
                .ok_or_else(|| format!("BT_SCREENSHOT_ACTIONS names no command “{name}”"))
        })
        .collect()
}

/// The panel tabs `BT_SCREENSHOT_TABS` names, by their titles separated by commas; a name that
/// is no tab stops the run.
pub(crate) fn screenshot_tabs(value: Option<&str>) -> Result<Vec<PanelTab>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            PanelTab::named(name)
                .ok_or_else(|| format!("BT_SCREENSHOT_TABS names no panel tab “{name}”"))
        })
        .collect()
}

/// Saves the window's picture as a PNG.
fn save_png(picture: &egui::ColorImage, path: &std::path::Path) -> Result<(), String> {
    let [width, height] = picture.size;
    let rgba: Vec<u8> = picture
        .pixels
        .iter()
        .flat_map(|pixel| pixel.to_srgba_unmultiplied())
        .collect();
    let width = u32::try_from(width).map_err(|_| "the picture is too wide".to_owned())?;
    let height = u32::try_from(height).map_err(|_| "the picture is too tall".to_owned())?;
    let image = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "the picture's pixels do not match its size".to_owned())?;
    image.save(path).map_err(|error| {
        format!(
            "--screenshot: {} could not be written: {error}",
            path.display()
        )
    })
}

/// `--bench` in progress.
pub(crate) struct BenchRun {
    started: Instant,
    settled: u32,
    zooms: Vec<f64>,
    level: usize,
    frame: usize,
    position: DVec2,
    direction: DVec2,
    last_frame: Option<Instant>,
    samples: Vec<Sample>,
    results: Vec<LevelResult>,
}

/// One measured frame.
#[derive(Clone, Debug)]
struct Sample {
    interval: Duration,
    cpu: Duration,
    stats: RenderStats,
}

/// What one zoom measured.
#[derive(Clone, Debug)]
struct LevelResult {
    zoom: f64,
    samples: Vec<Sample>,
}

/// What the bench needs from the window each frame.
pub(crate) struct BenchFrame<'a> {
    /// What the window waits on.
    pub(crate) readiness: Readiness,
    /// The current canvas.
    pub(crate) canvas: Option<&'a mut Canvas>,
    /// The current map.
    pub(crate) map: Option<&'a Map>,
    /// The time this frame's own work took so far.
    pub(crate) cpu: Duration,
    /// The renderer's account of the last frame.
    pub(crate) stats: RenderStats,
}

impl BenchRun {
    /// A bench at every power-of-two zoom in the zoom range.
    pub(crate) fn new() -> BenchRun {
        let mut zooms = Vec::new();
        let mut zoom = MIN_ZOOM;
        while zoom <= MAX_ZOOM {
            zooms.push(zoom);
            zoom *= 2.0;
        }
        BenchRun {
            started: Instant::now(),
            settled: 0,
            zooms,
            level: 0,
            frame: 0,
            position: DVec2::ZERO,
            direction: DVec2::new(0.866, 0.5),
            last_frame: None,
            samples: Vec::new(),
            results: Vec::new(),
        }
    }

    /// The next step: moves the camera for the next frame and measures the last.
    pub(crate) fn step(&mut self, frame: BenchFrame<'_>) -> DevStep {
        let now = Instant::now();
        if self.settled < SETTLED_FRAMES {
            if self.started.elapsed() > READY_TIMEOUT {
                return DevStep::Quit(Some(format!(
                    "--bench: the map was not ready within {} s",
                    READY_TIMEOUT.as_secs()
                )));
            }
            self.settled = if frame.readiness.ready() {
                self.settled + 1
            } else {
                0
            };
            return DevStep::Continue;
        }
        let Some(canvas) = frame.canvas else {
            return DevStep::Quit(Some("--bench: no map is open".to_owned()));
        };
        let Some(map) = frame.map else {
            return DevStep::Quit(Some("--bench: no map is open".to_owned()));
        };
        let Some(view) = canvas.last_view() else {
            return DevStep::Continue;
        };
        if let Some(last) = self.last_frame
            && self.frame > WARMUP_FRAMES
        {
            let interval = now - last;
            if interval > SLOW_FRAME {
                log::debug!(
                    "slow frame at zoom {}: {:.2} ms between frames, ours {:.2} ms, last prepare {:.2} ms, {} instances, {} uploaded bytes, {} squares pending",
                    zoom_label(self.zooms.get(self.level).copied().unwrap_or(1.0)),
                    millis(interval),
                    millis(frame.cpu),
                    millis(frame.stats.prepare_time),
                    frame.stats.instances,
                    frame.stats.uploaded_bytes,
                    frame.stats.pending_impostors
                );
            }
            self.samples.push(Sample {
                interval,
                cpu: frame.cpu,
                stats: frame.stats,
            });
        }
        self.last_frame = Some(now);
        if self.frame >= WARMUP_FRAMES + MEASURED_FRAMES {
            let zoom = self.zooms.get(self.level).copied().unwrap_or(1.0);
            self.results.push(LevelResult {
                zoom,
                samples: std::mem::take(&mut self.samples),
            });
            self.level += 1;
            self.frame = 0;
            if self.level >= self.zooms.len() {
                report(&self.results, map, &view);
                return DevStep::Quit(None);
            }
        }
        let Some(zoom) = self.zooms.get(self.level).copied() else {
            return DevStep::Quit(None);
        };
        let bounds = map.bounding_rect();
        let min = bounds.min().as_dvec2();
        let max = bounds.max().as_dvec2();
        if self.frame == 0 {
            self.position = (min + max) * 0.5;
        }
        let view_width = f64::from(view.rect.width()) / zoom;
        self.position += self.direction * view_width / FRAMES_PER_VIEW;
        for axis in 0..2 {
            let low = min[axis];
            let high = max[axis];
            if self.position[axis] < low || self.position[axis] > high {
                self.direction[axis] = -self.direction[axis];
                self.position[axis] = self.position[axis].clamp(low, high);
            }
        }
        canvas.set_camera(self.position, zoom);
        self.frame += 1;
        DevStep::Continue
    }
}

/// Prints the bench's table.
#[allow(clippy::print_stdout, reason = "the bench reports to the terminal")]
fn report(results: &[LevelResult], map: &Map, view: &ViewTransform) {
    println!(
        "better-tiled bench: {}×{} map of {}×{} px tiles, {:.0}×{:.0} pt view at {}× ({} frames a zoom)",
        map.width,
        map.height,
        map.tile_width,
        map.tile_height,
        view.rect.width(),
        view.rect.height(),
        view.pixels_per_point,
        MEASURED_FRAMES
    );
    println!(
        "{:>7}  {:>22}  {:>14}  {:>11}  {:>9}  {:>6}  {:>9}  {:>7}",
        "zoom",
        "interval p50/p95/max",
        "ours p50/p95",
        "prepare p95",
        "instances",
        "draws",
        "impostors",
        "pending"
    );
    let mut all_intervals = Vec::new();
    for level in results {
        let mut intervals: Vec<f64> = level.samples.iter().map(|s| millis(s.interval)).collect();
        let mut cpu: Vec<f64> = level.samples.iter().map(|s| millis(s.cpu)).collect();
        let mut prepare: Vec<f64> = level
            .samples
            .iter()
            .map(|s| millis(s.stats.prepare_time))
            .collect();
        all_intervals.extend(intervals.iter().copied());
        let last = level
            .samples
            .last()
            .map(|s| s.stats.clone())
            .unwrap_or_default();
        println!(
            "{:>7}  {:>6.2} /{:>6.2} /{:>6.2}  {:>6.2} /{:>6.2}  {:>11.2}  {:>9}  {:>6}  {:>9}  {:>7}",
            zoom_label(level.zoom),
            percentile(&mut intervals, 0.5),
            percentile(&mut intervals, 0.95),
            percentile(&mut intervals, 1.0),
            percentile(&mut cpu, 0.5),
            percentile(&mut cpu, 0.95),
            percentile(&mut prepare, 0.95),
            last.instances,
            last.draws,
            last.impostors,
            last.pending_images
        );
    }
    let median = percentile(&mut all_intervals, 0.5);
    if median < UNPRESENTED_INTERVAL_MS {
        println!(
            "frames came every {median:.2} ms: faster than any display, so the window did not present them (is the screen locked, or the window hidden?); the intervals measure nothing"
        );
    }
    let budget = median * 1.5;
    let missed = all_intervals
        .iter()
        .filter(|interval| **interval > budget)
        .count();
    println!(
        "display: a frame every {median:.2} ms (≈{:.0} Hz); {missed} of {} frames took over {budget:.2} ms",
        1000.0 / median.max(0.001),
        all_intervals.len()
    );
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// The value below which `share` of `values` fall (sorts them).
fn percentile(values: &mut [f64], share: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * share).round() as usize;
    values.get(index).copied().unwrap_or(0.0)
}

fn zoom_label(zoom: f64) -> String {
    if zoom >= 1.0 {
        format!("{zoom}")
    } else {
        format!("1/{}", (1.0 / zoom).round())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_and_labels() {
        let mut values = vec![5.0, 1.0, 3.0, 2.0, 4.0];
        assert_eq!(percentile(&mut values, 0.5), 3.0);
        assert_eq!(percentile(&mut values, 1.0), 5.0);
        assert_eq!(percentile(&mut [], 0.5), 0.0);
        assert_eq!(zoom_label(0.25), "1/4");
        assert_eq!(zoom_label(8.0), "8");
        assert_eq!(BenchRun::new().zooms.len(), 12);
    }

    #[test]
    fn a_screenshot_waits_until_the_window_has_settled() {
        let mut run = ScreenshotRun::new(PathBuf::from("unused.png"), Vec::new());
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let busy = Readiness {
            busy: true,
            pending_images: false,
            has_document: false,
            expects_document: true,
        };
        let ready = Readiness {
            busy: false,
            has_document: true,
            ..busy
        };
        assert_eq!(run.step(at(0), busy, None), DevStep::Continue);
        assert_eq!(run.step(at(8), ready, None), DevStep::Continue);
        assert_eq!(run.step(at(16), ready, None), DevStep::Continue);
        // Three frames, but a window just opened may still be fading in.
        assert_eq!(run.step(at(24), ready, None), DevStep::Continue);
        assert_eq!(run.step(at(308), ready, None), DevStep::TakeScreenshot);
        assert_eq!(run.step(at(316), ready, None), DevStep::Continue);
    }

    #[test]
    fn commands_run_before_the_picture_and_the_window_settles_again() {
        let mut run = ScreenshotRun::new(PathBuf::from("unused.png"), vec![ActionId::ShowGrid]);
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let ready = Readiness {
            busy: false,
            pending_images: false,
            has_document: true,
            expects_document: true,
        };
        assert_eq!(run.step(at(0), ready, None), DevStep::Continue);
        // Long enough, but not yet three frames.
        assert_eq!(run.step(at(400), ready, None), DevStep::Continue);
        assert_eq!(
            run.step(at(408), ready, None),
            DevStep::Run(vec![ActionId::ShowGrid])
        );
        // It settles again, as long, before the picture.
        for ms in [416, 424, 432] {
            assert_eq!(run.step(at(ms), ready, None), DevStep::Continue);
        }
        assert_eq!(run.step(at(716), ready, None), DevStep::TakeScreenshot);
        assert_eq!(
            screenshot_actions(Some("show_grid, select_all")),
            Ok(vec![ActionId::ShowGrid, ActionId::SelectAll])
        );
        assert_eq!(screenshot_actions(None), Ok(Vec::new()));
        assert!(screenshot_actions(Some("no_such_thing")).is_err());
        assert_eq!(screenshot_tabs(Some("automap, Terrains")).unwrap().len(), 2);
        assert!(screenshot_tabs(Some("no_such_tab")).is_err());
    }
}
