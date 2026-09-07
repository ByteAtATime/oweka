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

pub static REGISTRY: &[&dyn Matcher] = &[&NodeModules, &Target, &Venv];

pub fn claim(dir: &Path) -> Option<&'static dyn Matcher> {
    REGISTRY
        .iter()
        .find(|matcher| matcher.matches(dir))
        .copied()
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
        let matcher = claim(&venv).unwrap();
        assert_eq!(
            matcher.deletion_policy(),
            DeletionPolicy::Confirm(Some("Ensure dependencies are exported to requirements.txt"))
        );
    }
}
