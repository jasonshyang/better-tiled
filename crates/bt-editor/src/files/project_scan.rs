//! Project folder scans run away from the frame thread. Results carry the owning asset and
//! generation so an old scan cannot replace a newer tree, even on a manual refresh.

use crate::AssetId;
use std::path::{Path, PathBuf};

/// One file or folder in the project browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectEntry {
    /// Resolved path.
    pub path: PathBuf,
    /// Whether this is a directory, including an empty one.
    pub directory: bool,
    /// Children in natural order, directories first.
    pub children: Vec<ProjectEntry>,
}

impl ProjectEntry {
    /// Whether Open supports this file as a Tiled document or auxiliary asset.
    pub fn openable(&self) -> bool {
        !self.directory
            && self
                .path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "tmx"
                            | "tmj"
                            | "tsx"
                            | "tsj"
                            | "tx"
                            | "tj"
                            | "world"
                            | "tiled-project"
                            | "json"
                    )
                })
    }
}

/// A typed browser problem, retained until the next scan.
#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
pub enum ProjectProblem {
    /// A directory entry or its metadata could not be read.
    #[error("{}: {message}", path.display())]
    Read {
        /// The directory or entry being read.
        path: PathBuf,
        /// Stable IO classification.
        kind: std::io::ErrorKind,
        /// Operating system detail.
        message: String,
    },
    /// A tree exceeded the supported nesting depth.
    #[error("{}: folder nesting exceeds 64 levels", .0.display())]
    DepthLimit(PathBuf),
    /// A project exceeded the browser's scan budget.
    #[error("The project browser reached 100,000 entries; add narrower folders to browse the rest")]
    EntryLimit,
}

impl ProjectProblem {
    fn read(path: &Path, error: std::io::Error) -> Self {
        Self::Read {
            path: path.to_path_buf(),
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}

/// A snapshot of the project's folders to scan on a worker.
#[derive(Clone, Debug)]
pub struct ProjectScanJob {
    pub(super) asset: AssetId,
    pub(super) generation: u64,
    pub(super) folders: Vec<PathBuf>,
}

/// A completed scan, including folders that could not be read.
#[derive(Debug)]
pub struct ProjectFiles {
    pub(super) job: ProjectScanJob,
    pub(super) entries: Vec<ProjectEntry>,
    pub(super) problems: Vec<ProjectProblem>,
}

impl ProjectScanJob {
    /// Reads and sorts the tree. Nested directory symlinks are not followed.
    pub fn run(&self) -> ProjectFiles {
        let mut files = ProjectFiles {
            job: self.clone(),
            entries: Vec::new(),
            problems: Vec::new(),
        };
        let mut remaining = 100_000;
        for folder in &self.folders {
            files
                .entries
                .push(scan(folder, 0, &mut remaining, &mut files.problems));
        }
        if remaining == 0 {
            files.problems.push(ProjectProblem::EntryLimit);
        }
        files
    }
}

fn scan(
    path: &Path,
    depth: usize,
    remaining: &mut usize,
    problems: &mut Vec<ProjectProblem>,
) -> ProjectEntry {
    let mut entry = ProjectEntry {
        path: path.to_path_buf(),
        directory: true,
        children: Vec::new(),
    };
    if depth >= 64 {
        problems.push(ProjectProblem::DepthLimit(path.to_path_buf()));
        return entry;
    }
    let read = match std::fs::read_dir(path) {
        Ok(read) => read,
        Err(error) => {
            problems.push(ProjectProblem::read(path, error));
            return entry;
        }
    };
    for child in read {
        if *remaining == 0 {
            break;
        }
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                problems.push(ProjectProblem::read(path, error));
                continue;
            }
        };
        if child.file_name() == ".git" {
            continue;
        }
        let path = child.path();
        let kind = match child.file_type() {
            Ok(kind) => kind,
            Err(error) => {
                problems.push(ProjectProblem::read(&path, error));
                continue;
            }
        };
        *remaining -= 1;
        entry.children.push(if kind.is_dir() {
            scan(&path, depth + 1, remaining, problems)
        } else {
            ProjectEntry {
                path,
                directory: false,
                children: Vec::new(),
            }
        });
    }
    entry.children.sort_by(|a, b| {
        b.directory.cmp(&a.directory).then_with(|| {
            bt_utils::text::natural_cmp(
                &bt_utils::paths::display_name(&a.path),
                &bt_utils::paths::display_name(&b.path),
            )
        })
    });
    entry
}

impl crate::Editor {
    fn current_project_scan(&self) -> Option<&ProjectFiles> {
        let files = self.files.project_scan.as_ref()?;
        let project = self.store.project(files.job.asset)?;
        (self.files.project == Some(files.job.asset)
            && project
                .folders
                .iter()
                .map(|folder| &folder.path)
                .eq(files.job.folders.iter()))
        .then_some(files)
    }

    /// The last successful tree for the current project's current folders.
    pub fn project_entries(&self) -> &[ProjectEntry] {
        self.current_project_scan()
            .map_or(&[], |files| files.entries.as_slice())
    }
    /// Folder read errors and scan limits, retained until the next refresh.
    pub fn project_scan_problems(&self) -> &[ProjectProblem] {
        self.current_project_scan()
            .map_or(&[], |files| files.problems.as_slice())
    }
    pub(crate) fn sync_project_files(&mut self) -> Vec<crate::Effect> {
        let Some(asset) = self.files.project.and_then(|id| self.store.get(id)) else {
            return Vec::new();
        };
        let Some(project) = self.store.project(asset.id()) else {
            return Vec::new();
        };
        let folders = project
            .folders
            .iter()
            .map(|folder| folder.path.clone())
            .collect::<Vec<_>>();
        if self
            .files
            .project_scan_request
            .as_ref()
            .is_some_and(|job| job.asset == asset.id() && job.folders == folders)
        {
            return Vec::new();
        }
        self.files.project_scan_generation += 1;
        let job = ProjectScanJob {
            asset: asset.id(),
            generation: self.files.project_scan_generation,
            folders,
        };
        self.files.project_scan_request = Some(job.clone());
        vec![crate::Effect::ScanProject(job)]
    }
    pub(crate) fn project_files_scanned(&mut self, files: ProjectFiles) -> Vec<crate::Effect> {
        let current = self.files.project_scan_request.as_ref().is_some_and(|job| {
            job.asset == files.job.asset && job.generation == files.job.generation
        });
        let folders_match = self.store.project(files.job.asset).is_some_and(|project| {
            project
                .folders
                .iter()
                .map(|folder| &folder.path)
                .eq(files.job.folders.iter())
        });
        if self.files.project == Some(files.job.asset) && current && folders_match {
            self.files.project_scan = Some(files);
        }
        Vec::new()
    }
    pub(crate) fn refresh_project_files(&mut self) -> Vec<crate::Effect> {
        self.files.project_scan_request = None;
        self.sync_project_files()
    }
}
