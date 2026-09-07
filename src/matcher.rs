use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeletionPolicy {
    Instant,
    Confirm(Option<&'static str>),
}

pub trait Matcher: Send + Sync {
    fn id(&self) -> &'static str;
    fn matches(&self, dir: &Path) -> bool;
    fn deletion_policy(&self) -> DeletionPolicy;
    fn delete(&self, artifact: &Artifact) -> io::Result<()> {
        std::fs::remove_dir_all(artifact.path())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    pub matcher_id: &'static str,
    pub path: PathBuf,
}

impl Artifact {
    pub fn path(&self) -> &Path {
        &self.path
    }
}
