//! Versioned worker jobs. A late result can never restore a stale reference repair.
use super::resources::{Resource, resources_of};
use super::{Issue, deduplicate_issues};
use crate::{AssetId, AssetStore, DocumentId, Editor, Effect};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub(crate) struct Issues {
    records: HashMap<DocumentId, IssuesLoaded>,
    request: u64,
    pending: Option<u64>,
    current: Option<DocumentId>,
    next_ms: u64,
}

/// An immutable snapshot to validate away from the frame thread.
#[derive(Clone, Debug)]
pub struct IssueJob {
    doc: DocumentId,
    asset: AssetId,
    revision: u64,
    request: u64,
    store: AssetStore,
    scope: Vec<AssetId>,
}
/// A completed diagnostic snapshot.
#[derive(Debug)]
pub struct IssuesLoaded {
    doc: DocumentId,
    asset: AssetId,
    revision: u64,
    request: u64,
    issues: Vec<Issue>,
    resources: Vec<Resource>,
}
impl IssueJob {
    /// Checks model invariants, file access and image headers on the calling worker.
    pub fn run(&self) -> IssuesLoaded {
        let resources = resources_of(&self.store, &self.scope);
        let issues = super::collect::collect(&self.store, &self.scope, &resources);
        IssuesLoaded {
            doc: self.doc,
            asset: self.asset,
            revision: self.revision,
            request: self.request,
            issues,
            resources,
        }
    }
    /// A failure to start the diagnostics worker, delivered through the same result path.
    pub fn failed(&self, error: String) -> IssuesLoaded {
        IssuesLoaded {
            doc: self.doc,
            asset: self.asset,
            revision: self.revision,
            request: self.request,
            issues: vec![
                Issue::new(super::IssueKind::Worker, error).severity(crate::Severity::Error),
            ],
            resources: Vec::new(),
        }
    }
}
impl Editor {
    /// Current diagnostics, including the latest AutoMap results.
    pub fn issues(&self, doc: DocumentId) -> Vec<Issue> {
        let mut issues = self
            .issues
            .records
            .get(&doc)
            .filter(|record| record.revision == self.store.generation())
            .map(|record| record.issues.clone())
            .unwrap_or_default();
        issues.extend(self.automap_issues(doc));
        for problem in self.project_scan_problems() {
            issues.push(Issue::new(
                super::IssueKind::Project(problem.clone()),
                problem.to_string(),
            ));
        }
        deduplicate_issues(issues)
    }
    /// Whether file and model checks for this version are still pending.
    pub fn issues_pending(&self, doc: DocumentId) -> bool {
        self.issues
            .records
            .get(&doc)
            .is_none_or(|record| record.revision != self.store.generation())
    }
    /// Renderer failures for exactly the images referenced by this document's dependencies.
    pub fn image_issues(
        &self,
        doc: DocumentId,
        failures: &HashMap<bt_image::ImageKey, bt_image::Error>,
    ) -> Vec<Issue> {
        let Some(record) = self
            .issues
            .records
            .get(&doc)
            .filter(|record| record.revision == self.store.generation())
        else {
            return Vec::new();
        };
        record
            .resources
            .iter()
            .filter_map(|resource| {
                resource.image.as_ref()?;
                failures
                    .iter()
                    .find(|(key, _)| resource.image_key.as_ref() == Some(*key))
                    .map(|(_, error)| resource.issue(super::collect::image_problem(error.clone())))
            })
            .collect()
    }
    pub(crate) fn tick_issues(&mut self, now_ms: u64) -> Vec<Effect> {
        let Some(doc) = self.documents.current() else {
            return Vec::new();
        };
        let changed = self.issues.current != Some(doc);
        if !changed && (self.issues.pending.is_some() || now_ms < self.issues.next_ms) {
            return Vec::new();
        }
        let Some(asset) = self.documents.get(doc).map(|doc| doc.asset()) else {
            return Vec::new();
        };
        self.issues.request += 1;
        self.issues.pending = Some(self.issues.request);
        self.issues.current = Some(doc);
        self.issues.next_ms = now_ms.saturating_add(2_000);
        let mut scope = vec![asset];
        scope.extend(self.store.dependencies_of(asset));
        scope.extend(self.project_asset());
        scope.extend(self.world_of(doc));
        scope.sort_unstable();
        scope.dedup();
        vec![Effect::CheckIssues(IssueJob {
            doc,
            asset,
            revision: self.store.generation(),
            request: self.issues.request,
            store: self.store.clone(),
            scope,
        })]
    }
    pub(crate) fn issues_checked(&mut self, loaded: IssuesLoaded) -> Vec<Effect> {
        if self.issues.pending != Some(loaded.request) {
            return Vec::new();
        }
        self.issues.pending = None;
        if self
            .documents
            .get(loaded.doc)
            .is_some_and(|doc| doc.asset() == loaded.asset)
            && loaded.revision == self.store.generation()
        {
            self.issues.records.insert(loaded.doc, loaded);
        } else {
            self.issues.next_ms = 0;
        }
        self.issues
            .records
            .retain(|doc, _| self.documents.get(*doc).is_some());
        Vec::new()
    }
    pub(crate) fn refresh_issues(&mut self) -> Vec<Effect> {
        self.issues.pending = None;
        self.issues.next_ms = 0;
        self.tick_issues(0)
    }
}
