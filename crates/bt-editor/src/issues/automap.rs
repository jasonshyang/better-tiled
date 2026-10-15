//! Rules' typed problems, including the latest command and Live results.
use super::{Issue, IssueKind, IssueTarget};
use crate::{DocumentId, Editor, Severity};
fn problem_issue(problem: &bt_automap::Problem) -> Issue {
    let severity = match problem.severity() {
        bt_automap::Severity::Error => Severity::Error,
        bt_automap::Severity::Warning => Severity::Warning,
        bt_automap::Severity::Note => Severity::Info,
    };
    Issue::new(IssueKind::AutoMap(problem.kind), problem.text.clone()).severity(severity)
}
impl Editor {
    pub(super) fn automap_issues(&self, doc: DocumentId) -> Vec<Issue> {
        let mut issues = Vec::new();
        if let Some(problems) = self
            .document(doc)
            .and_then(|doc| self.automap.restoration.get(&doc.asset()))
        {
            issues.extend(problems.iter().map(problem_issue));
        }
        if let Some(state) = self.automap_rules(doc) {
            if let Some(error) = state.error() {
                issues.push(Issue::new(IssueKind::AutoMapRead, error).severity(Severity::Error));
            }
            if let Some(rules) = state.rules() {
                issues.extend(rules.problems().iter().map(problem_issue));
                for map in rules.rule_maps() {
                    issues.extend(map.problems().iter().map(|problem| {
                        problem_issue(problem).target(IssueTarget::File(map.path().to_path_buf()))
                    }));
                }
            }
            if let Some(run) = state.last_run() {
                issues.extend(run.report.problems.iter().map(problem_issue));
                for idle in &run.idle {
                    issues.extend(idle.problems(&run.rules).iter().map(problem_issue));
                }
            }
        }
        if let Some(live) = self.automap.live.get(&doc) {
            issues.extend(live.layers.problems().map(problem_issue));
        }
        issues
    }
}
