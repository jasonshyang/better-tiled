//! Background Godot exports, setup, refusal choices and source-linked Issues.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

use bt_core::{CustomTypes, LayerId, Map, ObjectId};
use bt_editor::{DocumentId, GodotOperation, Intent, Notice};
use bt_godot::{Element, Error, Found, Options, Report};

use super::frame::App;

#[derive(Clone)]
struct ExportJob {
    doc: DocumentId,
    map: Arc<Map>,
    path: PathBuf,
    types: Arc<CustomTypes>,
    target: PathBuf,
    options: Options,
    automatic: bool,
}
struct Finished {
    job: ExportJob,
    result: Result<Report, Error>,
}
enum Message {
    Export(Finished),
    Verified {
        doc: DocumentId,
        verdict: Result<bt_godot::verify::Verdict, String>,
    },
    Information {
        doc: DocumentId,
        text: Result<String, String>,
    },
    Preview {
        doc: DocumentId,
        text: String,
    },
    SetupSaved {
        doc: DocumentId,
        result: Result<(), String>,
    },
    ProjectPicked(Option<PathBuf>),
    All {
        doc: DocumentId,
        result: Result<String, String>,
    },
}
pub(super) struct Issue {
    text: String,
    elements: Vec<Element>,
    error: bool,
}
#[derive(Clone)]
struct SetupDialog {
    doc: DocumentId,
    file: PathBuf,
    project: String,
    settings: bt_project::GodotSetup,
    export_after: bool,
    preview: Option<String>,
}
enum Dialog {
    Setup(SetupDialog),
    Refused(Finished),
    Result(String),
}

pub(super) struct GodotState {
    exporter: Arc<Mutex<bt_godot::Exporter>>,
    sender: mpsc::Sender<Message>,
    results: mpsc::Receiver<Message>,
    busy: HashSet<DocumentId>,
    pending: HashMap<DocumentId, ExportJob>,
    pub(super) issues: HashMap<DocumentId, Vec<Issue>>,
    dialog: Option<Dialog>,
}
impl Default for GodotState {
    fn default() -> Self {
        let (sender, results) = mpsc::channel();
        Self {
            exporter: Arc::default(),
            sender,
            results,
            busy: HashSet::new(),
            pending: HashMap::new(),
            issues: HashMap::new(),
            dialog: None,
        }
    }
}
impl GodotState {
    pub(super) fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }
}

