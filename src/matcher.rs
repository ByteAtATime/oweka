use std::path::{Path, PathBuf};

pub trait Matcher: Send + Sync {
    fn id(&self) -> &'static str;
    fn matches(&self, dir: &Path) -> bool;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    pub matcher_id: &'static str,
    pub path: PathBuf,
}
