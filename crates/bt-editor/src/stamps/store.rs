//! A folder of Tiled stamp files; changes are written before they appear in the store.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_tmx::{Loader, Stamp, WriteRequest};

/// One saved stamp, including the path of the file that owns its relative references.
#[derive(Clone, Debug)]
pub struct SavedStamp {
    /// The file.
    pub path: PathBuf,
    /// Its weighted variation maps.
    pub stamp: Stamp,
}

/// The stamp library. File operations run through a `StampJob` on the window's workers.
#[derive(Clone, Debug, Default)]
pub struct StampStore {
    pub(super) folder: PathBuf,
    pub(super) entries: Vec<SavedStamp>,
    pub(super) problems: Vec<String>,
    pub(super) generation: u64,
    pub(super) request_generation: u64,
    pub(super) busy: bool,
}

impl StampStore {
    pub(crate) fn new(folder: PathBuf) -> StampStore {
        StampStore {
            folder,
            ..Default::default()
        }
    }
    /// The folder currently used.
    pub fn folder(&self) -> &Path {
        &self.folder
    }
    /// Stamps in path order.
    pub fn entries(&self) -> &[SavedStamp] {
        &self.entries
    }
    /// Files that failed to load; readable stamps remain usable.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }
    /// Changes when the library is reloaded or edited.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// An operation is in flight.
    pub fn busy(&self) -> bool {
        self.busy
    }
}

/// A read or immediate file edit for a worker.
#[derive(Clone, Debug)]
pub struct StampJob {
    folder: PathBuf,
    generation: u64,
    types: Arc<bt_core::CustomTypes>,
    operation: Operation,
}

#[derive(Clone, Debug)]
pub(super) enum Operation {
    Load,
    Write(SavedStamp),
    Delete(PathBuf),
    RestoreQuick {
        generation: u64,
        records: Vec<crate::SessionStamp>,
    },
}

/// A worker's result, accepted only by the generation that requested it.
#[derive(Debug)]
pub enum StampsLoaded {
    /// A library operation finished.
    Library {
        /// The library revision that asked.
        generation: u64,
        /// The reloaded folder, or a file-operation failure.
        result: Result<StampStore, String>,
    },
    /// Session quick stamps were read with their external tilesets on a worker.
    Quick {
        /// The quick-stamp revision that asked.
        generation: u64,
        /// The restored slots.
        stamps: Box<[Option<Stamp>; 9]>,
        /// Unreadable or invalid stamps and loader warnings.
        notices: Vec<crate::Notice>,
    },
}

impl StampJob {
    pub(super) fn new(store: &StampStore, types: Arc<bt_core::CustomTypes>) -> Self {
        Self {
            folder: store.folder.clone(),
            generation: store.request_generation,
            types,
            operation: Operation::Load,
        }
    }
    pub(super) fn operation(mut self, operation: Operation) -> Self {
        self.operation = operation;
        self
    }

    /// Performs the requested file edit, then reloads the folder.
    pub fn run(&self) -> StampsLoaded {
        match &self.operation {
            Operation::RestoreQuick {
                generation,
                records,
            } => self.restore_quick(*generation, records),
            _ => StampsLoaded::Library {
                generation: self.generation,
                result: self.perform(),
            },
        }
    }

    /// Reports a worker-start failure to the same owner, releasing the library's busy state.
    pub fn failed(&self, message: String) -> StampsLoaded {
        match self.operation {
            Operation::RestoreQuick { generation, .. } => StampsLoaded::Quick {
                generation,
                stamps: Box::default(),
                notices: vec![crate::Notice::error(message)],
            },
            _ => StampsLoaded::Library {
                generation: self.generation,
                result: Err(message),
            },
        }
    }

    fn restore_quick(&self, generation: u64, records: &[crate::SessionStamp]) -> StampsLoaded {
        let mut stamps: Box<[Option<Stamp>; 9]> = Box::default();
        let mut notices = Vec::new();
        let mut loader = Loader::builder()
            .custom_types(Arc::clone(&self.types))
            .build();
        for saved in records {
            let read = (|| -> Result<(), String> {
                let slot = stamps
                    .get_mut(saved.slot)
                    .ok_or("Quick stamp slots are 1–9")?;
                let loaded = loader
                    .parse_stamp(&saved.text, &saved.path)
                    .map_err(|error| error.to_string())?;
                for warning in loaded.warnings {
                    notices.push(crate::Notice::warning(warning.to_string()));
                }
                super::convert::brush_of(&loaded.value, None, None)?;
                *slot = Some(loaded.value);
                Ok(())
            })();
            if let Err(error) = read {
                notices.push(crate::Notice::error(format!(
                    "Cannot restore quick stamp {}: {error}",
                    saved.slot.saturating_add(1)
                )));
            }
        }
        StampsLoaded::Quick {
            generation,
            stamps,
            notices,
        }
    }

    fn perform(&self) -> Result<StampStore, String> {
        match &self.operation {
            Operation::Load | Operation::RestoreQuick { .. } => {}
            Operation::Write(saved) => {
                std::fs::create_dir_all(&self.folder).map_err(|error| error.to_string())?;
                WriteRequest::for_stamp(&saved.stamp, &saved.path)
                    .write()
                    .map_err(|error| error.to_string())?;
            }
            Operation::Delete(path) => std::fs::remove_file(path)
                .map_err(|error| format!("Cannot delete {}: {error}", path.display()))?,
        }
        let mut store = StampStore::new(self.folder.clone());
        store.generation = self.generation;
        store.request_generation = self.generation;
        let mut files = Vec::new();
        collect_files(&self.folder, &mut files)?;
        files.sort();
        let mut loader = Loader::builder()
            .custom_types(Arc::clone(&self.types))
            .build();
        for path in files {
            match loader.load_stamp(&path) {
                Ok(loaded) => {
                    for warning in loaded.warnings {
                        store.problems.push(warning.to_string());
                    }
                    store.entries.push(SavedStamp {
                        path,
                        stamp: loaded.value,
                    });
                }
                Err(error) => store.problems.push(error.to_string()),
            }
        }
        Ok(store)
    }
}

fn collect_files(folder: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "Cannot read stamp folder {}: {error}",
                folder.display()
            ));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir() {
            collect_files(&entry.path(), files)?;
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("stamp"))
        {
            files.push(entry.path());
        }
    }
    Ok(())
}