impl App {
    pub(super) fn start_godot(
        &mut self,
        ctx: &egui::Context,
        doc: DocumentId,
        operation: GodotOperation,
    ) {
        if let Err(error) = self.prepare_godot(ctx, doc, operation.clone()) {
            self.godot_error(doc, error, operation != GodotOperation::AfterSave);
        }
    }
    fn prepare_godot(
        &mut self,
        ctx: &egui::Context,
        doc: DocumentId,
        operation: GodotOperation,
    ) -> Result<(), String> {
        let path = self
            .editor
            .path_of(doc)
            .ok_or("Save the map before exporting it to Godot")?;
        let map = self
            .editor
            .shown_map(doc)
            .cloned()
            .ok_or("The document is not a map")?;
        let setup = bt_godot::setup::Setup::of(&path)?;
        if operation == GodotOperation::AfterSave
            && !setup
                .as_ref()
                .is_some_and(|s| s.godot.export_on_save.unwrap_or(false))
        {
            return Ok(());
        }
        if operation == GodotOperation::Setup {
            return self.open_godot_setup(doc, false);
        }
        if operation == GodotOperation::ExportAs {
            let suggested =
                bt_godot::scene_of(&path, &map)?.unwrap_or_else(|| path.with_extension("tscn"));
            return self.file_dialogs.ask(
                &mut self.workers,
                bt_editor::FileRequest::GodotScene { doc, suggested },
            );
        }
        let Some(target) = (match &operation {
            GodotOperation::ExportTo(target) => Some(target.clone()),
            _ => bt_godot::scene_of(&path, &map)?,
        }) else {
            if operation == GodotOperation::AfterSave {
                return Err(
                    "Map saved. Godot export needs a target; open Godot Setup or Export As.".into(),
                );
            }
            return self.open_godot_setup(doc, true);
        };
        if operation == GodotOperation::Verify {
            let godot = bt_godot::verify::executable().ok_or(bt_godot::verify::NO_GODOT)?;
            let sender = self.godot.sender.clone();
            return self.workers.spawn("Godot verify", move || {
                let result = bt_godot::verify::verify(&godot, &target);
                let _ = sender.send(Message::Verified {
                    doc,
                    verdict: result,
                });
                Vec::new()
            });
        }
        let job = ExportJob {
            doc,
            map,
            path,
            types: Arc::new(self.editor.custom_types().clone()),
            target,
            options: Options::default(),
            automatic: operation == GodotOperation::AfterSave,
        };
        self.spawn_godot(ctx, job)
    }
    fn open_godot_setup(&mut self, doc: DocumentId, export_after: bool) -> Result<(), String> {
        let path = self
            .editor
            .path_of(doc)
            .ok_or("Save the map before setting up Godot")?;
        let file = self.editor.export_project_file(doc)?;
        let old = bt_project::ProjectFile::load(&file).map_err(|e| e.to_string())?;
        let mut settings = old.godot().cloned().unwrap_or_default();
        let project = settings
            .project
            .as_deref()
            .map(|p| bt_project::paths::resolve(&file, p))
            .or_else(|| bt_godot::setup::find_project(&path));
        settings
            .levels
            .get_or_insert(bt_godot::setup::LEVELS.into());
        settings
            .images
            .get_or_insert(bt_godot::setup::IMAGES.into());
        settings
            .tile_set_folder
            .get_or_insert(bt_godot::setup::TILE_SETS.into());
        settings.tile_sets.get_or_insert("shared".into());
        self.godot.dialog = Some(Dialog::Setup(SetupDialog {
            doc,
            file,
            project: project.map(|p| p.display().to_string()).unwrap_or_default(),
            settings,
            export_after,
            preview: None,
        }));
        Ok(())
    }
    fn spawn_godot(&mut self, _ctx: &egui::Context, job: ExportJob) -> Result<(), String> {
        if self.godot.busy.contains(&job.doc) {
            self.godot.pending.insert(job.doc, job);
            return Ok(());
        }
        let doc = job.doc;
        let exporter = Arc::clone(&self.godot.exporter);
        let sender = self.godot.sender.clone();
        self.workers.spawn("Godot export", move || {
            let result = exporter
                .lock()
                .map_err(|_| Error::Failed("The Godot exporter lock is poisoned".into()))
                .and_then(|mut e| {
                    e.export_map(&job.map, &job.path, &job.types, &job.target, &job.options)
                });
            let result = result.and_then(|report| {
                if bt_godot::setup::Setup::of(&job.path)?.is_some_and(|s| s.level_scene()) {
                    bt_godot::starter::write(&job.target)?;
                }
                Ok(report)
            });
            let _ = sender.send(Message::Export(Finished { job, result }));
            Vec::new()
        })?;
        self.godot.busy.insert(doc);
        Ok(())
    }
    pub(super) fn take_godot_results(&mut self, ctx: &egui::Context) {
        while let Ok(message) = self.godot.results.try_recv() {
            match message {
                Message::Export(finished) => {
                    let doc = finished.job.doc;
                    self.godot.busy.remove(&doc);
                    match &finished.result {
                        Ok(report) => {
                            self.godot.issues.insert(
                                doc,
                                report
                                    .notes
                                    .iter()
                                    .map(|n| Issue {
                                        text: format!("{}: {}", n.code, n.message),
                                        elements: n.elements.clone(),
                                        error: false,
                                    })
                                    .collect(),
                            );
                            if !finished.job.automatic {
                                self.intents.push(Intent::RememberExport {
                                    doc,
                                    target: finished.job.target.clone(),
                                });
                            }
                            self.notify(Notice::info(format!(
                                "Godot: {} cells, {} tiles, {} objects exported to {}",
                                report.counts.cells,
                                report.counts.tiles,
                                report.counts.objects,
                                finished.job.target.display()
                            )));
                        }
                        Err(error) => {
                            self.godot_error(doc, error.to_string(), !finished.job.automatic);
                            if !finished.job.automatic && matches!(error, Error::Refused(_)) {
                                self.godot.dialog = Some(Dialog::Refused(finished));
                            }
                        }
                    }
                    if let Some(job) = self.godot.pending.remove(&doc)
                        && let Err(error) = self.spawn_godot(ctx, job)
                    {
                        self.godot_error(doc, error, false);
                    }
                }
                Message::Verified { doc, verdict } => match verdict {
                    Ok(verdict) => {
                        let text = verdict.text();
                        if !verdict.clean() {
                            self.godot.issues.insert(
                                doc,
                                vec![Issue {
                                    text: text.clone(),
                                    elements: Vec::new(),
                                    error: true,
                                }],
                            );
                        }
                        self.godot.dialog = Some(Dialog::Result(text));
                    }
                    Err(e) => self.godot_error(doc, e, true),
                },
                Message::Information { doc, text } => match text {
                    Ok(text) => self.godot.dialog = Some(Dialog::Result(text)),
                    Err(e) => self.godot_error(doc, e, true),
                },
                Message::Preview { doc, text } => {
                    if let Some(Dialog::Setup(setup)) = &mut self.godot.dialog
                        && setup.doc == doc
                    {
                        setup.preview = Some(text);
                    }
                }
                Message::SetupSaved { doc, result } => match result {
                    Ok(()) => {
                        self.notify(Notice::info("Godot setup saved"));
                        self.start_godot(ctx, doc, GodotOperation::Export);
                    }
                    Err(e) => self.godot_error(doc, e, true),
                },
                Message::ProjectPicked(path) => {
                    if let (Some(path), Some(Dialog::Setup(setup))) = (path, &mut self.godot.dialog)
                    {
                        setup.project = path.display().to_string();
                    }
                }
                Message::All { doc, result } => match result {
                    Ok(text) => self.godot.dialog = Some(Dialog::Result(text)),
                    Err(e) => self.godot_error(doc, e, true),
                },
            }
        }
    }
    fn godot_error(&mut self, doc: DocumentId, text: String, show: bool) {
        self.godot.issues.insert(
            doc,
            vec![Issue {
                text: text.clone(),
                elements: Vec::new(),
                error: true,
            }],
        );
        self.notify(Notice::error(text.clone()));
        if show {
            self.godot.dialog = Some(Dialog::Result(text));
        }
    }
    pub(super) fn show_godot_issues(&mut self, ui: &mut egui::Ui, doc: DocumentId) {
        if self.godot.busy.contains(&doc) {
            ui.spinner();
            ui.label("Exporting to Godot…");
        }
        let Some(issues) = self.godot.issues.get(&doc) else {
            ui.label("No export issues. Export to Godot to check this map.");
            return;
        };
        egui::ScrollArea::vertical().show(ui, |ui| {
            for issue in issues {
                let color = if issue.error {
                    egui::Color32::LIGHT_RED
                } else {
                    ui.visuals().text_color()
                };
                ui.colored_label(color, &issue.text);
                for element in &issue.elements {
                    if ui.small_button(format!("Go to {element:?}")).clicked() {
                        match *element {
                            Element::Layer(id) => self.intents.push(Intent::SelectLayer {
                                doc,
                                layer: LayerId(id),
                            }),
                            Element::Object(id) => {
                                if let Some(map) = self.editor.map_of(doc) {
                                    map.for_each_layer(|layer, _| {
                                        if let bt_core::LayerKind::Object(objects) = &layer.kind
                                            && let Some(object) = objects
                                                .objects
                                                .iter()
                                                .find(|o| o.id == ObjectId(id))
                                        {
                                            self.intents.push(Intent::SelectLayer {
                                                doc,
                                                layer: layer.id,
                                            });
                                            self.intents.push(Intent::SelectObjects {
                                                doc,
                                                ids: vec![ObjectId(id)],
                                                mode: bt_editor::SelectionMode::Replace,
                                            });
                                            self.intents.push(Intent::CenterOnObject {
                                                doc,
                                                object: object.id,
                                            });
                                        }
                                    });
                                }
                            }
                            Element::Tileset(slot) | Element::Tile { tileset: slot, .. } => {
                                if let Some(slot) = bt_core::TilesetSlot::new(slot) {
                                    self.intents.push(Intent::EditTileset { doc, slot });
                                    if let Element::Tile { id, .. } = *element {
                                        let tile = bt_core::TileId(id);
                                        self.intents.push(Intent::SelectTilesetTiles {
                                            doc,
                                            tiles: vec![tile],
                                            current: Some(tile),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                ui.separator();
            }
        });
    }
    pub(super) fn show_godot_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.godot.dialog.take() else {
            return;
        };
        let mut keep = true;
        let mut retry = None;
        let mut choose = None;
        let mut save = None;
        let mut pick_project = false;
        let mut preview = None;
        let mut rebuild = None;
        egui::Modal::new(egui::Id::new("godot_dialog")).show(ctx,|ui| {
            ui.set_max_width(650.0);
            match &mut dialog {
                Dialog::Result(text)=>{ui.heading("Godot");egui::ScrollArea::vertical().max_height(500.0).show(ui,|ui|{ui.label(&*text);});if ui.button("Close").clicked(){keep=false;}},
                Dialog::Setup(setup)=>{
                    ui.heading("Godot Setup");
                    field(ui,"Godot project folder (contains project.godot)",&mut setup.project);
                    if ui.button("Browse…").clicked(){pick_project=true;}
                    for (label,value) in [("Scenes",&mut setup.settings.levels),("Shared TileSets",&mut setup.settings.tile_set_folder),("External images",&mut setup.settings.images)] {
                        if let Some(value)=value {field(ui,label,value);}
                    }
                    if ui.button("Preview export").clicked(){preview=Some(setup.clone());}
                    if let Some(text)=&setup.preview {ui.label(text);}
                    ui.checkbox(setup.settings.export_on_save.get_or_insert(false),"Export after saving this map");
                    ui.checkbox(setup.settings.level_scene.get_or_insert(false),"Create a starter level scene once");
                    ui.label("Generated scenes are replaced on export. Put Godot scripts and your own nodes in the starter level scene.");
                    ui.horizontal(|ui|{if ui.button(if setup.export_after {"Save setup and export"}else{"Save setup"}).clicked(){save=Some(setup.clone());keep=false;}
if ui.button("Cancel").clicked(){keep=false;}});
                }
                Dialog::Refused(finished)=>{
                    ui.heading("Godot export needs a choice");
                    if let Err(Error::Refused(refusals))=&finished.result {
                        for refusal in refusals {ui.label(format!("{}: {}",refusal.path.display(),refusal.reason));for extra in &refusal.extras {ui.label(extra);}ui.label(refusal.way_out());}
                        if refusals.iter().all(|r|r.found.can_replace()) && ui.button("Keep backup and replace").clicked() {let mut job=finished.job.clone();job.options.replace=true;retry=Some(job);keep=false;}
                        if refusals.iter().any(|r|matches!(r.found,Found::Moved(_)|Found::UidInUse(_))) && ui.button("Start here with a new UID").clicked(){let mut job=finished.job.clone();job.options.new_uid=true;retry=Some(job);keep=false;}
                        for r in refusals {
                            if let Found::Moved(path)=&r.found && ui.button(format!("Export where it moved: {}",path.display())).clicked(){let mut job=finished.job.clone();job.target=path.clone();retry=Some(job);keep=false;}
                            if let Found::Missing(users)=&r.found && bt_godot::hand_made(users).is_empty() && ui.button("Export all generated maps again").clicked(){rebuild=Some(finished.job.clone());keep=false;}
                            if ui.button(format!("Show {}",r.path.display())).clicked(){ctx.open_url(egui::OpenUrl::new_tab(format!("file://{}", r.path.display())));}
                        }
                    }
                    if ui.button("Export to another scene…").clicked(){choose=Some(finished.job.doc);keep=false;}
                    if ui.button("Cancel").clicked(){keep=false;}
                }
            }
        });
        if keep {
            self.godot.dialog = Some(dialog);
        }
        if pick_project {
            let sender = self.godot.sender.clone();
            if let Err(e) = self.workers.spawn("Godot project", move || {
                let _ = sender.send(Message::ProjectPicked(rfd::FileDialog::new().pick_folder()));
                Vec::new()
            }) {
                self.notify(Notice::error(e));
            }
        }
        if let Some(SetupDialog {
            doc,
            file,
            project,
            mut settings,
            export_after,
            ..
        }) = save
        {
            let result = (|| {
                let project = std::fs::canonicalize(project).map_err(|e| e.to_string())?;
                settings.project =
                    Some(bt_project::paths::key(&file, &project).map_err(|e| e.to_string())?);
                let locks = self
                    .editor
                    .project_lock_folder()
                    .ok_or("No folder is configured for project-file locks")?
                    .to_path_buf();
                let sender = self.godot.sender.clone();
                self.workers.spawn("Godot setup", move || {
                    let result = bt_godot::setup::write(&file, settings, &locks);
                    let _ = sender.send(if export_after {
                        Message::SetupSaved { doc, result }
                    } else {
                        Message::Information {
                            doc,
                            text: result.map(|()| "Godot setup saved".into()),
                        }
                    });
                    Vec::new()
                })
            })();
            if let Err(error) = result {
                self.godot_error(doc, error, true);
            }
        }
        if let Some(SetupDialog {
            doc,
            project,
            settings,
            ..
        }) = preview
            && let (Some(path), Some(map)) = (
                self.editor.path_of(doc),
                self.editor.shown_map(doc).cloned(),
            )
        {
            let types = self.editor.custom_types().clone();
            let sender = self.godot.sender.clone();
            let result=self.workers.spawn("Godot preview",move||{
                let result=(|| {let project=std::fs::canonicalize(project).map_err(|e|e.to_string())?;let levels=bt_godot::setup::in_project(&project,settings.levels.as_deref().unwrap_or(bt_godot::setup::LEVELS)).ok_or("Scenes must be a res:// folder")?;let target=levels.join(path.file_stem().ok_or("Map has no file name")?).with_extension("tscn");let images=bt_godot::images_copied(&map,&path,&types,&target).map_err(|e|e.to_string())?;Ok::<_,String>(format!("Godot {:?}; stable node IDs: {}. {images} external images will be copied. Target: {}",bt_godot::setup::version(&project),bt_godot::setup::version(&project).is_some_and(bt_godot::setup::writes_unique_ids),target.display()))})();
                let _=sender.send(Message::Preview {doc,text:result.unwrap_or_else(|e|e)});Vec::new()
            });
            if let Err(e) = result {
                self.godot_error(doc, e, true);
            }
        }
        if let Some(job) = retry
            && let Err(e) = self.spawn_godot(ctx, job)
        {
            self.notify(Notice::error(e));
        }
        if let Some(doc) = choose {
            self.start_godot(ctx, doc, GodotOperation::ExportAs);
        }
        if let Some(job) = rebuild {
            let sender = self.godot.sender.clone();
            let doc = job.doc;
            let result = self.workers.spawn("Godot rebuild", move || {
                let result = (|| {
                    let project =
                        bt_godot::setup::find_project(&job.target).ok_or("No Godot project")?;
                    let mut loader = |_: &Path| {
                        Ok(bt_tmx::Loader::builder()
                            .custom_types(Arc::clone(&job.types))
                            .build())
                    };
                    let all = bt_godot::export_all(&project, &mut loader, &Options::default())
                        .map_err(|e| e.to_string())?;
                    let mut text = String::new();
                    for scene in all.scenes {
                        match scene.outcome {
                            Ok(_) => {
                                text.push_str(&format!("Exported {}\n", scene.scene.display()))
                            }
                            Err(e) => return Err(e.to_string()),
                        }
                    }
                    Ok(text)
                })();
                let _ = sender.send(Message::All { doc, result });
                Vec::new()
            });
            if let Err(e) = result {
                self.godot_error(doc, e, true);
            }
        }
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    let label = ui.label(label);
    ui.text_edit_singleline(value).labelled_by(label.id);
}
