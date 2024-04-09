use std::fs::{self, Metadata};
use std::io::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

// TODO use the .gitignore file instead of this.
const IGNORE: [&str; 2] = [".git", "target"];
pub struct Workspace {
    workspace_dir: PathBuf,
}

impl Workspace {
    pub fn new(workspace_dir: PathBuf) -> Self {
        if !workspace_dir.is_dir() {
            panic!(
                "Workspace dir provided should not be a dir: {:?}",
                workspace_dir
            );
        }

        Workspace { workspace_dir }
    }

    pub fn list_files(&self, filepath: &Path) -> Vec<PathBuf> {
        let canonicalized = filepath
            .to_path_buf()
            .clone()
            .canonicalize()
            .expect("failed to canonicalize");

        // Swallows errors when accessing dir entries and only shows the entries we can access.

        // Return all entries in dir except for ignored ones. If a file is given, WalkDir yields only that file in the iterator.
        WalkDir::new(canonicalized)
            .into_iter()
            .filter_entry(|entry| {
                !IGNORE.contains(
                    &entry
                        .path()
                        .strip_prefix(&self.workspace_dir)
                        .expect("failed to strip prefix in ignore filter")
                        .to_string_lossy()
                        .as_ref(),
                )
            })
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                entry.file_type().is_file().then(|| {
                    entry
                        .path()
                        .strip_prefix(&self.workspace_dir)
                        .expect("failed to strip prefix from dir entry")
                        .to_path_buf()
                })
            })
            .collect()
    }

    pub fn read_file<P: AsRef<Path>>(&self, filepath: P) -> Result<Vec<u8>> {
        fs::read(self.workspace_dir.join(filepath))
    }

    pub fn stat_file<P: AsRef<Path>>(&self, path: P) -> Result<Metadata> {
        fs::metadata(self.workspace_dir.join(path))
    }
}
