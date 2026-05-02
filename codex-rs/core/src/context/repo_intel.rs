use codex_repo_intel::RepoIntelSnapshot;

use super::ContextualUserFragment;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RepoIntelContext {
    snapshot: RepoIntelSnapshot,
}

impl RepoIntelContext {
    pub(crate) fn new(snapshot: RepoIntelSnapshot) -> Self {
        Self { snapshot }
    }
}

impl ContextualUserFragment for RepoIntelContext {
    const ROLE: &'static str = "developer";
    const START_MARKER: &'static str = "<repo_intel>";
    const END_MARKER: &'static str = "</repo_intel>";

    fn body(&self) -> String {
        let rendered = self.snapshot.render_for_model();
        rendered
            .trim()
            .trim_start_matches(Self::START_MARKER)
            .trim_end_matches(Self::END_MARKER)
            .to_string()
    }
}
