use std::path::Path;

use crate::matcher::{DeletionPolicy, Matcher};

pub struct NodeModules;

impl Matcher for NodeModules {
    fn id(&self) -> &'static str {
        "node_modules"
    }

    fn matches(&self, dir: &Path) -> bool {
        dir.file_name().is_some_and(|name| name == "node_modules")
    }

    fn deletion_policy(&self) -> DeletionPolicy {
        DeletionPolicy::Instant
    }
}

pub struct Target;

impl Matcher for Target {
    fn id(&self) -> &'static str {
        "target"
    }

    fn matches(&self, dir: &Path) -> bool {
        if !dir.file_name().is_some_and(|name| name == "target") {
            return false;
        }
        dir.parent()
            .is_some_and(|parent| parent.join("Cargo.toml").is_file())
    }

    fn deletion_policy(&self) -> DeletionPolicy {
        DeletionPolicy::Instant
    }
}

pub struct Venv;

impl Matcher for Venv {
    fn id(&self) -> &'static str {
        "venv"
    }

    fn matches(&self, dir: &Path) -> bool {
        if !dir
            .file_name()
            .is_some_and(|name| name == ".venv" || name == "venv" || name == ".virtualenv")
        {
            return false;
        }
        dir.join("pyvenv.cfg").is_file()
    }

    fn deletion_policy(&self) -> DeletionPolicy {
        DeletionPolicy::Confirm(Some("Ensure dependencies are exported to requirements.txt"))
    }
}

static NODE_MODULES: NodeModules = NodeModules;
static TARGET: Target = Target;
static VENV: Venv = Venv;

pub static REGISTRY: &[&dyn Matcher] = &[&NODE_MODULES, &TARGET, &VENV];

pub fn claim(dir: &Path) -> Option<&'static str> {
    REGISTRY
        .iter()
        .find(|matcher| matcher.matches(dir))
        .map(|matcher| matcher.id())
}

pub fn matcher_for(id: &str) -> Option<&'static dyn Matcher> {
    REGISTRY.iter().find(|matcher| matcher.id() == id).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn venv_confirm_carries_export_caution() {
        let root = tempfile::tempdir().unwrap();
        let venv = root.path().join(".venv");
        std::fs::create_dir(&venv).unwrap();
        std::fs::write(venv.join("pyvenv.cfg"), "").unwrap();
        let id = claim(&venv).unwrap();
        let matcher = matcher_for(id).unwrap();
        assert_eq!(
            matcher.deletion_policy(),
            DeletionPolicy::Confirm(Some("Ensure dependencies are exported to requirements.txt"))
        );
    }
}
