//! A directory tree standing in for a checked-out project.

use super::{scanning_everything, scratch};
use crate::model::{Candidate, ScanEvent};

/// A project directory that files and build output can be placed into.
pub struct a_project {
    dir: scratch,
}

impl a_project {
    pub fn new() -> Self {
        Self {
            dir: scratch::named("project"),
        }
    }

    /// A file in the project root, such as the manifest that proves what a
    /// build directory belongs to.
    pub fn with_a_file(self, name: &str) -> Self {
        let path = self.dir.path.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, "// fixture\n").unwrap();
        self
    }

    /// A directory holding one file, so it has a non-zero size.
    pub fn with_a_directory(self, name: &str) -> Self {
        let path = self.dir.path.join(name);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("output.bin"), vec![0u8; 2048]).unwrap();
        self
    }

    /// Everything the artifact scanner reports for this project.
    pub fn candidates(self) -> Vec<Candidate> {
        self.candidates_with(&crate::config::Config::default())
    }

    /// The same, under a given configuration.
    pub fn candidates_with(self, cfg: &crate::config::Config) -> Vec<Candidate> {
        let mut opts = scanning_everything(vec![self.dir.path.clone()]);
        opts.rules = std::sync::Arc::new(crate::scan::Rules::from_config(cfg));

        let (tx, rx) = std::sync::mpsc::channel();
        crate::scan::artifacts::scan(&opts, &tx);
        drop(tx);

        rx.into_iter()
            .filter_map(|e| match e {
                ScanEvent::Found(c) => Some(*c),
                _ => None,
            })
            .collect()
    }
}
