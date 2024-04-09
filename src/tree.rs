use crate::object::Object;
use crate::Blob;
use sha1_smol::{Digest, Sha1};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::fmt::Formatter;
use std::path::{Path, PathBuf};

const EXECUTABLE_MODE: &str = "100755";
const NON_EXECUTABLE_MODE: &str = "100644";
const DIRECTORY_MODE: &str = "40000";

enum TreeEntry {
    T(Tree),
    B(Blob),
}

impl Debug for TreeEntry {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            TreeEntry::T(t) => fmt.write_fmt(format_args!("\n{t:?}")),
            TreeEntry::B(b) => fmt
                .debug_struct("Blob")
                .field("oid", &b.get_oid().to_string())
                .finish(),
        }
    }
}

#[derive(Default)]
pub struct Tree {
    oid: Option<Digest>,
    content: Option<Vec<u8>>,
    entries: BTreeMap<PathBuf, TreeEntry>,
}

impl Debug for Tree {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        fmt.debug_struct("Tree")
            .field("oid", &self.oid)
            .field("entries", &self.entries)
            .finish()
    }
}

impl Tree {
    fn add_entry(&mut self, parents: Vec<PathBuf>, blob: Blob) {
        // Insert the blob at this point since we've bottomed out while recursing this subtree.
        if parents.is_empty() {
            self.entries.insert(
                blob.get_path()
                    .file_name()
                    .expect("could not get base filename in add_entry")
                    .into(),
                TreeEntry::B(blob),
            );
        } else {
            let base_dir = parents.first().unwrap();

            // Insert a tree if needed.
            if !self.entries.contains_key(base_dir) {
                self.entries
                    .insert(base_dir.to_path_buf(), TreeEntry::T(Tree::default()));
            }

            // Recurse into the tree.
            match self.entries.get_mut(base_dir).unwrap() {
                TreeEntry::T(ref mut tree) => {
                    tree.add_entry(parents.into_iter().skip(1).collect(), blob);
                }
                _ => panic!("supposed to be a tree here!"),
            }
        }
    }

    fn build(&mut self) -> Vec<u8> {
        // Each entry (blob or tree and its contents) will be represented as a Vec<u8>. We'll have a Vec of those entries' data.
        // let mut entries_data : Vec<String> = Vec::new();
        let mut entries_data: Vec<Vec<u8>> = Vec::new();

        for (path, entry) in &mut self.entries {
            match entry {
                TreeEntry::T(ref mut tree) => {
                    let entry_data = tree.build();

                    let content = format!("tree {}\0", entry_data.len());
                    let content = [content.as_bytes(), entry_data.as_slice()].concat();
                    let oid = Sha1::from(&content).digest();

                    tree.content = Some(content);
                    tree.oid = Some(oid);
                    // Now the tree is all set up and ready to go.

                    // Add this tree as an entry so its parent can use it.
                    let mode = DIRECTORY_MODE;
                    let prefix = format!("{mode} {}\0", path.to_string_lossy());
                    let oid_bytes = oid.bytes();
                    entries_data.push([prefix.as_bytes(), &oid_bytes[..]].concat());
                }
                TreeEntry::B(blob) => {
                    // Each entry is is represented as a string with the mode, a space, the filename, a null byte, and 20 bytes for the oid.
                    let mode = if blob.is_executable() {
                        EXECUTABLE_MODE
                    } else {
                        NON_EXECUTABLE_MODE
                    };
                    let prefix = format!("{mode} {}\0", path.to_string_lossy());
                    let oid_bytes = blob.get_oid().bytes();
                    entries_data.push([prefix.as_bytes(), &oid_bytes[..]].concat());
                }
            }
        }

        entries_data.into_iter().flatten().collect()
    }

    pub fn new(mut blobs: Vec<Blob>) -> Self {
        blobs.sort();

        // Create a tree filled with entries.
        let mut root = Tree::default();
        for blob in blobs {
            root.add_entry(Self::get_parent_directories(blob.get_path()), blob);
        }

        // Traverse those entries and fill out each Tree's oid and content on the way back from the recursion.
        // We couldn't have filled out the oid and content in the earlier loop because we never knew how many entries were in each directory.
        let tree_data = root.build();

        let content = format!("tree {}\0", tree_data.len());
        let content = [content.as_bytes(), tree_data.as_slice()].concat();
        // NOTE: remember to create a SHA of the actual content, not a Some(content). This bug cost me two days.
        root.oid = Some(Sha1::from(&content).digest());
        root.content = Some(content);

        root
    }

    pub fn traverse<F>(&self, f: &mut F)
    where
        // TODO find a way so we avoid this virtual/dynamic dispatch
        F: FnMut(&dyn Object),
    {
        for entry in self.entries.values() {
            match entry {
                TreeEntry::T(tree) => {
                    tree.traverse(f);
                }
                TreeEntry::B(blob) => {
                    f(blob);
                }
            }
        }
        f(self);
    }

    // TODO put this somewhere better so we don't have to import Tree in Index.
    pub fn get_parent_directories(path: &Path) -> Vec<PathBuf> {
        path.ancestors()
            .map(|p| p.to_path_buf())
            .collect::<Vec<_>>()
            .into_iter()
            .skip(1)
            .rev()
            .skip(1)
            .map(|path| path.file_name().unwrap().into())
            .collect()
    }
}

impl Object for Tree {
    fn get_oid(&self) -> &Digest {
        self.oid.as_ref().expect("Tree has no oid set")
    }
    fn get_content(&self) -> &[u8] {
        self.content.as_ref().expect("Tree has no content set")
    }
}
